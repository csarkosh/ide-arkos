//! Honours macOS's Reduce Motion setting (spec 7.1: the UI adds no motion the user turned off).
//! Read once at launch through `defaults`, which needs no extra dependency; the config reload in
//! milestone 2b re-reads it.
use std::sync::OnceLock;

static REDUCE: OnceLock<bool> = OnceLock::new();

pub fn reduce_motion() -> bool {
    *REDUCE.get_or_init(|| {
        let out = std::process::Command::new("defaults").args(["read", "com.apple.universalaccess", "reduceMotion"]).output();
        match out {
            Ok(o) => parse_reduce_motion(&String::from_utf8_lossy(&o.stdout)),
            Err(_) => false,
        }
    })
}

/// `defaults` prints `1` when the setting is on, `0` or nothing (an error on stderr) when off.
pub fn parse_reduce_motion(stdout: &str) -> bool {
    stdout.trim() == "1"
}

#[cfg(test)]
mod tests {
    use super::parse_reduce_motion;

    #[test]
    fn parses_defaults_output() {
        assert!(parse_reduce_motion("1\n"));
        assert!(!parse_reduce_motion("0\n"));
        assert!(!parse_reduce_motion(""));
    }
}
