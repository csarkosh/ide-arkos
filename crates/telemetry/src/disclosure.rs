//! The disclosure page (telemetry spec, section 4): `docs/telemetry.md`, compiled in, the one
//! text every "What is sent" and "What Arkos sends" surface opens and the website publishes. Its
//! field and event tables are not written by hand: `render_tables` makes them from this module's
//! field table and from `telemetry/events.toml`, and the page holds that render between two
//! markers. The tests hold all of it to the code: the field table names exactly the keys of real
//! reports built by `visits_app`, the render lists every event, key and value the allowlist
//! admits, and the page's marked region is the render byte for byte. `write_page` writes the page
//! out so the app can open it.
use crate::EVENTS_TOML;
use std::io;
use std::path::{Path, PathBuf};

/// The page, `docs/telemetry.md`, as compiled in.
pub const PAGE: &str = include_str!("../../../docs/telemetry.md");

/// The marker before the rendered tables in the page.
pub const START: &str = "<!-- tables:start -->";

/// The marker after the rendered tables in the page.
pub const END: &str = "<!-- tables:end -->";

/// The name the page is written out under, in the telemetry folder.
pub const FILE_NAME: &str = "what-arkos-sends.md";

/// One key of a report: an example of its value and, in plain words, why it is there.
pub struct Field {
    pub key: &'static str,
    pub example: &'static str,
    pub reason: &'static str,
}

/// The keys of one kind of report, with the heading and the sentence the page shows above them.
pub struct Table {
    pub title: &'static str,
    pub intro: &'static str,
    pub fields: &'static [Field],
}

const fn field(key: &'static str, example: &'static str, reason: &'static str) -> Field {
    Field { key, example, reason }
}

/// The keys every report and the check carry (lib-visits's spec, section 6.2).
pub const ENVELOPE: Table = Table {
    title: "Every report: the envelope",
    intro: "The check, the session report and the crash report all start with these keys. They describe the copy of Arkos, never the person or the Mac.",
    fields: &[
        field("v", "`2`", "The version of the report format, so a later format can be read beside this one."),
        field("app", "`arkos`", "The program the report is about; the collector refuses one it does not know."),
        field("ver", "`0.5.0`", "The Arkos version, to see which releases are in use and which of them crash."),
        field("bin", "`app`", "The part of Arkos that wrote it: `app`, the window, or `daemon`, the background process."),
        field("os", "`macos`", "The operating system. Arkos runs on macOS only, so it is always `macos`."),
        field("osv", "`15`", "The major macOS version, to know which versions to keep supporting. The minor version is not sent."),
        field("arch", "`arm64`", "The processor type, `arm64` for Apple silicon or `x86_64` for Intel, to know which builds matter."),
        field("ch", "`release`", "How this copy was built: `release` for the published app. A copy built from source says `source` and sends nothing."),
        field("k", "`session`", "The kind of report: `check`, `session` or `crash`."),
    ],
};

/// The update check's one key beyond the envelope (section 6.3).
pub const CHECK: Table = Table {
    title: "The update check",
    intro: "Sent by the daemon once a day at most, as the query of the first update check of the day. It needs no consent: an update check is what a self-updating program sends.",
    fields: &[field(
        "rd",
        "`1`",
        "Days since this copy's last check through this path, rounded down, or `-1` the first time. It counts installs in use and how often they return, without any id.",
    )],
};

/// The session report's keys beyond the envelope (section 6.4).
pub const SESSION: Table = Table {
    title: "The session report",
    intro: "One report for one run of a window, recorded only while usage reports are `local` or `on`, and sent only when they are `on`.",
    fields: &[
        field(
            "sid",
            "`9f2c4e1a7b3d5f60a1c2e3f4b5d6e7f8`",
            "A random number made when the window starts and forgotten when it ends. It ties one run's events together; no two runs share it.",
        ),
        field("len", "`5400`", "How long the run lasted, in seconds."),
        field(
            "end",
            "`quit`",
            "How the run ended: `quit`, `lost` when the window went away without quitting (a crash or a force quit), or `unknown` when it was found left behind. The share of `lost` runs is the crash rate.",
        ),
        field(
            "iw",
            "`20370`",
            "The week Arkos was first used on this Mac, as days since 1 January 1970 rounded down to a multiple of 7. It shows how long people keep using Arkos; everyone who started that week sends the same number.",
        ),
        field("rd", "`2`", "Days since the last session report from this copy, or `-1` for the first. It shows how often Arkos is used, without an id."),
        field(
            "focus",
            "`{\"terminal\": 3100, \"viewer\": 900}`",
            "Seconds the window was in front, per kind of tab and for Settings, to see which parts of Arkos are used.",
        ),
        field("ev", "`[{\"n\": \"launch\", \"t\": 0, \"d\": {\"how\": \"normal\"}}]`", "The run's events in order, at most 200. Each one is in the events table below."),
        field("ev_dropped", "`0`", "How many events came after the 200th, counted and not kept."),
        field("ev_rejected", "`0`", "How many events the allowlist refused, counted so a mistake in Arkos shows up. The events themselves are never written."),
    ],
};

/// The keys of one event in a session report's `ev`.
pub const EVENT: Table = Table {
    title: "An event",
    intro: "Each entry of `ev` has these keys.",
    fields: &[
        field("n", "`pane_open`", "The event's name, one of the events table below."),
        field("t", "`42`", "Seconds since the window started."),
        field("d", "`{\"kind\": \"terminal\", \"how\": \"key\"}`", "The event's details: only the keys and values the events table allows, never a name, a path or anything typed."),
    ],
};

/// The crash report's keys beyond the envelope (section 6.5).
pub const CRASH: Table = Table {
    title: "The crash report",
    intro: "Written when Arkos panics, and sent only after you agree: in the dialog for that crash, or for every crash once you choose to always send them.",
    fields: &[
        field(
            "inc",
            "`b7e14c09d2a35f8e6c1b0a9d8e7f6a5b`",
            "A random number for this one crash, shown under Reports in Settings so you can quote it in an issue.",
        ),
        field("ck", "`panic`", "The kind of crash. Arkos reports panics, Rust's own failures."),
        field("commit", "`4f2a9c1`", "The source commit of the build, to find the exact code that failed."),
        field("pk", "`other`", "The name of one of Arkos's own failure points when the panic came from one, else `other`."),
        field(
            "mh",
            "`5d41402abc4b2a76b9719d911017c592ae2d1b7a8e3f6c0d9b8a7f6e5d4c3b2a`",
            "For an `other` panic, the SHA-256 hash of the message's first line with every path and number taken out. Crashes with one cause share it; the message is never sent.",
        ),
        field(
            "file",
            "`/src/crates/arkos/src/md_view.rs`",
            "The source file the panic came from, as a path inside the Arkos source (`/src/`), a dependency (`/cargo/`) or Rust itself (`/rustc/`); any other path is cut to its file name.",
        ),
        field("line", "`812`", "The line in that file."),
        field(
            "frames",
            "`[[0, 1843200], [0, 1790466]]`",
            "The call stack, as offsets into the Arkos binary. The maintainer turns them into function names with the published debug symbols. No memory is sent.",
        ),
        field(
            "images",
            "`[[\"arkos\", \"6c3f0e2a9b8d4c7e1f5a0b9c8d7e6f5a\", 48234496]]`",
            "The Arkos binary the offsets belong to: its name, its build id and its size. No other program or library is listed.",
        ),
        field("sig", "`11`", "The signal number of a crash by signal. Arkos does not send it: it reports panics only."),
        field("up", "`<10m`", "How long the process had been running, in one of five buckets: `<1m`, `<10m`, `<1h`, `<1d`, `>=1d`."),
        field("mem", "`normal`", "How short of memory the Mac was. Arkos does not send it."),
    ],
};

/// The field tables in the order the page shows them; the events table comes between `EVENT`
/// and `CRASH`.
pub const TABLES: &[&Table] = &[&ENVELOPE, &CHECK, &SESSION, &EVENT, &CRASH];

/// When each event of `telemetry/events.toml` is recorded, in plain words. A test holds this list
/// to the file, so an event added there needs its sentence here before the page can render.
pub const EVENT_WHEN: &[(&str, &str)] = &[
    ("launch", "The window started; `recovered` when the last window had died."),
    ("window_open", "A project window opened."),
    ("window_close", "A project window closed."),
    ("pane_open", "A tab was opened, and how."),
    ("pane_close", "A tab was closed."),
    ("view", "The viewer showed a file, by its kind, never its name."),
    ("md", "A markdown feature was used for the first time in this run, once per feature."),
    ("agent_launch", "A coding agent started in a terminal."),
    ("agent_state", "An agent first asked for input, once per terminal."),
    ("palette", "A command ran from the command palette."),
    ("settings", "A setting was changed: the key, never its value."),
    ("theme", "A theme was chosen. A built-in theme is not named, and a theme of your own is just `false`."),
    ("update", "The updater moved a step."),
    ("notice", "The bell got a notice."),
    ("error", "Something went wrong where you would have seen it."),
    ("help", "A Help surface opened."),
];

/// The tables the page shows between its markers, as Markdown: the envelope, the check, the
/// session report, an event, the events with their keys and values (from `events.toml`, in its
/// order) and the crash report. It is framed by blank lines, so it sits between the markers as
/// blocks of its own.
pub fn render_tables() -> String {
    let mut out = String::from("\n");
    for table in [&ENVELOPE, &CHECK, &SESSION, &EVENT] {
        out.push_str(&field_table(table));
    }
    out.push_str(&events_table());
    out.push_str(&field_table(&CRASH));
    out.push('\n');
    out
}

/// One field table under its heading and its sentence.
fn field_table(table: &Table) -> String {
    let mut out = format!("\n### {}\n\n{}\n\n| Key | Example | Why it is there |\n|---|---|---|\n", table.title, table.intro);
    for f in table.fields {
        out.push_str(&format!("| `{}` | {} | {} |\n", f.key, f.example, f.reason));
    }
    out
}

/// The events table and the focus surfaces, from `events.toml`.
fn events_table() -> String {
    // The file is compiled in and the crate's tests parse it, so a failure here never ships.
    let file: toml::Table = EVENTS_TOML.parse().expect("telemetry/events.toml parses");
    let events = file.get("events").and_then(toml::Value::as_table).cloned().unwrap_or_default();
    let mut out = String::from(
        "\n### The events\n\nAn event is one of these names, with only these keys, each taking only the values listed. Anything else is dropped before it is written and counted in `ev_rejected`.\n\n| Event | Keys and allowed values | When |\n|---|---|---|\n",
    );
    let mut names: Vec<&str> = events.keys().map(String::as_str).collect();
    names.sort_by_key(|name| EVENTS_TOML.find(&format!("[events.{name}]")).unwrap_or(usize::MAX));
    for name in names {
        let keys = events[name].as_table().cloned().unwrap_or_default();
        // A key's place is its first line after the event's header, which is its own line in
        // the event's table, so the keys keep the file's order.
        let header = EVENTS_TOML.find(&format!("[events.{name}]")).unwrap_or(0);
        let mut ordered: Vec<&str> = keys.keys().map(String::as_str).collect();
        ordered.sort_by_key(|key| EVENTS_TOML[header..].find(&format!("\n{key} =")).unwrap_or(usize::MAX));
        let described: Vec<String> = ordered.iter().map(|key| format!("`{key}`: {}", values_of(&keys[*key]))).collect();
        let described = if described.is_empty() { "none".to_string() } else { described.join("; ") };
        let when = EVENT_WHEN.iter().find(|(n, _)| *n == name).map_or("", |(_, w)| w);
        out.push_str(&format!("| `{name}` | {described} | {when} |\n"));
    }
    let surfaces: Vec<String> = file
        .get("focus")
        .and_then(|f| f.get("surfaces"))
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_str).map(|s| format!("`{s}`")).collect())
        .unwrap_or_default();
    out.push_str(&format!("\nThe keys of `focus` are these surfaces only: {}.\n", surfaces.join(", ")));
    out
}

/// A key's allowed values as the page writes them: each value in backticks, `"bool"` as `true`
/// and `false`, `"int"` as a whole number.
fn values_of(allowed: &toml::Value) -> String {
    match allowed {
        toml::Value::Array(values) => values.iter().filter_map(toml::Value::as_str).map(|v| format!("`{v}`")).collect::<Vec<_>>().join(", "),
        toml::Value::String(kind) if kind == "bool" => "`true`, `false`".to_string(),
        toml::Value::String(kind) if kind == "int" => "a whole number".to_string(),
        _ => String::new(),
    }
}

/// The text between the page's markers, or `None` when either is missing or they are out of order.
pub fn region(page: &str) -> Option<&str> {
    let from = page.find(START)? + START.len();
    let to = page[from..].find(END)? + from;
    Some(&page[from..to])
}

/// The page as it is written out: `PAGE` without its two markers, which the viewer would show as
/// code (a website's Markdown hides them).
pub fn page() -> String {
    PAGE.replace(START, "").replace(END, "")
}

/// Writes the page to `<dir>/what-arkos-sends.md`, making `dir` when it is missing and replacing
/// an older copy, and returns the path. `dir` is the telemetry folder.
pub fn write_page(dir: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE_NAME);
    let text = page();
    if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        std::fs::write(&path, text)?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use visits_app::{Arch, Channel, Check, Crash, CrashKind, DValue, End, Envelope, Event, Memory, Os, Session, Uptime};

    fn env() -> Envelope {
        Envelope { app: "arkos".into(), ver: "0.5.0".into(), bin: "app".into(), os: Os::Macos, osv: 15, arch: Arch::Arm64, ch: Channel::Release }
    }

    fn keys(table: &Table) -> BTreeSet<&'static str> {
        table.fields.iter().map(|f| f.key).collect()
    }

    fn object_keys(value: &serde_json::Value) -> BTreeSet<String> {
        value.as_object().expect("an object").keys().cloned().collect()
    }

    fn as_strs(set: &BTreeSet<String>) -> BTreeSet<&str> {
        set.iter().map(String::as_str).collect()
    }

    /// A session report as `seal` makes one, with an event that carries a `d`.
    fn session() -> serde_json::Value {
        let d: BTreeMap<String, DValue> = [("kind".to_string(), DValue::Str("terminal".into())), ("how".to_string(), DValue::Str("key".into()))].into();
        let s = Session {
            env: env(),
            sid: visits_app::new_id(),
            len: 5400,
            end: End::Quit,
            iw: 20_370,
            rd: 2,
            focus: [("terminal".to_string(), 3100), ("viewer".to_string(), 900)].into(),
            ev: vec![Event { n: "pane_open".into(), t: 42, d }],
            ev_dropped: 0,
            ev_rejected: 0,
        };
        serde_json::from_slice(&s.to_bytes().expect("a valid session")).unwrap()
    }

    /// A crash report with every optional key set, so the table must name all of them.
    fn crash() -> serde_json::Value {
        let c = Crash {
            env: env(),
            inc: visits_app::new_id(),
            ck: CrashKind::Panic,
            commit: "4f2a9c1".into(),
            pk: "other".into(),
            mh: Some("a".repeat(64)),
            file: "/src/crates/arkos/src/md_view.rs".into(),
            line: 812,
            frames: vec![(0, 1_843_200)],
            images: vec![("arkos".into(), "0".repeat(32), 48_234_496)],
            sig: Some(11),
            up: Uptime::Lt10m,
            mem: Some(Memory::Normal),
        };
        serde_json::from_slice(&c.to_bytes().expect("a valid crash")).unwrap()
    }

    /// The envelope's keys, which every report and the check carry.
    fn envelope_keys() -> BTreeSet<&'static str> {
        keys(&ENVELOPE)
    }

    #[test]
    fn the_field_table_names_exactly_the_keys_of_a_real_session_report() {
        let report = session();
        let all = object_keys(&report);
        let own: BTreeSet<&str> = as_strs(&all).difference(&envelope_keys()).copied().collect();
        assert_eq!(own, keys(&SESSION), "the session table, no key missing and none extra");
        let envelope: BTreeSet<&str> = as_strs(&all).intersection(&envelope_keys()).copied().collect();
        assert_eq!(envelope, envelope_keys(), "the report carries the whole envelope");
        let event = &report["ev"][0];
        assert_eq!(as_strs(&object_keys(event)), keys(&EVENT), "the event table");
    }

    #[test]
    fn the_field_table_names_exactly_the_keys_of_a_real_crash_report() {
        let all = object_keys(&crash());
        let own: BTreeSet<&str> = as_strs(&all).difference(&envelope_keys()).copied().collect();
        assert_eq!(own, keys(&CRASH), "the crash table, no key missing and none extra");
        assert!(envelope_keys().is_subset(&as_strs(&all)));
    }

    #[test]
    fn the_field_table_names_exactly_the_keys_of_a_real_check() {
        let query = Check { env: env(), rd: 1 }.to_query().expect("a valid check");
        let all: BTreeSet<&str> = query.split('&').map(|pair| pair.split_once('=').expect("key=value").0).collect();
        assert_eq!(all.len(), query.split('&').count(), "no key twice");
        let own: BTreeSet<&str> = all.difference(&envelope_keys()).copied().collect();
        assert_eq!(own, keys(&CHECK), "the check table");
        assert_eq!(all.intersection(&envelope_keys()).count(), envelope_keys().len(), "the check carries the whole envelope");
    }

    #[test]
    fn every_field_has_an_example_and_a_reason_and_no_key_is_listed_twice() {
        for table in TABLES {
            assert_eq!(keys(table).len(), table.fields.len(), "{}: a key twice", table.title);
            for f in table.fields {
                assert!(!f.example.is_empty() && !f.reason.is_empty(), "{}.{}", table.title, f.key);
                assert!(!f.example.contains('|') && !f.reason.contains('|'), "{}.{}: a | breaks the table", table.title, f.key);
                assert!(f.reason.ends_with('.'), "{}.{}: the reason is a sentence", table.title, f.key);
            }
        }
    }

    #[test]
    fn every_event_of_the_allowlist_has_a_description_and_no_other_does() {
        let file: toml::Table = EVENTS_TOML.parse().unwrap();
        let in_file: BTreeSet<&str> = file["events"].as_table().unwrap().keys().map(String::as_str).collect();
        let described: BTreeSet<&str> = EVENT_WHEN.iter().map(|(n, _)| *n).collect();
        assert_eq!(described.len(), EVENT_WHEN.len(), "an event described twice");
        assert_eq!(described, in_file);
    }

    /// Review Focus 7: every event, key and value the allowlist admits is in the rendered event
    /// table, on its event's row, and so are the focus surfaces.
    #[test]
    fn the_render_lists_every_event_key_and_value_of_events_toml() {
        let render = render_tables();
        let file: toml::Table = EVENTS_TOML.parse().unwrap();
        for (name, event_keys) in file["events"].as_table().unwrap() {
            let row = render.lines().find(|l| l.starts_with(&format!("| `{name}` |"))).unwrap_or_else(|| panic!("no row for {name}"));
            for (key, allowed) in event_keys.as_table().unwrap() {
                assert!(row.contains(&format!("`{key}`:")), "{name}.{key}");
                let values: Vec<String> = match allowed {
                    toml::Value::Array(vs) => vs.iter().map(|v| v.as_str().unwrap().to_string()).collect(),
                    toml::Value::String(s) if s == "bool" => vec!["true".into(), "false".into()],
                    other => panic!("{name}.{key}: {other:?}"),
                };
                for v in values {
                    assert!(row.contains(&format!("`{v}`")), "{name}.{key} = {v}");
                }
            }
        }
        let surfaces = file["focus"]["surfaces"].as_array().unwrap();
        let line = render.lines().find(|l| l.contains("`focus`") && l.contains("surfaces")).expect("the focus line");
        for s in surfaces {
            assert!(line.contains(&format!("`{}`", s.as_str().unwrap())), "focus surface {s}");
        }
        for table in TABLES {
            assert!(render.contains(&format!("### {}", table.title)), "{}", table.title);
            for f in table.fields {
                assert!(render.contains(&format!("| `{}` | {} | {} |", f.key, f.example, f.reason)), "{}.{}", table.title, f.key);
            }
        }
    }

    /// Review Focus 7: the page's marked region is the render, byte for byte. When this fails
    /// after a change to the field table or to `events.toml`, run the ignored test below
    /// (`cargo test -p telemetry -- --ignored write_the_tables`) and commit the page it writes.
    #[test]
    fn the_marked_region_of_the_page_is_the_render() {
        assert_eq!(PAGE.matches(START).count(), 1, "one start marker");
        assert_eq!(PAGE.matches(END).count(), 1, "one end marker");
        assert_eq!(region(PAGE), Some(render_tables().as_str()));
    }

    #[test]
    #[ignore = "writes docs/telemetry.md; run by hand after the tables change"]
    fn write_the_tables_into_the_page() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/telemetry.md");
        let text = std::fs::read_to_string(&path).unwrap();
        let (before, rest) = text.split_once(START).expect("the start marker");
        let (_, after) = rest.split_once(END).expect("the end marker");
        std::fs::write(&path, format!("{before}{START}{}{END}{after}", render_tables())).unwrap();
    }

    #[test]
    fn the_page_carries_what_the_spec_asks_of_it() {
        for needle in [
            // The switches, the Dock note and `log`.
            "DO_NOT_TRACK",
            "ARKOS_TELEMETRY=off",
            "ARKOS_TELEMETRY=log",
            "`CI`",
            "Dock",
            "usage = ",
            "crashes = ",
            "arkos telemetry",
            // The authoritative files.
            "`telemetry/events.toml`",
            "`crates/telemetry/`",
            "`crates/arkos/src/telemetry.rs`",
            "`crates/arkosd/src/telemetry/`",
            "`crates/visits-core/src/app.rs`",
            // The legal sentences.
            "controller",
            "consent",
            "supervisory authority",
            "github.com/csarkosh",
        ] {
            assert!(PAGE.contains(needle), "the page mentions {needle}");
        }
    }

    #[test]
    fn neither_the_page_nor_the_render_has_an_em_dash() {
        assert!(!PAGE.contains('\u{2014}'), "the page");
        assert!(!render_tables().contains('\u{2014}'), "the render");
    }

    #[test]
    fn write_page_writes_the_page_without_its_markers_and_replaces_an_old_copy() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = dir.path().join("telemetry");
        let path = write_page(&telemetry).unwrap();
        assert_eq!(path, telemetry.join("what-arkos-sends.md"), "made in a folder that did not exist");
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, page());
        assert!(written.contains(&render_tables()), "the tables are in it");
        assert!(!written.contains(START) && !written.contains(END), "the markers are not shown in the viewer");
        std::fs::write(&path, "an older page").unwrap();
        assert_eq!(write_page(&telemetry).unwrap(), path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), page(), "an old copy is replaced");
    }
}
