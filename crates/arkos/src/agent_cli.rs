//! The agent theme interface (agent theme spec, sections 4 to 6): `arkos theme` (template,
//! contrast, add, use, preview and current; `list` and `validate` stay in `theme.rs`), `arkos
//! settings get|set` and `arkos config reload`. An agent drives these from a shell, with or
//! without a window: files are read and written here, and running windows are asked through the
//! daemon's `Customize` relay (`customize`). No gpui here. Every line goes through the `out` and
//! `err` writers; `--json` anywhere in the arguments prints one JSON value on stdout instead.
//! Exit codes: 0 done, 1 refused or failed, 2 usage, 3 a window was needed and none is running.

use crate::config::{self, AppearanceMode, TextPreset};
use crate::{hook_cli, theme};
use protocol::{CustomizeAction, CustomizeOutcome, Report, Request, Response};
use serde_json::{json, Value as Json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `arkos --help`.
pub const USAGE: &str = "\
usage: arkos guide                  how an agent changes the theme and settings; read it first
       arkos [<folder>]             open a window on a folder (default: the current one)
       arkos theme <command> ...    list, validate, template, contrast, add, use, preview, current
       arkos settings get|set ...   read the config, or set a key on the allow-list
       arkos config reload          ask every window to read its config again now
       arkos doctor                 read-only diagnostics
       arkos hooks print|install    the Claude Code hook entries
       arkos --version
       arkos --help
A folder named like a command opens with a path: arkos ./config
";

const THEME_USAGE: &str = "\
usage: arkos theme list [--json]
       arkos theme validate <file> [--json]
       arkos theme template [<built-in>] [--json]
       arkos theme contrast <file-or-name> [--json]
       arkos theme add <file> [--replace] [--json]
       arkos theme use <name-or-file> [--dark|--light] [--replace] [--json]
       arkos theme preview <name-or-file> [--json]
       arkos theme preview --end [--json]
       arkos theme current [--json]
see: arkos guide
";

const SETTINGS_USAGE: &str = "\
usage: arkos settings get [<key>] [--json]
       arkos settings set <key> <value> [--json]
see: arkos guide
";

const CONFIG_USAGE: &str = "usage: arkos config reload [--json]\nsee: arkos guide\n";

/// The keys `arkos settings set` may write. A `*` segment matches exactly one bare name.
pub const SETTABLE: &[&str] = &[
    "appearance.mode",
    "theme.dark",
    "theme.light",
    "font.family",
    "font.preset",
    "font.presets.*.size",
    "font.presets.*.line_height",
    "viewer.wide_tables",
    "update.auto",
    "recovery.start_at_login",
];

/// The daemon's answer to one `Customize`: the windows that answered, the pids of those asked
/// that did not answer in time, and the pids of project windows too old to be asked.
#[derive(Debug)]
pub struct Answer {
    pub outcomes: Vec<CustomizeOutcome>,
    pub missing: Vec<u32>,
    pub too_old: Vec<u32>,
}

impl Answer {
    /// No project window is connected at all.
    fn nobody(&self) -> bool {
        self.outcomes.is_empty() && self.missing.is_empty() && self.too_old.is_empty()
    }
}

/// Why a `Customize` got no answer.
#[derive(Debug)]
pub enum Reach {
    /// No daemon socket, or nothing listening on it.
    NoWindow,
    /// The daemon hung up without answering, as a 0.3 daemon that cannot decode the request does.
    OldDaemon,
    /// Any other failure, with its text.
    Io(String),
}

impl Reach {
    fn line(&self) -> String {
        match self {
            Reach::NoWindow => "no window running".into(),
            Reach::OldDaemon => "arkosd is older than this command; use Update now in Arkos IDE, or run pkill -x arkosd (ends every session)".into(),
            Reach::Io(e) => e.clone(),
        }
    }
}

/// Sends `action` to every project window through the daemon and waits for the answer: the
/// daemon replies within `wait` (its `CUSTOMIZE_WAIT`), so the socket waits one second longer.
/// No Hello: a tool client stays on protocol 0, outside every broadcast.
pub fn customize(socket: &Path, action: CustomizeAction, wait: Duration) -> Result<Answer, Reach> {
    let id = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    let timeout = wait + Duration::from_secs(1);
    match hook_cli::send_one_with(socket, Request::Customize { id, action }, timeout) {
        Ok(Response::Customized { outcomes, missing, too_old }) => Ok(Answer { outcomes, missing, too_old }),
        Ok(Response::Error(e)) => Err(Reach::Io(format!("arkosd answered: {e}"))),
        Ok(other) => Err(Reach::Io(format!("arkosd answered something else: {other:?}"))),
        Err(e) => Err(match e.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => Reach::NoWindow,
            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe => Reach::OldDaemon,
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                Reach::Io(format!("arkosd did not answer within {} s", timeout.as_secs()))
            }
            _ => Reach::Io(format!("cannot reach arkosd: {e}")),
        }),
    }
}

/// Where the commands read and write: the daemon socket, `config.toml`, the themes folder, and
/// how long the daemon waits for windows.
pub struct Env<'a> {
    pub socket: &'a Path,
    pub config: &'a Path,
    pub themes: &'a Path,
    pub wait: Duration,
}

type Command = fn(&[String], &Env, &mut dyn Write, &mut dyn Write) -> i32;

fn with_real_env(command: Command, args: &[String]) -> i32 {
    let (socket, config, themes) = (crate::paths::socket_path(), crate::paths::config_file(), crate::paths::themes_dir());
    let env = Env { socket: &socket, config: &config, themes: &themes, wait: protocol::CUSTOMIZE_WAIT };
    command(args, &env, &mut std::io::stdout(), &mut std::io::stderr())
}

/// `arkos theme ...` with the real paths and the process's streams.
pub fn theme_cli(args: &[String]) -> i32 {
    with_real_env(theme_cli_to, args)
}

/// `arkos settings ...` with the real paths and the process's streams.
pub fn settings_cli(args: &[String]) -> i32 {
    with_real_env(settings_cli_to, args)
}

/// `arkos config ...` with the real paths and the process's streams.
pub fn config_cli(args: &[String]) -> i32 {
    with_real_env(config_cli_to, args)
}

/// The positional words and the `--flags` of a command's arguments.
struct Args<'a> {
    words: Vec<&'a str>,
    flags: Vec<&'a str>,
}

impl Args<'_> {
    fn has(&self, flag: &str) -> bool {
        self.flags.contains(&flag)
    }
}

/// Splits `args`; a flag other than `--json` and those in `allowed` is a usage error (`None`).
fn split<'a>(args: &'a [String], allowed: &[&str]) -> Option<Args<'a>> {
    let mut a = Args { words: Vec::new(), flags: Vec::new() };
    for arg in args {
        if arg.starts_with("--") {
            if arg != "--json" && !allowed.contains(&arg.as_str()) {
                return None;
            }
            a.flags.push(arg);
        } else {
            a.words.push(arg);
        }
    }
    Some(a)
}

/// The first word (the subcommand) and every other argument, flags before it included.
fn subcommand(args: &[String]) -> (Option<&str>, Vec<String>) {
    match args.iter().position(|a| !a.starts_with("--")) {
        Some(i) => (Some(args[i].as_str()), args.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, a)| a.clone()).collect()),
        None => (None, args.to_vec()),
    }
}

fn usage(err: &mut dyn Write, text: &str) -> i32 {
    let _ = err.write_all(text.as_bytes());
    2
}

fn fail(err: &mut dyn Write, text: &str) -> i32 {
    let _ = writeln!(err, "{text}");
    1
}

/// `arkos theme ...` against `env`. `list` and `validate` are `theme::theme_cli_to`'s.
pub fn theme_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (sub, rest) = subcommand(args);
    match sub {
        Some("list") | Some("validate") => theme::theme_cli_to(args, env.themes, out, err),
        Some("template") => template(&rest, out, err),
        Some("contrast") => contrast(&rest, env, out, err),
        Some("add") => add(&rest, env, out, err),
        Some("use") => use_theme(&rest, env, out, err),
        Some("preview") => preview(&rest, env, out, err),
        Some("current") => current(&rest, env, out, err),
        _ => usage(err, THEME_USAGE),
    }
}

fn template(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, THEME_USAGE) };
    let name = match a.words.as_slice() {
        [] => theme::BUILTIN[0].0,
        [name] => name,
        _ => return usage(err, THEME_USAGE),
    };
    let Some((_, text)) = theme::BUILTIN.iter().find(|(n, _)| *n == name) else {
        return fail(err, &format!("unknown built-in theme: {name}"));
    };
    if a.has("--json") {
        let _ = writeln!(out, "{}", schema_json());
    } else {
        let _ = out.write_all(text.as_bytes());
    }
    0
}

/// `{ "tables": [{ "name", "keys": [{ "key", "meaning", "contrast_with"?, "needs"? }] }] }`.
fn schema_json() -> Json {
    let tables: Vec<Json> = theme::schema()
        .iter()
        .map(|t| {
            let keys: Vec<Json> = t
                .keys
                .iter()
                .map(|k| {
                    let mut v = json!({ "key": k.key, "meaning": k.meaning });
                    if let (Some(with), Some(needs)) = (k.contrast_with, k.needs) {
                        v["contrast_with"] = json!(with);
                        v["needs"] = json!(needs);
                    }
                    v
                })
                .collect();
            json!({ "name": t.name, "keys": keys })
        })
        .collect();
    json!({ "tables": tables })
}

/// A theme by name (`unknown theme: {name}` when it is neither a file in the folder nor built
/// in; a broken file's own error otherwise).
fn load_name(name: &str, themes: &Path) -> Result<theme::Theme, String> {
    if theme::is_bare_name(name) && !themes.join(format!("{name}.toml")).exists() && theme::builtin(name).is_none() {
        return Err(format!("unknown theme: {name}"));
    }
    theme::load(name, themes)
}

/// The TOML text of a theme given as a path ending in `.toml` or as a name.
fn theme_text(target: &str, themes: &Path) -> Result<String, String> {
    if target.ends_with(".toml") {
        return std::fs::read_to_string(target).map_err(|e| format!("cannot read {target}: {e}"));
    }
    load_name(target, themes)?;
    let file = themes.join(format!("{target}.toml"));
    match theme::BUILTIN.iter().find(|(n, _)| *n == target) {
        Some((_, text)) if !file.exists() => Ok(text.to_string()),
        _ => std::fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {e}", file.display())),
    }
}

/// A theme given as a path ending in `.toml` or as a name, parsed.
fn load_target(target: &str, themes: &Path) -> Result<theme::Theme, String> {
    let text = theme_text(target, themes)?;
    theme::parse(&text).map_err(|e| format!("{target}: {e}"))
}

fn contrast(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, THEME_USAGE) };
    let [target] = a.words.as_slice() else { return usage(err, THEME_USAGE) };
    let t = match load_target(target, env.themes) {
        Ok(t) => t,
        Err(e) => return fail(err, &e),
    };
    let (ok, rows) = theme::contrast_report(&t);
    if a.has("--json") {
        let pairs: Vec<Json> = rows
            .iter()
            .map(|(p, r)| json!({ "what": p.what, "fg": p.fg.hex(), "bg": p.bg.hex(), "ratio": r, "needs": p.needs, "ok": *r >= p.needs }))
            .collect();
        let _ = writeln!(out, "{}", json!({ "name": t.name, "ok": ok, "pairs": pairs }));
    } else {
        for (p, r) in &rows {
            // A low ratio is cut down, not rounded, to two decimals, so one just under the
            // threshold never reads as reaching it.
            let _ = if *r >= p.needs { writeln!(out, "{} {r:.1}:1 ok", p.what) } else { writeln!(out, "{} {:.2}:1 LOW (needs {})", p.what, (r * 100.0).floor() / 100.0, p.needs) };
        }
    }
    if ok {
        0
    } else {
        1
    }
}

fn add(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &["--replace"]) else { return usage(err, THEME_USAGE) };
    let [file] = a.words.as_slice() else { return usage(err, THEME_USAGE) };
    match theme::import_file(Path::new(file), env.themes, a.has("--replace")) {
        Ok((name, replaced)) => {
            let _ = if a.has("--json") {
                writeln!(out, "{}", json!({ "name": name, "replaced": replaced }))
            } else if replaced {
                writeln!(out, "replaced {name}")
            } else {
                writeln!(out, "added {name}")
            };
            0
        }
        Err(e) if e == "exists" => {
            let name = theme::validate_file(Path::new(file)).unwrap_or_default();
            fail(err, &format!("exists: {name}; pass --replace"))
        }
        Err(e) => fail(err, &e),
    }
}

/// Adds a theme file for `use` and returns its name. A file already in the folder with the
/// same text (`use` run twice on one file) is not a conflict.
fn add_for_use(src: &Path, themes: &Path, replace: bool) -> Result<String, String> {
    match theme::import_file(src, themes, replace) {
        Ok((name, _)) => Ok(name),
        Err(e) if e == "exists" => {
            let text = std::fs::read_to_string(src).map_err(|e| format!("cannot read {}: {e}", src.display()))?;
            let name = theme::parse(&text)?.name;
            match std::fs::read_to_string(themes.join(format!("{name}.toml"))) {
                Ok(there) if there == text => Ok(name),
                _ => Err(format!("exists: {name}; pass --replace")),
            }
        }
        Err(e) => Err(e),
    }
}

/// Which `[theme]` key `use` writes: the flag's, else the fixed appearance mode's, else (system)
/// the running windows' appearance, else dark.
pub fn theme_key_for_use(flag: Option<&str>, mode: AppearanceMode, windows_dark: Option<bool>) -> &'static str {
    match (flag, mode, windows_dark) {
        (Some("--dark"), _, _) => "dark",
        (Some("--light"), _, _) => "light",
        (_, AppearanceMode::Dark, _) => "dark",
        (_, AppearanceMode::Light, _) => "light",
        (_, AppearanceMode::System, Some(false)) => "light",
        (_, AppearanceMode::System, _) => "dark",
    }
}

fn use_theme(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &["--dark", "--light", "--replace"]) else { return usage(err, THEME_USAGE) };
    let [target] = a.words.as_slice() else { return usage(err, THEME_USAGE) };
    if a.has("--dark") && a.has("--light") {
        return usage(err, THEME_USAGE);
    }
    let flag = a.flags.iter().copied().find(|f| *f == "--dark" || *f == "--light");
    let name = if target.ends_with(".toml") {
        match add_for_use(Path::new(target), env.themes, a.has("--replace")) {
            Ok(name) => name,
            Err(e) => return fail(err, &e),
        }
    } else {
        target.to_string()
    };
    if let Err(e) = load_name(&name, env.themes) {
        return fail(err, &e);
    }
    // Creates a missing config from the defaults, so there is a file to write into.
    let cfg = match config::load(env.config) {
        Ok(cfg) => cfg,
        Err(e) => return fail(err, &e.to_string()),
    };
    let windows_dark = match (flag, cfg.appearance.mode) {
        (None, AppearanceMode::System) => customize(env.socket, CustomizeAction::ReportState, env.wait)
            .ok()
            .and_then(|answer| answer.outcomes.iter().find_map(|o| o.result.as_ref().ok().map(|r| r.system_is_dark))),
        _ => None,
    };
    let key = theme_key_for_use(flag, cfg.appearance.mode, windows_dark);
    if let Err(e) = config::set_values(env.config, &[(&["theme", key], toml_edit::Value::from(name.as_str()))]) {
        return fail(err, &e);
    }
    let mode = cfg.appearance.mode;
    let judge = |r: &Report| {
        // A window in the other appearance keeps showing that appearance's theme, as it should.
        let active = theme_key_for_use(None, mode, Some(r.system_is_dark));
        match (&r.theme_error, &r.config_error) {
            (Some(e), _) | (None, Some(e)) => Err(e.clone()),
            _ if active != key => Ok(format!("saved for {key} mode; shows {} in {active} mode", r.theme)),
            _ if r.theme != name || r.preview.is_some() => Err(format!("shows {}", r.theme)),
            _ => Ok(format!("{name} applied")),
        }
    };
    reload_after_save(env, a.has("--json"), json!({ "name": name, "key": key, "saved": true }), &judge, out, err)
}

/// One answering window, judged: `Ok` holds the line's text and the theme it shows.
struct Verdict {
    pid: u32,
    project: PathBuf,
    result: Result<(String, String), String>,
}

type Judge<'a> = dyn Fn(&Report) -> Result<String, String> + 'a;

fn verdicts(answer: &Answer, judge: &Judge) -> Vec<Verdict> {
    let judged = |o: &CustomizeOutcome| match &o.result {
        Ok(r) => judge(r).map(|text| (text, r.theme.clone())),
        Err(e) => Err(e.clone()),
    };
    answer.outcomes.iter().map(|o| Verdict { pid: o.pid, project: o.project.clone(), result: judged(o) }).collect()
}

/// `window {pid}: no answer` and `window {pid}: too old for this command`.
fn silent_lines(answer: &Answer, out: &mut dyn Write) {
    for pid in &answer.missing {
        let _ = writeln!(out, "window {pid}: no answer");
    }
    for pid in &answer.too_old {
        let _ = writeln!(out, "window {pid}: too old for this command");
    }
}

/// Prints the windows' lines, or adds `windows`, `missing` and `too_old` to `base` and prints
/// it. True when at least one window answered and every one that did succeeded.
fn print_verdicts(answer: &Answer, judge: &Judge, json: bool, mut base: Json, out: &mut dyn Write) -> bool {
    let v = verdicts(answer, judge);
    if json {
        let windows: Vec<Json> = v
            .iter()
            .map(|w| match &w.result {
                Ok((_, theme)) => json!({ "pid": w.pid, "project": w.project, "theme": theme }),
                Err(e) => json!({ "pid": w.pid, "project": w.project, "error": e }),
            })
            .collect();
        base["windows"] = json!(windows);
        base["missing"] = json!(answer.missing);
        base["too_old"] = json!(answer.too_old);
        let _ = writeln!(out, "{base}");
    } else {
        for w in &v {
            let text = match &w.result {
                Ok((text, _)) => text,
                Err(e) => e,
            };
            let _ = writeln!(out, "window {} ({}): {text}", w.pid, w.project.display());
        }
        silent_lines(answer, out);
    }
    !v.is_empty() && v.iter().all(|w| w.result.is_ok())
}

/// After `use` or `settings set` wrote the file: asks every window to reload and judges each
/// answer. No daemon or no window is `saved; no window running`, exit 0; silent and too old
/// windows print their lines and do not fail the command, and when none answered at all
/// `saved; no window answered` comes first.
fn reload_after_save(env: &Env, json: bool, mut base: Json, judge: &Judge, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let none = Answer { outcomes: Vec::new(), missing: Vec::new(), too_old: Vec::new() };
    match customize(env.socket, CustomizeAction::ReloadConfig, env.wait) {
        Err(Reach::NoWindow) => saved_alone(json, base, &none, out),
        Ok(answer) if answer.nobody() => saved_alone(json, base, &answer, out),
        Ok(answer) => {
            if !json && answer.outcomes.is_empty() {
                let _ = writeln!(out, "saved; no window answered");
            }
            let ok = print_verdicts(&answer, judge, json, base, out);
            if ok || answer.outcomes.is_empty() {
                0
            } else {
                1
            }
        }
        Err(reach) => {
            if json {
                base["windows"] = json!([]);
                base["missing"] = json!([]);
                base["too_old"] = json!([]);
                let _ = writeln!(out, "{base}");
            }
            fail(err, &reach.line())
        }
    }
}

fn saved_alone(json: bool, base: Json, answer: &Answer, out: &mut dyn Write) -> i32 {
    if json {
        print_verdicts(answer, &|_| Ok(String::new()), true, base, out);
    } else {
        let _ = writeln!(out, "saved; no window running");
    }
    0
}

/// A live action alone (`preview`, `config reload`): exit 3 with no window, 1 when a window
/// reported an error or none answered, else 0. JSON `{ "windows", "missing", "too_old" }`.
fn live(env: &Env, action: CustomizeAction, judge: &Judge, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match customize(env.socket, action, env.wait) {
        Err(Reach::NoWindow) => {
            let _ = writeln!(err, "{}", Reach::NoWindow.line());
            3
        }
        Ok(answer) if answer.nobody() => {
            let _ = writeln!(err, "{}", Reach::NoWindow.line());
            3
        }
        Ok(answer) => {
            if print_verdicts(&answer, judge, json, json!({}), out) {
                0
            } else {
                1
            }
        }
        Err(reach) => fail(err, &reach.line()),
    }
}

fn preview(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &["--end"]) else { return usage(err, THEME_USAGE) };
    let json = a.has("--json");
    if a.has("--end") {
        if !a.words.is_empty() {
            return usage(err, THEME_USAGE);
        }
        return live(env, CustomizeAction::EndPreview, &|r| Ok(format!("preview ended, {}", r.theme)), json, out, err);
    }
    let [target] = a.words.as_slice() else { return usage(err, THEME_USAGE) };
    let toml = match theme_text(target, env.themes) {
        Ok(text) => text,
        Err(e) => return fail(err, &e),
    };
    if let Err(e) = theme::parse(&toml) {
        return fail(err, &format!("{target}: {e}"));
    }
    let judge = |r: &Report| match &r.preview {
        Some(name) => Ok(format!("previewing {name}")),
        None => Err(format!("shows {}", r.theme)),
    };
    live(env, CustomizeAction::PreviewTheme { toml }, &judge, json, out, err)
}

fn mode_name(mode: AppearanceMode) -> &'static str {
    match mode {
        AppearanceMode::System => "system",
        AppearanceMode::Dark => "dark",
        AppearanceMode::Light => "light",
    }
}

fn shade(dark: bool) -> &'static str {
    if dark {
        "dark"
    } else {
        "light"
    }
}

/// `{theme} ({dark|light}, system is {dark|light}), preset {preset}, family {family}` and the
/// preview and errors when set.
fn report_line(r: &Report) -> String {
    let mut line = format!("{} ({}, system is {}), preset {}, family {}", r.theme, shade(r.dark), shade(r.system_is_dark), r.preset, r.family);
    if let Some(p) = &r.preview {
        line += &format!(", preview {p}");
    }
    if let Some(e) = &r.config_error {
        line += &format!(", config error: {e}");
    }
    if let Some(e) = &r.theme_error {
        line += &format!(", theme error: {e}");
    }
    line
}

/// A window's whole report with its `pid` and `project`, or its `error`.
fn report_json(o: &CustomizeOutcome) -> Json {
    match &o.result {
        Ok(r) => {
            let mut v = serde_json::to_value(r).unwrap_or_else(|_| json!({}));
            v["pid"] = json!(o.pid);
            v["project"] = json!(o.project);
            v
        }
        Err(e) => json!({ "pid": o.pid, "project": o.project, "error": e }),
    }
}

/// Every window's `ReportState`; no daemon or no window is an empty answer.
fn reports(env: &Env) -> Result<Answer, Reach> {
    match customize(env.socket, CustomizeAction::ReportState, env.wait) {
        Err(Reach::NoWindow) => Ok(Answer { outcomes: Vec::new(), missing: Vec::new(), too_old: Vec::new() }),
        other => other,
    }
}

fn report_lines(answer: &Answer, out: &mut dyn Write) {
    for o in &answer.outcomes {
        let text = match &o.result {
            Ok(r) => report_line(r),
            Err(e) => e.clone(),
        };
        let _ = writeln!(out, "window {} ({}): {text}", o.pid, o.project.display());
    }
    silent_lines(answer, out);
}

fn current(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, THEME_USAGE) };
    if !a.words.is_empty() {
        return usage(err, THEME_USAGE);
    }
    let file = std::fs::read_to_string(env.config)
        .map_err(|e| format!("cannot read {}: {e}", env.config.display()))
        .and_then(|text| config::parse(&text).map_err(|e| e.to_string()));
    let answer = reports(env);
    let empty = Answer { outcomes: Vec::new(), missing: Vec::new(), too_old: Vec::new() };
    let shown = answer.as_ref().unwrap_or(&empty);
    if a.has("--json") {
        let config = match &file {
            Ok(c) => json!({ "dark": c.theme.dark, "light": c.theme.light, "mode": mode_name(c.appearance.mode) }),
            Err(e) => json!({ "error": e }),
        };
        let windows: Vec<Json> = shown.outcomes.iter().map(report_json).collect();
        let _ = writeln!(out, "{}", json!({ "config": config, "windows": windows, "missing": shown.missing, "too_old": shown.too_old }));
    } else {
        let _ = match &file {
            Ok(c) => writeln!(out, "config: dark {}, light {}, mode {}", c.theme.dark, c.theme.light, mode_name(c.appearance.mode)),
            Err(e) => writeln!(out, "config: {e}"),
        };
        report_lines(shown, out);
    }
    match answer {
        Ok(_) => 0,
        Err(reach) => fail(err, &reach.line()),
    }
}

/// `arkos settings get|set ...` against `env`.
pub fn settings_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (sub, rest) = subcommand(args);
    match sub {
        Some("get") => settings_get(&rest, env, out, err),
        Some("set") => settings_set(&rest, env, out, err),
        _ => usage(err, SETTINGS_USAGE),
    }
}

/// A TOML value printed bare: a string without quotes, a table as TOML, anything else as TOML
/// writes it.
fn bare(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Table(_) => toml::to_string(v).unwrap_or_default().trim_end().to_string(),
        other => other.to_string(),
    }
}

fn settings_get(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, SETTINGS_USAGE) };
    let json = a.has("--json");
    let text = match std::fs::read_to_string(env.config) {
        Ok(text) => text,
        Err(e) => return fail(err, &format!("cannot read {}: {e}", env.config.display())),
    };
    match a.words.as_slice() {
        [] => {
            let answer = reports(env);
            let empty = Answer { outcomes: Vec::new(), missing: Vec::new(), too_old: Vec::new() };
            let shown = answer.as_ref().unwrap_or(&empty);
            if json {
                let windows: Vec<Json> = shown.outcomes.iter().map(report_json).collect();
                let _ = writeln!(out, "{}", json!({ "file": text, "windows": windows }));
            } else {
                let _ = out.write_all(text.as_bytes());
                if !text.is_empty() && !text.ends_with('\n') {
                    let _ = writeln!(out);
                }
                report_lines(shown, out);
            }
            match answer {
                Ok(_) => 0,
                Err(reach) => fail(err, &reach.line()),
            }
        }
        [key] => {
            let doc: toml::Value = match toml::from_str(&text) {
                Ok(doc) => doc,
                Err(e) => return fail(err, &format!("{}: {}", env.config.display(), e.message().lines().next().unwrap_or("invalid TOML"))),
            };
            let Some(value) = key.split('.').try_fold(&doc, |v, k| v.get(k)) else { return fail(err, "no such key") };
            let _ = if json {
                writeln!(out, "{}", json!({ "key": key, "value": serde_json::to_value(value).unwrap_or(Json::Null) }))
            } else {
                writeln!(out, "{}", bare(value))
            };
            0
        }
        _ => usage(err, SETTINGS_USAGE),
    }
}

fn is_bare_key(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// True when `key` is on `SETTABLE`.
pub fn settable(key: &str) -> bool {
    let segs: Vec<&str> = key.split('.').collect();
    SETTABLE.iter().any(|pattern| {
        let p: Vec<&str> = pattern.split('.').collect();
        p.len() == segs.len() && p.iter().zip(&segs).all(|(p, s)| if *p == "*" { is_bare_key(s) } else { p == s })
    })
}

fn number(key: &str, raw: &str, range: std::ops::RangeInclusive<f64>, words: &str) -> Result<toml_edit::Value, String> {
    match raw.trim().parse::<f64>() {
        Ok(n) if n.is_finite() && range.contains(&n) => Ok(n.into()),
        _ => Err(format!("{key} {raw} must be {words}")),
    }
}

fn one_of(key: &str, raw: &str, allowed: &[&str]) -> Result<toml_edit::Value, String> {
    if allowed.contains(&raw) {
        Ok(raw.into())
    } else {
        Err(format!("{key} must be one of {}, not {raw:?}", allowed.join(", ")))
    }
}

/// `raw` typed for `key` and checked with the config's own rules that need no file: a preset's
/// `size` 8 to 48 and `line_height` 1.0 to 2.5 (numbers), `update.auto` true or false, the two
/// enumerations, and a non-empty string for the rest.
pub fn typed_value(key: &str, raw: &str) -> Result<toml_edit::Value, String> {
    if !settable(key) {
        return Err(format!("not settable: {key}"));
    }
    match key {
        "update.auto" => match raw {
            "true" => Ok(true.into()),
            "false" => Ok(false.into()),
            _ => Err(format!("update.auto must be true or false, not {raw:?}")),
        },
        "appearance.mode" => one_of(key, raw, &["system", "dark", "light"]),
        "viewer.wide_tables" => one_of(key, raw, &["wrap", "scroll"]),
        "recovery.start_at_login" => one_of(key, raw, &["ask", "on", "off"]),
        _ if key.ends_with(".size") => number(key, raw, 8.0..=48.0, "8 to 48"),
        _ if key.ends_with(".line_height") => number(key, raw, 1.0..=2.5, "1.0 to 2.5"),
        _ if raw.trim().is_empty() => Err(format!("{key} must not be empty")),
        _ => Ok(raw.into()),
    }
}

/// An `f32` as the shortest decimal that reads back to it (1.3, not 1.2999999523162842).
fn tidy(v: f32) -> f64 {
    v.to_string().parse().unwrap_or(v as f64)
}

/// The edits for a preset's `size` or `line_height`: that key alone when the file has a table for
/// the preset, else both fields (the other from `merged`), since a preset needs both.
pub fn preset_edits<'a>(key_path: &'a [&'a str], value: toml_edit::Value, file_has_table: bool, merged: &TextPreset) -> Vec<(Vec<String>, toml_edit::Value)> {
    let path = |leaf: &str| -> Vec<String> {
        let mut p: Vec<String> = key_path[..key_path.len().saturating_sub(1)].iter().map(|s| s.to_string()).collect();
        p.push(leaf.to_string());
        p
    };
    let own = (key_path.iter().map(|s| s.to_string()).collect::<Vec<_>>(), value);
    if file_has_table {
        return vec![own];
    }
    match key_path.last() {
        Some(&"size") => vec![own, (path("line_height"), tidy(merged.line_height).into())],
        _ => vec![(path("size"), tidy(merged.size).into()), own],
    }
}

fn value_json(v: &toml_edit::Value) -> Json {
    v.as_bool().map(Json::from).or_else(|| v.as_float().map(Json::from)).or_else(|| v.as_str().map(Json::from)).unwrap_or(Json::Null)
}

fn settings_set(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, SETTINGS_USAGE) };
    let [key, raw] = a.words.as_slice() else { return usage(err, SETTINGS_USAGE) };
    if !settable(key) {
        let _ = writeln!(err, "not settable: {key}\nsettable keys:");
        for k in SETTABLE {
            let _ = writeln!(err, "  {k}");
        }
        return 2;
    }
    let value = match typed_value(key, raw) {
        Ok(v) => v,
        Err(e) => return fail(err, &e),
    };
    if matches!(*key, "theme.dark" | "theme.light") {
        if let Err(e) = load_name(raw, env.themes) {
            return fail(err, &e);
        }
    }
    // Every check runs on the text in memory, the defaults' when the file is missing, so nothing
    // is written before the value is known to be good.
    let text = match std::fs::read_to_string(env.config) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => config::DEFAULT_CONFIG.to_string(),
        Err(e) => return fail(err, &format!("cannot read {}: {e}", env.config.display())),
    };
    // A config that does not load still offers the built-in presets; the parse below decides.
    let presets = match config::parse(&text) {
        Ok(cfg) => cfg.font.presets,
        Err(_) => config::BUILTIN_PRESETS.iter().map(|(n, p)| (n.to_string(), *p)).collect(),
    };
    if *key == "font.preset" && !presets.contains_key(*raw) {
        return fail(err, &format!("no such preset: {raw}"));
    }
    let path: Vec<&str> = key.split('.').collect();
    let edits = match path.as_slice() {
        ["font", "presets", name, _] => {
            let has_table = toml::from_str::<toml::Value>(&text)
                .ok()
                .and_then(|doc| doc.get("font")?.get("presets")?.get(*name).map(toml::Value::is_table))
                .unwrap_or(false);
            // A new preset takes its other field from comfortable.
            let merged = config::resolve_preset(Some(name), "comfortable", &presets).1;
            preset_edits(&path, value.clone(), has_table, &merged)
        }
        _ => vec![(path.iter().map(|s| s.to_string()).collect(), value.clone())],
    };
    let keys: Vec<Vec<&str>> = edits.iter().map(|(k, _)| k.iter().map(String::as_str).collect()).collect();
    let edits: Vec<(&[&str], toml_edit::Value)> = keys.iter().zip(&edits).map(|(k, (_, v))| (k.as_slice(), v.clone())).collect();
    // Nothing is written unless the edited file would load.
    if let Err(e) = config::apply_edits(&text, &edits).and_then(|t| config::parse(&t).map_err(|e| e.to_string())) {
        return fail(err, &e);
    }
    // Writes the defaults when the file is missing; a config that does not load is still edited,
    // since the edit was just shown to fix it.
    let _ = config::load(env.config);
    if let Err(e) = config::set_values(env.config, &edits) {
        return fail(err, &e);
    }
    // A key that picks the theme fails on a theme error too; the others only on a config error.
    let picks_theme = matches!(*key, "appearance.mode" | "theme.dark" | "theme.light");
    let judge = |r: &Report| match (&r.config_error, &r.theme_error) {
        (Some(e), _) => Err(e.clone()),
        (None, Some(e)) if picks_theme => Err(e.clone()),
        _ => Ok(format!("{key} applied")),
    };
    reload_after_save(env, a.has("--json"), json!({ "key": key, "value": value_json(&value), "saved": true }), &judge, out, err)
}

/// `arkos config reload` against `env`.
pub fn config_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (sub, rest) = subcommand(args);
    let Some(a) = split(&rest, &[]) else { return usage(err, CONFIG_USAGE) };
    if sub != Some("reload") || !a.words.is_empty() {
        return usage(err, CONFIG_USAGE);
    }
    let judge = |r: &Report| match (&r.config_error, &r.theme_error) {
        (Some(e), _) | (None, Some(e)) => Err(e.clone()),
        _ => Ok(format!("reloaded, {}", r.theme)),
    };
    live(env, CustomizeAction::ReloadConfig, &judge, a.has("--json"), out, err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config, theme};
    use protocol::{ClientMessage, CustomizeAction, CustomizeOutcome, Report, Request, Response, ServerMessage};
    use std::time::Duration;

    #[test]
    fn the_contrast_report_flags_a_low_pair_and_passes_every_builtin() {
        for (name, _) in theme::BUILTIN {
            let (ok, rows) = theme::contrast_report(&theme::builtin(name).unwrap());
            assert!(ok, "{name}: {:?}", rows.iter().filter(|(p, r)| r < &p.needs).map(|(p, _)| &p.what).collect::<Vec<_>>());
        }
        let text = theme::BUILTIN[0].1.replace("ignored = \"", "ignored = \"#3a3a5a\" # was \"");
        let t = theme::parse(&text).unwrap();
        let (ok, rows) = theme::contrast_report(&t);
        assert!(!ok);
        assert_eq!(rows.iter().find(|(p, r)| r < &p.needs).unwrap().0.what, "tree.ignored on window_bg");
        assert_eq!(rows.iter().filter(|(p, _)| p.what == "chrome.text on window_bg").count(), 1);
        assert_eq!(rows.iter().find(|(p, _)| p.what == "chrome.text on window_bg").unwrap().0.needs, 7.0);
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("low.toml");
        std::fs::write(&f, &text).unwrap();
        let env = Env { socket: &dir.path().join("none.sock"), config: &dir.path().join("config.toml"), themes: dir.path(), wait: Duration::from_millis(10) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["contrast".into(), f.display().to_string()], &env, &mut out, &mut err), 1);
        let s = String::from_utf8(out).unwrap();
        let low = crate::palette::contrast(t.tree.ignored, t.chrome.window_bg);
        assert!(s.lines().any(|l| l == format!("tree.ignored on window_bg {:.2}:1 LOW (needs 4.5)", (low * 100.0).floor() / 100.0)), "{s}");
        let two_decimals = |l: &str| l.split(' ').find(|w| w.ends_with(":1")).is_some_and(|w| w.trim_end_matches(":1").split_once('.').is_some_and(|(_, d)| d.len() == 2));
        assert!(s.lines().filter(|l| l.contains(" LOW ")).all(two_decimals), "{s}");
        assert!(s.lines().any(|l| l == format!("chrome.text on window_bg {:.1}:1 ok", crate::palette::contrast(t.chrome.text, t.chrome.window_bg))));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["contrast".into(), "navy".into(), "--json".into()], &env, &mut out, &mut err), 0);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!((v["name"].as_str(), v["ok"].as_bool()), (Some("navy"), Some(true)));
        assert!(v["pairs"].as_array().unwrap().len() > 20);
    }

    #[test]
    fn use_writes_the_right_key_refuses_an_unknown_name_and_says_so_without_a_window() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "# mine\n[appearance]\nmode = \"light\"\n[theme]\ndark = \"navy\" # d\nlight = \"navy-light\"\n").unwrap();
        let themes = dir.path().join("themes");
        let env = Env { socket: &dir.path().join("none.sock"), config: &cfg, themes: &themes, wait: Duration::from_millis(10) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["use".into(), "timo-purple".into()], &env, &mut out, &mut err), 0);
        assert_eq!(String::from_utf8(out).unwrap(), "saved; no window running\n");
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("light = \"timo-purple\""), "mode light picks the light key: {text}");
        assert!(text.contains("# mine") && text.contains("dark = \"navy\" # d"), "comments kept: {text}");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["use".into(), "nope".into(), "--dark".into()], &env, &mut out, &mut err), 1);
        assert_eq!(String::from_utf8(err).unwrap(), "unknown theme: nope\n");
        assert!(std::fs::read_to_string(&cfg).unwrap().contains("dark = \"navy\""), "nothing written");
        let src = dir.path().join("ocean.toml");
        std::fs::write(&src, theme::BUILTIN[0].1.replace("name = \"navy\"", "name = \"ocean\"")).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["use".into(), src.display().to_string(), "--dark".into(), "--json".into()], &env, &mut out, &mut err), 0);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!((v["name"].as_str(), v["key"].as_str(), v["saved"].as_bool()), (Some("ocean"), Some("dark"), Some(true)));
        assert!(themes.join("ocean.toml").is_file());
        assert!(std::fs::read_to_string(&cfg).unwrap().contains("dark = \"ocean\" # d"));
        let missing = dir.path().join("fresh").join("config.toml");
        let env2 = Env { socket: &dir.path().join("none.sock"), config: &missing, themes: &themes, wait: Duration::from_millis(10) };
        assert_eq!(theme_cli_to(&["use".into(), "navy".into(), "--dark".into()], &env2, &mut Vec::new(), &mut Vec::new()), 0, "a missing config is created first");
        assert!(missing.is_file());
        assert_eq!(theme_key_for_use(None, AppearanceMode::System, Some(false)), "light");
        assert_eq!(theme_key_for_use(None, AppearanceMode::System, None), "dark");
        assert_eq!(theme_key_for_use(Some("--light"), AppearanceMode::Dark, Some(true)), "light");
    }

    #[test]
    fn settings_set_keeps_to_the_allow_list_types_the_value_and_refuses_bad_ones_before_writing() {
        assert!(settable("theme.dark") && settable("font.presets.big.size") && settable("update.auto"));
        assert!(!settable("keybindings.find") && !settable("font.presets.big.other") && !settable("font.presets.a.b.size"));
        assert!(settable("viewer.wide_tables"));
        assert_eq!(typed_value("viewer.wide_tables", "scroll").unwrap().as_str(), Some("scroll"));
        assert!(typed_value("viewer.wide_tables", "sideways").is_err());
        assert_eq!(typed_value("update.auto", "true").unwrap().as_bool(), Some(true));
        assert_eq!(typed_value("font.presets.big.size", "18").unwrap().as_float(), Some(18.0));
        assert!(typed_value("font.presets.big.size", "60").is_err(), "the config allows 8 to 48");
        assert!(typed_value("font.presets.big.line_height", "2.6").is_err(), "the config allows 1.0 to 2.5");
        assert!(typed_value("appearance.mode", "blue").is_err());
        assert!(typed_value("recovery.start_at_login", "on").is_ok());
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, config::DEFAULT_CONFIG).unwrap();
        let env = Env { socket: &dir.path().join("none.sock"), config: &cfg, themes: &dir.path().join("themes"), wait: Duration::from_millis(10) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(settings_cli_to(&["set".into(), "keybindings.find".into(), "cmd-g".into()], &env, &mut out, &mut err), 2);
        assert!(String::from_utf8(err).unwrap().contains("theme.dark"), "the list is printed");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(settings_cli_to(&["set".into(), "font.preset".into(), "huge".into()], &env, &mut out, &mut err), 1, "no such preset");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(settings_cli_to(&["set".into(), "font.preset".into(), "compact".into()], &env, &mut out, &mut err), 0, "a built-in preset counts");
        assert_eq!(String::from_utf8(out).unwrap(), "saved; no window running\n");
        let text = std::fs::read_to_string(&cfg).unwrap();
        assert!(text.contains("preset = \"compact\""));
        assert_eq!(text.matches('#').count(), config::DEFAULT_CONFIG.matches('#').count(), "comments kept");
        // A preset field alone on a file with no table for it: both fields are written and it loads.
        std::fs::write(&cfg, "[font]\npreset = \"compact\"\n").unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(settings_cli_to(&["set".into(), "font.presets.compact.size".into(), "15".into()], &env, &mut out, &mut err), 0);
        let loaded = config::load(&cfg).unwrap();
        assert_eq!(loaded.font.presets["compact"].size, 15.0);
        assert!(std::fs::read_to_string(&cfg).unwrap().contains("line_height"));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(settings_cli_to(&["get".into(), "font.preset".into()], &env, &mut out, &mut err), 0);
        assert_eq!(String::from_utf8(out).unwrap(), "compact\n");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(settings_cli_to(&["get".into(), "nope.key".into()], &env, &mut out, &mut err), 1);
    }

    #[test]
    fn the_client_reports_every_window_the_silent_ones_and_an_old_daemon() {
        use protocol::codec::{read_frame, write_frame};
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("d.sock");
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut r = std::io::BufReader::new(s.try_clone().unwrap());
            let mut w = std::io::BufWriter::new(s);
            let msg: ClientMessage = read_frame(&mut r).unwrap().unwrap();
            let Request::Customize { action: CustomizeAction::ReportState, .. } = msg.request else { panic!() };
            let one = CustomizeOutcome { pid: 3, project: "/p".into(), result: Ok(Report { theme: "navy".into(), ..Report::default() }) };
            write_frame(&mut w, &ServerMessage::Response { id: msg.id, response: Response::Customized { outcomes: vec![one], missing: vec![4], too_old: vec![5] } }).unwrap();
            w.flush().unwrap();
            // The second connection: an old daemon that cannot decode the frame hangs up.
            let (s, _) = listener.accept().unwrap();
            drop(s);
        });
        let got = customize(&sock, CustomizeAction::ReportState, Duration::from_secs(1)).unwrap();
        assert_eq!((got.outcomes[0].pid, got.missing.clone(), got.too_old.clone()), (3, vec![4], vec![5]));
        assert!(matches!(customize(&sock, CustomizeAction::ReportState, Duration::from_secs(1)), Err(Reach::OldDaemon)));
        assert_eq!(Reach::OldDaemon.line(), "arkosd is older than this command; use Update now in Arkos IDE, or run pkill -x arkosd (ends every session)");
        server.join().unwrap();
        assert!(matches!(customize(&dir.path().join("absent.sock"), CustomizeAction::ReportState, Duration::from_millis(10)), Err(Reach::NoWindow)));
        let env = Env { socket: &dir.path().join("absent.sock"), config: &dir.path().join("c.toml"), themes: dir.path(), wait: Duration::from_millis(10) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["preview".into(), "navy".into()], &env, &mut out, &mut err), 3);
        assert_eq!(String::from_utf8(err).unwrap(), "no window running\n");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(config_cli_to(&["reload".into()], &env, &mut out, &mut err), 3);
    }

    #[test]
    fn the_schema_names_every_key_of_the_builtins_once_and_the_template_prints_a_builtin() {
        let mut from_toml: std::collections::BTreeSet<String> = Default::default();
        for (_, text) in theme::BUILTIN {
            let doc: toml::Value = toml::from_str(text).unwrap();
            for (table, v) in doc.as_table().unwrap() {
                match v.as_table() {
                    Some(t) => {
                        for k in t.keys() {
                            from_toml.insert(format!("{table}.{k}"));
                        }
                    }
                    None => {
                        from_toml.insert(table.clone());
                    }
                }
            }
        }
        let from_schema: std::collections::BTreeSet<String> = theme::schema()
            .iter()
            .flat_map(|t| t.keys.iter().map(move |k| if t.name.is_empty() { k.key.to_string() } else { format!("{}.{}", t.name, k.key) }))
            .collect();
        assert_eq!(from_schema, from_toml);
        let dir = tempfile::tempdir().unwrap();
        let env = Env { socket: &dir.path().join("none.sock"), config: &dir.path().join("c.toml"), themes: dir.path(), wait: Duration::from_millis(10) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["template".into()], &env, &mut out, &mut err), 0);
        assert_eq!(String::from_utf8(out).unwrap(), theme::BUILTIN[0].1);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["template".into(), "nope".into()], &env, &mut out, &mut err), 1);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&[], &env, &mut out, &mut err), 2);
        assert!(String::from_utf8(err).unwrap().contains("see: arkos guide"));
    }

    /// A daemon stand-in for the commands: answers one connection per response, in order, and
    /// returns the actions it was asked for.
    fn fake_daemon(sock: &std::path::Path, answers: Vec<Response>) -> std::thread::JoinHandle<Vec<CustomizeAction>> {
        use protocol::codec::{read_frame, write_frame};
        use std::io::Write as _;
        let listener = std::os::unix::net::UnixListener::bind(sock).unwrap();
        std::thread::spawn(move || {
            let mut asked = Vec::new();
            for response in answers {
                let (s, _) = listener.accept().unwrap();
                let mut r = std::io::BufReader::new(s.try_clone().unwrap());
                let mut w = std::io::BufWriter::new(s);
                let msg: ClientMessage = read_frame(&mut r).unwrap().unwrap();
                let Request::Customize { action, .. } = msg.request else { panic!("not a Customize") };
                asked.push(action);
                write_frame(&mut w, &ServerMessage::Response { id: msg.id, response }).unwrap();
                w.flush().unwrap();
            }
            asked
        })
    }

    fn one_window(report: Report, missing: Vec<u32>, too_old: Vec<u32>) -> Response {
        Response::Customized { outcomes: vec![CustomizeOutcome { pid: 7, project: "/p".into(), result: Ok(report) }], missing, too_old }
    }

    #[test]
    fn use_fails_when_a_window_shows_another_theme_after_the_reload() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("d.sock");
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, config::DEFAULT_CONFIG).unwrap();
        let server = fake_daemon(&sock, vec![one_window(Report { theme: "navy".into(), system_is_dark: true, ..Report::default() }, vec![8], vec![])]);
        let env = Env { socket: &sock, config: &cfg, themes: &dir.path().join("themes"), wait: Duration::from_millis(10) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["use".into(), "timo-purple".into(), "--dark".into()], &env, &mut out, &mut err), 1);
        assert_eq!(String::from_utf8(out).unwrap(), "window 7 (/p): shows navy\nwindow 8: no answer\n");
        assert_eq!(server.join().unwrap(), vec![CustomizeAction::ReloadConfig], "the flag needs no appearance round trip");
        assert!(std::fs::read_to_string(&cfg).unwrap().contains("dark = \"timo-purple\""), "saved all the same");
    }

    #[test]
    fn current_prints_the_config_then_what_each_window_shows() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("d.sock");
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, config::DEFAULT_CONFIG).unwrap();
        let report = Report {
            theme: "draft".into(),
            dark: true,
            system_is_dark: false,
            preset: "comfortable".into(),
            family: "Menlo".into(),
            preview: Some("draft".into()),
            config_error: None,
            theme_error: Some("theme: x".into()),
        };
        let server = fake_daemon(&sock, vec![one_window(report.clone(), vec![8], vec![9]), one_window(report, vec![], vec![])]);
        let env = Env { socket: &sock, config: &cfg, themes: &dir.path().join("themes"), wait: Duration::from_millis(10) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["current".into()], &env, &mut out, &mut err), 0);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "config: dark navy, light navy-light, mode system\n\
             window 7 (/p): draft (dark, system is light), preset comfortable, family Menlo, preview draft, theme error: theme: x\n\
             window 8: no answer\n\
             window 9: too old for this command\n"
        );
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(theme_cli_to(&["current".into(), "--json".into()], &env, &mut out, &mut err), 0);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["config"]["mode"].as_str(), Some("system"));
        assert_eq!((v["windows"][0]["pid"].as_u64(), v["windows"][0]["preview"].as_str()), (Some(7), Some("draft")));
        assert_eq!(server.join().unwrap(), vec![CustomizeAction::ReportState, CustomizeAction::ReportState]);
    }

    #[test]
    fn settings_set_on_a_missing_config_writes_only_a_good_value() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("fresh").join("config.toml");
        let env = Env { socket: &dir.path().join("none.sock"), config: &cfg, themes: &dir.path().join("themes"), wait: Duration::from_millis(10) };
        assert_eq!(settings_cli_to(&["set".into(), "font.preset".into(), "huge".into()], &env, &mut Vec::new(), &mut Vec::new()), 1);
        assert!(!cfg.exists(), "a refusal writes nothing, not even the defaults");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(settings_cli_to(&["set".into(), "update.auto".into(), "false".into(), "--json".into()], &env, &mut out, &mut err), 0);
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!((v["key"].as_str(), v["value"].as_bool(), v["saved"].as_bool()), (Some("update.auto"), Some(false), Some(true)));
        assert!(config::load(&cfg).map(|c| !c.update.auto).unwrap(), "created from the defaults, then edited");
    }

    #[test]
    fn use_counts_a_window_in_the_other_appearance_as_saved_and_says_what_it_shows() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("d.sock");
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, config::DEFAULT_CONFIG).unwrap();
        let light = Report { theme: "navy-light".into(), system_is_dark: false, ..Report::default() };
        let silent = Response::Customized { outcomes: vec![], missing: vec![8], too_old: vec![9] };
        let server = fake_daemon(&sock, vec![one_window(light.clone(), vec![], vec![]), one_window(light, vec![], vec![]), silent]);
        let env = Env { socket: &sock, config: &cfg, themes: &dir.path().join("themes"), wait: Duration::from_millis(10) };
        let use_dark = |json: bool| {
            let mut args: Vec<String> = vec!["use".into(), "timo-purple".into(), "--dark".into()];
            if json {
                args.push("--json".into());
            }
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = theme_cli_to(&args, &env, &mut out, &mut err);
            (code, String::from_utf8(out).unwrap())
        };
        assert_eq!(use_dark(false), (0, "window 7 (/p): saved for dark mode; shows navy-light in light mode\n".to_string()));
        let (code, out) = use_dark(true);
        assert_eq!(code, 0);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["windows"][0]["theme"].as_str(), Some("navy-light"));
        assert!(v["windows"][0].get("error").is_none(), "{v}");
        assert_eq!(use_dark(false), (0, "saved; no window answered\nwindow 8: no answer\nwindow 9: too old for this command\n".to_string()));
        assert_eq!(server.join().unwrap(), vec![CustomizeAction::ReloadConfig; 3]);
    }

    #[test]
    fn settings_set_fails_on_a_theme_error_only_for_a_key_that_picks_the_theme() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("d.sock");
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, config::DEFAULT_CONFIG).unwrap();
        let broken = Report { theme: "navy".into(), theme_error: Some("draft.toml: missing key chrome.text".into()), ..Report::default() };
        let answers = vec![one_window(broken.clone(), vec![], vec![]); 4];
        let server = fake_daemon(&sock, answers);
        let env = Env { socket: &sock, config: &cfg, themes: &dir.path().join("themes"), wait: Duration::from_millis(10) };
        for (key, value) in [("theme.dark", "navy"), ("theme.light", "navy-light"), ("appearance.mode", "dark")] {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            assert_eq!(settings_cli_to(&["set".into(), key.into(), value.into()], &env, &mut out, &mut err), 1, "{key}");
            assert_eq!(String::from_utf8(out).unwrap(), "window 7 (/p): draft.toml: missing key chrome.text\n", "{key}");
        }
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(settings_cli_to(&["set".into(), "update.auto".into(), "false".into()], &env, &mut out, &mut err), 0, "a key that does not pick the theme");
        assert_eq!(String::from_utf8(out).unwrap(), "window 7 (/p): update.auto applied\n");
        assert_eq!(server.join().unwrap(), vec![CustomizeAction::ReloadConfig; 4]);
    }

    #[test]
    fn settings_get_ends_the_file_text_with_a_newline_before_the_window_lines() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("d.sock");
        let cfg = dir.path().join("config.toml");
        std::fs::write(&cfg, "[font]\npreset = \"compact\"").unwrap();
        let report = Report { theme: "navy".into(), dark: true, system_is_dark: true, preset: "compact".into(), family: "Menlo".into(), ..Report::default() };
        let server = fake_daemon(&sock, vec![one_window(report, vec![], vec![])]);
        let env = Env { socket: &sock, config: &cfg, themes: &dir.path().join("themes"), wait: Duration::from_millis(10) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(settings_cli_to(&["get".into()], &env, &mut out, &mut err), 0);
        assert_eq!(String::from_utf8(out).unwrap(), "[font]\npreset = \"compact\"\nwindow 7 (/p): navy (dark, system is dark), preset compact, family Menlo\n");
        server.join().unwrap();
    }
}
