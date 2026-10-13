mod ring;
mod server;
mod session;
mod watchdog;

use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_REPLAY_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_WATCHDOG_SECS: u64 = 30;

fn default_socket() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME is set");
    PathBuf::from(home).join("Library/Application Support/sarkos/sarkosd.sock")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut socket = default_socket();
    let mut replay = DEFAULT_REPLAY_BYTES;
    let mut watchdog = DEFAULT_WATCHDOG_SECS;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--version" => {
                println!("sarkosd {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--socket" => socket = PathBuf::from(args.next().expect("--socket needs a path")),
            "--replay-bytes" => replay = args.next().and_then(|v| v.parse().ok()).expect("--replay-bytes needs a number"),
            "--watchdog-secs" => watchdog = args.next().and_then(|v| v.parse().ok()).expect("--watchdog-secs needs a number"),
            other => {
                eprintln!("sarkosd: unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    let opts = server::Options { replay_capacity: replay, watchdog: Duration::from_secs(watchdog) };
    if let Err(e) = server::run(&socket, opts) {
        eprintln!("sarkosd: {e:#}");
        std::process::exit(1);
    }
}
