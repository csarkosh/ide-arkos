# Automatic Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The IDE fetches signed releases in the background, verifies them, and applies them while the window is idle, without ending a Claude Code session; releases are built by a manual GitHub Actions workflow into an app bundle.

**Architecture:** A `release-tool` crate signs manifests (ed25519) and the public key is compiled into `protocol`. The daemon gains an `updates` module: a poll thread fetches `manifest.json` with the system `curl`, verifies the signature, downloads and checks the zip, unpacks it into a staging folder and tells windows an update is staged; `ApplyUpdate` swaps the app bundle with two renames and the existing exec-in-place and relaunch do the rest; a marker file drives rollback if no window connects. The window keeps a last-input instant, decides idleness with a pure function, and runs the existing "Update now" flow on its own.

**Tech Stack:** Rust 2021, gpui 0.2.2 (unchanged), `ed25519-compact` 2, `sha2` 0.10, `base64` 0.22, `serde_json` (daemon), system `/usr/bin/curl` and `/usr/bin/ditto`, GitHub Actions `macos-15`.

**Spec:** `docs/superpowers/specs/2026-10-14-auto-update-design.md` (and, for the handover and relaunch it reuses, `docs/superpowers/specs/2026-10-14-update-in-place-design.md`).

## Global Constraints

- Machine rules (the user's MacBook Air locked up from build load): every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's local `.cargo/config.toml` (`jobs = 3`, shared `target-dir`) stays; no `--release` builds on the laptop (the warning check is `cargo check --workspace --all-targets`); never two cargo processes at once; focused test filters while iterating, the workspace suite once per task.
- New dependencies, all small and only these: `ed25519-compact = "2"` (release-tool, sarkosd), `sha2 = "0.10"` (sarkosd), `base64 = "0.22"` (release-tool, sarkosd), `serde_json = "1"` (sarkosd; sarkos has it). No gpui bump. The daemon gains no HTTP crate: fetches run `/usr/bin/curl` by absolute path; unpacking runs `/usr/bin/ditto`.
- Protocol rule (update-in-place spec, section 5): variants are appended, never inserted; existing structs never change shape. This milestone appends `Request::{UpdateStatus, CheckForUpdates, ApplyUpdate}`, `Response::UpdateStatus`, `Event::UpdateStaged`, and bumps `PROTOCOL_VERSION` to 4 with `PROTOCOL_MIN` still 1.
- Nothing from a download runs before the manifest's signature and the zip's size and sha256 have been checked. The manifest URL is a fixed HTTPS URL on github.com (the base is a daemon argument so tests use `file://`); redirects are accepted only to `github.com`, `objects.githubusercontent.com` or `release-assets.githubusercontent.com`.
- Versions compare by semver only: a manifest is applied only when its version is strictly greater than the running one (the git hash is informational; a re-release is a new patch version, so a replayed older signed manifest can never roll a build back). A version that failed verification or staging is not retried until the manifest changes.
- The daemon never shows UI; the window never touches the network. Every spawned process gets `stdin` null and a timeout.
- The release workflow has `on: workflow_dispatch` only. It builds on the runner; nothing in this plan builds a release on the laptop.
- No em-dashes in any file. Warning-free. Headless tests only; repository or process tests take the crate's spawn lock where one exists (`crate::client::fork_lock()` in sarkos; the daemon tests run real binaries).
- Durations in config are plain seconds (`check_every_secs`, `idle_after_secs`), like `watchdog_secs`.

## Review Focus

1. A manifest whose signature is valid but whose `version` is older than the running one (a mistaken re-publish): nothing is downloaded and nothing changes (Task 2 `an_older_or_equal_manifest_is_ignored`).
2. A zip whose size matches but whose bytes differ from the manifest's sha256 (a tampered or truncated asset): the staging folder is deleted, the version is marked bad, nothing is unpacked (Task 3 `a_zip_with_the_wrong_hash_is_deleted_and_the_version_marked_bad`).
3. `curl` following a redirect off github.com (a hijacked DNS or a bad release URL): the download is refused before the host check even starts reading (Task 3 `a_final_url_off_github_is_refused`).
4. The second rename of the swap failing after the first succeeded (a permission change between them): the installed bundle is restored from `.old` and the error reported, so the app is never left missing (Task 4 `a_failed_second_rename_restores_the_bundle`).
5. The relaunched window never connects (the new build crashes at start): within 20 s the old bundle is back and an old window is launched (Task 4 `a_window_that_never_connects_rolls_the_bundle_back`).

## File structure

- `crates/release-tool/` (new, binary): `keygen`, `sign`, `verify` subcommands; `src/main.rs`, `src/sig.rs`.
- `crates/protocol/src/release_key.rs` (new): `RELEASE_PUBLIC_KEYS: &[&str]` (base64, 32 bytes each) and `pub mod` in `lib.rs`; the protocol additions in `lib.rs`.
- `crates/sarkosd/src/updates/mod.rs` (new): `UpdateState`, `Options` fields, the module tree. `version.rs`, `manifest.rs`, `verify.rs`, `fetch.rs`, `stage.rs`, `poll.rs`, `swap.rs`, `apply.rs`.
- `crates/sarkosd/src/main.rs`: `--update-url`, `--check-every-secs`, `--bundle`, `--curl` arguments; `server.rs`: the three requests, `State.update`, the poll thread start, the rollback watch; `recovery.rs`: leftover cleanup on `--recover`.
- `crates/sarkos/src/idle.rs` (new): the idle rule. `config.rs`: `[update]`. `sessions.rs`: `Pending::{Status, Check, Apply}`, outcomes. `main.rs`: `last_input`, `staged`, `update_error`, the auto-apply timer, palette commands. `chrome.rs` unchanged (`status_items` already takes the text). `command_palette.rs`, `keymap.rs` (two commands, unbound).
- `.github/workflows/release.yml`, `packaging/Info.plist.in`, `packaging/bundle.sh`, `packaging/manifest.sh`, `scripts/release-key`, `scripts/app-management-probe.sh`, `homebrew/sarkos-ide.rb`, `README.md`.

## Rulings taken while planning

1. The spec's "minisign-compatible" signature format is dropped: the signature file is the base64 of the raw 64-byte ed25519 signature over the manifest bytes, and `release-tool verify` is the by-hand check. Minisign's prehashed format would add code for no user.
2. The spec's "exits twice within a minute" rollback clause is dropped: the daemon rolls back only when no window connects within 20 s of the new daemon's start. A window that connects and later crashes is the watchdog's existing business. The spec is amended in Task 6.
3. The staged state is not in `Response::Hello` (its shape is frozen): a protocol 4 window sends `UpdateStatus` after Hello.
4. The pending rollback is a marker file `updates/applied.json` beside the socket, read by every daemon start (resumed, recovered or fresh), so the handover format does not change.
5. The App Management probe (Task 1) touches `/Applications` on the user's Mac with a throwaway bundle named `Sarkos Probe.app` and removes it; the implementer runs it once and records the result in the ledger. If macOS prompts or refuses, the executor applies the spec's fallback (section 4) as a ruling: `--bundle` then points at `~/Library/Application Support/sarkos/Sarkos IDE.app`, and Task 6's bundle script also produces the launcher bundle.

---

### Task 1: The App Management probe and the release tool

**Files:**
- Create: `scripts/app-management-probe.sh`, `crates/release-tool/Cargo.toml`, `crates/release-tool/src/main.rs`, `crates/release-tool/src/sig.rs`, `crates/protocol/src/release_key.rs`, `scripts/release-key`
- Modify: `Cargo.toml` (workspace deps), `crates/protocol/src/lib.rs` (`pub mod release_key;`), `crates/protocol/Cargo.toml` (nothing new: the key is a string constant)
- Test: unit tests in `sig.rs`

**Interfaces:**
- Produces: `release_tool::sig::{keygen() -> (String, String) /* (secret b64 64 bytes, public b64 32 bytes) */, sign(secret_b64: &str, msg: &[u8]) -> Result<String, String> /* signature b64 */, verify(public_b64: &str, msg: &[u8], sig_b64: &str) -> Result<(), String>}`; `protocol::release_key::RELEASE_PUBLIC_KEYS: &[&str]`; the CLI `release-tool keygen | sign --key-env NAME FILE | verify --public B64 FILE SIGFILE`.

- [ ] **Step 1: The probe script**

Create `scripts/app-management-probe.sh`:

```sh
#!/bin/sh
# Does macOS let a background process replace an ad-hoc signed app bundle in /Applications by
# two renames while a copy of it runs? Makes "Sarkos Probe.app", runs it, swaps it the way the
# updater will, and reports. Removes everything it made. Run it once on the Mac that matters.
set -eu
APP="/Applications/Sarkos Probe.app"
STAGE="$(mktemp -d)"
cleanup() { rm -rf "$APP" "$APP.old" "$APP.new" "$STAGE"; }
trap cleanup EXIT
make_bundle() { # $1 dir, $2 version
  mkdir -p "$1/Contents/MacOS"
  cat > "$1/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>sh.csarko.sarkos-probe</string>
<key>CFBundleName</key><string>Sarkos Probe</string>
<key>CFBundleExecutable</key><string>probe</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$2</string>
<key>LSUIElement</key><true/>
</dict></plist>
EOF
  printf '#!/bin/sh\necho "probe %s running $$"; sleep 30\n' "$2" > "$1/Contents/MacOS/probe"
  chmod 755 "$1/Contents/MacOS/probe"
  codesign --force --deep --sign - "$1"
}
make_bundle "$APP" 1
make_bundle "$STAGE/Sarkos Probe.app" 2
"$APP/Contents/MacOS/probe" & RUNNING=$!
sleep 1
echo "swapping while pid $RUNNING runs the old bundle"
mv "$STAGE/Sarkos Probe.app" "$APP.new"
if ! mv "$APP" "$APP.old"; then echo "RESULT: first rename refused (App Management)"; exit 1; fi
if ! mv "$APP.new" "$APP"; then echo "RESULT: second rename refused (App Management)"; mv "$APP.old" "$APP"; exit 1; fi
sleep 1
if kill -0 "$RUNNING" 2>/dev/null; then echo "old copy still running: good"; else echo "RESULT: the running copy was killed by the swap"; exit 1; fi
NEW="$("$APP/Contents/MacOS/probe" & sleep 0.5; kill $! 2>/dev/null; wait $! 2>/dev/null || true)"
"$APP/Contents/MacOS/probe" | head -1 | grep -q "probe 2" && echo "RESULT: swap OK, no prompt seen (check System Settings > Privacy > App Management for a new entry)"
kill "$RUNNING" 2>/dev/null || true
```

Make it executable. Run it once: `sh scripts/app-management-probe.sh`. Record in the report: the RESULT line, whether a system prompt or notification appeared, and whether an entry appeared under System Settings > Privacy & Security > App Management. If the result is anything but "swap OK", stop and report it: the executor rules the fallback (ruling 5) before Tasks 4 and 6.

- [ ] **Step 2: Write the failing tests for the signature helpers**

Create `crates/release-tool/Cargo.toml`:

```toml
[package]
name = "release-tool"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[dependencies]
ed25519-compact = { version = "2", default-features = false, features = ["random", "std"] }
base64 = "0.22"
```

Create `crates/release-tool/src/sig.rs`:

```rust
//! ed25519 over raw bytes, base64 on the outside. The secret key is 64 bytes (seed plus public
//! key, ed25519-compact's own layout), the public key 32, the signature 64.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_compact::{KeyPair, PublicKey, SecretKey, Seed, Signature};

pub fn keygen() -> (String, String) {
    let kp = KeyPair::from_seed(Seed::generate());
    (B64.encode(kp.sk.as_ref()), B64.encode(kp.pk.as_ref()))
}

fn decode(label: &str, b64: &str, len: usize) -> Result<Vec<u8>, String> {
    let bytes = B64.decode(b64.trim()).map_err(|e| format!("{label}: not base64: {e}"))?;
    if bytes.len() != len {
        return Err(format!("{label}: {} bytes, expected {len}", bytes.len()));
    }
    Ok(bytes)
}

pub fn sign(secret_b64: &str, msg: &[u8]) -> Result<String, String> {
    let sk = SecretKey::from_slice(&decode("secret key", secret_b64, 64)?).map_err(|e| format!("secret key: {e}"))?;
    Ok(B64.encode(sk.sign(msg, None).as_ref()))
}

pub fn verify(public_b64: &str, msg: &[u8], sig_b64: &str) -> Result<(), String> {
    let pk = PublicKey::from_slice(&decode("public key", public_b64, 32)?).map_err(|e| format!("public key: {e}"))?;
    let sig = Signature::from_slice(&decode("signature", sig_b64, 64)?).map_err(|e| format!("signature: {e}"))?;
    pk.verify(msg, &sig).map_err(|_| "signature does not match".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_round_trips_and_any_change_fails_it() {
        let (sk, pk) = keygen();
        let msg = b"{\"version\":\"0.2.1\"}";
        let sig = sign(&sk, msg).unwrap();
        assert_eq!(verify(&pk, msg, &sig), Ok(()));
        assert!(verify(&pk, b"{\"version\":\"0.2.2\"}", &sig).is_err(), "another message");
        let mut flipped = base64::engine::general_purpose::STANDARD.decode(&sig).unwrap();
        flipped[10] ^= 1;
        assert!(verify(&pk, msg, &base64::engine::general_purpose::STANDARD.encode(flipped)).is_err(), "a flipped byte");
        let (_, other_pk) = keygen();
        assert!(verify(&other_pk, msg, &sig).is_err(), "another key");
        assert!(verify("not base64!", msg, &sig).unwrap_err().contains("not base64"));
        assert!(verify(&pk, msg, "AAAA").unwrap_err().contains("expected 64"));
    }
}
```

- [ ] **Step 3: Run it to see it fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p release-tool`
Expected: FAIL to compile (no `main.rs`; the crate is not in the workspace until `Cargo.toml` is read; `members = ["crates/*"]` picks it up once `Cargo.toml` and `src/main.rs` exist).

- [ ] **Step 4: Implement the CLI and the key constant**

`crates/release-tool/src/main.rs`:

```rust
//! Signs release manifests (auto-update spec, section 3). `keygen` prints a secret key (for the
//! GitHub Actions secret and the password manager) and a public key (for `release_key.rs`);
//! `sign` reads the secret from an environment variable so it never sits on a command line;
//! `verify` is the by-hand check of a published manifest.
mod sig;

use std::process::exit;

fn usage() -> ! {
    eprintln!("usage: release-tool keygen | sign --key-env NAME FILE | verify --public B64 FILE SIGFILE");
    exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["keygen"] => {
            let (sk, pk) = sig::keygen();
            println!("secret: {sk}");
            println!("public: {pk}");
        }
        ["sign", "--key-env", name, file] => {
            let sk = std::env::var(name).unwrap_or_else(|_| {
                eprintln!("release-tool: {name} is not set");
                exit(1)
            });
            let msg = std::fs::read(file).unwrap_or_else(|e| {
                eprintln!("release-tool: {file}: {e}");
                exit(1)
            });
            match sig::sign(&sk, &msg) {
                Ok(s) => println!("{s}"),
                Err(e) => {
                    eprintln!("release-tool: {e}");
                    exit(1)
                }
            }
        }
        ["verify", "--public", pk, file, sigfile] => {
            let msg = std::fs::read(file).unwrap_or_else(|e| {
                eprintln!("release-tool: {file}: {e}");
                exit(1)
            });
            let s = std::fs::read_to_string(sigfile).unwrap_or_else(|e| {
                eprintln!("release-tool: {sigfile}: {e}");
                exit(1)
            });
            match sig::verify(pk, &msg, &s) {
                Ok(()) => println!("ok"),
                Err(e) => {
                    eprintln!("release-tool: {e}");
                    exit(1)
                }
            }
        }
        _ => usage(),
    }
}
```

`crates/protocol/src/release_key.rs`:

```rust
//! The public halves of the keys that may sign a release manifest (auto-update spec, section
//! 3). The IDE accepts a manifest signed by any of them; rotating a key is a new entry here and
//! a release signed by both. Base64 of 32 raw ed25519 bytes.
pub const RELEASE_PUBLIC_KEYS: &[&str] = &[
    // Generated 2026-10-14 by `release-tool keygen`; the secret half is the GitHub Actions
    // secret SARKOS_RELEASE_KEY.
    "REPLACE_WITH_THE_PUBLIC_KEY_FROM_KEYGEN",
];
```

The implementer runs `nice -n 15 ~/.cargo/bin/cargo run -p release-tool -- keygen` once, pastes the `public:` value into the constant, and writes the `secret:` value to `/Users/csarko/Projects/ide-sarkos/.superpowers/sdd/2026-10-14-auto-update/RELEASE_KEY.txt` (the git-ignored workspace; the report names the path and the user moves it into the GitHub secret and their password manager, then deletes the file). The secret never goes into a commit, the report, or the reply.

`scripts/release-key`:

```sh
#!/bin/sh
# Makes a release key pair and prints what goes where. Run once; keep the secret in the GitHub
# Actions secret SARKOS_RELEASE_KEY and your password manager, the public key in
# crates/protocol/src/release_key.rs.
set -eu
cd "$(dirname "$0")/.."
exec nice -n 15 "${CARGO:-$HOME/.cargo/bin/cargo}" run -q -p release-tool -- keygen
```

Add `pub mod release_key;` to `crates/protocol/src/lib.rs`. Add to the workspace `[workspace.dependencies]`: `ed25519-compact = { version = "2", default-features = false, features = ["random", "std"] }`, `base64 = "0.22"`, `sha2 = "0.10"`, `serde_json = "1"` (and switch `crates/sarkos/Cargo.toml`'s `serde_json = "1"` to `serde_json.workspace = true`).

- [ ] **Step 5: Run the tests**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p release-tool -p protocol`
Expected: PASS. Then by hand: `echo hi > /tmp/m && SK=$(... keygen secret) ... sign --key-env SK /tmp/m > /tmp/m.sig && verify --public <pk> /tmp/m /tmp/m.sig` prints `ok` (put the exact commands and output in the report).

- [ ] **Step 6: Commit**

```bash
git add scripts/app-management-probe.sh scripts/release-key crates/release-tool crates/protocol/src/release_key.rs crates/protocol/src/lib.rs Cargo.toml Cargo.lock crates/sarkos/Cargo.toml
git commit -m "Add the release signing tool, the compiled-in release key, and the App Management probe"
```

---

### Task 2: Manifest, versions and verification in the daemon

**Files:**
- Create: `crates/sarkosd/src/updates/mod.rs`, `crates/sarkosd/src/updates/version.rs`, `crates/sarkosd/src/updates/manifest.rs`, `crates/sarkosd/src/updates/verify.rs`
- Modify: `crates/sarkosd/Cargo.toml` (deps), `crates/sarkosd/src/main.rs` (`mod updates;`)
- Test: unit tests in each file

**Interfaces:**
- Consumes: `protocol::release_key::RELEASE_PUBLIC_KEYS`; the signature layout from Task 1 (base64 of 64 raw bytes over the manifest bytes).
- Produces: `updates::version::{Version { major: u64, minor: u64, patch: u64, git: String }, parse_version_string(&str) -> Option<Version> /* "sarkosd 0.1.0 (abc1234)" */, parse_semver(&str) -> Option<(u64,u64,u64)>, Version::is_newer_than(&self, running: &Version) -> bool}`, `updates::manifest::{Manifest { format: u32, version: String, git: String, published: String, arm64: Asset { url: String, size: u64, sha256: String }, min_protocol: u32 }, parse(&[u8]) -> Result<Manifest, String>, Manifest::version(&self) -> Option<Version>}`, `updates::verify::{verify_manifest(bytes: &[u8], sig_b64: &str, keys: &[&str]) -> Result<(), String>, sha256_hex(path: &Path) -> io::Result<String>}`, `updates::MAX_ZIP_BYTES: u64 = 200 * 1024 * 1024`.

- [ ] **Step 1: Write the failing tests**

`crates/sarkosd/src/updates/version.rs`:

```rust
//! Version strings as both binaries print them (`sarkosd 0.1.0 (abc1234)`) and as the manifest
//! carries them, with the ordering the updater uses: semver first, then the git hash as a
//! tie-break (a re-release of the same version with a new hash is an update; the same hash is
//! not).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub git: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_strings_parse_and_compare_semver_then_hash() {
        let running = parse_version_string("sarkosd 0.1.0 (abc1234)").unwrap();
        assert_eq!(running, Version { major: 0, minor: 1, patch: 0, git: "abc1234".into() });
        assert_eq!(parse_version_string("sarkos 1.2.3 (deadbee)\n").map(|v| v.patch), Some(3));
        assert_eq!(parse_version_string("garbage"), None);
        assert_eq!(parse_version_string("sarkosd 0.1 (abc)"), None, "three numbers");
        assert_eq!(parse_semver("0.2.1"), Some((0, 2, 1)));
        assert_eq!(parse_semver("v0.2.1"), None);
        let v = |s: &str, g: &str| Version { git: g.into(), ..parse_version_string(&format!("x {s} (z)")).unwrap() };
        assert!(v("0.2.0", "abc1234").is_newer_than(&running));
        assert!(v("0.1.1", "abc1234").is_newer_than(&running));
        assert!(v("1.0.0", "abc1234").is_newer_than(&running), "major beats minor and patch");
        assert!(!v("0.0.9", "zzzzzzz").is_newer_than(&running), "older semver, whatever the hash");
        assert!(v("0.1.0", "def5678").is_newer_than(&running), "same version, another hash: a re-release");
        assert!(!v("0.1.0", "abc1234").is_newer_than(&running), "the same build");
        assert!(!v("0.1.0", "ABC1234").is_newer_than(&running), "hashes compare case-insensitively");
    }
}
```

`crates/sarkosd/src/updates/manifest.rs`:

```rust
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

#[cfg(test)]
mod tests {
    use super::*;

    pub const SAMPLE: &str = r#"{"format":1,"version":"0.2.1","git":"a1b2c3d","published":"2026-10-15T03:12:00Z","arm64":{"url":"https://github.com/csarkosh/ide-sarkos/releases/download/v0.2.1/Sarkos-IDE-0.2.1-arm64.zip","size":23456789,"sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"},"min_protocol":1}"#;

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
        let newer = parse(SAMPLE.replace("0.2.1\"", "0.2.2\"").as_bytes()).unwrap();
        assert!(newer.version().unwrap().is_newer_than(&running));
    }
}
```

`crates/sarkosd/src/updates/verify.rs`:

```rust
//! The two checks before anything from a download is trusted: the manifest's ed25519 signature
//! against the compiled-in keys, and the zip's sha256 against the manifest.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_compact::{PublicKey, Signature};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_is_accepted_only_with_a_known_keys_signature() {
        let (sk, pk) = test_keygen();
        let msg = b"{\"format\":1}";
        let sig = test_sign(&sk, msg);
        assert_eq!(verify_manifest(msg, &sig, &[&pk]), Ok(()));
        assert_eq!(verify_manifest(msg, &sig, &["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", &pk]), Ok(()), "any listed key");
        assert!(verify_manifest(b"{\"format\":1} ", &sig, &[&pk]).is_err(), "one byte more");
        assert!(verify_manifest(msg, &sig, &[]).is_err(), "no keys");
        assert!(verify_manifest(msg, "AAAA", &[&pk]).is_err());
        assert!(verify_manifest(msg, &sig, &["garbage"]).is_err(), "a malformed key is skipped, not a panic");
    }

    #[test]
    fn sha256_of_a_file_matches_shasum() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_hex(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        std::fs::write(&p, vec![7u8; 3 * 1024 * 1024]).unwrap();
        assert_eq!(sha256_hex(&p).unwrap().len(), 64, "streams a file bigger than one read");
    }

    pub fn test_keygen() -> (String, String) {
        let kp = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::generate());
        (B64.encode(kp.sk.as_ref()), B64.encode(kp.pk.as_ref()))
    }

    pub fn test_sign(sk_b64: &str, msg: &[u8]) -> String {
        let sk = ed25519_compact::SecretKey::from_slice(&B64.decode(sk_b64).unwrap()).unwrap();
        B64.encode(sk.sign(msg, None).as_ref())
    }
}
```

`crates/sarkosd/src/updates/mod.rs` (the skeleton; later tasks add to it):

```rust
//! Automatic updates (auto-update spec, sections 5 to 7): the daemon polls a signed manifest,
//! downloads and verifies the release, stages it, and swaps the app bundle when a window asks.
pub mod manifest;
pub mod verify;
pub mod version;

/// The most a release zip may be, whatever the manifest says.
pub const MAX_ZIP_BYTES: u64 = 200 * 1024 * 1024;
```

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- updates`
Expected: FAIL to compile (`parse`, `parse_version_string`, `verify_manifest` missing).

- [ ] **Step 3: Implement**

`version.rs`:

```rust
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
        let mine = (self.major, self.minor, self.patch);
        let theirs = (running.major, running.minor, running.patch);
        mine > theirs || (mine == theirs && !self.git.eq_ignore_ascii_case(&running.git))
    }
}
```

`manifest.rs`:

```rust
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
```

(`file://` is allowed only so the daemon tests can serve a manifest from disk; the fetcher's host check in Task 3 is what keeps production on github.com.)

`verify.rs`:

```rust
/// Accepts the manifest when any listed key signed exactly these bytes. A key that does not
/// decode is skipped.
pub fn verify_manifest(bytes: &[u8], sig_b64: &str, keys: &[&str]) -> Result<(), String> {
    let sig_bytes = B64.decode(sig_b64.trim()).map_err(|e| format!("signature: not base64: {e}"))?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|_| "signature: not 64 bytes".to_string())?;
    for key in keys {
        let Ok(raw) = B64.decode(key.trim()) else { continue };
        let Ok(pk) = PublicKey::from_slice(&raw) else { continue };
        if pk.verify(bytes, &sig).is_ok() {
            return Ok(());
        }
    }
    Err("signature: no release key signed this manifest".into())
}

/// Streams the file through SHA-256.
pub fn sha256_hex(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
```

`crates/sarkosd/Cargo.toml` gains `serde_json.workspace = true`, `sha2.workspace = true`, `base64.workspace = true`, `ed25519-compact.workspace = true`. `main.rs` gains `mod updates;`.

- [ ] **Step 4: Run the tests and the warning check**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- updates` then `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets 2>&1 | grep -i "^warning"`.
Expected: PASS; items unused until Task 3 carry `#[allow(dead_code)] // used from Task 3`.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkosd/Cargo.toml Cargo.lock crates/sarkosd/src/main.rs crates/sarkosd/src/updates
git commit -m "Parse and verify release manifests in the daemon: versions, signature, sha256"
```

---

### Task 3: Fetch, stage and poll, with the protocol additions

**Files:**
- Create: `crates/sarkosd/src/updates/fetch.rs`, `crates/sarkosd/src/updates/stage.rs`, `crates/sarkosd/src/updates/poll.rs`
- Modify: `crates/sarkosd/src/updates/mod.rs` (`UpdateState`), `crates/protocol/src/lib.rs` (protocol 4), `crates/sarkosd/src/main.rs` (`--update-url`, `--check-every-secs`, `--curl`, `--updates-dir`), `crates/sarkosd/src/server.rs` (`State.update`, the poll thread, `UpdateStatus`, `CheckForUpdates`, the event to clients), `crates/sarkos/src/sessions.rs` (the new `Event` variant in its two `match`es: `stash_if_restoring` returns `false` for it, `group_by_session` skips it)
- Test: unit tests in each new file; `crates/sarkosd/tests/daemon.rs` (`a_staged_update_is_announced_and_listed`); `crates/protocol/src/lib.rs` (variant indices)

**Interfaces:**
- Consumes: Task 2's `manifest::{parse, Manifest}`, `verify::{verify_manifest, sha256_hex}`, `version::{parse_version_string, Version}`, `MAX_ZIP_BYTES`; `protocol::release_key::RELEASE_PUBLIC_KEYS`; `crate::version_string()`.
- Produces: `protocol::{StagedUpdate { version: String, git: String }, Request::{UpdateStatus, CheckForUpdates, ApplyUpdate { version: String }}, Response::UpdateStatus { staged: Option<StagedUpdate>, last_error: Option<String> }, Event::UpdateStaged { update: StagedUpdate }, PROTOCOL_VERSION = 4}`; `updates::fetch::{Fetcher { base: String, curl: PathBuf }, Fetcher::fetch_manifest(&self, dir: &Path) -> Result<(Vec<u8>, String), String> /* (manifest bytes, signature b64) */, Fetcher::download(&self, url: &str, size: u64, dest: &Path) -> Result<(), String>, allowed_final_url(&str) -> bool, curl_args(url, dest, max_bytes, max_secs) -> Vec<String>}`; `updates::stage::{Staged { version: String, git: String, bundle: PathBuf }, stage(dir: &Path, m: &Manifest, zip: &Path, ditto: &Path) -> Result<Staged, String>, check_bundle(bundle: &Path, want_version: &str) -> Result<(), String>, clean_except(dir: &Path, keep: Option<&Path>)}`; `updates::poll::{run_check(fetcher, dir, keys, running: &Version, bad: &mut HashSet<String>, ditto) -> Outcome::{Nothing, Staged(Staged), Failed(String)}, start(state: Shared, fetcher, dir, every: Duration)}`; `updates::UpdateState { staged: Option<Staged>, last_error: Option<String>, checking: bool, bad: HashSet<String>, check_now: bool }`; `State.update: UpdateState`; the daemon arguments `--update-url URL` (default `https://github.com/csarkosh/ide-sarkos/releases/latest/download/`), `--check-every-secs N` (default 300; 0 disables polling), `--curl PATH` (default `/usr/bin/curl`), `--ditto PATH` (default `/usr/bin/ditto`), `--updates-dir PATH` (default `<socket dir>/updates`).

- [ ] **Step 1: Write the failing tests**

Protocol, in `new_variants_are_appended_after_the_old_ones` (lib.rs): change `assert_eq!(PROTOCOL_VERSION, 3)` to 4 and add

```rust
        assert_eq!(postcard::to_allocvec(&Request::UpdateStatus).unwrap()[0], 16);
        assert_eq!(postcard::to_allocvec(&Request::CheckForUpdates).unwrap()[0], 17);
        assert_eq!(postcard::to_allocvec(&Request::ApplyUpdate { version: "0.2.1".into() }).unwrap()[0], 18);
        let staged = StagedUpdate { version: "0.2.1".into(), git: "a1b2c3d".into() };
        assert_eq!(postcard::to_allocvec(&Response::UpdateStatus { staged: Some(staged.clone()), last_error: None }).unwrap()[0], 7);
        assert_eq!(postcard::to_allocvec(&Event::UpdateStaged { update: staged }).unwrap()[0], 3);
```

`fetch.rs` tests:

```rust
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
```

`stage.rs` tests (a fake bundle: directories plus two shell scripts; `fake_bundle(dir, version)` writes `Contents/Info.plist` with `CFBundleShortVersionString`, `Contents/MacOS/sarkos` and `sarkosd` scripts that print `sarkosd <version> (test)` for `--version`):

```rust
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::updates::manifest::{Asset, Manifest};

    pub fn fake_bundle(dir: &Path, version: &str, git: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let app = dir.join("Sarkos IDE.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), format!("<plist><dict><key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>")).unwrap();
        for name in ["sarkos", "sarkosd"] {
            let p = app.join("Contents/MacOS").join(name);
            std::fs::write(&p, format!("#!/bin/sh\n[ \"$1\" = --version ] && echo '{name} {version} ({git})'\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        app
    }

    pub fn zip_of(app: &Path, dest: &Path) {
        let st = std::process::Command::new("/usr/bin/ditto").args(["-c", "-k", "--keepParent"]).arg(app).arg(dest).status().unwrap();
        assert!(st.success());
    }

    pub fn manifest_for(zip: &Path, version: &str, git: &str) -> Manifest {
        Manifest { format: 1, version: version.into(), git: git.into(), published: String::new(), arm64: Asset { url: format!("file://{}", zip.display()), size: std::fs::metadata(zip).unwrap().len(), sha256: crate::updates::verify::sha256_hex(zip).unwrap() }, min_protocol: 1 }
    }

    #[test]
    fn a_zip_is_unpacked_checked_and_probed() {
        let src = tempfile::tempdir().unwrap();
        let app = fake_bundle(src.path(), "0.2.1", "abc1234");
        let zip = src.path().join("b.zip");
        zip_of(&app, &zip);
        let m = manifest_for(&zip, "0.2.1", "abc1234");
        let dir = tempfile::tempdir().unwrap();
        let s = stage(dir.path(), &m, &zip, Path::new("/usr/bin/ditto")).unwrap();
        assert_eq!(s.version, "0.2.1");
        assert_eq!(s.bundle, dir.path().join("0.2.1").join("Sarkos IDE.app"));
        assert!(s.bundle.join("Contents/MacOS/sarkosd").is_file());
        // The wrong version inside: refused and cleaned.
        let wrong = manifest_for(&zip, "0.2.2", "abc1234");
        assert!(stage(dir.path(), &wrong, &zip, Path::new("/usr/bin/ditto")).unwrap_err().contains("0.2.1"));
        assert!(!dir.path().join("0.2.2").exists());
    }

    #[test]
    fn check_bundle_wants_both_executables_and_the_version() {
        let dir = tempfile::tempdir().unwrap();
        let app = fake_bundle(dir.path(), "0.2.1", "x");
        assert_eq!(check_bundle(&app, "0.2.1"), Ok(()));
        assert!(check_bundle(&app, "0.3.0").unwrap_err().contains("0.2.1"));
        std::fs::remove_file(app.join("Contents/MacOS/sarkos")).unwrap();
        assert!(check_bundle(&app, "0.2.1").unwrap_err().contains("sarkos"));
    }

    #[test]
    fn clean_except_keeps_only_the_staged_version() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("0.1.9")).unwrap();
        std::fs::create_dir_all(dir.path().join("0.2.1")).unwrap();
        std::fs::write(dir.path().join("manifest.json"), b"x").unwrap();
        clean_except(dir.path(), Some(&dir.path().join("0.2.1")));
        assert!(!dir.path().join("0.1.9").exists());
        assert!(dir.path().join("0.2.1").exists());
        assert!(dir.path().join("manifest.json").exists(), "files at the top stay");
        clean_except(dir.path(), None);
        assert!(!dir.path().join("0.2.1").exists());
    }
}
```

`poll.rs` tests (`run_check` end to end over `file://` with a real signature):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::updates::stage::tests::{fake_bundle, manifest_for, zip_of};
    use crate::updates::verify::tests::{test_keygen, test_sign};

    struct Site {
        _dir: tempfile::TempDir,
        base: String,
        root: PathBuf,
        sk: String,
        pk: String,
    }

    fn publish(site: &Site, version: &str, git: &str, tamper: bool) {
        let app = fake_bundle(&site.root.join("src"), version, git);
        let zip = site.root.join("bundle.zip");
        zip_of(&app, &zip);
        let mut m = manifest_for(&zip, version, git);
        if tamper {
            std::fs::write(&zip, b"not the zip at all, same length? no").unwrap();
            let _ = m.arm64.size;
        }
        let json = serde_json::json!({"format": 1, "version": m.version, "git": m.git, "published": "", "arm64": {"url": m.arm64.url, "size": m.arm64.size, "sha256": m.arm64.sha256}, "min_protocol": 1}).to_string();
        std::fs::write(site.root.join("manifest.json"), &json).unwrap();
        std::fs::write(site.root.join("manifest.json.sig"), test_sign(&site.sk, json.as_bytes())).unwrap();
        std::fs::remove_dir_all(site.root.join("src")).unwrap();
    }

    fn site() -> Site {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("site");
        std::fs::create_dir_all(&root).unwrap();
        let (sk, pk) = test_keygen();
        Site { base: format!("file://{}/", root.display()), root, _dir: dir, sk, pk }
    }

    fn fetcher(s: &Site) -> Fetcher {
        Fetcher { base: s.base.clone(), curl: PathBuf::from("/usr/bin/curl") }
    }

    #[test]
    fn a_newer_signed_release_is_staged_and_the_same_one_is_not_staged_twice() {
        let s = site();
        publish(&s, "0.2.1", "abc1234", false);
        let dir = tempfile::tempdir().unwrap();
        let running = Version { major: 0, minor: 2, patch: 0, git: "zzz".into() };
        let mut bad = HashSet::new();
        match run_check(&fetcher(&s), dir.path(), &[&s.pk], &running, &mut bad, Path::new("/usr/bin/ditto"), None) {
            Outcome::Staged(st) => assert_eq!(st.version, "0.2.1"),
            other => panic!("{other:?}"),
        }
        let staged = dir.path().join("0.2.1").join("Sarkos IDE.app");
        assert!(matches!(run_check(&fetcher(&s), dir.path(), &[&s.pk], &running, &mut bad, Path::new("/usr/bin/ditto"), Some(&staged)), Outcome::Nothing), "already staged: nothing to do");
        let newer = Version { major: 0, minor: 2, patch: 1, git: "abc1234".into() };
        assert!(matches!(run_check(&fetcher(&s), dir.path(), &[&s.pk], &newer, &mut bad, Path::new("/usr/bin/ditto"), None), Outcome::Nothing), "running it already");
    }

    #[test]
    fn a_bad_signature_stages_nothing_and_reads_nothing_else() {
        let s = site();
        publish(&s, "0.2.1", "abc1234", false);
        let (_, other_pk) = test_keygen();
        let dir = tempfile::tempdir().unwrap();
        let running = Version { major: 0, minor: 1, patch: 0, git: "x".into() };
        let mut bad = HashSet::new();
        match run_check(&fetcher(&s), dir.path(), &[&other_pk], &running, &mut bad, Path::new("/usr/bin/ditto"), None) {
            Outcome::Failed(e) => assert!(e.contains("signature"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(!dir.path().join("0.2.1").exists());
        assert!(!dir.path().join("bundle.zip").exists(), "no download after a bad signature");
    }

    #[test]
    fn a_zip_with_the_wrong_hash_is_deleted_and_the_version_marked_bad() {
        let s = site();
        publish(&s, "0.2.1", "abc1234", true);
        let dir = tempfile::tempdir().unwrap();
        let running = Version { major: 0, minor: 1, patch: 0, git: "x".into() };
        let mut bad = HashSet::new();
        match run_check(&fetcher(&s), dir.path(), &[&s.pk], &running, &mut bad, Path::new("/usr/bin/ditto"), None) {
            Outcome::Failed(e) => assert!(e.contains("sha256") || e.contains("size"), "{e}"),
            other => panic!("{other:?}"),
        }
        assert!(bad.contains("0.2.1 abc1234"));
        assert!(!dir.path().join("0.2.1").exists());
        assert!(matches!(run_check(&fetcher(&s), dir.path(), &[&s.pk], &running, &mut bad, Path::new("/usr/bin/ditto"), None), Outcome::Nothing), "marked bad: not fetched again");
    }

    #[test]
    fn a_final_url_off_github_is_refused() {
        // `file://` cannot redirect, so this pins the pure check the download applies to curl's
        // reported final url before the file is read.
        assert!(!crate::updates::fetch::allowed_final_url("https://cdn.evil.example/Sarkos.zip"));
        assert!(crate::updates::fetch::allowed_final_url("https://objects.githubusercontent.com/a"));
    }
}
```

Daemon integration (`crates/sarkosd/tests/daemon.rs`), near the recover tests. It needs the test to publish a signed site, but the daemon only trusts `RELEASE_PUBLIC_KEYS`; so the daemon takes `--release-key B64` (test only, documented as such) to add a key:

```rust
#[test]
fn a_staged_update_is_announced_and_listed() {
    // A site on disk, signed with a throwaway key the daemon is told to trust.
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    let zip = publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk]);
    let mut c = d.connect();
    // Before the first check lands: nothing staged.
    let staged = loop {
        match c.request(Request::UpdateStatus) {
            Response::UpdateStatus { staged: Some(s), .. } => break s,
            Response::UpdateStatus { staged: None, last_error } => {
                assert!(last_error.is_none(), "{last_error:?}");
                std::thread::sleep(Duration::from_millis(100));
            }
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(staged.version, "9.9.9");
    let _ = zip;
    // A second client hears the event when the next check restages nothing, so ask for a check and
    // expect no second event; the status says staged.
    assert_eq!(c.request(Request::CheckForUpdates), Response::Ok);
    assert!(matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }));
}
```

with helpers `test_release_key()` (the same two lines as `verify.rs`'s test helpers, using `ed25519-compact` and `base64` as dev-dependencies of sarkosd) and `publish_fake_release(dir, version, git, sk) -> PathBuf` (the fake bundle, `ditto` zip, manifest JSON, signature; the same shape as `poll.rs`'s `publish`). And a test that the event reaches a connected client: `a_connected_window_hears_update_staged`: connect first, then let the 1 s check run, read frames until `ServerMessage::Event(Event::UpdateStaged { update })` with `update.version == "9.9.9"`.

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p protocol` then `nice -n 15 ~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- updates`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

Protocol (`lib.rs`): `PROTOCOL_VERSION = 4` with the doc line "Version 4 adds the update status, check and apply requests and the staged event."; `pub struct StagedUpdate { pub version: String, pub git: String }` (derive the usual); append `Request::UpdateStatus`, `Request::CheckForUpdates`, `Request::ApplyUpdate { version: String }`; append `Response::UpdateStatus { staged: Option<StagedUpdate>, last_error: Option<String> }`; append `Event::UpdateStaged { update: StagedUpdate }`.

`fetch.rs`:

```rust
//! Downloads through the system curl (TLS and proxies are macOS's business), never into memory
//! past what the manifest says, and never from a host that is not GitHub's.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
```

(`curl_args` ends with `--` then the url, so the test's "last is the url" holds and a url starting with `-` cannot become an option.) `Fetcher::get(&self, url, dest, max_bytes, max_secs) -> Result<(), String>`: runs `self.curl` with `curl_args`, `stdin` null, `stdout` piped, `stderr` piped, a minimal env (`env_clear()` then `PATH=/usr/bin:/bin`, `HOME`); on a non-zero exit returns `Err(format!("curl: {stderr trimmed}"))` and removes `dest`; reads stdout as the final url and, if `!allowed_final_url`, removes `dest` and returns `Err("refused: the download ended at <url>")`. `fetch_manifest(dir)`: `get(base + "manifest.json", dir/manifest.json, MANIFEST_MAX, 20)` then the `.sig` (MANIFEST_MAX, 20); returns the bytes and the trimmed signature text; errors name the file. `download(url, size, dest)`: `get(url, dest, size, 600)` then checks `metadata(dest).len() == size` else removes and errors `size`.

`stage.rs`:

```rust
//! Unpacking a verified zip into the staging folder and checking what came out before any window
//! hears about it.
use super::manifest::Manifest;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    pub version: String,
    pub git: String,
    pub bundle: PathBuf,
}

pub const BUNDLE_NAME: &str = "Sarkos IDE.app";

/// `<dir>/<version>/Sarkos IDE.app` from the zip, or an error with the folder removed.
pub fn stage(dir: &Path, m: &Manifest, zip: &Path, ditto: &Path) -> Result<Staged, String> {
    let into = dir.join(&m.version);
    let _ = std::fs::remove_dir_all(&into);
    std::fs::create_dir_all(&into).map_err(|e| format!("staging: {e}"))?;
    let result = (|| {
        let st = Command::new(ditto).args(["-x", "-k"]).arg(zip).arg(&into).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| format!("ditto: {e}"))?;
        if !st.status.success() {
            return Err(format!("ditto: {}", String::from_utf8_lossy(&st.stderr).trim()));
        }
        let bundle = into.join(BUNDLE_NAME);
        check_bundle(&bundle, &m.version)?;
        Ok(Staged { version: m.version.clone(), git: m.git.clone(), bundle })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&into);
    }
    result
}

/// Both executables present and runnable, and the daemon inside says the manifest's version.
pub fn check_bundle(bundle: &Path, want_version: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    for name in ["sarkos", "sarkosd"] {
        let p = bundle.join("Contents/MacOS").join(name);
        let meta = std::fs::metadata(&p).map_err(|e| format!("{name}: {e}"))?;
        if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
            return Err(format!("{name}: not an executable file"));
        }
    }
    let said = probe_version(&bundle.join("Contents/MacOS/sarkosd"), Duration::from_secs(2)).ok_or("the staged sarkosd did not print its version")?;
    match super::version::parse_version_string(&said) {
        Some(v) if format!("{}.{}.{}", v.major, v.minor, v.patch) == want_version => Ok(()),
        _ => Err(format!("the staged sarkosd says {said:?}, the manifest {want_version}")),
    }
}

/// `exe --version`, within `timeout`, else None (the child is killed).
pub fn probe_version(exe: &Path, timeout: Duration) -> Option<String> {
    let mut child = Command::new(exe).arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    use std::io::Read;
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// Removes every version folder in `dir` except `keep`'s parent; files at the top level stay.
pub fn clean_except(dir: &Path, keep: Option<&Path>) {
    let keep_dir = keep.and_then(|k| k.parent().map(Path::to_path_buf));
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() && keep_dir.as_deref() != Some(p.as_path()) {
            let _ = std::fs::remove_dir_all(&p);
        }
    }
}
```

`poll.rs`:

```rust
//! The check: manifest, signature, comparison, download, hash, stage. Pure apart from the
//! processes it runs, so the tests drive it over `file://`.
use super::fetch::Fetcher;
use super::stage::{stage, Staged};
use super::version::Version;
use super::{manifest, verify, MAX_ZIP_BYTES};
use crate::server::Shared;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug)]
pub enum Outcome {
    Nothing,
    Staged(Staged),
    Failed(String),
}

fn bad_key(version: &str, git: &str) -> String {
    format!("{version} {git}")
}

/// One check. `staged` is what is already staged (its bundle path), so the same release is not
/// fetched twice. A failure after the signature marks the version bad.
pub fn run_check(f: &Fetcher, dir: &Path, keys: &[&str], running: &Version, bad: &mut HashSet<String>, ditto: &Path, staged: Option<&Path>) -> Outcome {
    if let Err(e) = std::fs::create_dir_all(dir) {
        return Outcome::Failed(format!("updates dir: {e}"));
    }
    let (bytes, sig) = match f.fetch_manifest(dir) {
        Ok(x) => x,
        Err(e) => return Outcome::Failed(e),
    };
    if let Err(e) = verify::verify_manifest(&bytes, &sig, keys) {
        return Outcome::Failed(e);
    }
    let m = match manifest::parse(&bytes) {
        Ok(m) => m,
        Err(e) => return Outcome::Failed(e),
    };
    let Some(v) = m.version() else { return Outcome::Failed("manifest: bad version".into()) };
    if !v.is_newer_than(running) || bad.contains(&bad_key(&m.version, &m.git)) {
        return Outcome::Nothing;
    }
    if staged.is_some_and(|s| s.parent().and_then(|p| p.file_name()) == Some(std::ffi::OsStr::new(&m.version))) {
        return Outcome::Nothing;
    }
    if m.arm64.size > MAX_ZIP_BYTES {
        bad.insert(bad_key(&m.version, &m.git));
        return Outcome::Failed(format!("release {} is {} bytes, over the {MAX_ZIP_BYTES} cap", m.version, m.arm64.size));
    }
    let zip = dir.join(format!("{}.zip", m.version));
    let result = (|| {
        f.download(&m.arm64.url, m.arm64.size, &zip)?;
        let got = verify::sha256_hex(&zip).map_err(|e| format!("sha256: {e}"))?;
        if !got.eq_ignore_ascii_case(&m.arm64.sha256) {
            return Err(format!("sha256 of the download is {got}, the manifest says {}", m.arm64.sha256));
        }
        stage(dir, &m, &zip, ditto)
    })();
    let _ = std::fs::remove_file(&zip);
    match result {
        Ok(s) => {
            super::stage::clean_except(dir, Some(&s.bundle));
            Outcome::Staged(s)
        }
        Err(e) => {
            bad.insert(bad_key(&m.version, &m.git));
            Outcome::Failed(e)
        }
    }
}

/// The poll thread: every `every`, or sooner when a window asked (`check_now`).
pub fn start(state: Shared, f: Fetcher, dir: PathBuf, keys: Vec<String>, every: Duration, ditto: PathBuf) {
    std::thread::Builder::new().name("updates".into()).spawn(move || {
        let running = super::version::parse_version_string(&crate::version_string()).expect("our own version string parses");
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        let mut bad = HashSet::new();
        let mut last = None::<std::time::Instant>;
        loop {
            let due = {
                let mut st = state.lock().unwrap();
                let due = st.update.check_now || last.map_or(true, |t| t.elapsed() >= every);
                st.update.check_now = false;
                if due { st.update.checking = true; }
                due
            };
            if due {
                last = Some(std::time::Instant::now());
                let staged = state.lock().unwrap().update.staged.as_ref().map(|s| s.bundle.clone());
                let outcome = run_check(&f, &dir, &keys, &running, &mut bad, &ditto, staged.as_deref());
                let mut st = state.lock().unwrap();
                st.update.checking = false;
                match outcome {
                    Outcome::Nothing => {}
                    Outcome::Staged(s) => {
                        st.update.last_error = None;
                        let update = protocol::StagedUpdate { version: s.version.clone(), git: s.git.clone() };
                        st.update.staged = Some(s);
                        crate::server::broadcast(&st, protocol::Event::UpdateStaged { update });
                    }
                    Outcome::Failed(e) => {
                        eprintln!("sarkosd: update check: {e}");
                        st.update.last_error = Some(e);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }).expect("spawn the updates thread");
}
```

`mod.rs` gains:

```rust
#[derive(Default)]
pub struct UpdateState {
    pub staged: Option<stage::Staged>,
    pub last_error: Option<String>,
    pub checking: bool,
    pub check_now: bool,
}
```

`server.rs`: `State` gains `pub update: crate::updates::UpdateState` (Default); `pub fn broadcast(st: &State, event: Event)` sends `ServerMessage::Event(event)` to every connected client's `tx` (read how `Session::subscribe` and the `ClientSink` send: use the same call; a client whose queue refuses is skipped); `Options` gains `update_url: String, check_every: Duration, curl: PathBuf, ditto: PathBuf, updates_dir: PathBuf, release_keys: Vec<String>`; `run_with` and `run_resumed` start `updates::poll::start` when `check_every` is not zero (keys: `RELEASE_PUBLIC_KEYS` plus `--release-key` extras); `dispatch` gains:

```rust
        Request::UpdateStatus => {
            let st = state.lock().unwrap();
            Response::UpdateStatus { staged: st.update.staged.as_ref().map(|s| StagedUpdate { version: s.version.clone(), git: s.git.clone() }), last_error: st.update.last_error.clone() }
        }
        Request::CheckForUpdates => {
            state.lock().unwrap().update.check_now = true;
            Response::Ok
        }
        Request::ApplyUpdate { .. } => Response::Error("not yet".into()), // Task 4
```

`main.rs`: the five arguments (`--update-url`, `--check-every-secs`, `--curl`, `--ditto`, `--updates-dir`, `--release-key` which may repeat) into `Options`; defaults as in the Interfaces block; `--updates-dir` default `socket.parent()/updates`. `run_resumed` takes the same options: read how `run_resumed(h, socket)` builds its `Options` today (the handover carries them or it rebuilds defaults) and pass the update fields through the same way the socket is.

`sessions.rs` (window): the two `match`es over `Event` gain `Event::UpdateStaged { .. } => return false` (in `stash_if_restoring`) and are skipped in `group_by_session` (`filter_map` on session-bearing events) and ignored in `note_events`; `terminal.rs`'s `handle_events` (if it matches exhaustively) ignores it too.

- [ ] **Step 4: Run the tests**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p protocol`, `nice -n 15 ~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- updates`, `nice -n 15 ~/.cargo/bin/cargo test -p sarkosd --test daemon -- update`, then `nice -n 15 ~/.cargo/bin/cargo test --workspace` once and the warning check.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/protocol/src/lib.rs crates/sarkosd/Cargo.toml Cargo.lock crates/sarkosd/src/main.rs crates/sarkosd/src/server.rs crates/sarkosd/src/updates crates/sarkosd/tests/daemon.rs crates/sarkos/src/sessions.rs crates/sarkos/src/terminal.rs
git commit -m "Fetch, verify and stage signed releases in the daemon, and tell windows one is staged"
```

---

### Task 4: Swap the bundle, apply, and roll back

**Files:**
- Create: `crates/sarkosd/src/updates/swap.rs`, `crates/sarkosd/src/updates/apply.rs`
- Modify: `crates/sarkosd/src/updates/mod.rs`, `crates/sarkosd/src/server.rs` (`ApplyUpdate`, the rollback watch at start, `.old` cleanup on connect), `crates/sarkosd/src/main.rs` (`--bundle`), `crates/sarkosd/src/recovery.rs` (leftover cleanup on `--recover`)
- Test: unit tests in `swap.rs` and `apply.rs`; `crates/sarkosd/tests/daemon.rs` (`apply_update_swaps_the_bundle_and_the_new_window_confirms_it`, `a_window_that_never_connects_rolls_the_bundle_back`)

**Interfaces:**
- Consumes: Task 3's `UpdateState`, `Staged`, `Request::ApplyUpdate { version }`; `watchdog::launch_ui(exe, cwd)`; `State.windows`.
- Produces: `updates::swap::{swap_in(bundle: &Path, new: &Path) -> Result<(), String>, old_of(bundle) -> PathBuf /* "<bundle>.old" */, finish(bundle) -> io::Result<()> /* removes .old */, rollback(bundle) -> Result<(), String> /* .old back into place */, cleanup_leftovers(bundle) /* removes .old and .new */}`, `updates::apply::{Applied { version: String, old_kept: bool, at_unix: u64 }, marker_path(updates_dir) -> PathBuf /* applied.json */, write_marker, read_marker, CONNECT_GRACE: Duration = 20 s, decide(marker: &Applied, connected_since_start: bool, now_unix: u64) -> Decision::{Wait, Confirm, Rollback}}`, the daemon argument `--bundle PATH` (default: the bundle containing the running `sarkosd`, found as `current_exe().ancestors().nth(3)` when that path ends in `.app`; `None` otherwise, which refuses `ApplyUpdate` with "not running from an app bundle").

- [ ] **Step 1: Write the failing tests**

`swap.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(dir: &Path, name: &str, tag: &str) -> PathBuf {
        let b = dir.join(name);
        std::fs::create_dir_all(b.join("Contents/MacOS")).unwrap();
        std::fs::write(b.join("Contents/MacOS/sarkosd"), tag).unwrap();
        b
    }

    #[test]
    fn the_swap_is_two_renames_and_keeps_the_old_bundle_until_finish() {
        let dir = tempfile::tempdir().unwrap();
        let installed = bundle(dir.path(), "Sarkos IDE.app", "old");
        let new = bundle(&dir.path().join("staging"), "Sarkos IDE.app", "new");
        swap_in(&installed, &new).unwrap();
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/sarkosd")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(old_of(&installed).join("Contents/MacOS/sarkosd")).unwrap(), "old");
        assert!(!new.exists(), "moved, not copied");
        finish(&installed).unwrap();
        assert!(!old_of(&installed).exists());
    }

    #[test]
    fn rollback_puts_the_old_bundle_back() {
        let dir = tempfile::tempdir().unwrap();
        let installed = bundle(dir.path(), "Sarkos IDE.app", "old");
        let new = bundle(&dir.path().join("staging"), "Sarkos IDE.app", "new");
        swap_in(&installed, &new).unwrap();
        rollback(&installed).unwrap();
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/sarkosd")).unwrap(), "old");
        assert!(!old_of(&installed).exists());
        assert!(rollback(&installed).is_err(), "nothing to roll back to");
    }

    #[test]
    fn a_failed_second_rename_restores_the_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let installed = bundle(dir.path(), "Sarkos IDE.app", "old");
        // The new bundle is on another "volume" for rename: a path that does not exist makes the
        // second rename fail after the first succeeded.
        let missing = dir.path().join("staging/Sarkos IDE.app");
        let err = swap_in(&installed, &missing).unwrap_err();
        assert!(err.contains("rename"), "{err}");
        assert_eq!(std::fs::read_to_string(installed.join("Contents/MacOS/sarkosd")).unwrap(), "old", "restored from .old");
        assert!(!old_of(&installed).exists());
    }

    #[test]
    fn a_stale_old_or_new_is_removed_first() {
        let dir = tempfile::tempdir().unwrap();
        let installed = bundle(dir.path(), "Sarkos IDE.app", "old");
        bundle(dir.path(), "Sarkos IDE.app.old", "stale");
        bundle(dir.path(), "Sarkos IDE.app.new", "stale");
        cleanup_leftovers(&installed);
        assert!(!old_of(&installed).exists());
        assert!(!dir.path().join("Sarkos IDE.app.new").exists());
        assert!(installed.exists());
    }
}
```

`apply.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_round_trips_and_the_decision_waits_then_rolls_back() {
        let dir = tempfile::tempdir().unwrap();
        let m = Applied { version: "0.2.1".into(), old_kept: true, at_unix: 1_000 };
        write_marker(dir.path(), &m).unwrap();
        assert_eq!(read_marker(dir.path()), Some(m.clone()));
        assert_eq!(decide(&m, false, 1_005), Decision::Wait);
        assert_eq!(decide(&m, true, 1_005), Decision::Confirm, "a window connected: keep the new build");
        assert_eq!(decide(&m, false, 1_000 + CONNECT_GRACE.as_secs()), Decision::Rollback);
        assert_eq!(decide(&m, false, 900), Decision::Wait, "a clock that went backwards waits rather than rolling back");
        clear_marker(dir.path());
        assert_eq!(read_marker(dir.path()), None);
        std::fs::write(marker_path(dir.path()), b"junk").unwrap();
        assert_eq!(read_marker(dir.path()), None, "an unreadable marker is no marker");
    }
}
```

Daemon integration tests, with the fake bundle from Task 3's helpers (`publish_fake_release`), a `--bundle` pointing at an installed fake bundle whose `sarkos` script writes a marker file when launched, and `--update-url` on a site holding a newer fake release:

```rust
#[test]
fn apply_update_swaps_the_bundle_and_the_new_window_confirms_it() {
    let site = tempfile::tempdir().unwrap();
    let (sk, pk) = test_release_key();
    publish_fake_release(site.path(), "9.9.9", "fffffff", &sk);
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "0.0.1", "0000000"); // its sarkos script appends "launched" to <home>/launches
    let d = Daemon::start(&["--update-url", &format!("file://{}/", site.path().display()), "--check-every-secs", "1", "--release-key", &pk, "--bundle", installed.to_str().unwrap()]);
    let mut c = d.connect();
    wait_until(|| matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: Some(_), .. }), Duration::from_secs(10));
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Ok);
    assert!(std::fs::read_to_string(installed.join("Contents/Info.plist")).unwrap().contains("9.9.9"), "the new bundle is in place");
    assert!(installed.with_extension("app.old").exists(), "the old one is kept until a window confirms");
    assert!(d.updates_marker().exists());
    // The window's Upgrade and RelaunchClient follow in production; here a second Hello stands in
    // for the relaunched window.
    let _c2 = d.connect();
    wait_until(|| !installed.with_extension("app.old").exists(), Duration::from_secs(5));
    assert!(!d.updates_marker().exists());
    assert_eq!(c.request(Request::ApplyUpdate { version: "9.9.9".into() }), Response::Error("nothing staged".into()), "applied once");
}

#[test]
fn a_window_that_never_connects_rolls_the_bundle_back() {
    // The marker says an update was applied 25 s ago and nobody has connected: a daemon starting
    // now rolls back and launches the old window in each remembered project.
    let home = tempfile::tempdir().unwrap();
    let installed = fake_installed_bundle(home.path(), "9.9.9", "fffffff");
    let old = fake_bundle_at(&home.path().join("Sarkos IDE.app.old"), "0.0.1", "0000000");
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("updates")).unwrap();
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() - 25;
    std::fs::write(dir.path().join("updates/applied.json"), format!("{{\"version\":\"9.9.9\",\"old_kept\":true,\"at_unix\":{at}}}")).unwrap();
    let d = Daemon::start_in(dir, &["--check-every-secs", "0", "--bundle", installed.to_str().unwrap()]);
    wait_until(|| std::fs::read_to_string(installed.join("Contents/Info.plist")).unwrap().contains("0.0.1"), Duration::from_secs(5));
    assert!(!old.exists(), "renamed back");
    assert!(!d.updates_marker().exists());
    let mut c = d.connect();
    assert!(matches!(c.request(Request::UpdateStatus), Response::UpdateStatus { staged: None, last_error: Some(e) } if e.contains("rolled back")));
}
```

(`Daemon::updates_marker(&self)` is `self.socket.parent().join("updates/applied.json")`; `wait_until(f, timeout)` polls every 50 ms and panics at the deadline; `fake_installed_bundle` and `fake_bundle_at` build the same shape as Task 3's fake bundle, with `sarkos` appending a line to `<home>/launches`.)

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- updates::swap updates::apply`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`swap.rs`:

```rust
//! The bundle swap (auto-update spec, section 4): two renames on one volume, the old bundle kept
//! as `<bundle>.old` until a window confirms the new one runs.
use std::path::{Path, PathBuf};

pub fn old_of(bundle: &Path) -> PathBuf {
    with_suffix(bundle, ".old")
}

fn new_of(bundle: &Path) -> PathBuf {
    with_suffix(bundle, ".new")
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// Removes a stale `.old` or `.new` left by an interrupted swap. The installed bundle is never
/// touched.
pub fn cleanup_leftovers(bundle: &Path) {
    let _ = std::fs::remove_dir_all(old_of(bundle));
    let _ = std::fs::remove_dir_all(new_of(bundle));
}

/// `bundle` becomes `<bundle>.old` and `new` becomes `bundle`. If the second rename fails the
/// first is undone, so the app is never missing.
pub fn swap_in(bundle: &Path, new: &Path) -> Result<(), String> {
    cleanup_leftovers(bundle);
    let old = old_of(bundle);
    std::fs::rename(bundle, &old).map_err(|e| format!("rename {} to .old: {e}", bundle.display()))?;
    if let Err(e) = std::fs::rename(new, bundle) {
        let undo = std::fs::rename(&old, bundle);
        return Err(match undo {
            Ok(()) => format!("rename {} into place: {e} (the old bundle is back)", new.display()),
            Err(u) => format!("rename {} into place: {e}; and putting the old bundle back failed too: {u}", new.display()),
        });
    }
    Ok(())
}

pub fn finish(bundle: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(old_of(bundle)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        r => r,
    }
}

/// Puts `<bundle>.old` back in place of `bundle` (which is removed).
pub fn rollback(bundle: &Path) -> Result<(), String> {
    let old = old_of(bundle);
    if !old.is_dir() {
        return Err("no previous bundle to roll back to".into());
    }
    let failed = new_of(bundle);
    let _ = std::fs::remove_dir_all(&failed);
    std::fs::rename(bundle, &failed).map_err(|e| format!("move the failed bundle aside: {e}"))?;
    std::fs::rename(&old, bundle).map_err(|e| format!("rename .old back: {e}"))?;
    let _ = std::fs::remove_dir_all(&failed);
    Ok(())
}
```

`apply.rs`:

```rust
//! What happens on `ApplyUpdate` and at every daemon start afterwards: the marker that says a
//! swap happened, and the decision to keep it (a window connected) or roll it back (none did
//! within the grace).
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const CONNECT_GRACE: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    pub version: String,
    pub old_kept: bool,
    pub at_unix: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Wait,
    Confirm,
    Rollback,
}

pub fn marker_path(updates_dir: &Path) -> PathBuf {
    updates_dir.join("applied.json")
}

pub fn write_marker(updates_dir: &Path, m: &Applied) -> std::io::Result<()> {
    std::fs::create_dir_all(updates_dir)?;
    let tmp = updates_dir.join("applied.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(m).expect("serializes"))?;
    std::fs::rename(tmp, marker_path(updates_dir))
}

pub fn read_marker(updates_dir: &Path) -> Option<Applied> {
    serde_json::from_slice(&std::fs::read(marker_path(updates_dir)).ok()?).ok()
}

pub fn clear_marker(updates_dir: &Path) {
    let _ = std::fs::remove_file(marker_path(updates_dir));
}

/// A window connected since the swap: keep it. None within the grace: roll back. A clock before
/// the marker's time waits (it cannot say how long it has been).
pub fn decide(m: &Applied, connected_since_start: bool, now_unix: u64) -> Decision {
    if connected_since_start {
        Decision::Confirm
    } else if now_unix >= m.at_unix.saturating_add(CONNECT_GRACE.as_secs()) {
        Decision::Rollback
    } else {
        Decision::Wait
    }
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
```

`server.rs`: `Options` gains `bundle: Option<PathBuf>`; `dispatch`'s `ApplyUpdate { version }`:

```rust
        Request::ApplyUpdate { version } => {
            let (staged, bundle, dir) = {
                let st = state.lock().unwrap();
                (st.update.staged.clone(), opts.bundle.clone(), opts.updates_dir.clone())
            };
            match (staged, bundle) {
                (None, _) => Response::Error("nothing staged".into()),
                (Some(s), _) if s.version != version => Response::Error(format!("staged {} is not {version}", s.version)),
                (Some(_), None) => Response::Error("not running from an app bundle".into()),
                (Some(s), Some(bundle)) => match crate::updates::swap::swap_in(&bundle, &s.bundle) {
                    Ok(()) => {
                        let marker = crate::updates::apply::Applied { version: s.version.clone(), old_kept: true, at_unix: crate::updates::apply::now_unix() };
                        if let Err(e) = crate::updates::apply::write_marker(&dir, &marker) {
                            eprintln!("sarkosd: cannot write the update marker: {e}");
                        }
                        let mut st = state.lock().unwrap();
                        st.update.staged = None;
                        st.update.applied = Some(marker);
                        crate::updates::stage::clean_except(&dir, None);
                        Response::Ok
                    }
                    Err(e) => {
                        let mut st = state.lock().unwrap();
                        st.update.last_error = Some(format!("update failed: {e}"));
                        Response::Error(e)
                    }
                },
            }
        }
```

`UpdateState` gains `pub applied: Option<apply::Applied>`, `pub connected_since_start: bool` (set true in `Request::Hello` handling; the daemon start sets it false). The watch: in `run_with` and `run_resumed`, after the socket listens, if `read_marker(&dir)` is `Some(m)`: set `st.update.applied = Some(m)` and spawn `updates::apply::watch(state, opts)`: a thread that every 500 ms computes `decide(&m, st.update.connected_since_start, now_unix())`: `Wait` continues; `Confirm` runs `swap::finish(bundle)`, `clear_marker`, clears `applied` and stops; `Rollback` runs `swap::rollback(bundle)` (on success: `clear_marker`, `last_error = Some(format!("update {} rolled back: no window came up within {}s", m.version, CONNECT_GRACE.as_secs()))`, and launches the old window in every `st.windows` folder with `watchdog::launch_ui(&bundle.join("Contents/MacOS/sarkos"), Some(cwd))`; on failure: `last_error` names it) and stops. The test's second connection confirms within 500 ms. `cleanup_leftovers(bundle)` runs at every daemon start when no marker exists (an interrupted swap).

`recovery.rs`: `--recover` start goes through `run_with(.., recover: true)`, so the same start hook covers it; add nothing beyond a log line "update marker found" if the marker exists.

`main.rs`: `--bundle PATH`; the default from `std::env::current_exe()` when its path contains `.app/Contents/MacOS/`: `exe.ancestors().nth(3)` (MacOS, Contents, the bundle); `None` otherwise.

- [ ] **Step 4: Run the tests**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p sarkosd --bin sarkosd -- updates`, `nice -n 15 ~/.cargo/bin/cargo test -p sarkosd --test daemon -- update roll`, then `nice -n 15 ~/.cargo/bin/cargo test --workspace` once and the warning check.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkosd/src/updates crates/sarkosd/src/server.rs crates/sarkosd/src/main.rs crates/sarkosd/src/recovery.rs crates/sarkosd/tests/daemon.rs
git commit -m "Swap the app bundle on ApplyUpdate, keep the old one until a window connects, roll back otherwise"
```

---

### Task 5: The window: idle rule, config, status, auto-apply, palette commands

**Files:**
- Create: `crates/sarkos/src/idle.rs`
- Modify: `crates/sarkos/src/config.rs` (`[update]`), `crates/sarkos/src/sessions.rs` (`Pending::{Status, Check, Apply}`, `Outcome::{UpdateStatus, Applied, ApplyFailed}`, `note_events` for `UpdateStaged`), `crates/sarkos/src/main.rs` (`last_input`, `staged`, `update_error`, the timer, `apply_staged`, the outcome arms, daemon args, `Command::{CheckForUpdates, RetryUpdate}`), `crates/sarkos/src/keymap.rs`, `crates/sarkos/src/command_palette.rs`, `crates/sarkos/src/update.rs` (`status_text_for(&StagedUpdate)`), `README.md` (Config section)
- Test: unit tests in `idle.rs`, `config.rs`, `sessions.rs`, `update.rs`, `keymap.rs`

**Interfaces:**
- Consumes: Task 3's protocol additions; `update::{launch_path, installed_version, differs}`; `Root::update_now`; `chrome::status_items(update, daemon, waiting)`.
- Produces: `idle::{Chrome { palette: bool, menu: bool, prompt: bool, find: bool, drag: bool }, may_apply(now: Instant, last_input: Instant, idle_after: Duration, chrome: Chrome, in_flight: bool, connected: bool) -> bool}`; `config::UpdateConfig { auto: bool, check_every_secs: u64, idle_after_secs: u64 }` (`Config.update`); `SessionManager::{update_status(), check_for_updates(), apply_update(version)}`; `Outcome::{UpdateStatus { staged: Option<StagedUpdate>, last_error: Option<String> }, Applied, ApplyFailed(String)}`; `Root::{last_input: Instant, staged: Option<StagedUpdate>, update_error: Option<String>, apply_staged(cx), start_idle_timer(cx)}`; `Command::{CheckForUpdates, RetryUpdate}`; `update::status_text_for(&StagedUpdate) -> String` ("update ready: 0.2.1").

- [ ] **Step 1: Write the failing tests**

`idle.rs`:

```rust
//! When a staged update may apply itself (auto-update spec, section 6): the window has been
//! quiet for `idle_after`, nothing transient is open, nothing is in flight, the daemon is there.
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Chrome {
    pub palette: bool,
    pub menu: bool,
    pub prompt: bool,
    pub find: bool,
    pub drag: bool,
}

impl Chrome {
    pub fn any(self) -> bool {
        self.palette || self.menu || self.prompt || self.find || self.drag
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_only_after_a_quiet_spell_with_nothing_open() {
        let t0 = Instant::now();
        let after = Duration::from_secs(30);
        let quiet = Chrome::default();
        assert!(!may_apply(t0 + Duration::from_secs(29), t0, after, quiet, false, true), "not yet");
        assert!(may_apply(t0 + Duration::from_secs(30), t0, after, quiet, false, true));
        assert!(may_apply(t0 + Duration::from_secs(300), t0, after, quiet, false, true));
        for open in [Chrome { palette: true, ..quiet }, Chrome { menu: true, ..quiet }, Chrome { prompt: true, ..quiet }, Chrome { find: true, ..quiet }, Chrome { drag: true, ..quiet }] {
            assert!(!may_apply(t0 + Duration::from_secs(60), t0, after, open, false, true), "{open:?}");
        }
        assert!(!may_apply(t0 + Duration::from_secs(60), t0, after, quiet, true, true), "an update already running");
        assert!(!may_apply(t0 + Duration::from_secs(60), t0, after, quiet, false, false), "offline");
        assert!(may_apply(t0, t0 + Duration::from_secs(5), Duration::ZERO, quiet, false, true), "a last input in the future (clock skew) counts as idle only when idle_after is zero");
    }
}
```

`config.rs` test, next to the daemon config test:

```rust
    #[test]
    fn the_update_table_defaults_to_automatic_and_accepts_the_three_fields() {
        let c = parse_str("").unwrap();
        assert_eq!(c.update, UpdateConfig { auto: true, check_every_secs: 300, idle_after_secs: 30 });
        let c = parse_str("[update]\nauto = false\ncheck_every_secs = 60\nidle_after_secs = 5\n").unwrap();
        assert_eq!(c.update, UpdateConfig { auto: false, check_every_secs: 60, idle_after_secs: 5 });
        assert!(parse_str("[update]\nspeed = 1\n").is_err(), "unknown keys are refused like everywhere else");
    }
```

(`parse_str` is whatever the file's tests use to parse a TOML string into `Config`; read them.)

`sessions.rs` tests:

```rust
    #[test]
    fn update_requests_map_to_their_outcomes() {
        let rec = Recorder::new();
        let mut m = SessionManager::new(rec.clone(), None, PathBuf::from("/p"));
        m.update_status();
        assert!(matches!(rec.requests()[0], Request::UpdateStatus));
        let staged = StagedUpdate { version: "0.2.1".into(), git: "abc".into() };
        assert_eq!(m.handle_response(1, Response::UpdateStatus { staged: Some(staged.clone()), last_error: None }), Outcome::UpdateStatus { staged: Some(staged.clone()), last_error: None });
        m.check_for_updates();
        assert!(matches!(rec.requests()[1], Request::CheckForUpdates));
        assert_eq!(m.handle_response(2, Response::Ok), Outcome::None);
        m.apply_update("0.2.1".into());
        assert!(matches!(&rec.requests()[2], Request::ApplyUpdate { version } if version == "0.2.1"));
        assert_eq!(m.handle_response(3, Response::Ok), Outcome::Applied);
        m.apply_update("0.2.1".into());
        assert_eq!(m.handle_response(4, Response::Error("nothing staged".into())), Outcome::ApplyFailed("nothing staged".into()));
        m.note_events(&[Event::UpdateStaged { update: staged }]);
        assert!(!m.stash_if_restoring(&Event::UpdateStaged { update: StagedUpdate { version: "1".into(), git: "g".into() } }), "not a session event");
    }
```

`update.rs` test addition: `assert_eq!(status_text_for(&StagedUpdate { version: "0.2.1".into(), git: "abc".into() }), "update ready: 0.2.1");`. `keymap.rs`'s `SPEC_ROWS` gains `("", "check for updates", "CheckForUpdates")` and `("", "retry update", "RetryUpdate")` (unbound: the unbound count in `the_table_is_the_spec_table` goes up by two).

- [ ] **Step 2: Run them to see them fail**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p sarkos -- idle config::tests::the_update sessions::tests::update_requests update keymap`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`idle.rs`:

```rust
pub fn may_apply(now: Instant, last_input: Instant, idle_after: Duration, chrome: Chrome, in_flight: bool, connected: bool) -> bool {
    if chrome.any() || in_flight || !connected {
        return false;
    }
    now.saturating_duration_since(last_input) >= idle_after
}
```

`config.rs`: `UpdateConfig` with `#[serde(default, deny_unknown_fields)]`, `Default` as the test says, `Config.update`, and the comment block gains

```toml
[update]
# The daemon checks the release manifest every check_every_secs and downloads a newer, signed
# build in the background. With auto = true the window applies it once you have been idle for
# idle_after_secs (no typing, nothing open): the daemon swaps in place and the window relaunches
# with every shell still there. auto = false keeps the "update ready" badge and the palette's
# "Update now" instead.
auto = true
check_every_secs = 300
idle_after_secs = 30
```

`sessions.rs`: `Pending::{Status, Check, Apply}`; `update_status`, `check_for_updates`, `apply_update(version)` send the requests; `handle_response`: `Response::UpdateStatus { staged, last_error }` → `Outcome::UpdateStatus { staged, last_error }` regardless of pending (like Hello); `(Pending::Apply, Ok)` → `Applied`; an `Error` with `Pending::Apply` → `ApplyFailed(e)`; `(Pending::Check, Ok)` → `None`. `note_events` ignores `UpdateStaged` (the root reads events too: see below); `stash_if_restoring` and `group_by_session` skip it (Task 3 did this; keep).

`main.rs`:
- `Root` fields: `pub last_input: Instant` (set to now in `new`), `pub staged: Option<StagedUpdate>`, `pub update_error: Option<String>`, `pub auto_apply_timer: bool`.
- Input tracking: the root div's `on_mouse_move` and `on_any_mouse_down` listeners set `root.last_input = Instant::now()`; keystrokes: in `Root::new`, `cx.observe_keystrokes(|_ev, _window, cx| ...)` if gpui 0.2.2 has it on `Context` (grep gpui for `observe_keystrokes`); otherwise the root's `on_key_down` listener (the root already handles keys for the panes: find where `KeyDownEvent` reaches `Root` and set `last_input` there).
- `connected(..)`: after Hello's handling, `self.sessions.update_status()` when the daemon's `protocol_max >= 4` (from `daemon_version`/`DaemonHello`; a protocol 3 daemon answers an unknown request with `Error`, which `Outcome::Error` would log: so gate on the protocol).
- Outcome arms: `UpdateStatus { staged, last_error }` → `self.staged = staged; self.update_error = last_error; self.refresh_update_badge(cx)`; `Applied` → `self.update_now(cx)` (the existing flow: the bundle is swapped, so `installed_version(launch path)` differs and the daemon upgrade plus relaunch follow); `ApplyFailed(e)` → `self.updating = None; self.update_error = Some(e.clone()); self.show_notice(format!("update failed: {e}"), cx)`.
- Events: where `Event`s are dispatched to terminals, match `Event::UpdateStaged { update }` first: `self.staged = Some(update); self.update_error = None; self.refresh_update_badge(cx); self.start_idle_timer(cx)`.
- `refresh_update_badge`: `self.update_ready = self.staged.as_ref().map(update::status_text_for).or_else(|| self.update_ready_on_disk.clone())` where `update_ready_on_disk` is what `update::start`'s loop sets today (rename the field it writes to `update_ready_on_disk` and compute `update_ready` from both). The title bar shows `update_error` as "update failed" when it is set and nothing is staged (`chrome::status_items` already takes any text).
- `start_idle_timer(cx)`: if `auto_apply_timer` is set, return; set it; `cx.spawn` loop: every second, `root.update(..)`: if `staged.is_none()` or `!config.update.auto`: clear the flag and stop; else if `idle::may_apply(now, last_input, Duration::from_secs(config.update.idle_after_secs), self.chrome_state(), self.updating.is_some(), !self.sessions.is_offline())`: clear the flag, `apply_staged(cx)`, stop. `chrome_state()` builds `idle::Chrome` from `palette.value().is_some()`, the three menus' `value()`, `wt_prompt.value()`, `find`s open on the active pane, `drop_target.is_some()`.
- `apply_staged(cx)`: `let Some(s) = &self.staged else { return }; self.updating = Some(UpdateStage::DaemonUpgrading); self.show_notice(format!("updating to {}; sessions stay", s.version), cx); self.sessions.apply_update(s.version.clone())`.
- `Command::CheckForUpdates` → `self.sessions.check_for_updates()` and a notice "checking for updates"; `Command::RetryUpdate` → `self.update_error = None; self.sessions.check_for_updates()`. Both registered in `keymap::ACTIONS` (unbound) and `Command`; the palette shows "Retry update" only when `update_error.is_some()` (`command_palette::items` gains an `update_error: bool` parameter like `update_ready`).
- Daemon args (both places: `daemon_args` in `main` and `agent_plist`): `--check-every-secs <config.update.check_every_secs>`.

`update.rs`: `pub fn status_text_for(s: &StagedUpdate) -> String { format!("update ready: {}", s.version) }`.

README `Config` section: the `[update]` block as in the comment. README `Updating` section: a new first paragraph: "Updates arrive by themselves: the daemon checks the release manifest every five minutes, downloads and verifies a newer build, and the window applies it once you have been idle for thirty seconds (no typing, nothing open): `sarkosd` swaps in place and the window relaunches with every shell still there. `[update] auto = false` keeps the badge and "Update now" instead; "Check for updates" in the palette asks now." The existing paragraphs about `brew upgrade` and local builds stay.

- [ ] **Step 4: Run the tests**

Run: `nice -n 15 ~/.cargo/bin/cargo test -p sarkos` then the warning check and the em-dash grep.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/sarkos/src/idle.rs crates/sarkos/src/config.rs crates/sarkos/src/sessions.rs crates/sarkos/src/main.rs crates/sarkos/src/keymap.rs crates/sarkos/src/command_palette.rs crates/sarkos/src/update.rs README.md
git commit -m "Apply a staged update when the window is idle, with [update] config and palette commands"
```

---

### Task 6: The release workflow, the bundle, the cask, the docs

**Files:**
- Create: `.github/workflows/release.yml`, `packaging/Info.plist.in`, `packaging/bundle.sh`, `packaging/manifest.sh`, `homebrew/sarkos-ide.rb`
- Modify: `README.md` ("Releasing" section, the dogfooding checklist, the Build section's install note), `docs/superpowers/specs/2026-10-14-auto-update-design.md` (rulings 1 and 2), `.gitignore` (nothing: `dist/` is created on the runner only)
- Test: `packaging/bundle.sh` and `packaging/manifest.sh` run locally against the dev binaries (dev build, not release: `nice -n 15 ~/.cargo/bin/cargo build -p sarkos -p sarkosd` is an incremental dev build and is allowed), producing a bundle the daemon's `check_bundle` accepts; a shell test `packaging/test.sh` that runs both scripts on the dev binaries into a temp dir and checks the zip, manifest and signature with `release-tool verify`.

**Interfaces:**
- Consumes: Task 1's `release-tool`; Task 2's manifest format; Task 3's `stage::check_bundle` expectations (both executables in `Contents/MacOS`, `CFBundleShortVersionString`).
- Produces: the release assets `Sarkos-IDE-<version>-arm64.zip`, `manifest.json`, `manifest.json.sig` on a GitHub Release tagged `v<version>`; the cask text.

- [ ] **Step 1: The bundle and manifest scripts, with a local test**

`packaging/Info.plist.in`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>sh.csarko.sarkos</string>
	<key>CFBundleName</key>
	<string>Sarkos IDE</string>
	<key>CFBundleDisplayName</key>
	<string>Sarkos IDE</string>
	<key>CFBundleExecutable</key>
	<string>sarkos</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>@VERSION@</string>
	<key>CFBundleVersion</key>
	<string>@VERSION@</string>
	<key>LSMinimumSystemVersion</key>
	<string>14.0</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>LSApplicationCategoryType</key>
	<string>public.app-category.developer-tools</string>
</dict>
</plist>
```

`packaging/bundle.sh`:

```sh
#!/bin/sh
# bundle.sh VERSION BIN_DIR OUT_DIR: assembles OUT_DIR/"Sarkos IDE.app" from BIN_DIR/sarkos and
# BIN_DIR/sarkosd, ad-hoc signs it, and zips it to OUT_DIR/Sarkos-IDE-VERSION-arm64.zip.
set -eu
VERSION="$1"; BIN="$2"; OUT="$3"
APP="$OUT/Sarkos IDE.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
sed "s/@VERSION@/$VERSION/g" "$(dirname "$0")/Info.plist.in" > "$APP/Contents/Info.plist"
cp "$BIN/sarkos" "$BIN/sarkosd" "$APP/Contents/MacOS/"
chmod 755 "$APP/Contents/MacOS/sarkos" "$APP/Contents/MacOS/sarkosd"
if [ -f "$(dirname "$0")/AppIcon.icns" ]; then cp "$(dirname "$0")/AppIcon.icns" "$APP/Contents/Resources/"; fi
codesign --force --deep --sign - "$APP"
rm -f "$OUT/Sarkos-IDE-$VERSION-arm64.zip"
ditto -c -k --keepParent "$APP" "$OUT/Sarkos-IDE-$VERSION-arm64.zip"
echo "$OUT/Sarkos-IDE-$VERSION-arm64.zip"
```

`packaging/manifest.sh`:

```sh
#!/bin/sh
# manifest.sh VERSION GIT ZIP URL OUT_DIR: writes OUT_DIR/manifest.json for the zip.
set -eu
VERSION="$1"; GIT="$2"; ZIP="$3"; URL="$4"; OUT="$5"
SIZE=$(stat -f %z "$ZIP")
SHA=$(shasum -a 256 "$ZIP" | cut -d' ' -f1)
NOW=$(date -u +%Y-%m-%dT%H:%M:%SZ)
cat > "$OUT/manifest.json" <<EOF
{"format":1,"version":"$VERSION","git":"$GIT","published":"$NOW","arm64":{"url":"$URL","size":$SIZE,"sha256":"$SHA"},"min_protocol":1}
EOF
echo "$OUT/manifest.json"
```

`packaging/test.sh` (run locally by the implementer, and documented in the README's Tests section):

```sh
#!/bin/sh
# Builds a bundle from the dev binaries, a manifest, signs it with a throwaway key and verifies
# the lot: the same steps the release workflow runs, without a runner.
set -eu
cd "$(dirname "$0")/.."
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"
nice -n 15 "$CARGO" build -q -p sarkos -p sarkosd -p release-tool
OUT=$(mktemp -d)
ZIP=$(sh packaging/bundle.sh 0.0.0 target/debug "$OUT")
sh packaging/manifest.sh 0.0.0 "$(git rev-parse --short HEAD)" "$ZIP" "file://$ZIP" "$OUT" > /dev/null
KEYS=$(target/debug/release-tool keygen)
SK=$(echo "$KEYS" | sed -n 's/^secret: //p'); PK=$(echo "$KEYS" | sed -n 's/^public: //p')
SARKOS_TEST_KEY="$SK" target/debug/release-tool sign --key-env SARKOS_TEST_KEY "$OUT/manifest.json" > "$OUT/manifest.json.sig"
target/debug/release-tool verify --public "$PK" "$OUT/manifest.json" "$OUT/manifest.json.sig"
"$OUT/Sarkos IDE.app/Contents/MacOS/sarkosd" --version
echo "packaging ok: $OUT"
```

Run `sh packaging/test.sh`: expect `ok`, the version line, and `packaging ok`. (A dev build of `sarkos` links gpui; it is incremental in the shared target dir and allowed; it is the only build in this task.)

- [ ] **Step 2: The workflow**

`.github/workflows/release.yml`:

```yaml
name: release
# Manual only (the user's rule): nothing builds on push.
on:
  workflow_dispatch:
    inputs:
      version:
        description: "Version to release, like 0.2.1"
        required: true
        type: string

permissions:
  contents: write

jobs:
  release:
    runs-on: macos-15
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Check the version
        run: |
          echo "${{ inputs.version }}" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || { echo "not major.minor.patch"; exit 1; }
          git rev-parse -q --verify "refs/tags/v${{ inputs.version }}" && { echo "tag exists"; exit 1; } || true
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Bump the version and tag
        run: |
          sed -i '' -E 's/^version = "[0-9.]+"$/version = "${{ inputs.version }}"/' Cargo.toml
          grep -q 'version = "${{ inputs.version }}"' Cargo.toml
          cargo update -w -q
          git config user.name "release workflow"
          git config user.email "release@csarko.sh"
          git commit -am "Release ${{ inputs.version }}"
          git tag "v${{ inputs.version }}"
      - name: Build
        run: cargo build --release --locked -p sarkos -p sarkosd -p release-tool
      - name: Bundle and sign
        env:
          SARKOS_RELEASE_KEY: ${{ secrets.SARKOS_RELEASE_KEY }}
        run: |
          mkdir -p dist
          ZIP=$(sh packaging/bundle.sh "${{ inputs.version }}" target/release dist)
          URL="https://github.com/${{ github.repository }}/releases/download/v${{ inputs.version }}/$(basename "$ZIP")"
          sh packaging/manifest.sh "${{ inputs.version }}" "$(git rev-parse --short HEAD)" "$ZIP" "$URL" dist
          target/release/release-tool sign --key-env SARKOS_RELEASE_KEY dist/manifest.json > dist/manifest.json.sig
          "dist/Sarkos IDE.app/Contents/MacOS/sarkosd" --version
      - name: Push the release commit and tag
        run: git push origin HEAD:main "v${{ inputs.version }}"
      - name: Publish
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          PREV=$(git describe --tags --abbrev=0 HEAD~1 2>/dev/null || true)
          if [ -n "$PREV" ]; then NOTES=$(git log --format='- %s' "$PREV..HEAD~1"); else NOTES="First release."; fi
          gh release create "v${{ inputs.version }}" dist/Sarkos-IDE-*.zip dist/manifest.json dist/manifest.json.sig --title "Sarkos IDE ${{ inputs.version }}" --notes "$NOTES"
```

Notes for the implementer: the git hash in the manifest is the release commit's, which is also the hash the binaries built from that commit print (the build runs after the bump commit); `cargo update -w` refreshes `Cargo.lock`'s own-crate versions after the bump; the push happens before `gh release create` so the tag exists on the remote; the secret `SARKOS_RELEASE_KEY` is set by the user (the README says how); `--locked` fails the build if `Cargo.lock` is stale, which the bump step prevents.

- [ ] **Step 3: The cask, the README and the spec**

`homebrew/sarkos-ide.rb` (moved to the tap repository at the Homebrew step; kept here so the text is reviewed with the code):

```ruby
cask "sarkos-ide" do
  version "0.2.1"
  sha256 "REPLACED_BY_THE_TAP_UPDATE_SCRIPT"

  url "https://github.com/csarkosh/ide-sarkos/releases/download/v#{version}/Sarkos-IDE-#{version}-arm64.zip"
  name "Sarkos IDE"
  desc "Terminal-pane IDE for running many Claude Code sessions, with a daemon that keeps them alive"
  homepage "https://github.com/csarkosh/ide-sarkos"

  depends_on arch: :arm64
  depends_on macos: ">= :sonoma"
  auto_updates true

  app "Sarkos IDE.app"

  zap trash: [
    "~/Library/Application Support/sarkos",
    "~/Library/LaunchAgents/sh.csarko.sarkosd.plist",
  ]
end
```

README:
- A new section `## Releasing` after `## Updating`: "Releases are built by the `release` workflow, which runs only when you start it: `gh workflow run release -f version=0.2.1` (or the Actions tab). It bumps the version, tags `v0.2.1`, builds on a macOS runner, assembles and ad-hoc signs `Sarkos IDE.app`, signs `manifest.json` with the release key (the repository secret `SARKOS_RELEASE_KEY`; make a pair with `scripts/release-key`, keep the secret there and in your password manager, and put the public half in `crates/protocol/src/release_key.rs`), and publishes a GitHub Release. Every running daemon sees it within five minutes. `release-tool verify --public <key> manifest.json manifest.json.sig` checks a published manifest by hand. `packaging/test.sh` runs the same packaging steps locally on a dev build."
- The dogfooding checklist under Status: "- push, `gh workflow run release -f version=X`, wait for the run, see "update ready: X" in the title bar within five minutes, stop typing for thirty seconds: the window relaunches with every shell and tab where it was; `sarkos --version` in a tab prints X" and "- `[update] auto = false`: the badge stays and "Update now" applies it".
- Section `## Updating`: mention that the IDE is installed as `Sarkos IDE.app` (the cask) and that `brew upgrade` leaves it alone (`auto_updates`).
- The `.app` path in the "After a restart" section's launchd text if it names the binary path: `/Applications/Sarkos IDE.app/Contents/MacOS/sarkosd`.

Spec amendments (`2026-10-14-auto-update-design.md`): in section 3, replace "(minisign-compatible format, so `minisign -V` on a Mac verifies it by hand)" with "(the base64 of the raw 64-byte ed25519 signature over the manifest bytes; `release-tool verify` checks it by hand)"; in section 7, replace "If it does not, or if it exits twice within a minute," with "If it does not,"; in section 6 replace "`Response::Hello` gains `staged: Option<String>` and the daemon sends `Event::UpdateStaged`" with "A protocol 4 window sends `UpdateStatus` after Hello and the daemon sends `Event::UpdateStaged`".

- [ ] **Step 4: Check everything**

Run: `sh packaging/test.sh`; `nice -n 15 ~/.cargo/bin/cargo test --workspace` once; the warning check; `grep -n "—" README.md packaging/*.sh homebrew/*.rb .github/workflows/release.yml crates/*/src/*.rs crates/sarkosd/src/updates/*.rs`; `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml'))"` (or `ruby -ryaml -e 'YAML.load_file(".github/workflows/release.yml")'`) to be sure the YAML parses. The workflow itself is run by the user after merge (the README's steps); the implementer does not trigger it.
Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml packaging homebrew README.md docs/superpowers/specs/2026-10-14-auto-update-design.md
git commit -m "Release workflow (manual), app bundle packaging, the Homebrew cask text, and the docs"
```

---

## Self-review notes

- Spec coverage: section 3 (workflow, key, manifest, release) in Tasks 1 and 6; section 4 (install shape, swap, App Management probe and fallback) in Tasks 1, 4 and 6; section 5 (fetch, verify, stage, poll, `--update-url`, the staging folder, the on-disk check kept) in Tasks 2 and 3; section 6 (status request, event, idle rule, auto apply, `[update]`) in Tasks 3 and 5; section 7 (grace, rollback, `.old` cleanup, failure text, downgrade refusal) in Tasks 2 and 4; section 8 (signature before anything, host check, absolute paths, minimal env, 0700 folder: add `set_permissions(0o700)` on the updates dir in `run_check`'s `create_dir_all` step, Task 3) in Tasks 2, 3; section 9 in Task 5; section 10's tests are spread as named; section 11's order is the memory's.
- Type consistency: `StagedUpdate { version, git }` (protocol) versus `Staged { version, git, bundle }` (daemon) are two types on purpose; `Applied` is daemon-only; `Outcome` in `poll.rs` is the daemon's, `Outcome` in `sessions.rs` the window's (different crates); `check_every_secs` is the config key and `--check-every-secs` the daemon argument; `UpdateState.applied` is added in Task 4 (Task 3 defines the struct without it: Task 4's implementer adds the field).
- Placeholder scan: none; the one `REPLACE_WITH_THE_PUBLIC_KEY_FROM_KEYGEN` string is replaced within Task 1 by the implementer's keygen run.
- Review Focus pins: 1 in Task 2, 2 and 3 in Task 3, 4 and 5 in Task 4.
- Rulings 1 to 5 are applied where the code lives; the executor records more in the ledger.
