//! A thin FSEvents stream: the kernel's directory-change feed, delivered over a channel.
//!
//! The stream is per directory (the default flavour; `kFSEventStreamCreateFlagFileEvents` is not
//! set), watches its roots (`kFSEventStreamCreateFlagWatchRoot`) and is scheduled on a private
//! serial dispatch queue, so its callback runs on a libdispatch thread and hands each event to an
//! unbounded channel for the UI. The callback's `info` is a boxed `UnboundedSender<Event>`; the
//! box lives until `Drop` has stopped, invalidated and released the stream and then waited for the
//! queue to drain, so no callback can reach a freed sender.

// Temporary: nothing calls this yet; the allowance is removed when Task 7 uses the module.
#![allow(dead_code)]

use futures::channel::mpsc::UnboundedSender;
use libc::{c_char, c_void};
use std::ffi::{CStr, CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr;

/// How long FSEvents gathers events before delivering them, in seconds.
pub const LATENCY: f64 = 1.0;

/// One thing the stream reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Something changed in this directory (per-directory flavour; the path is as the kernel reports it, real).
    Changed(PathBuf),
    /// The kernel dropped events: rescan from the start.
    MustScan,
    /// A watched root was renamed, moved or removed.
    RootChanged,
}

type FSEventStreamRef = *mut c_void;
type CFArrayRef = *const c_void;
type CFStringRef = *const c_void;
type CFAllocatorRef = *const c_void;
type DispatchQueue = *mut c_void;

type FSEventStreamCallback = extern "C" fn(
    stream: FSEventStreamRef,
    info: *mut c_void,
    num_events: usize,
    event_paths: *mut c_void,
    event_flags: *const u32,
    event_ids: *const u64,
);

#[repr(C)]
struct FSEventStreamContext {
    version: isize,
    info: *mut c_void,
    retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<extern "C" fn(*const c_void)>,
    copy_description: Option<extern "C" fn(*const c_void) -> CFStringRef>,
}

// `CFArrayCallBacks` from `CoreFoundation/CFArray.h`; only its address is used.
#[repr(C)]
struct CFArrayCallBacks {
    version: isize,
    retain: *const c_void,
    release: *const c_void,
    copy_description: *const c_void,
    equal: *const c_void,
}

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_FS_EVENT_STREAM_CREATE_FLAG_NONE: u32 = 0x0;
const K_FS_EVENT_STREAM_CREATE_FLAG_WATCH_ROOT: u32 = 0x4;
const K_FS_EVENT_STREAM_EVENT_FLAG_MUST_SCAN_SUB_DIRS: u32 = 0x1;
const K_FS_EVENT_STREAM_EVENT_FLAG_HISTORY_DONE: u32 = 0x10;
const K_FS_EVENT_STREAM_EVENT_FLAG_ROOT_CHANGED: u32 = 0x20;
const K_FS_EVENT_STREAM_EVENT_ID_SINCE_NOW: u64 = 0xFFFF_FFFF_FFFF_FFFF;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFTypeArrayCallBacks: CFArrayCallBacks;
    fn CFStringCreateWithCString(alloc: CFAllocatorRef, c_str: *const c_char, encoding: u32) -> CFStringRef;
    fn CFArrayCreate(
        alloc: CFAllocatorRef,
        values: *const *const c_void,
        num_values: isize,
        callbacks: *const CFArrayCallBacks,
    ) -> CFArrayRef;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn FSEventStreamCreate(
        allocator: CFAllocatorRef,
        callback: FSEventStreamCallback,
        context: *const FSEventStreamContext,
        paths: CFArrayRef,
        since_when: u64,
        latency: f64,
        flags: u32,
    ) -> FSEventStreamRef;
    fn FSEventStreamSetDispatchQueue(stream: FSEventStreamRef, queue: DispatchQueue);
    fn FSEventStreamStart(stream: FSEventStreamRef) -> u8;
    fn FSEventStreamStop(stream: FSEventStreamRef);
    fn FSEventStreamInvalidate(stream: FSEventStreamRef);
    fn FSEventStreamRelease(stream: FSEventStreamRef);
    fn FSEventStreamGetLatestEventId(stream: FSEventStreamRef) -> u64;
    fn FSEventsGetCurrentEventId() -> u64;
}

// libdispatch lives in libSystem, which every binary links.
extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> DispatchQueue;
    fn dispatch_sync_f(queue: DispatchQueue, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn dispatch_release(object: DispatchQueue);
}

/// A running FSEvents stream. Dropping it stops the stream and then drops the sender.
pub struct Stream {
    stream: FSEventStreamRef,
    queue: DispatchQueue,
    tx: *mut UnboundedSender<Event>,
    /// The system's event id read just before the stream was made: a safe point to resume from
    /// while the stream has delivered nothing.
    created_at: u64,
    /// Whether `FSEventStreamStart` succeeded, so `Drop` stops only a started stream.
    started: bool,
}

// SAFETY: the stream and queue handles may be used from whichever single thread owns `Stream`,
// and the boxed sender is only read by the callback (through `&`, and `UnboundedSender` is
// `Sync`) until `Drop` frees it after the queue has drained. `Stream` is deliberately not `Sync`:
// FSEvents makes no thread-safety promise for calls on one stream from several threads at once,
// so the raw pointer fields are left to keep the auto trait off.
unsafe impl Send for Stream {}

impl Stream {
    /// Starts a stream over `paths` with `kFSEventStreamCreateFlagWatchRoot`, delivering since
    /// `since` (`None` for now) on `tx`. Err is the reason, for the section's notice.
    pub fn start(paths: &[PathBuf], since: Option<u64>, tx: UnboundedSender<Event>) -> Result<Stream, String> {
        if paths.is_empty() {
            return Err("no folders to watch".into());
        }
        let c_paths = paths
            .iter()
            .map(|p| CString::new(p.as_os_str().as_bytes()).map_err(|_| format!("{} holds a NUL byte", p.display())))
            .collect::<Result<Vec<_>, _>>()?;
        let mut strings: Vec<CFStringRef> = Vec::with_capacity(c_paths.len());
        for c in &c_paths {
            // SAFETY: `c` is a NUL-terminated C string that outlives the call; a null allocator
            // means the default one.
            let s = unsafe { CFStringCreateWithCString(ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8) };
            if s.is_null() {
                for s in &strings {
                    // SAFETY: each `s` was created above and is released once.
                    unsafe { CFRelease(*s) };
                }
                return Err("a folder's path is not valid UTF-8".into());
            }
            strings.push(s);
        }
        // SAFETY: `strings` holds `strings.len()` live CFStrings; `kCFTypeArrayCallBacks` makes
        // the array retain them, so they can be released right after.
        let array = unsafe {
            CFArrayCreate(ptr::null(), strings.as_ptr(), strings.len() as isize, &kCFTypeArrayCallBacks)
        };
        for s in &strings {
            // SAFETY: each string was created above and is released once; the array, if made,
            // holds its own retain.
            unsafe { CFRelease(*s) };
        }
        if array.is_null() {
            return Err("could not make the list of folders".into());
        }

        // SAFETY: takes nothing and only reads the system's event counter.
        let created_at = unsafe { FSEventsGetCurrentEventId() };
        let info = Box::into_raw(Box::new(tx));
        let context = FSEventStreamContext {
            version: 0,
            info: info.cast(),
            retain: None,
            release: None,
            copy_description: None,
        };
        // SAFETY: `context` is read during the call only (FSEvents copies it); `array` is a live
        // CFArray of CFStrings; `callback` matches `FSEventStreamCallback`.
        let stream = unsafe {
            FSEventStreamCreate(
                ptr::null(),
                callback,
                &context,
                array,
                since.unwrap_or(K_FS_EVENT_STREAM_EVENT_ID_SINCE_NOW),
                LATENCY,
                K_FS_EVENT_STREAM_CREATE_FLAG_NONE | K_FS_EVENT_STREAM_CREATE_FLAG_WATCH_ROOT,
            )
        };
        // SAFETY: the stream keeps its own copy of the paths, so this release is the array's last.
        unsafe { CFRelease(array) };
        if stream.is_null() {
            // SAFETY: no stream exists, so nothing else holds `info`; it came from `Box::into_raw`.
            drop(unsafe { Box::from_raw(info) });
            return Err("FSEvents refused the stream".into());
        }

        // SAFETY: the label is a static C string; a null attribute makes a serial queue.
        let queue = unsafe { dispatch_queue_create(c"sh.csarko.arkos.fsevents".as_ptr(), ptr::null()) };
        if queue.is_null() {
            // SAFETY: the stream was never scheduled or started, so releasing it is enough and no
            // callback can run; `info` is then unreferenced.
            unsafe {
                FSEventStreamRelease(stream);
                drop(Box::from_raw(info));
            }
            return Err("could not make a dispatch queue".into());
        }
        let mut s = Stream { stream, queue, tx: info, created_at, started: false };
        // SAFETY: `stream` and `queue` are live; the stream retains the queue.
        let started = unsafe {
            FSEventStreamSetDispatchQueue(s.stream, s.queue);
            FSEventStreamStart(s.stream)
        };
        if started == 0 {
            // `Drop` skips Stop on a stream that never started, then invalidates, releases and
            // frees, in that order.
            drop(s);
            return Err("FSEvents could not start the stream".into());
        }
        s.started = true;
        Ok(s)
    }

    /// The id of the newest event the stream has seen, to resume from with `start`'s `since`.
    /// Before a stream started "since now" has delivered anything FSEvents answers with the
    /// "since now" marker itself, so the system's event id read just before the stream was made
    /// is given instead: events still waiting out the latency come after it, so a resume from it
    /// loses none of them.
    pub fn latest_event_id(&self) -> u64 {
        // SAFETY: `self.stream` is live until `Drop`.
        match unsafe { FSEventStreamGetLatestEventId(self.stream) } {
            K_FS_EVENT_STREAM_EVENT_ID_SINCE_NOW => self.created_at,
            id => id,
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the handles are live and dropped once; Stop is called only on a started stream,
        // and Invalidate is valid because the queue was set first. After Invalidate FSEvents queues
        // no new callback; the empty `dispatch_sync_f` waits out one already running or queued on
        // the serial queue (this thread is never that queue's, so it cannot deadlock). Only then
        // is the queue released and the sender freed, so no callback reads a freed `info`.
        unsafe {
            if self.started {
                FSEventStreamStop(self.stream);
            }
            FSEventStreamInvalidate(self.stream);
            FSEventStreamRelease(self.stream);
            dispatch_sync_f(self.queue, ptr::null_mut(), drained);
            dispatch_release(self.queue);
            drop(Box::from_raw(self.tx));
        }
    }
}

/// The empty work item `Drop` runs on the queue to know it has drained.
extern "C" fn drained(_: *mut c_void) {}

/// What one event's flags and path become, or `None` for bookkeeping events.
fn event_for(flags: u32, path: &[u8]) -> Option<Event> {
    if flags & K_FS_EVENT_STREAM_EVENT_FLAG_ROOT_CHANGED != 0 {
        Some(Event::RootChanged)
    } else if flags & K_FS_EVENT_STREAM_EVENT_FLAG_MUST_SCAN_SUB_DIRS != 0 {
        Some(Event::MustScan)
    } else if flags & K_FS_EVENT_STREAM_EVENT_FLAG_HISTORY_DONE != 0 || path.is_empty() {
        None
    } else {
        Some(Event::Changed(PathBuf::from(OsStr::from_bytes(path))))
    }
}

/// The stream's callback, on the queue's thread. It never panics: a send to a dropped receiver is
/// ignored, and null pointers deliver nothing.
extern "C" fn callback(
    _stream: FSEventStreamRef,
    info: *mut c_void,
    num_events: usize,
    event_paths: *mut c_void,
    event_flags: *const u32,
    _event_ids: *const u64,
) {
    if info.is_null() || event_paths.is_null() || event_flags.is_null() {
        return;
    }
    // SAFETY: `info` is the boxed sender `start` made, alive until `Drop` has drained the queue;
    // it is only read here.
    let tx = unsafe { &*(info as *const UnboundedSender<Event>) };
    let paths = event_paths as *const *const c_char;
    for i in 0..num_events {
        // SAFETY: FSEvents passes `num_events` flags and `num_events` C string paths (the stream
        // is made without `kFSEventStreamCreateFlagUseCFTypes`), valid for this call.
        let (flags, path) = unsafe { (*event_flags.add(i), *paths.add(i)) };
        let bytes = if path.is_null() {
            &[][..]
        } else {
            // SAFETY: a non-null path from FSEvents is NUL terminated and lives for the call.
            unsafe { CStr::from_ptr(path) }.to_bytes()
        };
        if let Some(e) = event_for(flags, bytes) {
            let _ = tx.unbounded_send(e);
        }
    }
}

/// Whether the volume holding `path` is local (`statfs`, `MNT_LOCAL`); a network volume takes a
/// stream and delivers nothing.
pub fn local_volume(path: &Path) -> bool {
    let Ok(c) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `statfs` is plain old data, so zeroed is a valid value to be overwritten.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is NUL terminated and `st` is a valid, writable `statfs`.
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return false;
    }
    (st.f_flags as u64) & (libc::MNT_LOCAL as u64) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::mpsc::{unbounded, UnboundedReceiver};
    use std::path::Path;
    use std::time::{Duration, Instant};

    /// Waits up to `deadline` for an event `want` accepts, polling the receiver without blocking.
    fn wait_for(rx: &mut UnboundedReceiver<Event>, deadline: Duration, want: impl Fn(&Event) -> bool) -> bool {
        let end = Instant::now() + deadline;
        while Instant::now() < end {
            match rx.try_recv() {
                Ok(e) if want(&e) => return true,
                Ok(_) => {}
                Err(e) if e.is_closed() => return false,
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        false
    }

    fn canonical_tempdir() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().canonicalize().unwrap();
        (t, d)
    }

    #[test]
    fn a_write_in_a_watched_folder_is_reported_within_seconds() {
        let (_t, d) = canonical_tempdir();
        std::fs::create_dir(d.join("sub")).unwrap();
        let (tx, mut rx) = unbounded();
        // A CI runner may refuse a stream; the reason is printed and the test passes there, since
        // what it checks is the Mac's own delivery, not the runner's sandbox.
        let s = match Stream::start(&[d.clone()], None, tx) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("FSEvents stream not available here: {e}");
                return;
            }
        };
        std::fs::write(d.join("sub/file.txt"), b"hello").unwrap();
        let seen = wait_for(&mut rx, Duration::from_secs(10), |e| match e {
            Event::Changed(p) => {
                let p = p.canonicalize().unwrap_or_else(|_| p.clone());
                p == d || p == d.join("sub")
            }
            _ => false,
        });
        assert!(seen, "no Changed event for {} within 10 seconds", d.display());
        drop(s);
    }

    #[test]
    fn the_latest_event_id_moves_after_an_event() {
        let (_t, d) = canonical_tempdir();
        let (tx, mut rx) = unbounded();
        // As above: a runner that refuses a stream prints the reason and passes.
        let s = match Stream::start(&[d.clone()], None, tx) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("FSEvents stream not available here: {e}");
                return;
            }
        };
        let before = s.latest_event_id();
        std::fs::write(d.join("file.txt"), b"hello").unwrap();
        assert!(wait_for(&mut rx, Duration::from_secs(10), |e| matches!(e, Event::Changed(_))));
        let after = s.latest_event_id();
        assert!(after > before, "latest event id {after} is not past {before}");
    }

    #[test]
    fn a_stream_resumed_from_a_saved_id_delivers_what_happened_in_between() {
        let (_t, d) = canonical_tempdir();
        let (tx, _rx) = unbounded();
        // As above: a runner that refuses a stream prints the reason and passes.
        let s = match Stream::start(&[d.clone()], None, tx) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("FSEvents stream not available here: {e}");
                return;
            }
        };
        let id = s.latest_event_id();
        drop(s);
        std::fs::write(d.join("file.txt"), b"hello").unwrap();
        let (tx, mut rx) = unbounded();
        let s = match Stream::start(&[d.clone()], Some(id), tx) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("FSEvents stream not available here: {e}");
                return;
            }
        };
        let seen = wait_for(&mut rx, Duration::from_secs(10), |e| match e {
            Event::Changed(p) => p.canonicalize().unwrap_or_else(|_| p.clone()) == d,
            _ => false,
        });
        assert!(seen, "the resumed stream did not report the write in {}", d.display());
        drop(s);
    }

    #[test]
    fn a_temporary_folder_is_on_a_local_volume_and_a_missing_path_is_not() {
        let (_t, d) = canonical_tempdir();
        assert!(local_volume(&d));
        assert!(!local_volume(Path::new("/nonexistent/arkos")));
    }
}
