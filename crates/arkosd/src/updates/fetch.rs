//! Downloads through the system curl (TLS and proxies are macOS's business), never into memory
//! past what the manifest says, and never from a host that is not GitHub's: the first-party
//! check is only a start, whose redirect must end there.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub struct Fetcher {
    /// Ends with `/`; `manifest.json` and `manifest.json.sig` are appended.
    pub base: String,
    pub curl: PathBuf,
    /// `arkos/<version>`, sent with every request; the first-party check requires it.
    pub user_agent: String,
}

const MANIFEST_MAX: u64 = 64 * 1024;

/// How long a manifest or signature fetch from GitHub may take.
pub const MANIFEST_SECS: u64 = 20;

/// How long the first-party check may take, its redirect to GitHub's manifest included: a slow
/// collector costs the check at most this before the fetch falls back to GitHub.
pub const FIRST_PARTY_SECS: u64 = 10;

/// GitHub's hosts, where every download ends.
const GITHUB_HOSTS: [&str; 3] = ["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];

/// The first-party check's host: a permitted start whose redirect must end on GitHub.
const FIRST_PARTY_HOST: &str = "apps.csarko.sh";

/// The url curl ended on must be GitHub's: a redirect anywhere else is refused, and the file
/// curl wrote is deleted unread. The request must have started on GitHub or at the first-party
/// check, whose answer is a redirect to GitHub's manifest (one it answered itself, a `204`, is
/// no manifest and is refused). A `file://` url (the tests' sites) is accepted only when the
/// request itself was one; redirects are https only, so an https request cannot end on a file.
pub fn allowed_final_url(requested: &str, url: &str) -> bool {
    if url.starts_with("file:///") {
        return requested.starts_with("file:///");
    }
    let start = https_host(requested).is_some_and(|h| h == FIRST_PARTY_HOST || GITHUB_HOSTS.contains(&h));
    start && https_host(url).is_some_and(|h| GITHUB_HOSTS.contains(&h))
}

/// The host of an https url, None for any other scheme or a host with userinfo or a port.
fn https_host(url: &str) -> Option<&str> {
    let host = url.strip_prefix("https://")?.split(['/', '?', '#']).next().unwrap_or("");
    (!host.contains('@') && !host.contains(':')).then_some(host)
}

pub fn curl_args(url: &str, dest: &Path, max_bytes: u64, max_secs: u64, user_agent: &str) -> Vec<String> {
    vec![
        "--user-agent".into(), user_agent.into(),
        "--fail".into(), "--silent".into(), "--show-error".into(), "--location".into(),
        "--max-redirs".into(), "5".into(),
        "--proto".into(), "=https,file".into(),
        "--proto-redir".into(), "=https".into(),
        "--max-time".into(), max_secs.to_string(),
        "--max-filesize".into(), max_bytes.to_string(),
        "--output".into(), dest.to_string_lossy().into_owned(),
        "--write-out".into(), "%{url_effective}".into(),
        "--".into(), url.to_string(),
    ]
}

/// Why a release download failed, which decides whether it is tried again.
#[derive(Debug, PartialEq, Eq)]
pub enum DownloadError {
    /// curl or the network failed (a wifi drop, a captive portal's redirect): retried at the
    /// next check.
    Transport(String),
    /// The release itself is wrong (bigger or smaller than its manifest says): not retried
    /// until the manifest changes.
    Release(String),
}

/// curl's exit code for a file over `--max-filesize`.
const CURLE_FILESIZE_EXCEEDED: i32 = 63;

impl Fetcher {
    /// `url` into `dest`, at most `max_bytes` and `max_secs`. On any failure `dest` is gone.
    pub fn get(&self, url: &str, dest: &Path, max_bytes: u64, max_secs: u64) -> Result<(), String> {
        self.run_curl(url, dest, max_bytes, max_secs).map_err(|(e, _)| e)
    }

    /// `get`, with curl's exit code when it failed with one.
    fn run_curl(&self, url: &str, dest: &Path, max_bytes: u64, max_secs: u64) -> Result<(), (String, Option<i32>)> {
        let mut cmd = Command::new(&self.curl);
        // `--disable` must come first: no ~/.curlrc can add options to this call.
        cmd.arg("--disable").args(curl_args(url, dest, max_bytes, max_secs, &self.user_agent));
        // curl's own --max-time ends it first; this is the backstop should it hang regardless.
        let out = match super::run_bounded(&mut cmd, Duration::from_secs(max_secs + 10)) {
            Ok(out) => out,
            Err(e) => {
                let _ = std::fs::remove_file(dest);
                return Err((format!("curl: {e}"), None));
            }
        };
        if !out.status.success() {
            let _ = std::fs::remove_file(dest);
            return Err((format!("curl: {}", String::from_utf8_lossy(&out.stderr).trim()), out.status.code()));
        }
        let final_url = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !allowed_final_url(url, &final_url) {
            let _ = std::fs::remove_file(dest);
            return Err((format!("refused: the download ended at {final_url}"), None));
        }
        Ok(())
    }

    /// GitHub's manifest url, the one every check falls back to.
    pub fn manifest_url(&self) -> String {
        format!("{}manifest.json", self.base)
    }

    /// The manifest's bytes from `url` (GitHub's, or the first-party check that redirects to it),
    /// fetched into `dir` within `max_secs`.
    pub fn manifest(&self, url: &str, dir: &Path, max_secs: u64) -> Result<Vec<u8>, String> {
        self.read(url, &dir.join("manifest.json"), max_secs).map_err(|e| format!("manifest.json: {e}"))
    }

    /// The signature's text, fetched into `dir`, always from GitHub.
    pub fn signature(&self, dir: &Path) -> Result<String, String> {
        let name = "manifest.json.sig";
        let sig = self.read(&format!("{}{name}", self.base), &dir.join(name), MANIFEST_SECS).map_err(|e| format!("{name}: {e}"))?;
        Ok(String::from_utf8_lossy(&sig).trim().to_string())
    }

    fn read(&self, url: &str, dest: &Path, max_secs: u64) -> Result<Vec<u8>, String> {
        self.get(url, dest, MANIFEST_MAX, max_secs)?;
        std::fs::read(dest).map_err(|e| e.to_string())
    }

    /// The release zip into `dest`, refused unless it is exactly `size` bytes.
    pub fn download(&self, url: &str, size: u64, dest: &Path) -> Result<(), DownloadError> {
        match self.run_curl(url, dest, size, 600) {
            Ok(()) => {}
            Err((e, Some(CURLE_FILESIZE_EXCEEDED))) => return Err(DownloadError::Release(e)),
            Err((e, _)) => return Err(DownloadError::Transport(e)),
        }
        let got = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        if got != size {
            let _ = std::fs::remove_file(dest);
            return Err(DownloadError::Release(format!("the download is {got} bytes, the manifest's size is {size}")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_github_hosts_or_file_urls_are_accepted_as_the_final_url() {
        let gh = "https://github.com/csarkosh/ide-arkos/releases/latest/download/x.zip";
        assert!(allowed_final_url(gh, "https://github.com/csarkosh/ide-arkos/releases/download/v0.2.1/x.zip"));
        assert!(allowed_final_url(gh, "https://objects.githubusercontent.com/github-production-release-asset/abc?x=1"));
        assert!(allowed_final_url(gh, "https://release-assets.githubusercontent.com/x"));
        assert!(allowed_final_url("file:///tmp/x/manifest.json", "file:///tmp/x/manifest.json"));
        assert!(!allowed_final_url(gh, "file:///tmp/x/manifest.json"), "an https request never ends on a file");
        assert!(!allowed_final_url(gh, "https://github.com.evil.example/x"));
        assert!(!allowed_final_url(gh, "https://evil.example/github.com/x"));
        assert!(!allowed_final_url(gh, "http://github.com/x"), "https only");
        assert!(!allowed_final_url(gh, "https://user@github.com/x"), "no userinfo tricks");
        assert!(!allowed_final_url(gh, ""));
    }

    #[test]
    fn a_first_party_start_may_end_only_on_github() {
        let fp = "https://apps.csarko.sh/a/check?v=2&app=arkos&ver=0.5.0&bin=daemon&os=macos&osv=15&arch=arm64&ch=release&k=check&rd=-1";
        assert!(allowed_final_url(fp, "https://github.com/csarkosh/ide-arkos/releases/download/v0.5.0/manifest.json"));
        assert!(allowed_final_url(fp, "https://release-assets.githubusercontent.com/x"));
        assert!(!allowed_final_url(fp, fp), "a check answered without a redirect (a 204) is no manifest");
        assert!(!allowed_final_url(fp, "https://evil.example/manifest.json"));
        // GitHub and the collector are the only starts an https fetch may have.
        assert!(!allowed_final_url("https://evil.example/a/check", "https://github.com/x"));
        assert!(!allowed_final_url("https://apps.csarko.sh.evil.example/a/check", "https://github.com/x"));
        assert!(!allowed_final_url("https://user@apps.csarko.sh/a/check", "https://github.com/x"));
        assert!(!allowed_final_url("http://apps.csarko.sh/a/check", "https://github.com/x"));
        assert!(!allowed_final_url("file:///tmp/site/a/check", "https://github.com/x"), "a file never redirects");
    }

    #[test]
    fn curl_is_told_to_fail_loudly_cap_the_size_and_report_the_final_url() {
        let args = curl_args("https://github.com/x", Path::new("/tmp/out"), 1234, 20, "arkos/0.5.0");
        for want in ["--user-agent", "arkos/0.5.0", "--fail", "--silent", "--show-error", "--location", "--max-redirs", "--proto", "=https,file", "--proto-redir", "=https", "--max-time", "20", "--max-filesize", "1234", "--output", "/tmp/out", "--write-out", "%{url_effective}", "https://github.com/x"] {
            assert!(args.iter().any(|a| a == want), "missing {want} in {args:?}");
        }
        assert_eq!(args.last().map(String::as_str), Some("https://github.com/x"), "the url is last so it cannot be read as an option");
    }

    #[test]
    fn a_manifest_and_its_signature_are_fetched_from_a_file_url_and_a_missing_one_is_an_error() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("manifest.json"), b"{}").unwrap();
        std::fs::write(src.path().join("manifest.json.sig"), b"c2ln\n").unwrap();
        let f = Fetcher { base: format!("file://{}/", src.path().display()), curl: PathBuf::from("/usr/bin/curl"), user_agent: "arkos/0.5.0".into() };
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(f.manifest_url(), format!("file://{}/manifest.json", src.path().display()));
        assert_eq!(f.manifest(&f.manifest_url(), dir.path(), MANIFEST_SECS).unwrap(), b"{}");
        assert_eq!(f.signature(dir.path()).unwrap(), "c2ln");
        std::fs::remove_file(src.path().join("manifest.json.sig")).unwrap();
        assert!(f.signature(dir.path()).unwrap_err().contains("manifest.json.sig"));
        let missing = format!("file://{}/a/check", src.path().display());
        assert!(f.manifest(&missing, dir.path(), FIRST_PARTY_SECS).unwrap_err().starts_with("manifest.json: "));
    }

    #[test]
    fn a_download_is_capped_at_the_manifests_size() {
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("big.zip"), vec![0u8; 10_000]).unwrap();
        let f = Fetcher { base: String::new(), curl: PathBuf::from("/usr/bin/curl"), user_agent: "arkos/0.5.0".into() };
        let dir = tempfile::tempdir().unwrap();
        let url = format!("file://{}/big.zip", src.path().display());
        assert!(f.download(&url, 10_000, &dir.path().join("a.zip")).is_ok());
        let err = f.download(&url, 5_000, &dir.path().join("b.zip")).unwrap_err();
        assert!(matches!(&err, DownloadError::Release(e) if e.contains("curl")), "curl refuses a file over --max-filesize: {err:?}");
        assert!(!dir.path().join("b.zip").exists(), "nothing is left behind");
        let missing = format!("file://{}/gone.zip", src.path().display());
        assert!(matches!(f.download(&missing, 10, &dir.path().join("c.zip")), Err(DownloadError::Transport(_))), "a fetch that fails is retried later");
    }
}
