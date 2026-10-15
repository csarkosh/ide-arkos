//! `manifest.json` (auto-update spec, section 3). Parsed only after its signature checked out.
use super::version::{parse_semver, Version};
use serde::Deserialize;

pub const FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub version: String,
    pub git: String,
    #[serde(default)]
    pub published: String,
    pub arm64: Asset,
    #[serde(default)]
    pub min_protocol: u32,
}

pub fn parse(bytes: &[u8]) -> Result<Manifest, String> {
    let m: Manifest = serde_json::from_slice(bytes).map_err(|e| format!("manifest: {e}"))?;
    if m.format != FORMAT {
        return Err(format!("manifest: format {} is not {FORMAT}", m.format));
    }
    if parse_semver(&m.version).is_none() {
        return Err(format!("manifest: version {:?} is not major.minor.patch", m.version));
    }
    if m.arm64.sha256.len() != 64 || !m.arm64.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("manifest: sha256 is not 64 hex characters".into());
    }
    if !m.arm64.url.starts_with("https://") && !m.arm64.url.starts_with("file://") {
        return Err("manifest: the url must be https".into());
    }
    Ok(m)
}

impl Manifest {
    pub fn version(&self) -> Option<Version> {
        let (major, minor, patch) = parse_semver(&self.version)?;
        Some(Version { major, minor, patch, git: self.git.clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const SAMPLE: &str = r#"{"format":1,"version":"0.2.1","git":"a1b2c3d","published":"2026-10-15T03:12:00Z","arm64":{"url":"https://github.com/csarkosh/ide-arkos/releases/download/v0.2.1/Arkos-IDE-0.2.1-arm64.zip","size":23456789,"sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"},"min_protocol":1}"#;

    #[test]
    fn a_manifest_parses_and_the_wrong_format_or_a_bad_version_is_refused() {
        let m = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(m.version, "0.2.1");
        assert_eq!(m.arm64.size, 23456789);
        assert_eq!(m.version().unwrap(), Version { major: 0, minor: 2, patch: 1, git: "a1b2c3d".into() });
        assert!(parse(SAMPLE.replace("\"format\":1", "\"format\":2").as_bytes()).unwrap_err().contains("format 2"));
        assert!(parse(b"not json").is_err());
        assert!(parse(SAMPLE.replace("0.2.1\"", "two\"").as_bytes()).unwrap_err().contains("version"), "a version that is not semver is refused at parse time");
        assert!(parse(SAMPLE.replace("e3b0c442", "xyz").as_bytes()).unwrap_err().contains("sha256"), "a hash that is not 64 hex chars");
        assert!(parse(SAMPLE.replace("https://github.com", "http://github.com").as_bytes()).unwrap_err().contains("https"), "only https urls");
    }

    #[test]
    fn an_older_or_equal_manifest_is_ignored() {
        let running = Version { major: 0, minor: 2, patch: 1, git: "a1b2c3d".into() };
        let m = parse(SAMPLE.as_bytes()).unwrap();
        assert!(!m.version().unwrap().is_newer_than(&running), "the same build");
        let older = parse(SAMPLE.replace("0.2.1\"", "0.2.0\"").as_bytes()).unwrap();
        assert!(!older.version().unwrap().is_newer_than(&running), "a mistaken re-publish of an old version changes nothing");
        assert!(!parse(SAMPLE.replace("a1b2c3d", "ffffff0").as_bytes()).unwrap().version().unwrap().is_newer_than(&running), "a re-release of the same version is not an update: bump the patch");
        let newer = parse(SAMPLE.replace("0.2.1\"", "0.2.2\"").as_bytes()).unwrap();
        assert!(newer.version().unwrap().is_newer_than(&running));
    }
}
