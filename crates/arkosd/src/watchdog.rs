//! Relaunches a UI whose main thread has stopped heartbeating (spec 2.4). Sessions are untouched.
//!
//! A silent window is not always a hung one. While the Mac sleeps, and through the dark wakes of
//! Power Nap, macOS runs the daemon but schedules no window, so every heartbeat stops at once; a
//! window relaunched then is frozen the same way. So a scan after a sleep, or after the person
//! comes back, starts every window's clock again, a window is never killed while the screen is
//! locked or the display asleep, and a window that keeps going silent is relaunched after a
//! growing wait and then left alone.
use crate::server::{ClientRecord, Shared, State};
use std::collections::{BTreeSet, HashMap};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

/// How much further the wall clock may move than the monotonic one between two scans before the
/// system is taken to have slept (the monotonic clock stops while it sleeps).
const SLEEP_SLACK: Duration = Duration::from_secs(5);

/// A window's silent relaunches are forgotten at a scan that finds it heartbeating (a heartbeat
/// heard, not one a sleep gave it) this long after the last of them. A record with no window of
/// its identity connected is dropped once its last relaunch is this old.
const HEALTHY_RESET: Duration = Duration::from_secs(10 * 60);

/// The wait before each silent relaunch in a row, counted from the one before: none for the
/// first, two minutes, then eight. After the last a window that goes silent again is left as it
/// is.
const WAITS: [Duration; 3] = [Duration::ZERO, Duration::from_secs(2 * 60), Duration::from_secs(8 * 60)];

/// A window as the watchdog remembers it across reconnects: the binary it relaunches and the
/// folder it relaunches it in. Two windows may share one (two launch choosers, with no folder),
/// and then share its count too.
pub type WindowId = (Option<PathBuf>, Option<PathBuf>);

/// How often a window was relaunched for silence in a row, and when last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relaunches {
    pub count: u32,
    pub last: Instant,
    /// The pids of the windows of this identity the watchdog gave up on. Once none of them is
    /// connected, a window of this identity with another pid is one the person opened again,
    /// which starts from nothing.
    pub stopped: BTreeSet<u32>,
}

/// Both clocks, read together: the monotonic one for heartbeats and waits, the wall clock only
/// to tell a sleep from a quiet stretch.
#[derive(Clone, Copy, Debug)]
pub struct Clocks {
    pub mono: Instant,
    pub wall: SystemTime,
}

impl Clocks {
    pub fn now() -> Clocks {
        Clocks { mono: Instant::now(), wall: SystemTime::now() }
    }
}

/// What the watchdog thread carries from one scan to the next.
#[derive(Default)]
pub struct Watch {
    /// The clocks at the previous scan; None before the first.
    prev: Option<Clocks>,
    /// When a sleep or a return last restarted every heartbeat clock: a heartbeat no newer than
    /// that was given, not heard, and never counts as a healthy window.
    refreshed: Option<Instant>,
    /// The last answer to whether the person is away, kept while nobody is asked.
    away: bool,
    /// Whether the daemon log already says silent windows are left while the person is away;
    /// once per absence.
    told_away: bool,
}

/// What one scan decides for one silent or healthy window.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// It heartbeats.
    Healthy,
    /// The watchdog already gave up on it.
    Stopped,
    /// Nobody can be at the Mac, so its silence says nothing.
    Away,
    /// It was relaunched for silence a moment ago and waits for the next.
    Backoff,
    /// Silent again after three relaunches: give up on it.
    Stop,
    /// Kill it and start another.
    Relaunch,
}

/// What a scan leaves to be done outside the lock.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    Relaunch { pid: u32, exe: Option<PathBuf>, cwd: Option<PathBuf> },
    /// The line saying the watchdog gave up on a window, for its log and the update journal.
    Stop(String),
    /// A line for the daemon log alone.
    Log(String),
}

/// Starts the watchdog thread. `presence` answers whether nobody can be at the Mac:
/// `person_away` for a real daemon, a fixed answer for `--presence` (the tests).
pub fn start(state: Shared, timeout: Duration, presence: fn() -> bool) {
    if timeout.is_zero() {
        return;
    }
    let interval = std::cmp::max(timeout / 6, Duration::from_millis(500));
    std::thread::Builder::new()
        .name("watchdog".into())
        .spawn(move || {
            let mut watch = Watch::default();
            loop {
                std::thread::sleep(interval);
                scan(&state, timeout, interval, &mut watch, presence);
            }
        })
        .expect("spawn watchdog");
}

/// The answer `--presence` fixes: `away` or `present`. None for anything else.
pub fn presence_flag(v: &str) -> Option<fn() -> bool> {
    match v {
        "away" => Some(|| true),
        "present" => Some(|| false),
        _ => None,
    }
}

fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Whether the time between two scans passed without the daemon: the wall clock moved further
/// than the monotonic one (the system slept, which stops the monotonic clock), or the monotonic
/// gap is far longer than the scan's interval (the daemon itself was not scheduled). A wall clock
/// set back, or moved less, is not a sleep.
fn slept(prev: Clocks, now: Clocks, interval: Duration) -> bool {
    let mono = now.mono.saturating_duration_since(prev.mono);
    if mono > (interval * 3).max(interval + SLEEP_SLACK) {
        return true;
    }
    now.wall.duration_since(prev.wall).is_ok_and(|wall| wall > mono + SLEEP_SLACK)
}

/// The wait before the next silent relaunch of a window relaunched `count` times in a row, or
/// None once it is to be left alone.
fn wait_before(count: u32) -> Option<Duration> {
    WAITS.get(count as usize).copied()
}

/// Whether a window heartbeating now does so long enough after its last silent relaunch for that
/// run of relaunches to be forgotten.
fn healthy_long_enough(history: &Relaunches, now: Instant) -> bool {
    now.saturating_duration_since(history.last) >= HEALTHY_RESET
}

/// The verdict on one connected window, `pid`, given whether it is silent, whether the person is
/// away, and the silent relaunches of its identity so far.
fn decide(silent: bool, away: bool, history: Option<&Relaunches>, pid: u32, now: Instant) -> Verdict {
    if !silent {
        return Verdict::Healthy;
    }
    if history.is_some_and(|h| h.stopped.contains(&pid)) {
        return Verdict::Stopped;
    }
    if away {
        return Verdict::Away;
    }
    let Some(h) = history else { return Verdict::Relaunch };
    match wait_before(h.count) {
        None => Verdict::Stop,
        Some(wait) if now.saturating_duration_since(h.last) < wait => Verdict::Backoff,
        Some(_) => Verdict::Relaunch,
    }
}

/// The one line written when the watchdog gives up on a window, saying what to do.
fn stop_line(exe: Option<&Path>, cwd: Option<&Path>, pid: u32) -> String {
    let exe = exe.map_or("a window".into(), |e| e.display().to_string());
    let folder = cwd.map_or("the launch chooser".into(), |c| c.display().to_string());
    format!("watchdog: {exe} in {folder} (pid {pid}) went silent again after three relaunches in a row, so it is left as it is and not relaunched again; quit it and open Arkos again")
}

/// The pid of a window the watchdog may judge: a connected UI client whose pid is safe to signal.
fn watched_pid(rec: &ClientRecord) -> Option<u32> {
    let pid = rec.ui_pid?;
    // Signalling pid 0 would hit the daemon's whole process group, and its own pid itself;
    // a pid above i32::MAX is negative as a pid_t and would signal a group or everything.
    // Hello already refuses these; this keeps the watchdog safe on its own.
    if pid < 2 || pid > i32::MAX as u32 || pid == std::process::id() {
        return None;
    }
    // A disconnected client's pid may have been reused by an unrelated process.
    rec.connected.then_some(pid)
}

fn is_silent(rec: &ClientRecord, now: Instant, timeout: Duration) -> bool {
    now.saturating_duration_since(rec.last_heartbeat) >= timeout
}

/// Drops the relaunch records nothing needs any more: one with no window of its identity
/// connected whose last relaunch is `HEALTHY_RESET` old, and one whose given-up windows have all
/// gone while another window of its identity came (the person opened Arkos again).
fn prune(st: &mut State, now: Instant) {
    let mut connected: HashMap<WindowId, Vec<u32>> = HashMap::new();
    for rec in st.clients.values() {
        if let Some(pid) = watched_pid(rec) {
            connected.entry((rec.ui_exe.clone(), rec.ui_cwd.clone())).or_default().push(pid);
        }
    }
    st.relaunches.retain(|id, h| match connected.get(id) {
        None => now.saturating_duration_since(h.last) < HEALTHY_RESET,
        Some(pids) => h.stopped.is_empty() || pids.iter().any(|p| h.stopped.contains(p)),
    });
}

/// Decides one scan under the lock: restarts every clock after a sleep or when the person comes
/// back, forgets the relaunches nothing needs, and returns what is to be killed, launched and
/// logged. `away` says whether nobody can be at the Mac, or is None when nobody was asked (no
/// window was silent when `scan` looked); a window silent by now is then judged next scan.
fn plan(st: &mut State, timeout: Duration, interval: Duration, now: Clocks, watch: &mut Watch, away: Option<bool>) -> Vec<Action> {
    let prev = watch.prev.replace(now);
    prune(st, now.mono);
    let back = watch.away && away == Some(false);
    if let Some(a) = away {
        watch.away = a;
        if !a {
            watch.told_away = false;
        }
    }
    if back || prev.is_some_and(|p| slept(p, now, interval)) {
        if back {
            eprintln!("arkosd: the screen is unlocked and the display awake again; every window's heartbeat clock starts again");
        } else {
            eprintln!("arkosd: the system slept or the daemon was not scheduled; every window's heartbeat clock starts again");
        }
        for rec in st.clients.values_mut() {
            rec.last_heartbeat = now.mono;
        }
        watch.refreshed = Some(now.mono);
        return Vec::new();
    }
    let mut actions = Vec::new();
    let State { clients, relaunches, .. } = st;
    for rec in clients.values_mut() {
        let Some(pid) = watched_pid(rec) else { continue };
        let id: WindowId = (rec.ui_exe.clone(), rec.ui_cwd.clone());
        let silent = is_silent(rec, now.mono, timeout);
        match decide(silent, away.unwrap_or(true), relaunches.get(&id), pid, now.mono) {
            Verdict::Healthy => {
                let heard = watch.refreshed.is_none_or(|r| rec.last_heartbeat > r);
                if heard && relaunches.get(&id).is_some_and(|h| healthy_long_enough(h, now.mono)) {
                    relaunches.remove(&id);
                }
            }
            // Its clock is left as it is, so it is judged again once things change.
            Verdict::Away => {
                if away == Some(true) && !watch.told_away {
                    watch.told_away = true;
                    actions.push(Action::Log(format!("UI pid {pid} is silent, but the screen is locked or the display asleep, so no window is relaunched until someone is back")));
                }
            }
            Verdict::Stopped | Verdict::Backoff => {}
            Verdict::Stop => {
                if !pid_alive(pid) {
                    continue;
                }
                if let Some(h) = relaunches.get_mut(&id) {
                    h.stopped.insert(pid);
                }
                actions.push(Action::Stop(stop_line(rec.ui_exe.as_deref(), rec.ui_cwd.as_deref(), pid)));
            }
            Verdict::Relaunch => {
                if !pid_alive(pid) {
                    continue;
                }
                eprintln!("arkosd: UI pid {pid} silent for {:?}, relaunching", timeout);
                let h = relaunches.entry(id).or_insert(Relaunches { count: 0, last: now.mono, stopped: BTreeSet::new() });
                h.count += 1;
                h.last = now.mono;
                actions.push(Action::Relaunch { pid, exe: rec.ui_exe.clone(), cwd: rec.ui_cwd.clone() });
                // Reset so one stuck UI is not killed again on the next scan while it restarts.
                rec.last_heartbeat = now.mono;
            }
        }
    }
    actions
}

fn scan(state: &Shared, timeout: Duration, interval: Duration, watch: &mut Watch, presence: fn() -> bool) {
    // Whether anyone is at the Mac is asked of the window server only when a window is silent,
    // and outside the lock.
    let looked = Instant::now();
    let any_silent = state.lock().unwrap().clients.values().any(|rec| watched_pid(rec).is_some() && is_silent(rec, looked, timeout));
    let away = any_silent.then(presence);
    // Both clocks are read after the question, so a window server slow to answer through a sleep
    // is caught by the sleep check on this same scan.
    let now = Clocks::now();
    let actions = plan(&mut state.lock().unwrap(), timeout, interval, now, watch, away);
    // The lock is released: killing and spawning never block other threads.
    for action in actions {
        match action {
            Action::Log(line) => eprintln!("arkosd: {line}"),
            Action::Stop(line) => {
                eprintln!("arkosd: {line}");
                crate::journal::note(line);
            }
            Action::Relaunch { pid, exe, cwd } => {
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
                if let Some(exe) = exe {
                    if let Err(e) = launch_ui(&exe, cwd.as_deref(), "the watchdog, for a silent window") {
                        eprintln!("arkosd: relaunch {} failed: {e}", exe.display());
                    }
                }
            }
        }
    }
}

/// What the window server said about the screen lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lock {
    Locked,
    Unlocked,
    /// No session to ask, or an answer that could not be read.
    Unknown,
}

/// Away when the display sleeps, the screen is locked, or the lock could not be learned: without
/// an answer the watchdog would rather leave a hung window than kill a sleeping one.
fn away_from(display_asleep: bool, lock: Lock) -> bool {
    display_asleep || lock != Lock::Unlocked
}

/// Whether nobody can be at the Mac: the main display asleep or the screen locked, or no session
/// to ask (see `away_from`). A window is not drawn then, so its silence says nothing about
/// whether it hangs.
#[cfg(target_os = "macos")]
pub fn person_away() -> bool {
    use std::ffi::{c_char, c_void};
    type CFTypeRef = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const c_char, encoding: u32) -> CFTypeRef;
        fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(b: CFTypeRef) -> u8;
        fn CFRelease(cf: CFTypeRef);
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSessionCopyCurrentDictionary() -> CFTypeRef;
        fn CGMainDisplayID() -> u32;
        fn CGDisplayIsAsleep(display: u32) -> u32;
    }
    const UTF8: u32 = 0x0800_0100;
    // SAFETY: each Create or Copy is released once; the dictionary's value is borrowed from it
    // and read before the dictionary is released.
    unsafe {
        let display_asleep = CGDisplayIsAsleep(CGMainDisplayID()) != 0;
        let session = CGSessionCopyCurrentDictionary();
        if session.is_null() {
            return away_from(display_asleep, Lock::Unknown);
        }
        let key = CFStringCreateWithCString(std::ptr::null(), c"CGSSessionScreenIsLocked".as_ptr(), UTF8);
        let lock = if key.is_null() {
            Lock::Unknown
        } else {
            let v = CFDictionaryGetValue(session, key);
            // The key is there only while the screen is locked.
            let lock = if v.is_null() {
                Lock::Unlocked
            } else if CFGetTypeID(v) != CFBooleanGetTypeID() {
                Lock::Unknown
            } else if CFBooleanGetValue(v) != 0 {
                Lock::Locked
            } else {
                Lock::Unlocked
            };
            CFRelease(key);
            lock
        };
        CFRelease(session);
        away_from(display_asleep, lock)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn person_away() -> bool {
    false
}

/// Starts a window in its own session, with stdin null and its output appended to `window.log`,
/// in `cwd` (the window picks its project from its working directory), and reaps it when it
/// exits. Without a folder (a window that said Hello from the launch chooser) it starts at `/`,
/// which shows the chooser again, rather than in whatever folder the daemon was started from.
/// Used for the watchdog's relaunch, `RelaunchClient`, a rollback and a recovery. `why` is for
/// the update journal, which records every launch with its pid.
pub fn launch_ui(exe: &Path, cwd: Option<&Path>, why: &str) -> std::io::Result<u32> {
    let folder = cwd.unwrap_or(Path::new("/"));
    let launched = spawn_ui(exe, folder);
    match &launched {
        Ok(pid) => crate::journal::note(format_args!("relaunch spawned ({why}): {} in {}, pid {pid}", exe.display(), folder.display())),
        Err(e) => crate::journal::note(format_args!("relaunch failed ({why}): {} in {}: {e}", exe.display(), folder.display())),
    }
    launched
}

fn spawn_ui(exe: &Path, cwd: &Path) -> std::io::Result<u32> {
    let mut cmd = Command::new(exe);
    cmd.stdin(Stdio::null());
    // Both outputs to window.log, so a window that dies at start (or waits in the chooser) says
    // why; with no log, nowhere.
    match crate::journal::open_window_log().and_then(|f| Some((f.try_clone().ok()?, f))) {
        Some((out, err)) => cmd.stdout(out).stderr(err),
        None => cmd.stdout(Stdio::null()).stderr(Stdio::null()),
    };
    // The window opens without taking focus: the user may be in another app.
    cmd.env("ARKOS_RELAUNCH", "1");
    cmd.current_dir(cwd);
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    // No fork while a session's PTY descriptors are still inheritable, or the relaunched UI
    // would hold that terminal open for its whole life.
    let mut child = {
        let _no_open_pty_inheritable = crate::pty::spawn_lock();
        cmd.spawn()?
    };
    let pid = child.id();
    // Reap the child when it exits so it does not linger as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::ClientSink;
    use std::os::unix::net::UnixStream;

    const TIMEOUT: Duration = Duration::from_secs(30);
    const INTERVAL: Duration = Duration::from_secs(5);
    const MIN: Duration = Duration::from_secs(60);
    const PID: u32 = 4242;

    fn at(mono: Instant, wall: SystemTime) -> Clocks {
        Clocks { mono, wall }
    }

    #[test]
    fn a_scan_on_time_is_not_a_sleep() {
        let (m, w) = (Instant::now(), SystemTime::now());
        let next = at(m + INTERVAL, w + INTERVAL);
        assert!(!slept(at(m, w), next, INTERVAL));
        // The wall clock a little ahead (an NTP step, the scan's own jitter) is not a sleep.
        assert!(!slept(at(m, w), at(m + INTERVAL, w + INTERVAL + Duration::from_secs(2)), INTERVAL));
    }

    #[test]
    fn a_wall_clock_far_ahead_of_the_monotonic_one_is_a_sleep() {
        let (m, w) = (Instant::now(), SystemTime::now());
        // The monotonic clock stops while the Mac sleeps; the wall clock went on for an hour.
        assert!(slept(at(m, w), at(m + INTERVAL, w + INTERVAL + Duration::from_secs(3600)), INTERVAL));
        assert!(slept(at(m, w), at(m + INTERVAL, w + INTERVAL + Duration::from_secs(6)), INTERVAL));
    }

    #[test]
    fn a_daemon_not_scheduled_for_many_intervals_counts_as_a_sleep() {
        let (m, w) = (Instant::now(), SystemTime::now());
        let gap = Duration::from_secs(120);
        assert!(slept(at(m, w), at(m + gap, w + gap), INTERVAL));
        // A scan one interval late is only late.
        assert!(!slept(at(m, w), at(m + INTERVAL * 2, w + INTERVAL * 2), INTERVAL));
    }

    #[test]
    fn a_wall_clock_set_back_is_not_a_sleep() {
        let (m, w) = (Instant::now(), SystemTime::now());
        assert!(!slept(at(m, w), at(m + INTERVAL, w - Duration::from_secs(3600)), INTERVAL));
        assert!(!slept(at(m, w), at(m + INTERVAL, w), INTERVAL));
    }

    fn history(count: u32, last: Instant) -> Relaunches {
        Relaunches { count, last, stopped: BTreeSet::new() }
    }

    #[test]
    fn a_heartbeating_window_is_healthy_whatever_else_holds() {
        let now = Instant::now();
        assert_eq!(decide(false, false, None, PID, now), Verdict::Healthy);
        assert_eq!(decide(false, true, Some(&history(3, now)), PID, now), Verdict::Healthy);
    }

    #[test]
    fn a_silent_window_with_the_person_there_is_relaunched() {
        let now = Instant::now();
        assert_eq!(decide(true, false, None, PID, now), Verdict::Relaunch);
    }

    #[test]
    fn a_silent_window_with_the_person_away_is_left() {
        let now = Instant::now();
        assert_eq!(decide(true, true, None, PID, now), Verdict::Away);
        assert_eq!(decide(true, true, Some(&history(3, now)), PID, now), Verdict::Away, "no stop while away either");
    }

    #[test]
    fn a_window_silent_again_soon_after_a_relaunch_waits() {
        let last = Instant::now();
        assert_eq!(decide(true, false, Some(&history(1, last)), PID, last + MIN), Verdict::Backoff);
        assert_eq!(decide(true, false, Some(&history(1, last)), PID, last + 2 * MIN), Verdict::Relaunch);
        assert_eq!(decide(true, false, Some(&history(2, last)), PID, last + 7 * MIN), Verdict::Backoff);
        assert_eq!(decide(true, false, Some(&history(2, last)), PID, last + 8 * MIN), Verdict::Relaunch);
    }

    #[test]
    fn a_window_silent_after_three_relaunches_is_given_up_on_once() {
        let last = Instant::now();
        assert_eq!(decide(true, false, Some(&history(3, last)), PID, last + MIN), Verdict::Stop);
        assert_eq!(decide(true, false, Some(&history(3, last)), PID, last + 60 * MIN), Verdict::Stop);
        let stopped = Relaunches { stopped: BTreeSet::from([PID]), ..history(3, last) };
        assert_eq!(decide(true, false, Some(&stopped), PID, last + MIN), Verdict::Stopped);
        // Another silent window of the same identity is given up on too, not relaunched.
        assert_eq!(decide(true, false, Some(&stopped), PID + 1, last + MIN), Verdict::Stop);
    }

    #[test]
    fn the_waits_grow_and_then_stop() {
        assert_eq!(wait_before(0), Some(Duration::ZERO));
        assert_eq!(wait_before(1), Some(2 * MIN));
        assert_eq!(wait_before(2), Some(8 * MIN));
        assert_eq!(wait_before(3), None);
        assert_eq!(wait_before(7), None);
    }

    #[test]
    fn ten_healthy_minutes_after_a_relaunch_forget_it() {
        let last = Instant::now();
        assert!(!healthy_long_enough(&history(2, last), last + 9 * MIN));
        assert!(healthy_long_enough(&history(2, last), last + 10 * MIN));
    }

    #[test]
    fn the_stop_line_names_the_window_and_says_what_to_do() {
        let line = stop_line(Some(Path::new("/Applications/Arkos.app/Contents/MacOS/arkos")), Some(Path::new("/Users/me/project")), 4242);
        assert!(line.contains("/Users/me/project"), "{line}");
        assert!(line.contains("4242"), "{line}");
        assert!(line.contains("three"), "{line}");
        assert!(line.contains("open Arkos again"), "{line}");
        assert!(!line.contains('\n') && !line.contains('\u{2014}'), "{line}");
        // A window from the launch chooser has no folder.
        assert!(stop_line(None, None, 7).contains("open Arkos again"));
    }

    #[test]
    fn an_unreadable_lock_or_no_session_is_away() {
        assert!(!away_from(false, Lock::Unlocked));
        assert!(away_from(true, Lock::Unlocked), "the display asleep");
        assert!(away_from(false, Lock::Locked), "the screen locked");
        assert!(away_from(false, Lock::Unknown), "no session to ask, or no answer");
    }

    #[test]
    fn the_presence_flag_fixes_the_answer() {
        assert!(presence_flag("away").expect("away is a value")());
        assert!(!presence_flag("present").expect("present is a value")());
        assert!(presence_flag("maybe").is_none());
        assert!(presence_flag("").is_none());
    }

    /// A connected window whose pid is `pid` (alive and never signalled: only `scan` kills) and
    /// whose last heartbeat was `silent_for` ago.
    fn add_window_as(st: &mut State, client: u64, pid: u32, cwd: Option<&str>, silent_for: Duration, now: Instant) {
        let (a, _b) = UnixStream::pair().unwrap();
        let rec = ClientRecord {
            tx: ClientSink::new(client, a, 1 << 20),
            attached: Vec::new(),
            last_heartbeat: now - silent_for,
            ui_pid: Some(pid),
            ui_exe: Some(PathBuf::from("/nowhere/arkos")),
            ui_cwd: cwd.map(PathBuf::from),
            connected: true,
            protocol: 0,
            relaunch: None,
            sid: None,
            end: None,
        };
        st.clients.insert(client, rec);
    }

    /// The test's parent as the window: alive, and not ours to signal, which the plan never does.
    fn add_window(st: &mut State, client: u64, silent_for: Duration, now: Instant) {
        add_window_as(st, client, std::os::unix::process::parent_id(), Some("/nowhere/project"), silent_for, now);
    }

    /// Makes `now` the scan right after the previous one, however far the test moved the clocks.
    fn on_time(watch: &mut Watch, now: Clocks) {
        watch.prev = Some(at(now.mono - INTERVAL, now.wall - INTERVAL));
    }

    fn window() -> WindowId {
        (Some(PathBuf::from("/nowhere/arkos")), Some(PathBuf::from("/nowhere/project")))
    }

    fn relaunches(actions: &[Action]) -> usize {
        actions.iter().filter(|a| matches!(a, Action::Relaunch { .. })).count()
    }

    #[test]
    fn a_scan_after_a_sleep_restarts_every_clock_and_relaunches_nothing() {
        let (m, w) = (Instant::now() + 10 * MIN, SystemTime::now());
        let woke = at(m + INTERVAL, w + Duration::from_secs(3600));
        // The window looks silent for five minutes, but the Mac slept: nothing is killed.
        let mut silent = State::default();
        add_window(&mut silent, 1, 5 * MIN, woke.mono);
        let mut watch = Watch { prev: Some(at(m, w)), ..Watch::default() };
        assert!(plan(&mut silent, TIMEOUT, INTERVAL, woke, &mut watch, Some(false)).is_empty());
        assert_eq!(silent.clients[&1].last_heartbeat, woke.mono);
        assert!(silent.relaunches.is_empty());
    }

    #[test]
    fn the_person_coming_back_restarts_every_clock_and_relaunches_nothing() {
        let mut st = State::default();
        let (m, w) = (Instant::now() + 10 * MIN, SystemTime::now());
        add_window(&mut st, 1, 5 * MIN, m);
        let mut watch = Watch::default();
        on_time(&mut watch, at(m, w));
        assert!(relaunches(&plan(&mut st, TIMEOUT, INTERVAL, at(m, w), &mut watch, Some(true))) == 0, "left while away");
        // The display wakes with no lock: the window, silent for minutes, has had no chance to
        // heartbeat yet.
        let back = at(m + INTERVAL, w + INTERVAL);
        assert!(plan(&mut st, TIMEOUT, INTERVAL, back, &mut watch, Some(false)).is_empty());
        assert_eq!(st.clients[&1].last_heartbeat, back.mono);
        // Still silent a whole timeout later, with the person there: now it is relaunched.
        let later = at(back.mono + TIMEOUT, back.wall + TIMEOUT);
        on_time(&mut watch, later);
        assert_eq!(relaunches(&plan(&mut st, TIMEOUT, INTERVAL, later, &mut watch, Some(false))), 1);
    }

    #[test]
    fn being_away_is_logged_once_per_absence() {
        let mut st = State::default();
        let (m, w) = (Instant::now() + 10 * MIN, SystemTime::now());
        add_window(&mut st, 1, 5 * MIN, m);
        let mut watch = Watch::default();
        let logs = |a: &[Action]| a.iter().filter(|a| matches!(a, Action::Log(_))).count();
        assert_eq!(logs(&plan(&mut st, TIMEOUT, INTERVAL, at(m, w), &mut watch, Some(true))), 1);
        let next = at(m + INTERVAL, w + INTERVAL);
        assert_eq!(logs(&plan(&mut st, TIMEOUT, INTERVAL, next, &mut watch, Some(true))), 0, "not every scan");
    }

    #[test]
    fn a_window_silent_when_nobody_was_asked_waits_for_the_next_scan() {
        let mut st = State::default();
        let (m, w) = (Instant::now() + 10 * MIN, SystemTime::now());
        add_window(&mut st, 1, TIMEOUT, m);
        let mut watch = Watch::default();
        assert!(plan(&mut st, TIMEOUT, INTERVAL, at(m, w), &mut watch, None).is_empty());
    }

    #[test]
    fn a_silent_window_is_relaunched_counted_and_then_waits() {
        let mut st = State::default();
        let (m, w) = (Instant::now() + 10 * MIN, SystemTime::now());
        add_window(&mut st, 1, TIMEOUT, m);
        let mut watch = Watch::default();
        let actions = plan(&mut st, TIMEOUT, INTERVAL, at(m, w), &mut watch, Some(false));
        assert!(matches!(actions.as_slice(), [Action::Relaunch { .. }]), "{actions:?}");
        assert_eq!(st.relaunches[&window()].count, 1);
        // The relaunched window, a new client, goes silent again a minute later: it waits.
        st.clients.clear();
        let later = at(m + MIN, w + MIN);
        add_window(&mut st, 2, TIMEOUT, later.mono);
        on_time(&mut watch, later);
        assert!(plan(&mut st, TIMEOUT, INTERVAL, later, &mut watch, Some(false)).is_empty());
        assert_eq!(st.clients[&2].last_heartbeat, later.mono - TIMEOUT, "its clock is left as it is");
        // Away, it is left too, and its clock with it.
        let away = at(later.mono + 2 * MIN, later.wall + 2 * MIN);
        on_time(&mut watch, away);
        assert_eq!(relaunches(&plan(&mut st, TIMEOUT, INTERVAL, away, &mut watch, Some(true))), 0);
        assert_eq!(st.clients[&2].last_heartbeat, later.mono - TIMEOUT);
        assert_eq!(st.relaunches[&window()].count, 1);
    }

    #[test]
    fn the_third_silent_relaunch_is_the_last_and_the_stop_is_written_once() {
        let mut st = State::default();
        let (m, w) = (Instant::now() + 10 * MIN, SystemTime::now());
        st.relaunches.insert(window(), history(3, m));
        add_window(&mut st, 1, TIMEOUT, m + MIN);
        let mut watch = Watch::default();
        let actions = plan(&mut st, TIMEOUT, INTERVAL, at(m + MIN, w), &mut watch, Some(false));
        assert!(matches!(actions.as_slice(), [Action::Stop(_)]), "{actions:?}");
        let next = at(m + MIN + INTERVAL, w + INTERVAL);
        assert!(plan(&mut st, TIMEOUT, INTERVAL, next, &mut watch, Some(false)).is_empty(), "the stop is written once");
        // The person opens Arkos again: the given-up window is gone and one of that identity
        // with another pid starts afresh.
        st.relaunches.get_mut(&window()).unwrap().stopped = BTreeSet::from([1_000_000]);
        let again = at(next.mono + INTERVAL, next.wall + INTERVAL);
        let actions = plan(&mut st, TIMEOUT, INTERVAL, again, &mut watch, Some(false));
        assert!(matches!(actions.as_slice(), [Action::Relaunch { .. }]), "{actions:?}");
        assert_eq!(st.relaunches[&window()].count, 1);
    }

    #[test]
    fn two_windows_of_one_identity_are_both_given_up_on_and_stay_so() {
        // Two launch choosers: the same binary, no folder.
        let mut a = Command::new("/bin/sleep").arg("60").spawn().unwrap();
        let mut b = Command::new("/bin/sleep").arg("60").spawn().unwrap();
        let mut st = State::default();
        let (m, w) = (Instant::now() + 10 * MIN, SystemTime::now());
        let chooser: WindowId = (Some(PathBuf::from("/nowhere/arkos")), None);
        st.relaunches.insert(chooser.clone(), history(3, m));
        add_window_as(&mut st, 1, a.id(), None, TIMEOUT, m + MIN);
        add_window_as(&mut st, 2, b.id(), None, TIMEOUT, m + MIN);
        let mut watch = Watch::default();
        let mut now = at(m + MIN, w);
        let first = plan(&mut st, TIMEOUT, INTERVAL, now, &mut watch, Some(false));
        assert_eq!(first.iter().filter(|a| matches!(a, Action::Stop(_))).count(), 2, "{first:?}");
        for _ in 0..5 {
            now = at(now.mono + INTERVAL, now.wall + INTERVAL);
            let actions = plan(&mut st, TIMEOUT, INTERVAL, now, &mut watch, Some(false));
            assert!(actions.is_empty(), "{actions:?}");
        }
        assert_eq!(st.relaunches[&chooser].stopped, BTreeSet::from([a.id(), b.id()]));
        // One of them is quit; the other, still given up on, keeps the record.
        st.clients.remove(&1);
        now = at(now.mono + INTERVAL, now.wall + INTERVAL);
        assert!(plan(&mut st, TIMEOUT, INTERVAL, now, &mut watch, Some(false)).is_empty());
        assert!(st.relaunches.contains_key(&chooser));
        let _ = (a.kill(), b.kill(), a.wait(), b.wait());
    }

    #[test]
    fn a_record_with_no_window_is_dropped_once_its_last_relaunch_is_old() {
        let mut st = State::default();
        let (m, w) = (Instant::now() + 30 * MIN, SystemTime::now());
        let other: WindowId = (Some(PathBuf::from("/nowhere/arkos")), Some(PathBuf::from("/nowhere/other")));
        st.relaunches.insert(window(), history(1, m - 11 * MIN));
        st.relaunches.insert(other.clone(), history(1, m - MIN));
        let mut watch = Watch::default();
        assert!(plan(&mut st, TIMEOUT, INTERVAL, at(m, w), &mut watch, None).is_empty());
        assert!(!st.relaunches.contains_key(&window()), "old and with no window");
        assert!(st.relaunches.contains_key(&other), "its relaunched window may still be starting");
    }

    #[test]
    fn ten_heartbeating_minutes_reset_the_count_but_a_sleep_given_clock_does_not() {
        let mut st = State::default();
        let (m, w) = (Instant::now() + 10 * MIN, SystemTime::now());
        st.relaunches.insert(window(), history(2, m));
        let woke = at(m + 11 * MIN, w + 60 * MIN);
        add_window(&mut st, 1, 5 * MIN, woke.mono);
        // A sleep gives the hung window a fresh clock; that is not a heartbeat heard.
        let mut watch = Watch { prev: Some(at(m + 11 * MIN - INTERVAL, w)), ..Watch::default() };
        assert!(plan(&mut st, TIMEOUT, INTERVAL, woke, &mut watch, Some(false)).is_empty());
        let next = at(woke.mono + INTERVAL, woke.wall + INTERVAL);
        assert!(plan(&mut st, TIMEOUT, INTERVAL, next, &mut watch, None).is_empty());
        assert_eq!(st.relaunches[&window()].count, 2, "a given clock kept the count");
        // A real heartbeat after it does.
        st.clients.get_mut(&1).unwrap().last_heartbeat = next.mono;
        let after = at(next.mono + INTERVAL, next.wall + INTERVAL);
        assert!(plan(&mut st, TIMEOUT, INTERVAL, after, &mut watch, None).is_empty());
        assert!(st.relaunches.is_empty(), "ten healthy minutes forgot the relaunches");
    }
}
