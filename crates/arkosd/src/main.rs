mod handover;
mod journal;
mod pty;
mod reaper;
mod recovery;
mod ring;
mod server;
mod session;
mod telemetry;
mod upgrade;
mod updates;
mod watchdog;

use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_REPLAY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_WATCHDOG_SECS: u64 = 30;

/// `<name> <crate version> (<short git hash>)`, the same shape for both binaries.
pub fn version_string() -> String {
    format!("{} {} ({})", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), env!("ARKOS_GIT_HASH"))
}

fn default_socket() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME is set");
    PathBuf::from(home).join("Library/Application Support/arkos/arkosd.sock")
}

fn main() {
    recovery::block_termination_signals();
    let mut args = std::env::args().skip(1);
    let mut socket = default_socket();
    let mut replay = DEFAULT_REPLAY_BYTES;
    let mut watchdog = DEFAULT_WATCHDOG_SECS;
    let mut reaper_millis: u64 = 1000;
    let mut dead_session_secs: u64 = 600;
    let mut flush_quiet_millis = handover::DEFAULT_FLUSH_QUIET_MILLIS;
    let mut pending_cap = session::CLIENT_PENDING_CAP;
    let mut backpressure = session::Backpressure::default();
    let mut resume = None;
    let mut recover = false;
    // The update arguments; the staging folder's default follows `--socket`, wherever it comes.
    let mut update_url = None;
    let mut check_every_secs = None;
    let mut curl = None;
    let mut ditto = None;
    let mut updates_dir = None;
    let mut release_keys = Vec::new();
    let mut bundle = None;
    let mut connect_grace_secs = None;
    let mut running_version = None;
    // The telemetry folder's default follows `--socket` too, wherever it comes.
    let mut telemetry_dir = None;
    let mut telemetry_url = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" => {
                println!("{}", version_string());
                return;
            }
            "--check-handover" => {
                // Exactly this line: the current main's daemon compares the whole answer.
                println!("handover {}", handover::OLDEST_READ);
                return;
            }
            "--handover-formats" => {
                println!("{} {}", handover::OLDEST_READ, handover::FORMAT);
                return;
            }
            "--recover" => recover = true,
            "--socket" => socket = PathBuf::from(args.next().expect("--socket needs a path")),
            "--resume" => resume = Some(PathBuf::from(args.next().expect("--resume needs a path"))),
            "--replay-bytes" => replay = args.next().and_then(|v| v.parse().ok()).expect("--replay-bytes needs a number"),
            "--client-pending-bytes" => {
                pending_cap = args.next().and_then(|v| v.parse().ok()).expect("--client-pending-bytes needs a number")
            }
            "--subscriber-high-water" => {
                let high: usize = args.next().and_then(|v| v.parse().ok()).expect("--subscriber-high-water needs a number");
                backpressure.high_water = high;
                backpressure.low_water = high / 2;
            }
            "--subscriber-stall-secs" => {
                let secs: u64 = args.next().and_then(|v| v.parse().ok()).expect("--subscriber-stall-secs needs a number");
                backpressure.stall = Duration::from_secs(secs);
            }
            "--watchdog-secs" => watchdog = args.next().and_then(|v| v.parse().ok()).expect("--watchdog-secs needs a number"),
            "--reaper-millis" => reaper_millis = args.next().and_then(|v| v.parse().ok()).expect("--reaper-millis needs a number"),
            "--dead-session-secs" => dead_session_secs = args.next().and_then(|v| v.parse().ok()).expect("--dead-session-secs needs a number"),
            "--flush-quiet-millis" => {
                flush_quiet_millis = args.next().and_then(|v| v.parse().ok()).expect("--flush-quiet-millis needs a number")
            }
            // Where `manifest.json` and its `.sig` are fetched from; a `file://` folder in tests.
            "--update-url" => {
                let mut url = args.next().expect("--update-url needs a url");
                if !url.ends_with('/') {
                    url.push('/');
                }
                update_url = Some(url)
            }
            // How often to look for a release; 0 turns polling off.
            "--check-every-secs" => {
                check_every_secs = Some(args.next().and_then(|v| v.parse::<u64>().ok()).expect("--check-every-secs needs a number"))
            }
            "--curl" => curl = Some(PathBuf::from(args.next().expect("--curl needs a path"))),
            "--ditto" => ditto = Some(PathBuf::from(args.next().expect("--ditto needs a path"))),
            "--updates-dir" => updates_dir = Some(PathBuf::from(args.next().expect("--updates-dir needs a path"))),
            // Test only: trusts one more release key (base64), so the tests can sign a release.
            // Never read from the environment; a release is signed by a compiled-in key.
            "--release-key" => release_keys.push(args.next().expect("--release-key needs a key")),
            // The installed app that `ApplyUpdate` swaps; by default the one this binary runs from.
            "--bundle" => bundle = Some(PathBuf::from(args.next().expect("--bundle needs a path"))),
            // Test only: how long the new window has to connect after an applied update.
            "--connect-grace-secs" => {
                connect_grace_secs = Some(args.next().and_then(|v| v.parse::<u64>().ok()).expect("--connect-grace-secs needs a number"))
            }
            // Test only: the version releases are compared against instead of this binary's.
            "--running-version" => {
                let v = args.next().expect("--running-version needs a version");
                assert!(updates::version::parse_semver(&v).is_some(), "--running-version needs <major>.<minor>.<patch>");
                running_version = Some(v)
            }
            // The telemetry folder: the state file, the windows' journals and the queue.
            "--telemetry-dir" => telemetry_dir = Some(PathBuf::from(args.next().expect("--telemetry-dir needs a path"))),
            // Test only: where the day's first update check and the queued reports go instead of
            // the collector. Only the collector itself or a `file://` folder is taken, so no flag
            // can send either elsewhere (and the reports' curl takes https alone).
            "--telemetry-url" => {
                let url = args.next().expect("--telemetry-url needs a url");
                if !telemetry::allowed_url(&url) {
                    eprintln!("arkosd: --telemetry-url takes {} or a file:// folder", telemetry::DEFAULT_URL);
                    std::process::exit(2);
                }
                telemetry_url = Some(url)
            }
            other => {
                eprintln!("arkosd: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let mut update = updates::UpdateOptions::defaults(&socket);
    update.url = update_url.unwrap_or(update.url);
    update.check_every = check_every_secs.map(Duration::from_secs).unwrap_or(update.check_every);
    update.curl = curl.unwrap_or(update.curl);
    update.ditto = ditto.unwrap_or(update.ditto);
    update.dir = updates_dir.unwrap_or(update.dir);
    update.extra_keys = release_keys;
    update.bundle = bundle.or(update.bundle);
    update.connect_grace = connect_grace_secs.map(Duration::from_secs).unwrap_or(update.connect_grace);
    update.running_version = running_version;
    update.telemetry.dir = telemetry_dir.unwrap_or(update.telemetry.dir);
    update.telemetry.url = telemetry_url.unwrap_or(update.telemetry.url);
    journal::init(&update.dir, &socket);
    if let Some(path) = resume {
        let h = match handover::read(&path) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("arkosd: cannot resume: {e}");
                // It may still hold every session's terminal contents; nothing can use it now.
                let _ = std::fs::remove_file(&path);
                std::process::exit(1);
            }
        };
        let _ = std::fs::remove_file(&path);
        if let Err(e) = server::run_resumed(h, socket, update) {
            eprintln!("arkosd: {e:#}");
            std::process::exit(1);
        }
        return;
    }
    if replay == 0 {
        eprintln!("arkosd: --replay-bytes must be at least 1");
        std::process::exit(2);
    }
    let opts = server::Options {
        replay_capacity: replay,
        watchdog: Duration::from_secs(watchdog),
        client_pending_cap: pending_cap,
        backpressure,
        reaper_every: Duration::from_millis(reaper_millis.max(10)),
        dead_session_ttl: Duration::from_secs(dead_session_secs),
        socket,
        flush_quiet: Duration::from_millis(flush_quiet_millis),
        update,
    };
    if let Err(e) = server::run_with(opts, recover) {
        if recover && e.to_string().contains(server::ALREADY_LISTENING) {
            eprintln!("arkosd: already running; nothing to recover");
            return;
        }
        eprintln!("arkosd: {e:#}");
        std::process::exit(1);
    }
}
