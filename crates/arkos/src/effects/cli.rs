//! `arkos effect` (spec 4.7): list, contract, template, check, bench, use and current. Files are
//! read and written here with no window needed; `use` writes through `arkos settings set
//! effect.name`, and `current` and `bench` ask the running windows through the daemon. Every line
//! goes through the `out` and `err` writers; `--json` prints one JSON value on stdout instead.
//! Exit codes as `arkos theme`'s: 0 done, 1 refused or failed, 2 usage, 3 a window was needed and
//! none is running.

use std::io::Write;
use std::path::Path;

use metal::{CompileOptions, Device, DeviceRef, MTLPixelFormat, RenderPipelineDescriptor};
use protocol::{CustomizeAction, CustomizeOutcome, WindowDetail};
use serde_json::{json, Value as Json};

use super::manifest::{self, Effect, Highlights, Source};
use super::{active, bench, layer, BUILTIN, PREAMBLE};
use crate::agent_cli::{self, fail, split, subcommand, usage, DetailedAnswer, Env, Reach};
use crate::config::{self, EffectChoice};
use crate::theme;

pub const EFFECT_USAGE: &str = "\
usage: arkos effect list [--json]
       arkos effect contract [--glyphs]
       arkos effect template --as <new-name> [<built-in>]
       arkos effect check <folder> [--json]
       arkos effect bench <folder-or-name> [--seconds N] [--json]
       arkos effect use <name|none|theme> [--json]
       arkos effect current [--json]
see: arkos guide
";

/// The line `contract --glyphs` prints after gpui's sprite declarations: the two functions a
/// glyph shader supplies, with their signatures.
const GLYPH_SIGNATURES: &str = include_str!("../../effects/glyph-signatures.metal");

/// What `bench` draws without a window: a window of this many points, in this theme.
const BENCH_SIZE: (f32, f32) = (1200.0, 800.0);
const BENCH_THEME: &str = "peach-light";
/// How long `bench` draws without `--seconds`, and the longest it may be asked to.
const BENCH_SECONDS: f32 = 3.0;
const BENCH_MAX_SECONDS: f32 = 60.0;

/// `arkos effect ...` against `env`.
pub fn effect_cli_to(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let (sub, rest) = subcommand(args);
    match sub {
        Some("list") => list(&rest, env, out, err),
        Some("contract") => contract(&rest, out, err),
        Some("template") => template(&rest, env, out, err),
        Some("check") => check(&rest, out, err),
        Some("bench") => bench_command(&rest, env, out, err),
        Some("use") => use_effect(&rest, env, out, err),
        Some("current") => current(&rest, env, out, err),
        _ => usage(err, EFFECT_USAGE),
    }
}

/// Takes `flag` and the value after it out of `args`: `Ok((None, args))` when the flag is
/// absent, `Err` when it has no value.
fn take_value(args: &[String], flag: &str) -> Result<(Option<String>, Vec<String>), ()> {
    let Some(i) = args.iter().position(|a| a == flag) else { return Ok((None, args.to_vec())) };
    let value = args.get(i + 1).filter(|v| !v.starts_with("--")).ok_or(())?;
    let rest = args.iter().enumerate().filter(|(j, _)| *j != i && *j != i + 1).map(|(_, a)| a.clone()).collect();
    Ok((Some(value.clone()), rest))
}

fn highlights_name(h: Highlights) -> &'static str {
    match h {
        Highlights::Effect => "effect",
        Highlights::App => "app",
    }
}

fn list(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, EFFECT_USAGE) };
    if !a.words.is_empty() {
        return usage(err, EFFECT_USAGE);
    }
    let dir = env.effects_dir();
    let listed = manifest::list(&dir);
    if a.has("--json") {
        let rows: Vec<Json> = listed
            .into_iter()
            .map(|(name, source)| match source {
                Source::BuiltIn => json!({ "name": name, "source": "built-in" }),
                Source::Folder(path) => json!({ "name": name, "source": "folder", "path": path }),
                Source::Invalid(e) => json!({ "name": name, "source": "invalid", "error": e, "path": dir.join(&name) }),
            })
            .collect();
        let _ = writeln!(out, "{}", Json::Array(rows));
    } else {
        for (name, source) in listed {
            let _ = match source {
                Source::BuiltIn => writeln!(out, "{name} (built in)"),
                Source::Folder(_) => writeln!(out, "{name} (folder)"),
                Source::Invalid(e) => writeln!(out, "{name} (folder, invalid: {e})"),
            };
        }
    }
    0
}

fn contract(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &["--glyphs"]) else { return usage(err, EFFECT_USAGE) };
    if !a.words.is_empty() || a.has("--json") {
        return usage(err, EFFECT_USAGE);
    }
    let _ = if a.has("--glyphs") { write!(out, "{}{} // the window's pixels per point: 2 on a Retina screen, 1 on most others\n{GLYPH_SIGNATURES}", gpui::GLYPH_SHADER_PRELUDE, active::scale_line(2.0)) } else { write!(out, "{PREAMBLE}") };
    0
}

/// `template --as <new-name> [<built-in>]`: the built-in's files in a new folder of the effects
/// folder, its manifest's `name` set to the folder's, comments kept.
fn template(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Ok((Some(new), rest)) = take_value(args, "--as") else { return usage(err, EFFECT_USAGE) };
    let Some(a) = split(&rest, &[]) else { return usage(err, EFFECT_USAGE) };
    let from = match a.words.as_slice() {
        [] => "watercolor",
        [from] => from,
        _ => return usage(err, EFFECT_USAGE),
    };
    if a.has("--json") {
        return usage(err, EFFECT_USAGE);
    }
    if !theme::is_bare_name(&new) {
        return fail(err, &format!("effect name {new:?} must be a bare folder name"));
    }
    let Some(b) = BUILTIN.iter().find(|b| b.name == from) else { return fail(err, &format!("unknown built-in effect: {from}")) };
    let dest = env.effects_dir().join(&new);
    if dest.exists() {
        return fail(err, &format!("exists: {}", dest.display()));
    }
    let made = (|| -> Result<(), String> {
        // The built-in manifests are tested to parse.
        let m = manifest::parse(b.manifest)?;
        let text = config::apply_edits(b.manifest, &[(&["name"], new.as_str().into())])?;
        let mut files = vec![("effect.toml".to_string(), text), (m.wash, b.wash.to_string())];
        if let (Some(file), Some(text)) = (m.glyphs, b.glyphs) {
            files.push((file, text.to_string()));
        }
        std::fs::create_dir_all(&dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
        for (file, text) in files {
            let path = dest.join(file);
            std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        Ok(())
    })();
    match made {
        Ok(()) => {
            let _ = writeln!(out, "{}", dest.display());
            0
        }
        Err(e) => fail(err, &e),
    }
}

/// `check <folder>`: the manifest and the shaders' files first, then both shaders compiled.
fn check(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, EFFECT_USAGE) };
    let [path] = a.words.as_slice() else { return usage(err, EFFECT_USAGE) };
    let folder = Path::new(path);
    let result = if folder.is_dir() { manifest::load_folder(folder).and_then(|e| compile(&e).map(|()| e)) } else { Err(format!("{path}: not a folder")) };
    let json = a.has("--json");
    match result {
        Ok(e) => {
            let (name, highlights) = (&e.manifest.name, highlights_name(e.manifest.highlights));
            let _ = if json { writeln!(out, "{}", json!({ "ok": true, "name": name, "highlights": highlights })) } else { write!(out, "ok: {name}\nhighlights: {highlights}\n") };
            0
        }
        Err(e) => {
            let _ = if json { writeln!(out, "{}", json!({ "ok": false, "error": e })) } else { writeln!(err, "{}", e.trim_end()) };
            1
        }
    }
}

/// Compiles `effect`'s wash as the window's layer does and its glyph shader as gpui does, the
/// compiler's lines pointing into each one's own file.
fn compile(effect: &Effect) -> Result<(), String> {
    let device = Device::system_default().ok_or("no Metal device")?;
    let m = &effect.manifest;
    layer::pipeline(&device, PREAMBLE, &effect.wash).map_err(|e| e.replace("program_source:", &format!("{}:", m.wash)))?;
    if let (Some(file), Some(source)) = (&m.glyphs, &effect.glyphs) {
        compile_glyphs(&device, source, file)?;
    }
    Ok(())
}

/// Builds the glyph stages from `source` (the file `file`) appended to gpui's own shader source,
/// as `Window::set_glyph_shader` does.
fn compile_glyphs(device: &DeviceRef, source: &str, file: &str) -> Result<(), String> {
    // A window puts its scale's line first; a Retina one stands in for every window here.
    let base = format!("{}\n{}", gpui::GLYPH_SHADER_BASE, active::scale_line(2.0));
    let library = device.new_library_with_source(&format!("{base}\n{source}"), &CompileOptions::new()).map_err(|e| layer::point_into(&e, &base, "gpui", file))?;
    let function = |name: &str| library.get_function(name, None).map_err(|_| format!("{file}: the glyph shader has no function named {name}"));
    let (vs, fs) = (function("arkos_glyph_vertex")?, function("arkos_glyph_fragment")?);
    let desc = RenderPipelineDescriptor::new();
    desc.set_vertex_function(Some(&vs));
    desc.set_fragment_function(Some(&fs));
    desc.color_attachments().object_at(0).ok_or("no colour attachment")?.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
    device.new_render_pipeline_state(&desc).map(|_| ()).map_err(|e| format!("{file}: {e}"))
}

/// The effect `target` names: a folder when it has a `/`, else an effect by name.
fn find(target: &str, env: &Env) -> Result<Effect, String> {
    if target.contains('/') {
        let folder = Path::new(target);
        if !folder.is_dir() {
            return Err(format!("{target}: not a folder"));
        }
        manifest::load_folder(folder)
    } else {
        manifest::load(target, &env.effects_dir())
    }
}

/// The first answering window's size and theme, or 1200 by 800 points in Peach Light.
fn bench_window(env: &Env) -> ((f32, f32), String) {
    let found = agent_cli::customize_detailed(env.socket, CustomizeAction::ReportState, env.wait).ok().and_then(|answer| {
        answer.outcomes.into_iter().find_map(|(o, detail)| Some((detail.size?, o.result.ok()?.theme)))
    });
    found.unwrap_or((BENCH_SIZE, BENCH_THEME.to_string()))
}

fn bench_command(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Ok((seconds, rest)) = take_value(args, "--seconds") else { return usage(err, EFFECT_USAGE) };
    let seconds = match seconds.map(|s| s.parse::<f32>()) {
        None => BENCH_SECONDS,
        Some(Ok(n)) if n > 0.0 && n <= BENCH_MAX_SECONDS => n,
        Some(_) => return usage(err, EFFECT_USAGE),
    };
    let Some(a) = split(&rest, &[]) else { return usage(err, EFFECT_USAGE) };
    let [target] = a.words.as_slice() else { return usage(err, EFFECT_USAGE) };
    let effect = match find(target, env) {
        Ok(e) => e,
        Err(e) => return fail(err, &e),
    };
    let (size, theme_name) = bench_window(env);
    // A window showing a theme that no longer loads (a preview's) is benched in Peach Light.
    let (theme_name, theme) = match agent_cli::load_name(&theme_name, env.themes) {
        Ok(t) => (theme_name, t),
        Err(_) => (BENCH_THEME.to_string(), theme::builtin(BENCH_THEME).expect("Peach Light is built in")),
    };
    let ms = match bench::bench(&effect, &theme, size, seconds) {
        Ok(ms) => ms,
        Err(e) => return fail(err, &format!("effect {}: {e}", effect.manifest.name)),
    };
    let rate = effect.manifest.frame_rate;
    // The share of a frame's time at the manifest's rate, in percent.
    let share = ms as f64 * rate as f64 / 10.0;
    let verdict = bench::verdict(ms);
    let name = &effect.manifest.name;
    let _ = if a.has("--json") {
        writeln!(
            out,
            "{}",
            json!({ "name": name, "ms": agent_cli::tidy(ms), "size": [size.0, size.1], "theme": theme_name, "frame_rate": rate, "share": share, "verdict": verdict })
        )
    } else {
        writeln!(out, "{name}: {ms:.2} ms per frame at {:.0}x{:.0} ({share:.0}% of a frame at {rate} fps): {verdict}", size.0, size.1)
    };
    0
}

/// `use <name|none|theme>`: an effect by name must load; then `settings set effect.name`.
fn use_effect(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, EFFECT_USAGE) };
    let [name] = a.words.as_slice() else { return usage(err, EFFECT_USAGE) };
    if !matches!(*name, "theme" | "none") {
        if let Err(e) = manifest::load(name, &env.effects_dir()) {
            return fail(err, &e);
        }
    }
    let mut set = vec!["set".to_string(), "effect.name".to_string(), name.to_string()];
    if a.has("--json") {
        set.push("--json".into());
    }
    agent_cli::settings_cli_to(&set, env, out, err)
}

fn choice_name(choice: &EffectChoice) -> &str {
    match choice {
        EffectChoice::Theme => "theme",
        EffectChoice::None => "none",
        EffectChoice::Named(n) => n,
    }
}

/// What a window draws, as `current` says it. A window that sends no size is older than the
/// detail and cannot say.
fn drawn_line(detail: &WindowDetail) -> String {
    match (&detail.size, &detail.effect) {
        (None, _) => "too old to report its effect".into(),
        (Some(_), None) => "none".into(),
        (Some(_), Some((name, Some(ms)))) => format!("{name}, {ms:.2} ms per frame"),
        (Some(_), Some((name, None))) => format!("{name}, not measured yet"),
    }
}

fn window_json(o: &CustomizeOutcome, detail: &WindowDetail) -> Json {
    match &o.result {
        Ok(_) => json!({
            "pid": o.pid,
            "project": o.project,
            "effect": detail.effect.as_ref().map(|(n, _)| n),
            "gpu_ms": detail.effect.as_ref().and_then(|(_, ms)| ms.map(agent_cli::tidy)),
            "size": detail.size.map(|(w, h)| [w, h]),
        }),
        Err(e) => json!({ "pid": o.pid, "project": o.project, "error": e }),
    }
}

fn current(args: &[String], env: &Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(a) = split(args, &[]) else { return usage(err, EFFECT_USAGE) };
    if !a.words.is_empty() {
        return usage(err, EFFECT_USAGE);
    }
    let file = std::fs::read_to_string(env.config)
        .map_err(|e| format!("cannot read {}: {e}", env.config.display()))
        .and_then(|text| config::parse(&text).map_err(|e| e.to_string()));
    let answer = match agent_cli::customize_detailed(env.socket, CustomizeAction::ReportState, env.wait) {
        Err(Reach::NoWindow) => Ok(DetailedAnswer { outcomes: Vec::new(), missing: Vec::new(), too_old: Vec::new() }),
        other => other,
    };
    let empty = DetailedAnswer { outcomes: Vec::new(), missing: Vec::new(), too_old: Vec::new() };
    let shown = answer.as_ref().unwrap_or(&empty);
    if a.has("--json") {
        let config = match &file {
            Ok(c) => json!({ "name": choice_name(&c.effect.name), "intensity": agent_cli::tidy(c.effect.intensity) }),
            Err(e) => json!({ "error": e }),
        };
        let windows: Vec<Json> = shown.outcomes.iter().map(|(o, d)| window_json(o, d)).collect();
        let _ = writeln!(out, "{}", json!({ "config": config, "windows": windows, "missing": shown.missing, "too_old": shown.too_old }));
    } else {
        let _ = match &file {
            Ok(c) => writeln!(out, "config: effect {}, intensity {}", choice_name(&c.effect.name), c.effect.intensity),
            Err(e) => writeln!(out, "config: {e}"),
        };
        for (o, detail) in &shown.outcomes {
            let text = match &o.result {
                Ok(_) => drawn_line(detail),
                Err(e) => e.clone(),
            };
            let _ = writeln!(out, "window {} ({}): {text}", o.pid, o.project.display());
        }
        for pid in &shown.missing {
            let _ = writeln!(out, "window {pid}: no answer");
        }
        for pid in &shown.too_old {
            let _ = writeln!(out, "window {pid}: too old for this command");
        }
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
    use crate::effects::manifest::{self, Source};
    use crate::effects::{bench, BUILTIN, PREAMBLE};
    use protocol::{ClientMessage, CustomizeAction, CustomizeOutcome, Report, Request, Response, ServerMessage, WindowDetail};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    /// The built-in watercolor's text, by file.
    fn watercolor() -> &'static crate::effects::BuiltIn {
        BUILTIN.iter().find(|b| b.name == "watercolor").unwrap()
    }

    fn run(args: &[&str], env: &Env) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let code = effect_cli_to(&args, env, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    /// Whether this machine has a Metal device; the tests that compile or draw return early
    /// without one, and say so.
    fn metal() -> bool {
        let there = metal::Device::system_default().is_some();
        if !there {
            eprintln!("no Metal device: the Metal half of this test did not run");
        }
        there
    }

    /// A folder `name` in `effects` holding `effect.toml` and the files given.
    fn folder(effects: &Path, name: &str, manifest: &str, files: &[(&str, &str)]) -> PathBuf {
        let f = effects.join(name);
        std::fs::create_dir_all(&f).unwrap();
        std::fs::write(f.join("effect.toml"), manifest).unwrap();
        for (file, text) in files {
            std::fs::write(f.join(file), text).unwrap();
        }
        f
    }

    /// `text` with `line` put in as its line number `at`.
    fn with_line(text: &str, at: usize, line: &str) -> String {
        let mut lines: Vec<&str> = text.lines().collect();
        lines.insert(at - 1, line);
        lines.join("\n") + "\n"
    }

    /// A temporary config folder: `config.toml` with the defaults, `themes` and `effects` beside
    /// it, and a socket nothing listens on.
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
        fn effects(&self) -> PathBuf {
            self.path("effects")
        }
    }

    macro_rules! env {
        ($setup:expr, $sock:expr) => {
            Env { socket: &$sock, config: &$setup.path("config.toml"), themes: &$setup.path("themes"), wait: Duration::from_millis(10) }
        };
    }

    /// A daemon stand-in: answers one connection per response, in order, and returns the
    /// requests it was sent.
    fn fake_daemon(sock: &Path, answers: Vec<Response>) -> std::thread::JoinHandle<Vec<Request>> {
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
                asked.push(msg.request);
                write_frame(&mut w, &ServerMessage::Response { id: msg.id, response }).unwrap();
                w.flush().unwrap();
            }
            asked
        })
    }

    #[test]
    fn effect_list_names_the_built_in_and_a_folder_and_marks_a_broken_one() {
        let s = Setup::new();
        let env = env!(s, s.path("none.sock"));
        let effects = s.effects();
        folder(&effects, "neon", "name = \"neon\"\nwash = \"wash.metal\"\n", &[("wash.metal", "// neon\n")]);
        folder(&effects, "broken", "name = \"broken\"\nwash = \"wash.metal\"\nscale = 2.0\n", &[("wash.metal", "// broken\n")]);
        let (code, out, _) = run(&["list"], &env);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            format!(
                "watercolor (built in)\nbroken (folder, invalid: {}: scale must be 0.1 to 1.0 (was 2.0))\nneon (folder)\n",
                effects.join("broken").join("effect.toml").display()
            )
        );
        let (code, out, _) = run(&["list", "--json"], &env);
        assert_eq!(code, 0);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[0], serde_json::json!({ "name": "watercolor", "source": "built-in" }));
        assert_eq!((v[1]["name"].as_str(), v[1]["source"].as_str()), (Some("broken"), Some("invalid")));
        assert!(v[1]["error"].as_str().unwrap().contains("scale must be 0.1 to 1.0"));
        assert_eq!(v[2], serde_json::json!({ "name": "neon", "source": "folder", "path": effects.join("neon") }));
        assert_eq!(run(&["list", "extra"], &env).0, 2);
    }

    #[test]
    fn effect_contract_prints_the_preamble_and_the_glyph_prelude_on_request() {
        let s = Setup::new();
        let env = env!(s, s.path("none.sock"));
        assert_eq!(run(&["contract"], &env), (0, PREAMBLE.to_string(), String::new()));
        let (code, out, _) = run(&["contract", "--glyphs"], &env);
        assert_eq!(code, 0);
        assert!(out.starts_with(gpui::GLYPH_SHADER_PRELUDE), "the prelude comes first");
        let prelude = gpui::GLYPH_SHADER_PRELUDE.lines().count();
        assert_eq!(out.lines().count(), prelude + 3, "then three lines");
        let scale = out.lines().nth(prelude).unwrap();
        assert!(scale.starts_with("constant float ARKOS_SCALE"), "the window's scale is declared: {scale}");
        let signatures = out.lines().nth(prelude + 1).unwrap();
        assert!(signatures.contains("vertex Out arkos_glyph_vertex(uint unit_vertex_id [[vertex_id]]"), "{signatures}");
        assert!(signatures.contains("fragment float4 arkos_glyph_fragment(In input [[stage_in]]"), "{signatures}");
        let clock = out.lines().last().unwrap();
        assert!(clock.contains("constant float *time [[buffer(5)]]"), "the clock's buffer and its index: {clock}");
        assert!(!out.contains("struct ArkosInputs"), "the wash's contract is not the glyphs'");
        assert_eq!(run(&["contract", "--json"], &env).0, 2);
        assert_eq!(run(&["contract", "watercolor"], &env).0, 2);
    }

    #[test]
    fn effect_template_copies_a_built_in_under_a_new_name_and_refuses_an_existing_folder() {
        let s = Setup::new();
        let env = env!(s, s.path("none.sock"));
        let effects = s.effects();
        let (code, out, err) = run(&["template", "--as", "mywash"], &env);
        assert_eq!((code, err.as_str()), (0, ""));
        assert_eq!(out, format!("{}\n", effects.join("mywash").display()));
        let e = manifest::load("mywash", &effects).unwrap();
        assert_eq!(e.source, Source::Folder(effects.join("mywash")));
        assert_eq!(e.manifest.name, "mywash");
        assert_eq!(e.wash, watercolor().wash);
        assert_eq!(e.glyphs.as_deref(), watercolor().glyphs);
        let text = std::fs::read_to_string(effects.join("mywash").join("effect.toml")).unwrap();
        assert_eq!(text, watercolor().manifest.replace("name = \"watercolor\"", "name = \"mywash\""), "only the name changes; the comments stay");
        let (code, _, err) = run(&["template", "--as", "mywash"], &env);
        assert_eq!(code, 1);
        assert_eq!(err, format!("exists: {}\n", effects.join("mywash").display()));
        assert_eq!(run(&["template", "watercolor", "--as", "second"], &env).0, 0, "the built-in may come first");
        let (code, _, err) = run(&["template", "--as", "other", "nope"], &env);
        assert_eq!((code, err.as_str()), (1, "unknown built-in effect: nope\n"));
        assert!(!effects.join("other").exists());
        assert_eq!(run(&["template", "--as", "../out"], &env).0, 1);
        assert_eq!(run(&["template"], &env).0, 2);
        assert_eq!(run(&["template", "--as"], &env).0, 2);
        assert_eq!(run(&["template", "--as", "x", "--json"], &env).0, 2);
    }

    #[test]
    fn effect_check_parses_the_manifest_before_touching_metal() {
        let s = Setup::new();
        let env = env!(s, s.path("none.sock"));
        let effects = s.effects();
        let bad = folder(&effects, "bad", "name = \"bad\"\nwash = \"wash.metal\"\nframe_rate = 0\n", &[("wash.metal", "// bad\n")]);
        let (code, out, err) = run(&["check", &bad.display().to_string()], &env);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.contains("frame_rate must be 1 to 60 (was 0)"), "{err}");
        let (code, out, _) = run(&["check", &bad.display().to_string(), "--json"], &env);
        assert_eq!(code, 1);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["ok"].as_bool(), Some(false));
        assert!(v["error"].as_str().unwrap().contains("frame_rate"));
        let foo = folder(&effects, "foo", "name = \"bar\"\nwash = \"wash.metal\"\n", &[("wash.metal", "// foo\n")]);
        let (code, _, err) = run(&["check", &foo.display().to_string()], &env);
        assert_eq!(code, 1);
        assert!(err.contains("name 'bar' does not match its folder 'foo'"), "{err}");
        let (code, _, err) = run(&["check", &s.path("absent").display().to_string()], &env);
        assert_eq!(code, 1);
        assert!(err.contains("not a folder"), "{err}");
        assert_eq!(run(&["check"], &env).0, 2);
    }

    #[test]
    fn effect_check_accepts_a_glyph_shader_that_reads_the_clock_at_buffer_five() {
        if !metal() {
            return;
        }
        let s = Setup::new();
        let env = env!(s, s.path("none.sock"));
        let ink = watercolor().glyphs.unwrap();
        let own = "texture2d<float> atlas_texture [[texture(SpriteInputIndex_AtlasTexture)]]) {";
        assert_eq!(ink.matches(own).count(), 1, "watercolor's fragment ends its parameters so");
        // The clock's index is gpui's own name for it, and the fragment reads it.
        let timed = format!(
            "static_assert(SpriteInputIndex_GlyphTime == 5, \"the glyph clock is buffer 5\");\n{}",
            ink.replacen(own, "texture2d<float> atlas_texture [[texture(SpriteInputIndex_AtlasTexture)]],\n    constant float *time [[buffer(5)]]) {\n  float tick = *time;\n  (void)tick;", 1)
        );
        let manifest = "name = \"ticking\"\nwash = \"wash.metal\"\nglyphs = \"glyphs.metal\"\n";
        let ticking = folder(&s.effects(), "ticking", manifest, &[("wash.metal", watercolor().wash), ("glyphs.metal", &timed)]);
        assert_eq!(run(&["check", &ticking.display().to_string()], &env), (0, "ok: ticking\nhighlights: app\n".to_string(), String::new()));
    }

    #[test]
    fn effect_check_compiles_both_shaders_and_offsets_the_preamble_s_lines() {
        if !metal() {
            return;
        }
        let s = Setup::new();
        let env = env!(s, s.path("none.sock"));
        let effects = s.effects();
        assert_eq!(run(&["template", "--as", "mywash"], &env).0, 0);
        let mine = effects.join("mywash").display().to_string();
        assert_eq!(run(&["check", &mine], &env), (0, "ok: mywash\nhighlights: effect\n".to_string(), String::new()));
        let (code, out, _) = run(&["check", &mine, "--json"], &env);
        assert_eq!(code, 0);
        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap(), serde_json::json!({ "ok": true, "name": "mywash", "highlights": "effect" }));
        // The app paints the highlights unless the manifest says otherwise.
        let manifest = "name = \"plain\"\nwash = \"wash.metal\"\n";
        let plain = folder(&effects, "plain", manifest, &[("wash.metal", watercolor().wash)]);
        assert_eq!(run(&["check", &plain.display().to_string()], &env), (0, "ok: plain\nhighlights: app\n".to_string(), String::new()));
        // A mistake on the wash's line 3 is reported on line 3 of wash.metal.
        let broken = folder(&effects, "broken", "name = \"broken\"\nwash = \"wash.metal\"\n", &[("wash.metal", &with_line(watercolor().wash, 3, "constant float oops = ;"))]);
        let (code, out, err) = run(&["check", &broken.display().to_string()], &env);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.contains("wash.metal:3:"), "{err}");
        assert!(!err.contains("program_source"), "{err}");
        // So is one on the glyph shader's line 2, compiled after gpui's own source.
        let inky = folder(
            &effects,
            "inky",
            "name = \"inky\"\nwash = \"wash.metal\"\nglyphs = \"glyphs.metal\"\n",
            &[("wash.metal", watercolor().wash), ("glyphs.metal", &with_line(watercolor().glyphs.unwrap(), 2, "constant float oops = ;"))],
        );
        let (code, _, err) = run(&["check", &inky.display().to_string()], &env);
        assert_eq!(code, 1);
        assert!(err.contains("glyphs.metal:2:"), "{err}");
        // A wash without the function the app draws with says so.
        let hollow = folder(&effects, "hollow", "name = \"hollow\"\nwash = \"wash.metal\"\n", &[("wash.metal", "// nothing here\n")]);
        let (code, _, err) = run(&["check", &hollow.display().to_string()], &env);
        assert_eq!(code, 1);
        assert!(err.contains("arkos_wash"), "{err}");
        // So does a glyph shader without its two functions.
        let half = folder(
            &effects,
            "half",
            "name = \"half\"\nwash = \"wash.metal\"\nglyphs = \"glyphs.metal\"\n",
            &[("wash.metal", watercolor().wash), ("glyphs.metal", "// nothing here\n")],
        );
        let (code, _, err) = run(&["check", &half.display().to_string()], &env);
        assert_eq!(code, 1);
        assert!(err.contains("arkos_glyph_vertex"), "{err}");
    }

    #[test]
    fn effect_use_sets_the_config_and_says_which_windows_answered() {
        let s = Setup::new();
        let sock = s.path("d.sock");
        let reloaded = Response::Customized { outcomes: vec![CustomizeOutcome { pid: 7, project: "/p".into(), result: Ok(Report::default()) }], missing: vec![8], too_old: vec![] };
        let server = fake_daemon(&sock, vec![reloaded]);
        let env = env!(s, sock);
        let (code, out, _) = run(&["use", "watercolor"], &env);
        assert_eq!(code, 0);
        assert_eq!(out, "window 7 (/p): effect.name applied\nwindow 8: no answer\n");
        let asked = server.join().unwrap();
        assert!(matches!(asked.as_slice(), [Request::Customize { action: CustomizeAction::ReloadConfig, .. }]), "{asked:?}");
        assert_eq!(config::load(&s.path("config.toml")).unwrap().effect.name, config::EffectChoice::Named("watercolor".into()));
        let alone = env!(s, s.path("none.sock"));
        assert_eq!(run(&["use", "none"], &alone), (0, "saved; no window running\n".to_string(), String::new()));
        assert_eq!(config::load(&s.path("config.toml")).unwrap().effect.name, config::EffectChoice::None);
        let (code, out, _) = run(&["use", "theme", "--json"], &alone);
        assert_eq!(code, 0);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!((v["key"].as_str(), v["value"].as_str(), v["saved"].as_bool()), (Some("effect.name"), Some("theme"), Some(true)));
        // An effect that does not load is refused and nothing is written.
        folder(&s.effects(), "broken", "name = \"broken\"\nwash = \"wash.metal\"\n", &[]);
        for name in ["nope", "broken"] {
            let (code, _, err) = run(&["use", name], &alone);
            assert_eq!(code, 1, "{name}");
            assert!(err.contains(name), "{err}");
        }
        assert_eq!(config::load(&s.path("config.toml")).unwrap().effect.name, config::EffectChoice::Theme);
        assert_eq!(run(&["use"], &alone).0, 2);
    }

    #[test]
    fn effect_current_prints_the_choice_then_what_each_window_draws_and_its_cost() {
        let s = Setup::new();
        let sock = s.path("d.sock");
        let ok = |pid: u32, project: &str| CustomizeOutcome { pid, project: project.into(), result: Ok(Report::default()) };
        let drawn = |effect: Option<(&str, Option<f32>)>| WindowDetail { effect: effect.map(|(n, ms)| (n.to_string(), ms)), size: Some((1200.0, 800.0)) };
        let answer = Response::CustomizedDetailed {
            outcomes: vec![
                (ok(7, "/p"), drawn(Some(("watercolor", Some(2.734))))),
                (ok(9, "/q"), drawn(Some(("ink", None)))),
                (ok(10, "/r"), drawn(None)),
                (ok(11, "/s"), WindowDetail::default()),
                (CustomizeOutcome { pid: 12, project: "/t".into(), result: Err("busy".into()) }, WindowDetail::default()),
            ],
            missing: vec![8],
            too_old: vec![5],
        };
        let server = fake_daemon(&sock, vec![answer.clone(), answer]);
        let env = env!(s, sock);
        let (code, out, _) = run(&["current"], &env);
        assert_eq!(code, 0);
        assert_eq!(
            out,
            "config: effect theme, intensity 1\n\
             window 7 (/p): watercolor, 2.73 ms per frame\n\
             window 9 (/q): ink, not measured yet\n\
             window 10 (/r): none\n\
             window 11 (/s): too old to report its effect\n\
             window 12 (/t): busy\n\
             window 8: no answer\n\
             window 5: too old for this command\n"
        );
        let (code, out, _) = run(&["current", "--json"], &env);
        assert_eq!(code, 0);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["config"], serde_json::json!({ "name": "theme", "intensity": 1.0 }));
        let w = &v["windows"][0];
        assert_eq!((w["pid"].as_u64(), w["effect"].as_str(), w["size"].clone()), (Some(7), Some("watercolor"), serde_json::json!([1200.0, 800.0])));
        assert!((w["gpu_ms"].as_f64().unwrap() - 2.734).abs() < 1e-4);
        assert!(v["windows"][2]["effect"].is_null() && v["windows"][1]["gpu_ms"].is_null());
        assert_eq!(v["windows"][4]["error"].as_str(), Some("busy"));
        assert_eq!((v["missing"].clone(), v["too_old"].clone()), (serde_json::json!([8]), serde_json::json!([5])));
        let asked = server.join().unwrap();
        assert!(asked.iter().all(|r| matches!(r, Request::CustomizeDetailed { action: CustomizeAction::ReportState, .. })), "{asked:?}");
        // No window: the config's line alone.
        let alone = env!(s, s.path("none.sock"));
        assert_eq!(run(&["current"], &alone), (0, "config: effect theme, intensity 1\n".to_string(), String::new()));
    }

    #[test]
    fn effect_bench_prints_the_cost_and_the_verdict() {
        let s = Setup::new();
        let env = env!(s, s.path("none.sock"));
        let (code, _, err) = run(&["bench", "nope"], &env);
        assert_eq!(code, 1);
        assert!(err.contains("nope"), "{err}");
        assert_eq!(run(&["bench", "watercolor", "--seconds", "0"], &env).0, 2);
        assert_eq!(run(&["bench", "watercolor", "--seconds"], &env).0, 2);
        assert_eq!(run(&["bench"], &env).0, 2);
        if !metal() {
            return;
        }
        // No window: 1200 by 800 points, Peach Light, the manifest's 30 frames a second.
        let (code, out, err) = run(&["bench", "watercolor", "--seconds", "0.2"], &env);
        assert_eq!((code, err.as_str()), (0, ""));
        let line = out.strip_suffix('\n').unwrap();
        let (head, verdict) = line.split_once("): ").unwrap();
        assert!(head.starts_with("watercolor: ") && head.contains(" ms per frame at 1200x800 (") && head.ends_with("% of a frame at 30 fps"), "{line}");
        let ms: f32 = head["watercolor: ".len()..].split(' ').next().unwrap().parse().unwrap();
        assert!(ms > 0.0, "{line}");
        assert_eq!(verdict, bench::verdict(ms));
        // A folder by its path, as JSON.
        assert_eq!(run(&["template", "--as", "mywash"], &env).0, 0);
        let (code, out, _) = run(&["bench", &s.effects().join("mywash").display().to_string(), "--seconds", "0.2", "--json"], &env);
        assert_eq!(code, 0);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!((v["name"].as_str(), v["theme"].as_str(), v["frame_rate"].as_u64()), (Some("mywash"), Some("peach-light"), Some(30)));
        assert_eq!(v["size"], serde_json::json!([1200.0, 800.0]));
        let ms = v["ms"].as_f64().unwrap() as f32;
        assert!(ms > 0.0);
        assert_eq!(v["verdict"].as_str(), Some(bench::verdict(ms)));
        assert!((v["share"].as_f64().unwrap() - (ms as f64 * 30.0 / 10.0)).abs() < 0.01);
    }
}
