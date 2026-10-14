//! Honours macOS's Reduce Motion setting (spec 7.1: the UI adds no motion the user turned off).
//! Read through `defaults`, which needs no extra dependency, at launch and on every config
//! reload (`refresh` runs on the background executor; it spawns a process).
use std::sync::atomic::{AtomicBool, Ordering};

static REDUCE: AtomicBool = AtomicBool::new(false);

pub fn reduce_motion() -> bool {
    REDUCE.load(Ordering::Relaxed)
}

fn set(on: bool) {
    REDUCE.store(on, Ordering::Relaxed);
}

/// Re-reads the setting. Blocks on `defaults` for a few milliseconds.
pub fn refresh() {
    let out = std::process::Command::new("defaults").args(["read", "com.apple.universalaccess", "reduceMotion"]).output();
    set(match out {
        Ok(o) => parse_reduce_motion(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => false,
    });
}

/// `defaults` prints `1` when the setting is on, `0` or nothing (an error on stderr) when off.
pub fn parse_reduce_motion(stdout: &str) -> bool {
    stdout.trim() == "1"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_output_and_refresh_sets_the_flag() {
        assert!(parse_reduce_motion("1\n"));
        assert!(!parse_reduce_motion("0\n"));
        assert!(!parse_reduce_motion(""));
        set(true);
        assert!(reduce_motion());
        set(false);
        assert!(!reduce_motion());
    }
}
