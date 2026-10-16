//! What Arkos adds to lib-visits's `visits-app` (telemetry spec, section 5), shared by the app
//! and the daemon: the event allowlist compiled in from `telemetry/events.toml`, the envelope
//! every report carries, the config file's `[telemetry]` table and the mode it gives, and the
//! folders both binaries read. The crate opens no connection and starts no thread; the daemon is
//! the only process that uploads.
mod config;
mod paths;

pub use config::{parse_table, Error, TelemetryConfig};
pub use paths::{app_support, app_support_from, config_dir, config_dir_from, config_file};
pub use visits_app;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use visits_app::{Allowlist, Arch, Channel, Crashes, Envelope, Mode, Os, Settings, Usage};

/// The allowlist file, `telemetry/events.toml`, as compiled in: one `[events.<name>]` table per
/// event and a `[focus]` table (its shape is `visits_app::allowlist`'s).
pub const EVENTS_TOML: &str = include_str!("../../../telemetry/events.toml");

/// The variable that turns telemetry off (`off`) or prints what would be sent (`log`): the
/// crate's `<APP>_TELEMETRY`.
pub const ENV_VAR: &str = "ARKOS_TELEMETRY";

/// Which binary a report comes from: the window (`app`) or the background daemon (`daemon`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bin {
    App,
    Daemon,
}

impl Bin {
    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Bin::App => "app",
            Bin::Daemon => "daemon",
        }
    }
}

/// The envelope of this process's reports: the version, the binary, macOS's major version, the
/// processor and the channel of the bundle it runs from.
pub fn envelope(bin: Bin) -> Envelope {
    Envelope {
        app: "arkos".into(),
        ver: env!("CARGO_PKG_VERSION").into(),
        bin: bin.as_str().into(),
        os: Os::Macos,
        osv: osv(),
        arch: if std::env::consts::ARCH == "aarch64" { Arch::Arm64 } else { Arch::X86_64 },
        ch: channel(),
    }
}

/// `release` when the bundle carries `Contents/Resources/channel` saying `release` (the release
/// workflow writes it), else `source`: every dev build, the shoot and a hand-made bundle.
pub fn channel_of(bundle: &Path) -> Channel {
    match std::fs::read_to_string(bundle.join("Contents/Resources/channel")) {
        Ok(text) if text.trim() == "release" => Channel::Release,
        _ => Channel::Source,
    }
}

/// The channel of the bundle this process runs from, read once. A binary outside a bundle (a dev
/// build in `target/`) is `source`.
pub fn channel() -> Channel {
    static CHANNEL: OnceLock<Channel> = OnceLock::new();
    CHANNEL
        .get_or_init(|| match std::env::current_exe().ok().and_then(|exe| bundle_of(&exe)) {
            Some(bundle) => channel_of(&bundle),
            None => Channel::Source,
        })
        .clone()
}

/// The app bundle holding `exe` when it runs from one (`<bundle>.app/Contents/MacOS/<exe>`).
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_app = bundle.extension().is_some_and(|e| e == "app");
    (is_app && macos.file_name()? == "MacOS" && contents.file_name()? == "Contents").then(|| bundle.to_path_buf())
}

/// The major of `sw_vers -productVersion`, read once; 0 when it cannot be read.
pub fn osv() -> u32 {
    static OSV: OnceLock<u32> = OnceLock::new();
    *OSV.get_or_init(|| {
        let out = std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output();
        out.ok().filter(|o| o.status.success()).map_or(0, |o| major_of(&String::from_utf8_lossy(&o.stdout)))
    })
}

/// The major version in a `sw_vers -productVersion` answer ("15.6.1" is 15); 0 for anything else.
fn major_of(version: &str) -> u32 {
    version.trim().split('.').next().and_then(|major| major.parse().ok()).unwrap_or(0)
}

/// The allowlist compiled in from `telemetry/events.toml`, parsed once.
pub fn allowlist() -> &'static Allowlist {
    static ALLOWLIST: OnceLock<Allowlist> = OnceLock::new();
    // The file is compiled in and a test parses it, so a failure here is a build that never shipped.
    ALLOWLIST.get_or_init(|| Allowlist::from_toml(EVENTS_TOML).expect("telemetry/events.toml parses"))
}

/// The mode for this process: `config` is the parsed `[telemetry]` table, or the error that kept
/// it from parsing; the environment and the running bundle's channel do the rest.
pub fn mode_for<E>(config: Result<&TelemetryConfig, E>, interactive: bool) -> Mode {
    mode_with(config, &|k| std::env::var(k).ok(), &channel(), interactive)
}

/// `mode_for` with the environment and the channel given, for tests. A config error is usage
/// `Off` and crashes `Never` for this run, never the defaults, with the reason `config error`
/// when no earlier rule names one.
pub fn mode_with<E>(config: Result<&TelemetryConfig, E>, env: &dyn Fn(&str) -> Option<String>, channel: &Channel, interactive: bool) -> Mode {
    let failed = config.is_err();
    let settings = match config {
        Ok(c) => Settings { usage: Some(c.usage), crashes: Some(c.crashes) },
        Err(_) => Settings { usage: Some(Usage::Off), crashes: Some(Crashes::Never) },
    };
    let mut mode = visits_app::mode(&settings, env, ENV_VAR, channel, interactive);
    if failed && mode.reason.is_none() {
        mode.reason = Some("config error");
    }
    mode
}

/// Whether this launch is on a later UTC day than the install recorded in `installed` (the unix
/// seconds `startup::installed_since` writes). A missing or unreadable file is the first launch.
pub fn second_launch(installed: &Path, now: u64) -> bool {
    const DAY: u64 = 86_400;
    let Ok(text) = std::fs::read_to_string(installed) else { return false };
    text.trim().parse::<u64>().is_ok_and(|at| now / DAY > at / DAY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use visits_app::{DValue, Event, Rejected};

    /// The spec's section 2.2 table, but for the two lists that come from the app's own code
    /// (the palette's commands and the config keys), which the app's tests check.
    fn spec_table() -> BTreeMap<&'static str, BTreeMap<&'static str, Vec<&'static str>>> {
        let rows: &[(&str, &[(&str, &[&str])])] = &[
            ("launch", &[("how", &["normal", "relaunch", "updated", "recovered"])]),
            ("window_open", &[]),
            ("window_close", &[]),
            (
                "pane_open",
                &[("kind", &["terminal", "viewer", "browser"]), ("how", &["palette", "key", "click", "explorer", "link", "restore"])],
            ),
            ("pane_close", &[("kind", &["terminal", "viewer", "browser"])]),
            (
                "view",
                &[("format", &["markdown", "code", "plain", "image", "video", "binary", "too_large"]), ("mode", &["rendered", "raw"])],
            ),
            ("md", &[("feature", &["table_edit", "mermaid", "details", "checklist", "remote_image", "video", "inline_diff"])]),
            ("agent_launch", &[("agent", &["claude", "other"]), ("how", &["new", "resume"])]),
            ("agent_state", &[("agent", &["claude", "other"]), ("state", &["waiting"])]),
            ("update", &[("step", &["staged", "applied", "failed", "rolled_back"])]),
            ("notice", &[("kind", &["update_ready", "updated", "daemon", "config_error", "theme_error"])]),
            (
                "error",
                &[("kind", &["daemon_unreachable", "daemon_restarted", "session_lost", "chrome_missing", "mermaid_failed", "save_failed"])],
            ),
            ("help", &[("what", &["keybindings", "documentation", "about", "telemetry"])]),
        ];
        rows.iter().map(|(n, keys)| (*n, keys.iter().map(|(k, vs)| (*k, vs.to_vec())).collect())).collect()
    }

    fn event(n: &str, d: &[(&str, DValue)]) -> Event {
        Event { n: n.into(), t: 0, d: d.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
    }

    fn file() -> toml::Table {
        include_str!("../../../telemetry/events.toml").parse().unwrap()
    }

    #[test]
    fn the_allowlist_lists_every_event_of_the_spec_with_every_value() {
        let events = file()["events"].as_table().unwrap().clone();
        let mut want = spec_table();
        let names: Vec<&str> = events.keys().map(String::as_str).collect();
        let mut spec_names: Vec<&str> = want.keys().copied().chain(["palette", "settings", "theme"]).collect();
        spec_names.sort();
        assert_eq!(names, spec_names, "the file's events are the spec's");
        for (name, keys) in &events {
            let Some(spec) = want.remove(name.as_str()) else { continue };
            let keys = keys.as_table().unwrap();
            assert_eq!(keys.len(), spec.len(), "{name}'s keys");
            for (key, values) in spec {
                let listed: Vec<&str> = keys[key].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
                assert_eq!(listed, values, "{name}.{key}, in the spec's order");
                for v in values {
                    assert_eq!(allowlist().check(&event(name, &[(key, DValue::Str(v.into()))])), Ok(()), "{name}.{key} = {v}");
                }
            }
        }
        assert!(want.is_empty(), "every spec event is in the file: {want:?}");
        assert!(events["palette"].as_table().unwrap()["command"].as_array().is_some_and(|a| !a.is_empty()));
        assert!(events["settings"].as_table().unwrap()["key"].as_array().is_some_and(|a| !a.is_empty()));
    }

    #[test]
    fn the_allowlist_parses_through_the_crate_and_refuses_what_it_does_not_list() {
        let text = include_str!("../../../telemetry/events.toml");
        assert_eq!(&Allowlist::from_toml(text).unwrap(), allowlist());
        let a = allowlist();
        assert_eq!(a.check(&event("window_open", &[])), Ok(()));
        assert_eq!(a.check(&event("theme", &[("builtin", DValue::Bool(true))])), Ok(()));
        assert_eq!(a.check(&event("theme", &[("builtin", DValue::Bool(false))])), Ok(()));
        assert_eq!(a.check(&event("theme", &[("name", DValue::Str("navy".into()))])), Err(Rejected::Key("theme".into(), "name".into())));
        assert_eq!(
            a.check(&event("pane_open", &[("kind", DValue::Str("editor".into()))])),
            Err(Rejected::Value("pane_open".into(), "kind".into()))
        );
        assert_eq!(a.check(&event("file_opened", &[])), Err(Rejected::Name("file_opened".into())));
        assert_eq!(
            a.check(&event("settings", &[("key", DValue::Str("font.presets.mine".into()))])),
            Err(Rejected::Value("settings".into(), "key".into())),
            "a preset's name is never sent"
        );
        for s in ["terminal", "viewer", "browser", "settings"] {
            assert!(a.allows_surface(s), "{s}");
        }
        assert!(!a.allows_surface("chooser"));
    }

    #[test]
    fn the_channel_is_release_only_when_the_bundle_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("Arkos IDE.app");
        std::fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        assert_eq!(channel_of(&bundle), Channel::Source, "no channel file");
        std::fs::write(bundle.join("Contents/Resources/channel"), "release\n").unwrap();
        assert_eq!(channel_of(&bundle), Channel::Release);
        std::fs::write(bundle.join("Contents/Resources/channel"), "source\n").unwrap();
        assert_eq!(channel_of(&bundle), Channel::Source);
        std::fs::write(bundle.join("Contents/Resources/channel"), "").unwrap();
        assert_eq!(channel_of(&bundle), Channel::Source);
        assert_eq!(channel_of(&dir.path().join("missing.app")), Channel::Source);
    }

    #[test]
    fn a_binary_outside_a_bundle_has_no_bundle() {
        let exe = Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkosd");
        assert_eq!(bundle_of(exe), Some(PathBuf::from("/Applications/Arkos IDE.app")));
        assert_eq!(bundle_of(Path::new("/Users/me/ide-arkos/target/debug/arkos")), None);
        assert_eq!(bundle_of(Path::new("/x/Contents/MacOS/arkos")), None, "not an .app");
        assert_eq!(bundle_of(Path::new("arkos")), None);
        // The test binary runs from target/, so this process is a source build.
        assert_eq!(channel(), Channel::Source);
    }

    #[test]
    fn the_os_version_is_the_major_or_zero() {
        assert_eq!(major_of("15.6.1"), 15);
        assert_eq!(major_of("26.0\n"), 26);
        assert_eq!(major_of("14"), 14);
        assert_eq!(major_of(""), 0);
        assert_eq!(major_of("unknown"), 0);
        assert!(osv() >= 11, "this Mac runs macOS 11 or later: {}", osv());
        assert_eq!(osv(), osv(), "read once");
    }

    #[test]
    fn the_envelope_names_arkos_the_binary_and_the_build() {
        let env = envelope(Bin::Daemon);
        assert_eq!(env.app, "arkos");
        assert_eq!(env.ver, env!("CARGO_PKG_VERSION"));
        assert_eq!(env.bin, "daemon");
        assert_eq!(env.os, Os::Macos);
        assert_eq!(env.osv, osv());
        assert_eq!(env.arch, if cfg!(target_arch = "aarch64") { Arch::Arm64 } else { Arch::X86_64 });
        assert_eq!(env.ch, Channel::Source);
        assert_eq!(envelope(Bin::App).bin, "app");
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn the_defaults_record_locally_and_invite_only_a_person_on_a_release() {
        let defaults = TelemetryConfig::default();
        let m = mode_with(Ok::<_, Error>(&defaults), &no_env, &Channel::Release, true);
        assert_eq!((m.usage, m.crashes, m.uploads, m.invite, m.reason), (Usage::Local, Crashes::Ask, false, true, None));
        let m = mode_with(Ok::<_, Error>(&defaults), &no_env, &Channel::Release, false);
        assert_eq!((m.usage, m.uploads, m.invite, m.reason), (Usage::Local, false, false, Some("not interactive")));
        let m = mode_with(Ok::<_, Error>(&defaults), &no_env, &Channel::Source, true);
        assert_eq!((m.usage, m.uploads, m.invite, m.reason), (Usage::Local, false, false, Some("source build")));
        let on = TelemetryConfig { usage: Usage::On, crashes: Crashes::Always };
        let m = mode_with(Ok::<_, Error>(&on), &no_env, &Channel::Release, true);
        assert_eq!((m.usage, m.crashes, m.uploads, m.invite), (Usage::On, Crashes::Always, true, false));
    }

    #[test]
    fn the_environment_variable_is_arkos_telemetry() {
        let on = TelemetryConfig { usage: Usage::On, crashes: Crashes::Always };
        let off = |k: &str| (k == "ARKOS_TELEMETRY").then(|| "off".to_string());
        let m = mode_with(Ok::<_, Error>(&on), &off, &Channel::Release, true);
        assert_eq!((m.usage, m.crashes, m.uploads, m.reason), (Usage::Off, Crashes::Never, false, Some("telemetry off")));
        let log = |k: &str| (k == "ARKOS_TELEMETRY").then(|| "log".to_string());
        let m = mode_with(Ok::<_, Error>(&on), &log, &Channel::Release, true);
        assert!(m.log && !m.uploads);
    }

    #[test]
    fn a_config_error_turns_telemetry_off_never_to_the_defaults() {
        let parsed = parse_table("[telemetry]\nusage = \"maybe\"\n");
        let m = mode_with(parsed.as_ref(), &no_env, &Channel::Release, true);
        assert_eq!((m.usage, m.crashes, m.uploads, m.invite, m.log), (Usage::Off, Crashes::Never, false, false, false));
        assert_eq!(m.reason, Some("config error"));
        let m = mode_with(Err::<&TelemetryConfig, _>("unreadable"), &no_env, &Channel::Source, true);
        assert_eq!((m.usage, m.crashes, m.uploads, m.invite), (Usage::Off, Crashes::Never, false, false));
        assert_eq!(m.reason, Some("source build"), "an earlier rule keeps its reason");
    }

    #[test]
    fn the_second_launch_is_a_later_utc_day_than_the_install() {
        let dir = tempfile::tempdir().unwrap();
        let installed = dir.path().join("installed");
        // 2026-10-16 09:00 UTC.
        let at = 1_792_141_200;
        assert!(!second_launch(&installed, at), "no file: this is the first launch");
        std::fs::write(&installed, format!("{at}\n")).unwrap();
        assert!(!second_launch(&installed, at), "the install itself");
        assert!(!second_launch(&installed, at + 14 * 3600), "later on the install day, 23:00 UTC");
        assert!(second_launch(&installed, at + 15 * 3600), "00:00 UTC the next day");
        assert!(second_launch(&installed, at + 30 * 86_400));
        assert!(!second_launch(&installed, at - 86_400), "a clock set back is not a later day");
        std::fs::write(&installed, "garbage\n").unwrap();
        assert!(!second_launch(&installed, at + 30 * 86_400), "an unreadable file counts as missing");
    }

    #[test]
    fn the_settings_mode_reads_the_table_as_given() {
        // The table's values are the person's choices, so the mode sees them as `Some`.
        let off = TelemetryConfig { usage: Usage::Off, crashes: Crashes::Never };
        let m = mode_with(Ok::<_, Error>(&off), &no_env, &Channel::Release, true);
        assert_eq!(m, visits_app::mode(&Settings { usage: Some(Usage::Off), crashes: Some(Crashes::Never) }, &no_env, ENV_VAR, &Channel::Release, true));
    }
}
