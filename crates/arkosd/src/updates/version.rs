//! Version strings as both binaries print them (`arkosd 0.1.0 (abc1234)`) and as the manifest
//! carries them, with the ordering the updater uses: only a strictly greater semver is newer.
//! The git hash is informational and never decides, so a replayed older signed manifest cannot
//! roll the app back (a re-release must bump the patch).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub git: String,
}

/// `<name> <major>.<minor>.<patch> (<git>)`, as `--version` prints.
pub fn parse_version_string(s: &str) -> Option<Version> {
    let s = s.trim();
    let (_, rest) = s.split_once(' ')?;
    let (ver, git) = rest.split_once(" (")?;
    let git = git.strip_suffix(')')?;
    let (major, minor, patch) = parse_semver(ver)?;
    Some(Version { major, minor, patch, git: git.to_string() })
}

/// `0.2.1`: three decimal numbers and nothing else.
pub fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let mut it = s.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((a, b, c))
}

impl Version {
    pub fn is_newer_than(&self, running: &Version) -> bool {
        (self.major, self.minor, self.patch) > (running.major, running.minor, running.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_strings_parse_and_compare_semver_then_hash() {
        let running = parse_version_string("arkosd 0.1.0 (abc1234)").unwrap();
        assert_eq!(running, Version { major: 0, minor: 1, patch: 0, git: "abc1234".into() });
        assert_eq!(parse_version_string("arkos 1.2.3 (deadbee)\n").map(|v| v.patch), Some(3));
        assert_eq!(parse_version_string("garbage"), None);
        assert_eq!(parse_version_string("arkosd 0.1 (abc)"), None, "three numbers");
        assert_eq!(parse_semver("0.2.1"), Some((0, 2, 1)));
        assert_eq!(parse_semver("v0.2.1"), None);
        let v = |s: &str, g: &str| Version { git: g.into(), ..parse_version_string(&format!("x {s} (z)")).unwrap() };
        assert!(v("0.2.0", "abc1234").is_newer_than(&running));
        assert!(v("0.1.1", "abc1234").is_newer_than(&running));
        assert!(v("1.0.0", "abc1234").is_newer_than(&running), "major beats minor and patch");
        assert!(!v("0.0.9", "zzzzzzz").is_newer_than(&running), "older semver, whatever the hash");
        assert!(!v("0.1.0", "def5678").is_newer_than(&running), "same version, another hash: not an update");
        assert!(!v("0.1.0", "abc1234").is_newer_than(&running), "the same build");
        assert!(!v("0.1.0", "ABC1234").is_newer_than(&running), "hashes compare case-insensitively");
    }
}
