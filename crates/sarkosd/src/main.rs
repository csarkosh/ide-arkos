mod handover;
mod pty;
mod reaper;
mod ring;
mod server;
mod session;
mod watchdog;

use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_REPLAY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_WATCHDOG_SECS: u64 = 30;

/// `<name> <crate version> (<short git hash>)`, the same shape for both binaries.
pub fn version_string() -> String {
    format!("{} {} ({})", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), env!("SARKOS_GIT_HASH"))
}

fn default_socket() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME is set");
    PathBuf::from(home).join("Library/Application Support/sarkos/sarkosd.sock")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut socket = default_socket();
    let mut replay = DEFAULT_REPLAY_BYTES;
    let mut watchdog = DEFAULT_WATCHDOG_SECS;
    let mut reaper_millis: u64 = 1000;
    let mut dead_session_secs: u64 = 600;
    let mut pending_cap = session::CLIENT_PENDING_CAP;
    let mut backpressure = session::Backpressure::default();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" => {
                println!("{}", version_string());
                return;
            }
            "--check-handover" => {
                println!("handover {}", handover::FORMAT);
                return;
            }
            "--socket" => socket = PathBuf::from(args.next().expect("--socket needs a path")),
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
            other => {
                eprintln!("sarkosd: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    if replay == 0 {
        eprintln!("sarkosd: --replay-bytes must be at least 1");
        std::process::exit(2);
    }
    let opts = server::Options {
        replay_capacity: replay,
        watchdog: Duration::from_secs(watchdog),
        client_pending_cap: pending_cap,
        backpressure,
        reaper_every: Duration::from_millis(reaper_millis.max(10)),
        dead_session_ttl: Duration::from_secs(dead_session_secs),
    };
    if let Err(e) = server::run(&socket, opts) {
        eprintln!("sarkosd: {e:#}");
        std::process::exit(1);
    }
}
