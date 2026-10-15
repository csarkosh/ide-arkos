//! Downloads through the system curl (TLS and proxies are macOS's business), never into memory
//! past what the manifest says, and never from a host that is not GitHub's.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub struct Fetcher {
    /// Ends with `/`; `manifest.json` and `manifest.json.sig` are appended.
    pub base: String,
    pub curl: PathBuf,
}

const MANIFEST_MAX: u64 = 64 * 1024;

/// The url curl ended on must be GitHub's (or a file for the tests): a redirect anywhere else
/// is refused, and the file curl wrote is deleted unread.
pub fn allowed_final_url(url: &str) -> bool {
    if url.starts_with("file:///") {
        return true;
    }
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.contains('@') || host.contains(':') {
        return false;
    }
    matches!(host, "github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com")
}

pub fn curl_args(url: &str, dest: &Path, max_bytes: u64, max_secs: u64) -> Vec<String> {
    vec![
        "--fail".into(), "--silent".into(), "--show-error".into(), "--location".into(),
        "--max-redirs".into(), "5".into(),
        "--proto".into(), "=https,file".into(),
        "--max-time".into(), max_secs.to_string(),
        "--max-filesize".into(), max_bytes.to_string(),
        "--output".into(), dest.to_string_lossy().into_owned(),
        "--write-out".into(), "%{url_effective}".into(),
        "--".into(), url.to_string(),
    ]
}

impl Fetcher {
    /// `url` into `dest`, at most `max_bytes` and `max_secs`. On any failure `dest` is gone.
    pub fn get(&self, url: &str, dest: &Path, max_bytes: u64, max_secs: u64) -> Result<(), String> {
        let mut cmd = Command::new(&self.curl);
        // `--disable` must come first: no ~/.curlrc can add options to this call.
        cmd.arg("--disable").args(curl_args(url, dest, max_bytes, max_secs));
        // curl's own --max-time ends it first; this is the backstop should it hang regardless.
        let out = match super::run_bounded(&mut cmd, Duration::from_secs(max_secs + 10)) {
            Ok(out) => out,
            Err(e) => {
                let _ = std::fs::remove_file(dest);
                return Err(format!("curl: {e}"));
            }
        };
        if !out.status.success() {
            let _ = std::fs::remove_file(dest);
            return Err(format!("curl: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let final_url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !allowed_final_url(&final_url) {
            let _ = std::fs::remove_file(dest);
            return Err(format!("refused: the download ended at {final_url}"));
        }
        Ok(())
    }

    /// The manifest's bytes and its signature's text, both fetched into `dir`.
    pub fn fetch_manifest(&self, dir: &Path) -> Result<(Vec<u8>, String), String> {
        let read = |name: &str| -> Result<Vec<u8>, String> {
            let dest = dir.join(name);
            self.get(&format!("{}{name}", self.base), &dest, MANIFEST_MAX, 20).map_err(|e| format!("{name}: {e}"))?;
            std::fs::read(&dest).map_err(|e| format!("{name}: {e}"))
        };
        let bytes = read("manifest.json")?;
        let sig = read("manifest.json.sig")?;
        Ok((bytes, String::from_utf8_lossy(&sig).trim().to_string()))
    }

    /// The release zip into `dest`, refused unless it is exactly `size` bytes.
    pub fn download(&self, url: &str, size: u64, dest: &Path) -> Result<(), String> {
        self.get(url, dest, size, 600)?;
        let got = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        if got != size {
            let _ = std::fs::remove_file(dest);
            return Err(format!("the download is {got} bytes, the manifest's size is {size}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_github_hosts_or_file_urls_are_accepted_as_the_final_url() {
        assert!(allowed_final_url("https://github.com/csarkosh/ide-sarkos/releases/download/v0.2.1/x.zip"));
        assert!(allowed_final_url("https://objects.githubusercontent.com/github-production-release-asset/abc?x=1"));
        assert!(allowed_final_url("https://release-assets.githubusercontent.com/x"));
        assert!(allowed_final_url("file:///tmp/x/manifest.json"));
        assert!(!allowed_final_url("https://github.com.evil.example/x"));
        assert!(!allowed_final_url("https://evil.example/github.com/x"));
        assert!(!allowed_final_url("http://github.com/x"), "https only");
        assert!(!allowed_final_url("https://user@github.com/x"), "no userinfo tricks");
        assert!(!allowed_final_url(""));
    }

    #[test]
    fn curl_is_told_to_fail_loudly_cap_the_size_and_report_the_final_url() {
        let args = curl_args("https://github.com/x", Path::new("/tmp/out"), 1234, 20);
        for want in ["--fail", "--silent", "--show-error", "--location", "--max-redirs", "--proto", "=https,file", "--max-time", "20", "--max-filesize", "1234", "--output", "/tmp/out", "--write-out", "%{url_effective}", "https://github.com/x"] {
            assert!(args.iter().any(|a| a == want), "missing {want} in {args:?}");
        }
        assert_eq!(args.last().map(String::as_str), Some("https://github.com/x"), "the url is last so it cannot be read as an option");
    }

    #[test]
    fn a_manifest_and_its_signature_are_fetched_from_a_file_url_and_a_missing_one_is_an_error() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("manifest.json"), b"{}").unwrap();
        std::fs::write(src.path().join("manifest.json.sig"), b"c2ln\n").unwrap();
        let f = Fetcher { base: format!("file://{}/", src.path().display()), curl: PathBuf::from("/usr/bin/curl") };
        let dir = tempfile::tempdir().unwrap();
        let (m, sig) = f.fetch_manifest(dir.path()).unwrap();
        assert_eq!(m, b"{}");
        assert_eq!(sig, "c2ln");
        std::fs::remove_file(src.path().join("manifest.json.sig")).unwrap();
        assert!(f.fetch_manifest(dir.path()).unwrap_err().contains("manifest.json.sig"));
    }

    #[test]
    fn a_download_is_capped_at_the_manifests_size() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("big.zip"), vec![0u8; 10_000]).unwrap();
        let f = Fetcher { base: String::new(), curl: PathBuf::from("/usr/bin/curl") };
        let dir = tempfile::tempdir().unwrap();
        let url = format!("file://{}/big.zip", src.path().display());
        assert!(f.download(&url, 10_000, &dir.path().join("a.zip")).is_ok());
        assert!(f.download(&url, 5_000, &dir.path().join("b.zip")).unwrap_err().contains("curl"), "curl refuses a file over --max-filesize");
        assert!(!dir.path().join("b.zip").exists(), "nothing is left behind");
    }
}
