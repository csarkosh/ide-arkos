//! The panic hook (telemetry spec, section 2.3): on a panic in a release build, a crash report in
//! `incidents/<inc>.json` under the telemetry folder, and then the hook that was there before.
//! The report says where the panic happened and in which build: the `pk` of a named failure point
//! (`Kind`) or else a hash of the message's first line with its paths and numbers taken out, the
//! remapped `file` and `line`, the faulting thread's frames as offsets into the Arkos binary, and
//! the uptime bucket. Nothing in it names the person, the machine or a folder.
//!
//! Everything that can be known before a panic is worked out by `install`: the envelope, the
//! commit, the binary's name, LC_UUID, base and size, and the incidents folder, made and opened
//! once and kept open. The hook runs on the panicking thread, which may hold any lock in the
//! process, so it takes none but the allocator's: the frames come from
//! `backtrace::trace_unsynchronized` unresolved (resolving a symbol, like the synchronized
//! `trace`, takes the backtrace crate's global lock), and it reads no config, no recorder and no
//! gpui state. It allocates only on that thread, and writes the report with one `write` to a file
//! it creates with `openat` on the kept folder, so no path is resolved after the panic either. A
//! `source` build's hook returns at once and writes nothing.
use crate::Kind;
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::File;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd};
use std::panic::PanicHookInfo;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::Instant;
use visits_app::{Crash, CrashKind, Envelope, Uptime};

/// At most this many frames go in a report: the wire's cap.
const MAX_FRAMES: usize = 64;
/// The largest `line` the wire takes.
const MAX_LINE: u32 = 10_000_000;
/// The `commit` of a build made outside a git checkout, whose `ARKOS_GIT_HASH` says `unknown`.
const NO_COMMIT: &str = "0000000";
/// What a path-like token of a panic message becomes before the message is hashed.
const PATH: &str = "<path>";
/// What a run of digits, or a `0x` hex number, becomes.
const NUMBER: &str = "<n>";
/// What a token holding an `@` (a mail address, `user@host`) becomes.
const ADDRESS: &str = "<addr>";
/// At most this many reports a run: a panic that a loop catches and meets again (the daemon's
/// recovery flusher runs every second or faster) must not fill the folder.
const MAX_REPORTS: u32 = 4;

/// What `install` worked out, for the hook to read without a lock.
struct Installed {
    env: Envelope,
    commit: String,
    /// The incidents folder, open: `None` for a source build, or when it could not be made.
    incidents: Option<File>,
    /// The Arkos binary this process runs, the report's only image.
    image: Option<Image>,
    start: Instant,
}

/// The binary a report's frames point into, and where it is loaded.
struct Image {
    name: String,
    uuid: String,
    base: usize,
    size: u64,
}

static INSTALLED: OnceLock<Installed> = OnceLock::new();
/// The panics this run has tried to report, counted before the report is made.
static WRITTEN: AtomicU32 = AtomicU32::new(0);

/// Installs the hook for this process, once. `env` is the envelope its reports carry (`main`
/// passes `envelope(..)`, a test a release one), `commit` the binary's `ARKOS_GIT_HASH`, and `dir`
/// its telemetry folder, whose `incidents/` is made and opened here. For a source build nothing
/// is made or opened, and the hook only passes the panic on. A second call changes nothing.
pub fn install(env: Envelope, commit: &str, dir: &Path) {
    let source = env.ch.is_source();
    let installed = Installed {
        commit: if is_commit(commit) { commit.to_string() } else { NO_COMMIT.to_string() },
        incidents: if source { None } else { open_incidents(&dir.join("incidents")) },
        image: if source { None } else { main_image(image_name(&env.bin)) },
        env,
        start: Instant::now(),
    };
    if INSTALLED.set(installed).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        record(info);
        previous(info);
    }));
}

/// Makes the incidents folder (only its owner may read it) and opens it for `openat`.
fn open_incidents(path: &Path) -> Option<File> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path).ok()?;
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY).open(path).ok()
}

/// The hook's own work: the report of this panic, written to the incidents folder.
fn record(info: &PanicHookInfo<'_>) {
    let Some(at) = INSTALLED.get() else { return };
    if at.env.ch.is_source() {
        return;
    }
    if WRITTEN.fetch_add(1, Ordering::Relaxed) >= MAX_REPORTS {
        return;
    }
    let Some(dir) = &at.incidents else { return };
    let (pk, mh) = match info.payload().downcast_ref::<Kind>() {
        Some(kind) => (kind.name().to_string(), None),
        None => ("other".to_string(), message(info).map(message_hash)),
    };
    let (file, line) = info.location().map_or((String::new(), 0), |l| (file_of(l.file()), l.line().min(MAX_LINE)));
    let crash = Crash {
        env: at.env.clone(),
        inc: visits_app::new_id(),
        ck: CrashKind::Panic,
        commit: at.commit.clone(),
        pk,
        mh,
        file,
        line,
        frames: frames(at.image.as_ref()),
        images: at.image.iter().map(|i| (i.name.clone(), i.uuid.clone(), i.size)).collect(),
        sig: None,
        up: uptime_of(at.start.elapsed().as_secs()),
        mem: None,
    };
    // A report the wire would refuse is not written; the previous hook still says what happened.
    let Ok(bytes) = crash.to_bytes() else { return };
    write_new(dir, &crash.inc, &bytes);
}

/// The panic's message when it has one: `panic!` with a literal or with arguments.
fn message<'a>(info: &'a PanicHookInfo<'_>) -> Option<&'a str> {
    let payload = info.payload();
    payload.downcast_ref::<&str>().copied().or_else(|| payload.downcast_ref::<String>().map(String::as_str))
}

/// The faulting thread's frames that fall in `image`, as offsets from its base, at most
/// `MAX_FRAMES` of them; a frame in a system library is left out.
fn frames(image: Option<&Image>) -> Vec<(u32, u64)> {
    let mut frames = Vec::with_capacity(MAX_FRAMES);
    let Some(image) = image else { return frames };
    // SAFETY: the unsynchronized walk is unsafe because it skips the backtrace crate's global
    // lock, which exists for Windows' dbghelp; on macOS it calls the system unwinder, which is
    // safe to run on several threads at once. Skipping that lock is the point: the panicking
    // thread may hold it. The callback only reads each frame's instruction pointer.
    unsafe {
        backtrace::trace_unsynchronized(|frame| {
            let offset = (frame.ip() as usize).wrapping_sub(image.base) as u64;
            if offset < image.size {
                frames.push((0, offset));
            }
            frames.len() < MAX_FRAMES
        });
    }
    frames
}

/// Writes `bytes` as `<inc>.json` in the kept folder, whole or not at all: one `write` to a new
/// `.<inc>.tmp`, then a rename onto the report's name, both relative to the kept descriptor. A
/// short write removes the temporary, so the dialog never meets half a report.
fn write_new(dir: &File, inc: &str, bytes: &[u8]) {
    let (Ok(tmp), Ok(name)) = (CString::new(format!(".{inc}.tmp")), CString::new(format!("{inc}.json"))) else { return };
    let dirfd = dir.as_raw_fd();
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: `dir` is an open directory descriptor that `INSTALLED` keeps for the life of the
    // process (a test's is open for the call), and `tmp` is a NUL-terminated string that outlives
    // the call. The mode is passed as the `unsigned int` the variadic argument is promoted to.
    let fd = unsafe { libc::openat(dirfd, tmp.as_ptr(), flags, 0o600 as libc::c_uint) };
    if fd < 0 {
        return;
    }
    // SAFETY: `fd` was just returned by `openat`, is open, and nothing else owns it, so the file
    // may close it when it is dropped.
    let mut file = unsafe { File::from_raw_fd(fd) };
    let whole = file.write(bytes).is_ok_and(|n| n == bytes.len());
    drop(file);
    if !whole {
        // SAFETY: both arguments are as for the `openat` above, and the name is the temporary
        // this call created.
        unsafe { libc::unlinkat(dirfd, tmp.as_ptr(), 0) };
        return;
    }
    // SAFETY: as for the `openat` above; both names are NUL-terminated strings that outlive the
    // call, and both are resolved relative to the kept descriptor alone.
    unsafe { libc::renameat(dirfd, tmp.as_ptr(), dirfd, name.as_ptr()) };
}

/// The hash a report's `mh` carries: SHA-256 of the message's first line once `stripped`, in
/// lowercase hex. A maintainer matches a known message by running it through this function.
pub fn message_hash(message: &str) -> String {
    hex(&Sha256::digest(stripped(message.lines().next().unwrap_or("")).as_bytes()))
}

/// `line` as its hash sees it: the first token holding a `/` or a `\\` and everything after it on
/// the line become one `<path>` (a path with spaces leaves no piece behind), a token holding an
/// `@` becomes `<addr>`, and every run of digits or `0x` hex number becomes `<n>`; the tokens are
/// joined by single spaces. An accepted limit: a word without a slash (a host name, a user name, a
/// bare file name) still reaches the hash input, though never the report.
fn stripped(line: &str) -> String {
    let mut tokens = Vec::new();
    for token in line.split_whitespace() {
        if token.contains(['/', '\\']) {
            tokens.push(PATH.to_string());
            break;
        }
        tokens.push(if token.contains('@') { ADDRESS.to_string() } else { numbers_out(token) });
    }
    tokens.join(" ")
}

/// `token` with each run of ASCII digits, and each `0x` followed by hex digits, replaced by `<n>`.
fn numbers_out(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    let mut out = String::with_capacity(token.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if !c.is_ascii_digit() {
            out.push(c);
            i += 1;
            continue;
        }
        out.push_str(NUMBER);
        let hex = c == '0' && matches!(chars.get(i + 1), Some('x' | 'X')) && chars.get(i + 2).is_some_and(char::is_ascii_hexdigit);
        if hex {
            i += 2;
            while chars.get(i).is_some_and(char::is_ascii_hexdigit) {
                i += 1;
            }
        } else {
            while chars.get(i).is_some_and(char::is_ascii_digit) {
                i += 1;
            }
        }
    }
    out
}

/// A panic location's file as a report carries it: whole when the release build remapped it
/// under `/src/`, `/cargo/` or `/rustc/` and it fits the wire's pattern, else only its basename,
/// cleaned to the pattern and cut to the cap. A dev build's path never leaves whole.
fn file_of(path: &str) -> String {
    let remapped = ["/src/", "/cargo/", "/rustc/"].iter().any(|root| path.starts_with(root));
    let fits = path.len() <= 256 && path.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-'));
    if remapped && fits && !path.split('/').any(|part| part == "..") {
        return path.to_string();
    }
    cleaned(path.rsplit('/').next().unwrap_or(""), 256)
}

/// `name` with every character outside `[A-Za-z0-9_.-]` made `_`, cut to `cap` characters.
fn cleaned(name: &str, cap: usize) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') { c } else { '_' }).take(cap).collect()
}

/// How long the process had run, in the spec's buckets.
fn uptime_of(secs: u64) -> Uptime {
    match secs {
        0..60 => Uptime::Lt1m,
        60..600 => Uptime::Lt10m,
        600..3_600 => Uptime::Lt1h,
        3_600..86_400 => Uptime::Lt1d,
        _ => Uptime::Ge1d,
    }
}

/// Whether `s` is a commit the wire takes: 7 to 40 lowercase hex characters.
fn is_commit(s: &str) -> bool {
    (7..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `bytes` in lowercase hex.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 0xf)] as char);
    }
    out
}

/// The main executable as a report's image: `name`, its LC_UUID and the span its segments
/// cover, with the address its header is loaded at as the base the frames count from. Only the
/// main executable: Arkos's binaries link Rust statically, so nothing else of Arkos is loaded,
/// and a system library is never listed.
#[cfg(target_os = "macos")]
fn main_image(name: &str) -> Option<Image> {
    /// The size of a `mach_header_64`; the load commands start right after it.
    const HEADER: usize = 32;
    const MH_MAGIC_64: u32 = 0xfeed_facf;
    const MH_EXECUTE: u32 = 0x2;
    // libSystem's image list (`<mach-o/dyld.h>`), declared here because libc's copies are
    // deprecated in favour of a crate this one does not otherwise need.
    extern "C" {
        fn _dyld_image_count() -> u32;
        fn _dyld_get_image_header(image_index: u32) -> *const u8;
    }
    // SAFETY: dyld's image count takes no argument and may be read from any thread.
    let count = unsafe { _dyld_image_count() };
    for index in 0..count {
        // SAFETY: `index` is below the count; dyld answers null for an image that went away since.
        let header = unsafe { _dyld_get_image_header(index) };
        if header.is_null() {
            continue;
        }
        // SAFETY: a non-null header points at the image's mach header, mapped for as long as the
        // image is loaded (the main executable is never unloaded), and a 64-bit header is
        // `HEADER` bytes long. They are read only, never written.
        let fixed = unsafe { std::slice::from_raw_parts(header, HEADER) };
        let word = |at: usize| u32::from_ne_bytes([fixed[at], fixed[at + 1], fixed[at + 2], fixed[at + 3]]);
        let (magic, filetype, ncmds, sizeofcmds) = (word(0), word(12), word(16), word(20));
        if magic != MH_MAGIC_64 || filetype != MH_EXECUTE {
            continue;
        }
        // SAFETY: the load commands follow the header in the same mapping, `sizeofcmds` bytes of
        // them, read only.
        let commands = unsafe { std::slice::from_raw_parts(header.add(HEADER), sizeofcmds as usize) };
        let (uuid, size) = uuid_and_size(commands, ncmds)?;
        return Some(Image { name: name.to_string(), uuid, base: header as usize, size });
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn main_image(_name: &str) -> Option<Image> {
    None
}

/// The image's name for the binary a report comes from (`Envelope::bin`): `arkosd` for the
/// daemon and `arkos` for the app. It names the binary, never the file it was loaded from, so a
/// renamed copy or a dev build's path says nothing.
fn image_name(bin: &str) -> &'static str {
    if bin == crate::Bin::Daemon.as_str() {
        "arkosd"
    } else {
        "arkos"
    }
}

/// The LC_UUID in lowercase hex and the span from `__TEXT`'s start to the end of the last
/// segment (`__PAGEZERO` left out), read from `ncmds` load commands. `None` when a command runs
/// past the bytes or either is missing.
fn uuid_and_size(commands: &[u8], ncmds: u32) -> Option<(String, u64)> {
    const LC_SEGMENT_64: u32 = 0x19;
    const LC_UUID: u32 = 0x1b;
    let u32_at = |b: &[u8], at: usize| b.get(at..at + 4).and_then(|s| <[u8; 4]>::try_from(s).ok()).map(u32::from_ne_bytes);
    let u64_at = |b: &[u8], at: usize| b.get(at..at + 8).and_then(|s| <[u8; 8]>::try_from(s).ok()).map(u64::from_ne_bytes);
    let (mut at, mut uuid, mut text, mut end) = (0usize, None, None, 0u64);
    for _ in 0..ncmds {
        let cmd = u32_at(commands, at)?;
        let len = u32_at(commands, at + 4)? as usize;
        if len < 8 {
            return None;
        }
        let body = commands.get(at..at.checked_add(len)?)?;
        match cmd {
            LC_UUID => uuid = Some(hex(body.get(8..24)?)),
            LC_SEGMENT_64 => {
                let name = body.get(8..24)?;
                let (vmaddr, vmsize) = (u64_at(body, 24)?, u64_at(body, 32)?);
                if !name.starts_with(b"__PAGEZERO\0") {
                    if name.starts_with(b"__TEXT\0") {
                        text = Some(vmaddr);
                    }
                    end = end.max(vmaddr.saturating_add(vmsize));
                }
            }
            _ => {}
        }
        at += len;
    }
    Some((uuid?, end.checked_sub(text?)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{envelope, Bin, Kind};
    use sha2::{Digest, Sha256};
    use std::process::{Command, Output, Stdio};
    use std::time::{Duration, Instant};
    use visits_app::{Channel, Crash, CrashKind};

    /// Set in a child run to `release`, `source` or `kind`: how the child installs and panics.
    const CHILD: &str = "ARKOS_PANIC_TEST_CHILD";
    /// The telemetry folder the child installs into.
    const DIR: &str = "ARKOS_PANIC_TEST_DIR";
    /// The commit the child passes, as `main` passes `ARKOS_GIT_HASH`.
    const COMMIT: &str = "0123abc";
    /// The child's message: a home folder's path and a number, neither of which may leave.
    const MESSAGE: &str = "could not read /Users/someone/project/file.rs at line 42";

    fn sha256_hex(text: &str) -> String {
        Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn is_hex(s: &str, len: usize) -> bool {
        s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    #[test]
    fn a_path_like_token_and_every_run_of_digits_are_replaced() {
        assert_eq!(stripped(MESSAGE), "could not read <path>", "the path and everything after it");
        assert_eq!(stripped("index out of bounds: the len is 3 but the index is 10"), "index out of bounds: the len is <n> but the index is <n>");
        assert_eq!(stripped("open 'src/main.rs:12:5' failed"), "open <path>", "a relative path is a path too");
        assert_eq!(stripped("'/Users/a b c/My Project/notes.md' is gone"), "<path>", "no piece of a path with spaces is left");
        assert_eq!(stripped("cannot open C:\\Users\\someone\\x.txt now"), "cannot open <path>", "a backslash starts a path too");
        assert_eq!(stripped("mail someone@example.com or <someone@host> today"), "mail <addr> or <addr> today");
        assert_eq!(stripped("version v1.22.3 and abc123def"), "version v<n>.<n>.<n> and abc<n>def");
        assert_eq!(stripped("bad address 0x7ffee4a1c0 and 0XFF, but 0xg and 10x5"), "bad address <n> and <n>, but <n>xg and <n>x<n>");
        assert_eq!(stripped("Os { code: 2, kind: NotFound }"), "Os { code: <n>, kind: NotFound }");
        assert_eq!(stripped("  spaced   out\tline "), "spaced out line", "whitespace is one space");
        assert_eq!(stripped("https://example.com/x?id=7"), "<path>");
        assert_eq!(stripped(""), "");
    }

    #[test]
    fn the_message_hash_is_of_the_first_line_stripped() {
        assert_eq!(message_hash(MESSAGE), sha256_hex("could not read <path>"));
        assert_eq!(message_hash("failed 3 times\nat /Users/someone/secret"), sha256_hex("failed <n> times"));
        assert_eq!(message_hash(""), sha256_hex(""));
        assert!(is_hex(&message_hash("anything"), 64));
    }

    #[test]
    fn a_file_is_kept_only_when_remapped_else_its_basename() {
        assert_eq!(file_of("/src/crates/arkos/src/md_view.rs"), "/src/crates/arkos/src/md_view.rs");
        assert_eq!(file_of("/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-0.2.0/src/app.rs"), "/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/gpui-0.2.0/src/app.rs");
        assert_eq!(file_of("/rustc/b940084d7/library/core/src/option.rs"), "/rustc/b940084d7/library/core/src/option.rs");
        assert_eq!(file_of("/Users/someone/ide-arkos/crates/arkos/src/main.rs"), "main.rs");
        assert_eq!(file_of("crates/arkos/src/main.rs"), "main.rs", "a dev build's relative path");
        assert_eq!(file_of("/Users/someone/.cargo/registry/src/x/foo-1.0/src/lib.rs"), "lib.rs");
        assert_eq!(file_of("/srcx/a.rs"), "a.rs", "the prefix is a whole folder");
        assert_eq!(file_of("/src/../Users/someone/a.rs"), "a.rs", "no climbing out of the remapped root");
        assert_eq!(file_of("/src/crates/my crate/a.rs"), "a.rs", "a kept path must fit the wire pattern");
        assert_eq!(file_of("/Users/someone/My Notes/é.rs"), "_.rs", "a basename is cleaned to the pattern");
        assert_eq!(file_of("main.rs"), "main.rs");
        assert_eq!(file_of(""), "");
        let long = format!("/src/{}.rs", "a".repeat(300));
        assert_eq!(file_of(&long).len(), 256, "a basename is cut to the cap");
    }

    #[test]
    fn the_uptime_falls_in_the_spec_buckets() {
        assert_eq!(uptime_of(0), Uptime::Lt1m);
        assert_eq!(uptime_of(59), Uptime::Lt1m);
        assert_eq!(uptime_of(60), Uptime::Lt10m);
        assert_eq!(uptime_of(599), Uptime::Lt10m);
        assert_eq!(uptime_of(600), Uptime::Lt1h);
        assert_eq!(uptime_of(3_599), Uptime::Lt1h);
        assert_eq!(uptime_of(3_600), Uptime::Lt1d);
        assert_eq!(uptime_of(86_399), Uptime::Lt1d);
        assert_eq!(uptime_of(86_400), Uptime::Ge1d);
    }

    /// A load command: its number, its size and the bytes after those two words.
    fn command(cmd: u32, body: &[u8]) -> Vec<u8> {
        let mut out = cmd.to_ne_bytes().to_vec();
        out.extend(u32::try_from(8 + body.len()).unwrap().to_ne_bytes());
        out.extend(body);
        out
    }

    fn segment(name: &str, vmaddr: u64, vmsize: u64) -> Vec<u8> {
        let mut body = [0u8; 16].to_vec();
        body[..name.len()].copy_from_slice(name.as_bytes());
        body.extend(vmaddr.to_ne_bytes());
        body.extend(vmsize.to_ne_bytes());
        body.extend([0u8; 32]);
        command(0x19, &body)
    }

    #[test]
    fn the_load_commands_give_the_uuid_and_the_span_past_the_page_zero() {
        let uuid: Vec<u8> = (0..16).map(|i| i * 17).collect();
        let commands = [
            segment("__PAGEZERO", 0, 0x1_0000_0000),
            segment("__TEXT", 0x1_0000_0000, 0x4000),
            command(0x1b, &uuid),
            command(0x32, &[0u8; 16]),
            segment("__DATA", 0x1_0000_4000, 0x1000),
            segment("__LINKEDIT", 0x1_0000_8000, 0x2000),
        ]
        .concat();
        assert_eq!(uuid_and_size(&commands, 6), Some(("00112233445566778899aabbccddeeff".into(), 0xa000)));
        assert_eq!(uuid_and_size(&commands, 2), None, "no LC_UUID among the first two");
        assert_eq!(uuid_and_size(&commands[..commands.len() - 4], 6), None, "a command past the end");
        assert_eq!(uuid_and_size(&command(0x1b, &uuid), 1), None, "no __TEXT");
        assert_eq!(uuid_and_size(&[0x1b, 0, 0, 0, 0, 0, 0, 0], 1), None, "a command shorter than its header");
    }

    #[test]
    fn an_image_is_named_by_its_binary_and_a_commit_is_hex() {
        assert_eq!(image_name("app"), "arkos");
        assert_eq!(image_name("daemon"), "arkosd");
        assert!(is_commit("0123abc") && is_commit(&"f".repeat(40)));
        for bad in ["unknown", "012345", &"f".repeat(41), "0123ABC", ""] {
            assert!(!is_commit(bad), "{bad}");
        }
    }

    /// Not a test of its own: the body a child run executes, installing the hook as `main` would
    /// (with the channel the parent asked for) and then panicking.
    #[test]
    #[ignore = "run by the subprocess tests below"]
    fn child() {
        let (Ok(how), Some(dir)) = (std::env::var(CHILD), std::env::var_os(DIR)) else { return };
        let ch = if how == "source" { Channel::Source } else { Channel::Release };
        install(Envelope { ch, ..envelope(Bin::App) }, COMMIT, Path::new(&dir));
        // Another thread holds the backtrace crate's own lock for the rest of the run, so a hook
        // that took it (the synchronized `backtrace::trace`, or any symbol resolution) would hang
        // here and the parent would see it time out.
        let (held, wait) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            backtrace::trace(|_| {
                let _ = held.send(());
                loop {
                    std::thread::park();
                }
            })
        });
        wait.recv().unwrap();
        if how == "kind" {
            std::panic::panic_any(Kind::Placeholder);
        }
        if how == "repeat" {
            // A panic a loop catches and meets again, as the daemon's recovery flusher would.
            for n in 0..10 {
                let _ = std::panic::catch_unwind(|| panic!("tick {n} failed"));
            }
        }
        panic!("could not read /Users/someone/project/file.rs at line {}", 42);
    }

    /// Runs `child` in a fresh copy of this test binary and returns the folder it installed
    /// into with what it printed. A child that does not end within a minute is killed: the hook
    /// hung.
    fn run_child(how: &str) -> (tempfile::TempDir, Output) {
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "panic::tests::child", "--ignored", "--nocapture", "--test-threads=1"])
            .env(CHILD, how)
            .env(DIR, dir.path())
            .env_remove("RUST_BACKTRACE")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("the {how} child hung in the panic hook");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let out = child.wait_with_output().unwrap();
        assert!(!out.status.success(), "the child panicked, so its test failed");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("panicked at"), "the previous hook ran after ours: {stderr}");
        (dir, out)
    }

    /// The one incident a child wrote: its file name's `inc` and its bytes.
    fn the_incident(dir: &Path) -> (String, Vec<u8>) {
        let incidents = dir.join("incidents");
        let files: Vec<_> = std::fs::read_dir(&incidents).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(files.len(), 1, "one incident: {files:?}");
        let name = files[0].file_name().unwrap().to_str().unwrap().to_string();
        let inc = name.strip_suffix(".json").expect("<inc>.json").to_string();
        (inc, std::fs::read(&files[0]).unwrap())
    }

    #[test]
    fn a_release_panic_writes_a_report_with_nothing_of_the_machine_in_it() {
        let (dir, _) = run_child("release");
        let (inc, bytes) = the_incident(dir.path());
        let text = String::from_utf8(bytes.clone()).unwrap();
        let crash = Crash::from_bytes(&bytes).unwrap_or_else(|e| panic!("the report validates: {e:?}\n{text}"));
        assert_eq!(crash.inc, inc);
        assert!(is_hex(&inc, 32), "{inc}");
        assert_eq!((crash.env.ch, crash.env.bin.as_str(), crash.env.app.as_str()), (Channel::Release, "app", "arkos"));
        assert_eq!(crash.ck, CrashKind::Panic);
        assert_eq!(crash.commit, COMMIT);
        assert_eq!(crash.pk, "other");
        assert_eq!(crash.mh.as_deref(), Some(sha256_hex("could not read <path>").as_str()));
        assert!(crash.file == "panic.rs" || crash.file.starts_with("/src/"), "remapped or a basename: {}", crash.file);
        assert!(crash.line > 0);
        assert_eq!(crash.up, Uptime::Lt1m);
        assert_eq!((crash.sig, crash.mem), (None, None));
        // The images are this binary alone, by the name of the binary it installed as (never the
        // file's name or path), its LC_UUID and its size.
        assert_eq!(crash.images.len(), 1, "{:?}", crash.images);
        let (name, uuid, size) = &crash.images[0];
        assert_eq!(name, "arkos");
        assert!(is_hex(uuid, 32), "{uuid}");
        assert_ne!(uuid, &"0".repeat(32));
        assert!(*size > 0);
        // The frames are offsets into it, and the panic's own path through the code is among them.
        assert!(crash.frames.len() >= 3, "{:?}", crash.frames);
        assert!(crash.frames.len() <= 64);
        for &(image, offset) in &crash.frames {
            assert_eq!(image, 0);
            assert!(offset < *size, "{offset} is inside the image");
        }
        // Nothing of the machine: no home folder, no user name, no piece of the message's path.
        let home = std::env::var("HOME").unwrap();
        for leak in [home.as_str(), "/Users", "someone", "project", "file.rs"] {
            assert!(!text.contains(leak), "{leak} in {text}");
        }
    }

    #[test]
    fn a_repeating_panic_writes_at_most_four_reports_a_run() {
        let (dir, _) = run_child("repeat");
        let names: Vec<String> = std::fs::read_dir(dir.path().join("incidents")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names.len(), 4, "ten caught panics and the last one: {names:?}");
        for name in &names {
            assert!(name.ends_with(".json") && is_hex(name.trim_end_matches(".json"), 32), "whole reports only: {name}");
            assert!(Crash::from_bytes(&std::fs::read(dir.path().join("incidents").join(name)).unwrap()).is_ok());
        }
    }

    #[test]
    fn a_report_is_written_whole_under_its_name_and_no_temporary_is_left() {
        let dir = tempfile::tempdir().unwrap();
        let kept = File::open(dir.path()).unwrap();
        write_new(&kept, "0123456789abcdef0123456789abcdef", b"{\"a\":1}");
        let names: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names, ["0123456789abcdef0123456789abcdef.json"]);
        assert_eq!(std::fs::read(dir.path().join(&names[0])).unwrap(), b"{\"a\":1}");
        write_new(&kept, "fedcba9876543210fedcba9876543210", b"{}");
        let mut names: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(names, ["0123456789abcdef0123456789abcdef.json", "fedcba9876543210fedcba9876543210.json"]);
    }

    #[test]
    fn a_named_failure_point_carries_its_name_and_no_hash() {
        let (dir, _) = run_child("kind");
        let (_, bytes) = the_incident(dir.path());
        let crash = Crash::from_bytes(&bytes).unwrap();
        assert_eq!(crash.pk, "placeholder");
        assert_eq!(crash.mh, None);
    }

    #[test]
    fn a_source_build_writes_nothing() {
        let (dir, _) = run_child("source");
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert!(left.is_empty(), "not even the incidents folder: {left:?}");
    }
}
