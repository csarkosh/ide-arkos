//! `arkos color` and `arkos font`: list, use and current, for the colour schemes and font sets
//! that the Custom look picks from. Modelled on `arkos effect`: `use` writes `custom.color` or
//! `custom.font` through the config edit path and puts the Custom look in force, keeping the
//! other two parts (the first time, it pins what is on screen as `settings::look_edits` does).
//! Every line goes through the `out` and `err` writers; `--json` prints one JSON value on stdout.
//! Exit codes as `arkos theme`'s: 0 done, 1 refused or failed, 2 usage or an unknown name.

use crate::agent_cli::{self, fail, reload_after_save, split, subcommand, usage, Env};
use crate::config::{self, Look};
use crate::theme::{self, Source};
use protocol::Report;
use serde_json::{json, Value as Json};
use std::io::Write;

pub const COLOR_USAGE: &str = "\
usage: arkos color list [--json]
       arkos color use <name> [--json]
       arkos color current [--json]
see: arkos guide
";

pub const FONT_USAGE: &str = "\
usage: arkos font list [--json]
       arkos font use <name|default> [--json]
       arkos font current [--json]
see: arkos guide
";

/// Which part of the look a command is about.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Color,
    Font,
}

impl Part {
    fn usage(self) -> &'static str {
        match self {
            Part::Color => COLOR_USAGE,
            Part::Font => FONT_USAGE,
        }
    }
    fn word(self) -> &'static str {
        match self {
            Part::Color => "color",
            Part::Font => "font",
        }
    }
}

/// `arkos color ...` against `env`.
pub fn color_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    dispatch(Part::Color, args, env, out, err)
}

/// `arkos font ...` against `env`.
pub fn font_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    dispatch(Part::Font, args, env, out, err)
}

fn dispatch(part: Part, args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (sub, rest) = subcommand(args);
    match sub {
        Some("list") => list(part, &rest, env, out, err),
        Some("use") => use_it(part, &rest, env, out, err),
        Some("current") => current(part, &rest, env, out, err),
        _ => usage(err, part.usage()),
    }
}

fn source_word(s: &Source) -> &'static str {
    match s {
        Source::BuiltIn => "built in",
        Source::File => "file",
        Source::Invalid(_) => "invalid",
    }
}

/// `list`: one line per name, `name  title  dark|light  source`, or a JSON array.
fn list(part: Part, args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, part.usage()) };
    if !a.words.is_empty() {
        return usage(err, part.usage());
    }
    let mut rows: Vec<Json> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    match part {
        Part::Color => {
            for e in theme::colors(env.themes) {
                let shade = e.appearance.map(|x| x.word()).unwrap_or("-");
                let src = source_word(&e.source);
                let note = if let Source::Invalid(m) = &e.source { format!("  ({m})") } else { String::new() };
                lines.push(format!("{}  {}  {shade}  {src}{note}", e.name, e.title));
                let mut row = json!({ "name": e.name, "title": e.title, "appearance": e.appearance.map(|x| x.word()), "source": src });
                if let Source::Invalid(m) = &e.source {
                    row["error"] = json!(m);
                }
                rows.push(row);
            }
        }
        Part::Font => {
            lines.push(format!("{}  Default  the plain fonts  built in", config::PLAIN_FONT));
            rows.push(json!({ "name": config::PLAIN_FONT, "title": "Default", "source": "built in" }));
            let folder = crate::font_sets::dir_of(env.themes);
            for e in theme::font_sets(env.themes) {
                let own_file = folder.join(format!("{}.toml", e.name)).is_file();
                let src = if !own_file && crate::font_sets::BUILTIN.iter().any(|(n, _)| *n == e.name) { "built in" } else { "file" };
                let family = e.font.terminal.clone().or(e.font.ui.clone()).or(e.font.prose.clone()).unwrap_or_default();
                lines.push(format!("{}  {}  {family}  {src}", e.name, e.title));
                rows.push(json!({ "name": e.name, "title": e.title, "terminal": e.font.terminal, "ui": e.font.ui, "prose": e.font.prose, "source": src }));
            }
        }
    }
    if a.has("--json") {
        let _ = writeln!(out, "{}", Json::Array(rows));
    } else {
        for l in lines {
            let _ = writeln!(out, "{l}");
        }
    }
    0
}

/// Whether `name` is something `part` can use.
fn known(part: Part, name: &str, env: &Env) -> Result<(), String> {
    if !theme::is_bare_name(name) {
        return Err(format!("{name:?} is not a name"));
    }
    match part {
        Part::Color => agent_cli::load_name(name, env.themes).map(|_| ()),
        Part::Font if name == config::PLAIN_FONT => Ok(()),
        Part::Font => {
            let shown = theme::BUILTIN.first().and_then(|(n, _)| theme::builtin(n)).expect("a built-in theme");
            theme::font_for(Some(name), &shown, env.themes).map(|_| ())
        }
    }
}

/// `use <name>`: the edits that put Custom in force with `name` as its colour or font, keeping
/// the other two. The first time, what is on screen is pinned for the parts not being set, as the
/// Settings window's first Custom click does.
fn use_it(part: Part, args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, part.usage()) };
    let [name] = a.words.as_slice() else { return usage(err, part.usage()) };
    if let Err(e) = known(part, name, env) {
        let _ = writeln!(err, "{e}\nsee: arkos {} list", part.word());
        return 2;
    }
    let cfg = match config::load(env.config) {
        Ok(cfg) => cfg,
        Err(e) => return fail(err, &e.to_string()),
    };
    // What is on screen now: the theme the config shows, in the appearance a window reports.
    let windows_dark = agent_cli::reports(env).ok().and_then(|r| r.outcomes.iter().find_map(|o| o.result.as_ref().ok().map(|r| r.system_is_dark)));
    let shown_name = cfg.theme_name(windows_dark.unwrap_or(true)).to_string();
    let shown = theme::load_look(&cfg, windows_dark.unwrap_or(true), env.themes).ok();
    let plain = shown.as_ref().is_none_or(|t| t.font.is_default());
    let own = shown.as_ref().and_then(|t| t.recommended_effect().map(str::to_string));
    let key = match part {
        Part::Color => "color",
        Part::Font => "font",
    };
    let scene = shown.as_ref().and_then(|t| crate::effects::scene::shown_scene(&cfg, t, env.themes, &env.effects_dir()));
    let mut edits: Vec<(Vec<&'static str>, String)> = crate::settings::look_edits(&cfg, Look::Custom, &shown_name, plain, own.as_deref(), scene.as_deref());
    edits.retain(|(k, _)| k[1] != key);
    // A custom colour already chosen leaves the font to pin when it is next set.
    if part == Part::Color && cfg.custom.font.is_none() && !edits.iter().any(|(k, _)| k[1] == "font") {
        edits.push((vec!["custom", "font"], if plain { config::PLAIN_FONT.to_string() } else { shown_name.clone() }));
    }
    edits.push((vec!["custom", key], name.to_string()));
    let typed: Vec<(&[&str], toml_edit::Value)> = edits.iter().map(|(k, v)| (k.as_slice(), toml_edit::Value::from(v.as_str()))).collect();
    if let Err(e) = config::set_values(env.config, &typed) {
        return fail(err, &e);
    }
    let setting = format!("custom.{key}");
    let judge = |r: &Report| match (&r.config_error, &r.theme_error) {
        (Some(e), _) | (None, Some(e)) => Err(e.clone()),
        _ => Ok(format!("{setting} applied")),
    };
    reload_after_save(env, a.has("--json"), json!({ "key": setting, "value": name, "saved": true }), &judge, out, err)
}

/// `current`: what the config says for this part, then each window's theme.
fn current(part: Part, args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, part.usage()) };
    if !a.words.is_empty() {
        return usage(err, part.usage());
    }
    let file = std::fs::read_to_string(env.config)
        .map_err(|e| format!("cannot read {}: {e}", env.config.display()))
        .and_then(|text| config::parse(&text).map_err(|e| e.to_string()));
    let answer = agent_cli::reports(env);
    let empty = agent_cli::Answer { outcomes: Vec::new(), missing: Vec::new(), too_old: Vec::new() };
    let shown = answer.as_ref().unwrap_or(&empty);
    // The custom look's own choice; None under the presets or with nothing chosen.
    let choice = |c: &config::Config| match part {
        Part::Color => c.custom.color.clone(),
        Part::Font => c.custom.font.clone(),
    }
    .filter(|_| c.look() == Look::Custom);
    if a.has("--json") {
        let config = match &file {
            Ok(c) => json!({ "look": agent_cli::look_name(c.look()), part.word(): choice(c) }),
            Err(e) => json!({ "error": e }),
        };
        let windows: Vec<Json> = shown.outcomes.iter().map(agent_cli::report_json).collect();
        let _ = writeln!(out, "{}", json!({ "config": config, "windows": windows, "missing": shown.missing, "too_old": shown.too_old }));
    } else {
        let _ = match &file {
            Ok(c) => match choice(c) {
                Some(name) => writeln!(out, "config: {} {name}, custom", part.word()),
                None => writeln!(out, "config: {} of the preset showing, {}", part.word(), agent_cli::look_name(c.look())),
            },
            Err(e) => writeln!(out, "config: {e}"),
        };
        agent_cli::report_lines(shown, out);
    }
    match answer {
        Ok(_) => 0,
        Err(reach) => fail(err, &reach.line()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    struct Setup {
        dir: tempfile::TempDir,
    }

    impl Setup {
        fn new() -> Setup {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("config.toml"), config::DEFAULT_CONFIG).unwrap();
            Setup { dir }
        }
        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }
        fn env<'a>(&'a self, sock: &'a Path, config: &'a Path, themes: &'a Path) -> Env<'a> {
            Env { socket: sock, config, themes, wait: Duration::from_millis(10) }
        }
    }

    fn run(f: fn(&[String], &Env, &mut dyn Write, &mut dyn Write) -> i32, s: &Setup, args: &[&str]) -> (i32, String, String) {
        let (sock, config, themes) = (s.path("none.sock"), s.path("config.toml"), s.path("themes"));
        let env = s.env(&sock, &config, &themes);
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = f(&args, &env, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn color_list_names_each_scheme_once() {
        let s = Setup::new();
        let (code, out, _) = run(color_cli_to, &s, &["list"]);
        assert_eq!(code, 0);
        let names: Vec<&str> = out.lines().filter_map(|l| l.split_whitespace().next()).collect();
        assert_eq!(names.len(), crate::color_schemes::BUILTIN.len(), "{out}");
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len(), "{out}");
        assert!(names.contains(&"lavender") && names.contains(&"navy"), "{out}");
        let (code, json, _) = run(color_cli_to, &s, &["list", "--json"]);
        assert_eq!(code, 0);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_array().unwrap().len(), names.len());
        assert_eq!(v[0]["source"].as_str(), Some("built in"));
    }

    #[test]
    fn color_use_puts_custom_in_force_with_that_scheme() {
        let s = Setup::new();
        let (code, out, _) = run(color_cli_to, &s, &["use", "lavender"]);
        assert_eq!((code, out.as_str()), (0, "saved; no window running\n"));
        let saved = config::load(&s.path("config.toml")).unwrap();
        assert_eq!((saved.look(), saved.custom.color.as_deref()), (config::Look::Custom, Some("lavender")));
        assert!(saved.custom.font.is_some() && saved.custom.effect.is_some(), "the other two are pinned: {:?}", saved.custom);
        let (code, out, _) = run(color_cli_to, &s, &["current"]);
        assert_eq!(code, 0);
        assert!(out.contains("lavender"), "{out}");
    }

    #[test]
    fn font_use_default_sets_the_plain_fonts() {
        let s = Setup::new();
        let (code, _, _) = run(font_cli_to, &s, &["use", "default"]);
        assert_eq!(code, 0);
        let saved = config::load(&s.path("config.toml")).unwrap();
        assert_eq!((saved.look(), saved.custom.font.as_deref()), (config::Look::Custom, Some(config::PLAIN_FONT)));
        assert!(saved.custom.color.is_some(), "the colour on screen is pinned");
        let (code, _, _) = run(font_cli_to, &s, &["use", "lavender"]);
        assert_eq!(code, 0);
        assert_eq!(config::load(&s.path("config.toml")).unwrap().custom.font.as_deref(), Some("lavender"));
        let (code, out, _) = run(font_cli_to, &s, &["list"]);
        assert_eq!(code, 0);
        assert!(out.lines().any(|l| l.starts_with("default ")) && out.contains("lavender"), "{out}");
    }

    #[test]
    fn an_unknown_colour_exits_2_naming_color_list() {
        let s = Setup::new();
        let before = std::fs::read_to_string(s.path("config.toml")).unwrap();
        let (code, _, err) = run(color_cli_to, &s, &["use", "nope"]);
        assert_eq!(code, 2);
        assert!(err.contains("nope") && err.contains("arkos color list"), "{err}");
        let (code, _, err) = run(font_cli_to, &s, &["use", "nope"]);
        assert_eq!(code, 2);
        assert!(err.contains("nope") && err.contains("arkos font list"), "{err}");
        assert_eq!(std::fs::read_to_string(s.path("config.toml")).unwrap(), before, "nothing is written");
    }

    #[test]
    fn theme_contrast_takes_a_colour_scheme_s_name() {
        let s = Setup::new();
        std::fs::create_dir_all(s.path("colors")).unwrap();
        let text = crate::color_schemes::BUILTIN.iter().find(|(n, _)| *n == "lavender").unwrap().1.replace("name = \"lavender\"", "name = \"mine\"");
        std::fs::write(s.path("colors/mine.toml"), text).unwrap();
        let (code, out, err) = run(crate::agent_cli::theme_cli_to, &s, &["contrast", "mine"]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("chrome.text on window_bg"), "{out}");
        let (code, out, err) = run(crate::agent_cli::settings_cli_to, &s, &["set", "custom.color", "mine"]);
        assert_eq!((code, out.as_str()), (0, "saved; no window running\n"), "{err}");
        let (code, _, _) = run(crate::agent_cli::settings_cli_to, &s, &["set", "custom.font", "departure"]);
        assert_eq!(code, 0);
        let saved = config::load(&s.path("config.toml")).unwrap();
        assert_eq!((saved.custom.color.as_deref(), saved.custom.font.as_deref()), (Some("mine"), Some("departure")));
    }
}
