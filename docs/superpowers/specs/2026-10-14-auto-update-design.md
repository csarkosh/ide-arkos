# Arkos IDE: automatic updates

Date: 2026-10-14. Status: approved in chat; this is the written design. Builds on the
update-in-place design (`2026-10-14-update-in-place-design.md`): that milestone made an update
survivable (the daemon swaps itself in place, the window relaunches with every session
reattached). This one makes updates arrive and apply on their own, the way Claude Code's CLI
updates during a session, so the IDE can be lived in while it keeps changing.

## 1. Why this exists

Cyrus wants to use the IDE daily, feed UX changes back, and have them land in the running IDE
without stopping anything. That needs three things the update-in-place milestone left out: a
build published from a known place, a fetch that verifies what it got, and an apply that does
not wait for a click. It comes before branding and the Homebrew publication because it fixes
the install shape (an app bundle) that both depend on.

Decisions taken in chat on 2026-10-14:

- Releases are signed with the project's own ed25519 key; no Apple Developer ID yet.
  Notarization is added with branding, when other people install it.
- A staged update applies automatically once the window is idle.
- Homebrew installs an app bundle through a cask marked `auto_updates`; the updater replaces
  the bundle in place.
- The release workflow runs only when Cyrus triggers it. Nothing builds on push, and nothing
  builds on the laptop.

## 2. Pieces

| Piece | Lives in | Does |
|---|---|---|
| Release workflow | `.github/workflows/release.yml` | Builds, bundles, signs and publishes one release, on `workflow_dispatch` only |
| Release key | GitHub Actions secret `ARKOS_RELEASE_KEY`; public half in `crates/protocol/src/release_key.rs` | Signs the manifest; the IDE verifies it |
| Manifest | A release asset, `manifest.json` | Says what the latest release is and how to check it |
| Cask | `csarkosh/homebrew-tap`, `Casks/arkos-ide.rb` | First install: `Arkos IDE.app` into `/Applications`, `auto_updates true` |
| Fetcher | `crates/arkosd/src/updates/{manifest,fetch,verify,stage}.rs` | Polls, downloads, verifies, unpacks into a staging folder |
| Swap | `crates/arkosd/src/updates/swap.rs` | Renames the new bundle into place, keeps the old one for rollback |
| Idle apply | `crates/arkos/src/idle.rs` and the existing `update.rs` | Decides when a window may relaunch itself and runs the existing update flow |
| Protocol | `crates/protocol` | One additive message pair, protocol 4 |

The window never touches the network. The daemon never shows UI. Both read the same config.

## 3. Publishing

`release.yml` takes one input, `version` (such as `0.2.1`), and runs on the `macos-15`
arm64 runner (free on this public repository; it is manual so no minute is spent unasked):

1. Check out `main`, install stable Rust, restore the cargo cache.
2. Set the workspace version in `Cargo.toml` to the input, commit "Release 0.2.1" and tag
   `v0.2.1`, pushed back to `main` by the workflow (so the binaries' `--version` and the
   manifest agree, and the tag marks the exact source).
3. `cargo build --release` for both crates.
4. Assemble `Arkos IDE.app`: `Contents/Info.plist` (bundle id `sh.csarko.arkos`, name
   "Arkos IDE", `CFBundleShortVersionString` from the input, `LSUIElement` false,
   `CFBundleExecutable` `arkos`), `Contents/MacOS/arkos` and `Contents/MacOS/arkosd`,
   `Contents/Resources/` (the icon arrives with branding; empty until then).
5. `codesign --force --deep --sign -` (ad-hoc) on the bundle.
6. `ditto -c -k --keepParent` into `Arkos-IDE-0.2.1-arm64.zip`.
7. Write `manifest.json`:
   ```json
   {
     "format": 1,
     "version": "0.2.1",
     "git": "a1b2c3d",
     "published": "2026-10-15T03:12:00Z",
     "arm64": {
       "url": "https://github.com/csarkosh/ide-arkos/releases/download/v0.2.1/Arkos-IDE-0.2.1-arm64.zip",
       "size": 23456789,
       "sha256": "..."
     },
     "min_protocol": 1
   }
   ```
   and sign the manifest bytes with `scripts/release-sign` (a tiny Rust binary in
   `crates/release-tool`, run on the runner) into `manifest.json.sig` (the base64 of the raw
   64-byte ed25519 signature over the manifest bytes; `release-tool verify` checks it by hand).
8. `gh release create v0.2.1` with the zip, `manifest.json` and `manifest.json.sig`, and
   release notes from the commits since the previous tag.

`scripts/release-key` generates the key pair once (`release-tool keygen`): the secret key is
pasted into the repository secret and into Cyrus's password manager and never written to the
repository; the public key is committed as a constant. Rotating the key is a new constant and a
release signed by both keys (the manifest carries a list of signatures; the IDE accepts any
known key).

`min_protocol` lets a release say which daemons it can talk to, so a window never updates past
a daemon it cannot replace (section 4 of the update-in-place design has the daemon
update first, so this is belt and braces).

## 4. Install shape

Homebrew is the first install only: `brew install csarkosh/tap/arkos-ide` puts
`Arkos IDE.app` in `/Applications`. The cask has `auto_updates true`, so `brew upgrade` skips
it; the IDE keeps itself current. (`brew reinstall` still works as a reset.)

Paths inside the bundle never move between versions, so the launchd agent
(`~/Library/LaunchAgents/sh.csarko.arkosd.plist` pointing at
`/Applications/Arkos IDE.app/Contents/MacOS/arkosd`), the window's on-disk version check
(its own launch path), the Claude Code hooks (`arkos notify` by launch path) and the socket
all keep working across updates. A moved bundle is the existing "install again" case.

The bundle swap is Sparkle's: unpack the verified zip to `Arkos IDE.app.new` beside the
installed bundle, rename the installed one to `Arkos IDE.app.old`, rename `.new` into
place, and delete `.old` after the next successful window launch. Before the swap the daemon
writes the rollback marker `updates/applied.json`, which records the version and the window pids
present at apply, so only a new window's connect confirms the update (the old windows
reconnecting after the daemon's exec confirm nothing). Two renames on one volume
are atomic each; the running processes keep their open files (the old inodes), which is what
the update-in-place handover relies on.

**App Management.** Since macOS 13, modifying another team's app bundle in `/Applications`
needs the App Management permission; apps may update themselves. Our bundle is ad-hoc signed
(no team), and the daemon doing the renames is part of the same bundle. Reports differ on
whether that prompts. So the plan's first task is an experiment on this Mac: a bundle whose
executable is a shell script, ad-hoc signed, placed in `/Applications`, swapped by a
background process with the two renames above, while a copy of it runs. If macOS prompts or
blocks, the fallback is fixed now: the real bundle lives at
`~/Library/Application Support/arkos/Arkos IDE.app` and `/Applications/Arkos IDE.app` is a
launcher bundle that never changes (its executable `exec`s the real one); the cask installs
both and the updater touches only the first. Nothing else in this design changes under the
fallback.

## 5. Fetch and verify (the daemon)

The daemon runs the check because it is always up after login (the launch agent) and has no
UI to block. Every `check_every` (default five minutes) and on `CheckForUpdates` from a window:

1. `curl --fail --silent --location --max-time 20 --output <staging>/manifest.json` of
   `https://github.com/csarkosh/ide-arkos/releases/latest/download/manifest.json`, and the
   same for `manifest.json.sig`. The system `curl` carries macOS's TLS roots; the daemon
   gains no HTTP crate. The base URL is a daemon argument (`--update-url`), so tests use
   `file://` and the config can point at a fork.
2. Verify the signature over the manifest bytes with the compiled-in public key (crate
   `ed25519-compact`, no dependencies of its own). A bad signature deletes the staging folder
   and logs it; nothing else is read from an unsigned manifest.
3. Compare: the manifest's version must be strictly greater than the running daemon's and
   than any staged one (semver only; a re-release is a new patch version, so a replayed older
   manifest can never roll back); otherwise there is nothing to do. A version
   already marked bad (step 5) is skipped until the manifest changes.
4. Download the zip to `<staging>/<version>/bundle.zip` with `curl` (`--max-filesize` from
   the manifest's `size` plus a 200 MB cap, `--max-time 600`), then check the size and the
   sha256 (crate `sha2`) against the manifest.
5. Unpack with `ditto -x -k` into `<staging>/<version>/Arkos IDE.app`, check the structure
   (both executables present and executable, `Info.plist` version equal to the manifest's),
   and run `<new>/Contents/MacOS/arkosd --version` with a two-second limit to be sure it
   runs. Any failure deletes the folder and marks the version bad.
6. Record `staged = { version, git, path }` and notify every connected window
   (`Event::UpdateStaged { version }`).

Staging lives under `~/Library/Application Support/arkos/updates/`; the daemon deletes
anything there that is not the current staged version on every check. Only one fetch runs at
a time; a check during a download is ignored. The on-disk check the window already does (its
launch path's `--version` every five minutes) stays, so a manual `brew reinstall` or a local
build into the bundle still shows up as "update ready".

## 6. Apply when idle (the window)

A protocol 4 window sends `UpdateStatus` after Hello and the daemon sends
`Event::UpdateStaged` when it stages one, so a window knows at connect time and after. The
title bar shows "update ready: 0.2.1" (the existing exception item). The window applies it
when all of these hold for 30 s (`idle_after`, config): no key or mouse event, no drag, no
palette, menu, prompt, find bar or notice-with-action open, no update already running, and the
daemon connected. The idle rule is a pure function of the last input instant and the open
chrome (`idle.rs`), checked once a second while an update is staged and otherwise not at all.

Applying is the existing "Update now" flow with one addition at the front: the window sends
`ApplyUpdate` and the daemon does the bundle swap (section 4), then the daemon upgrade
(exec-in-place with the handover file, from `<bundle>/Contents/MacOS/arkosd`, now the new
file), then `RelaunchClient` with the new `arkos`. The window quits, the daemon launches the
new window in the same project, the shells never notice. A second window that is still busy
keeps the badge and applies itself when it goes idle; it finds the daemon already new and
only relaunches itself. `auto = false` in `[update]` turns the automatic apply off: the badge
and the palette's "Update now" remain.

## 7. Failure and rollback

- The new window must connect within 20 s of launch. If it does not, the daemon renames `.old`
  back into place, launches the old window, and marks the version bad; the title bar says
  "update 0.2.1 rolled back". The daemon itself was already exec'd from the new binary; a daemon
  that fails to come up after exec is the update-in-place design's section 7 case (the window's
  reconnect loop starts the binary at the launch path, which after the rollback is the old one).
- `.old` is deleted after the next successful window connect; if the machine restarts in
  between, the launch agent's `--recover` run deletes a leftover `.old` or `.new`.
- A download or signature failure never changes anything on disk outside the staging
  folder. A swap failure (a rename refused) leaves the installed bundle untouched and logs the
  errno; the title bar shows "update failed: <reason>" with the palette's "Retry update".
- A manifest older than the running version is ignored, so a mistaken release cannot
  downgrade anyone; a rollback is a new, higher release.
- App Management refusal shows as the rename failing with EPERM; the experiment in section 4
  is meant to catch it before any of this is built.

## 8. Security

- Only a manifest signed by a known release key is read; the zip is accepted only if its
  size and sha256 match that manifest. The IDE never runs anything from the download before
  both checks pass.
- The fetch is a fixed HTTPS URL on github.com; redirects are followed only to github.com or
  objects.githubusercontent.com (`curl --proto =https` and a host check on `--write-out
  %{url_effective}`).
- The secret key exists in the Actions secret and the password manager. The workflow prints
  only the public key.
- `curl` and `ditto` are run by absolute path (`/usr/bin/curl`, `/usr/bin/ditto`) with a
  minimal environment, like every process the daemon spawns.
- The staging folder is `0700`; the manifest's URL is never shown to a shell.

## 9. Config

```toml
[update]
auto = true            # apply a staged update when the window is idle
check_every = "5m"     # the daemon's manifest poll
idle_after = "30s"     # how long the window must be quiet before it relaunches
```

The daemon reads `check_every` and `--update-url`; the window reads the other two.

## 10. Testing

- `release-tool`: keygen writes a pair, sign then verify round-trips, a flipped byte fails.
- Manifest: parse, reject unknown `format`, version ordering (semver then git hash), the
  bad-version list.
- Fetch: against `file://` URLs through the real `curl` path, a wrong sha256, a wrong size,
  an oversize file, a missing signature, a redirect off github.com (a `file://` test cannot
  redirect: the host check is a pure function with its own test).
- Stage and swap on temporary directories: a bundle is a directory with two scripts; the two
  renames, the `.old` cleanup, the rollback, a refused rename.
- Idle: a pure function of (last input, open chrome, in flight) against time.
- Protocol: `Hello.staged`, `UpdateStaged`, `ApplyUpdate` additive; protocol 3 windows still
  connect to a protocol 4 daemon.
- Daemon integration: a staged update followed by `ApplyUpdate` swaps a fake bundle, execs
  the handover and relaunches the client (the existing upgrade test, extended).
- The workflow: run once by hand; the README's "Releasing" section is the checklist.
- Dogfooding checklist (README): push, `gh workflow run release -f version=0.2.1`, wait for
  the run, see "update ready" within five minutes, stop typing, see the window relaunch with
  every shell still there; `brew install csarkosh/tap/arkos-ide` on a clean machine puts the
  app in `/Applications` and the first update lands by itself.

## 11. Where it sits

Milestone order from here: this (auto-update), then branding (icon, name, notarization when
there is a Developer ID), then the Homebrew tap and cask, then Cyrus dogfoods, then
milestone 5 (browser pane), 6 (polish), Codex. About six tasks: the App Management experiment
and the release tool; the manifest and verification; fetch and stage; swap and rollback with
the protocol additions; the window's idle apply and config; the workflow, the cask and the
README.
