//! `arkos open`, `arkos video` and `arkos panes` (video pane spec, sections 5 to 7): the agent's
//! command line for the panes of its own window. Each command checks its file and flags here,
//! sends one `Direct` through the daemon (`direct`) and prints what the window answered. No gpui
//! here. Exit codes as the rest of the toolkit: 0 done, 1 refused or failed, 2 usage, 3 no window
//! to aim at. `--json` prints one JSON value on stdout and nothing else; an error is
//! `{ "error": text, "exit": n }` there.

use crate::agent_cli::{self, fail, reach_from_io, usage, Reach};
use crate::video::{fmt_time, parse_range, parse_time};
use crate::{hook_cli, media, remote_image, viewer};
use protocol::{
    CustomizeAction, LoopChange, OpenKind, PaneAction, PaneOutcome, PanesReport, Placement, Request, Response, SessionId, SplitSide, VideoOp, VideoState,
    WindowTarget,
};
use serde_json::{json, Value as Json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const OPEN_USAGE: &str = "usage: arkos open <file> [--pane N | --split right|down|left|up] [--at TIME] [--paused | --playing]\n                  [--loop A-B | --loop off] [--raw] [--new] [--wait] [--focus] [--window PID] [--json]\nsee: arkos guide\n";

pub const VIDEO_USAGE: &str = "\
usage: arkos video status [<file>] [--json]
       arkos video play|pause [<file>] [--json]
       arkos video seek <TIME> [<file>] [--json]
       arkos video step [+N|-N] [<file>] [--json]
       arkos video loop <A-B>|on|off [<file>] [--json]
       arkos video reload [<file>] [--json]
       arkos video close [<file>] [--json]
       arkos video mute|unmute [<file>] [--json]
see: arkos guide
";

pub const PANES_USAGE: &str = "usage: arkos panes [--all | --window PID] [--json]\nsee: arkos guide\n";

/// The extensions of images, which a pane does not show yet.
const IMAGES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "heic", "tif", "tiff", "bmp", "svg"];

/// Where the commands talk: the daemon socket, the calling session, the shell's folder and how
/// long the daemon waits for a window.
pub struct DirectEnv<'a> {
    pub socket: &'a Path,
    pub session: Option<SessionId>,
    pub cwd: &'a Path,
    pub wait: Duration,
}

type Command = fn(&[String], &DirectEnv, &mut dyn Write, &mut dyn Write) -> i32;

fn with_real_env(command: Command, args: &[String]) -> i32 {
    let socket = crate::paths::socket_path();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let session = std::env::var("ARKOS_SESSION_ID").ok().and_then(|s| s.trim().parse::<u64>().ok()).map(SessionId);
    let env = DirectEnv { socket: &socket, session, cwd: &cwd, wait: protocol::CUSTOMIZE_WAIT };
    command(args, &env, &mut std::io::stdout(), &mut std::io::stderr())
}

/// `arkos open ...` with the real socket, session and folder.
pub fn open_cli(args: &[String]) -> i32 {
    with_real_env(open_cli_to, args)
}

/// `arkos video ...` with the real socket, session and folder.
pub fn video_cli(args: &[String]) -> i32 {
    with_real_env(video_cli_to, args)
}

/// `arkos panes ...` with the real socket, session and folder.
pub fn panes_cli(args: &[String]) -> i32 {
    with_real_env(panes_cli_to, args)
}

/// The words, the value flags and the switches of a command's arguments.
struct Parsed {
    words: Vec<String>,
    values: Vec<(String, String)>,
    switches: Vec<String>,
}

impl Parsed {
    fn value(&self, flag: &str) -> Option<&str> {
        self.values.iter().rev().find(|(f, _)| f == flag).map(|(_, v)| v.as_str())
    }

    fn has(&self, flag: &str) -> bool {
        self.switches.iter().any(|f| f == flag)
    }
}

/// Splits `args`; `valued` flags take the next argument, `switches` stand alone, `--json` is
/// always allowed. Any other flag, or a valued flag with no value, is a usage error (`None`).
fn parse(args: &[String], valued: &[&str], switches: &[&str]) -> Option<Parsed> {
    let mut p = Parsed { words: Vec::new(), values: Vec::new(), switches: Vec::new() };
    let mut it = args.iter().peekable();
    while let Some(arg) = it.next() {
        if !arg.starts_with("--") {
            p.words.push(arg.clone());
        } else if valued.contains(&arg.as_str()) {
            match it.peek() {
                Some(v) if !v.starts_with("--") => p.values.push((arg.clone(), it.next()?.clone())),
                _ => return None,
            }
        } else if arg == "--json" || switches.contains(&arg.as_str()) {
            p.switches.push(arg.clone());
        } else {
            return None;
        }
    }
    Some(p)
}

/// The exit code a refusal text means: 3 when there is no window to aim at, else 1.
fn exit_for(text: &str) -> i32 {
    let none = text == "no window running"
        || (text.starts_with("session ") && text.ends_with(" is not shown in a window"))
        || text.starts_with("your session is not shown in this window");
    if none {
        3
    } else {
        1
    }
}

/// Reports a failure: on stderr as a line, or with `json` as `{ error, exit }` on stdout.
fn finish_error(json: bool, text: &str, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let code = exit_for(text);
    if json {
        let _ = writeln!(out, "{}", json!({ "error": text, "exit": code }));
        code
    } else {
        fail(err, text);
        code
    }
}

/// `path` made absolute against `cwd`, and canonical when it exists.
fn absolute(cwd: &Path, path: &str) -> PathBuf {
    let p = Path::new(path);
    let joined = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    std::fs::canonicalize(&joined).unwrap_or(joined)
}

fn extension(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default()
}

/// The file checks before anything is sent: canonical path and kind, or the refusal text.
pub fn classify_open(path: &Path, cwd: &Path, raw: bool, wait: bool) -> Result<(PathBuf, OpenKind), String> {
    let given = path.to_string_lossy();
    if remote_image::is_remote(&given) {
        return Err("a web address is not a file; open it in the browser pane (Cmd+Shift+B)".into());
    }
    // Every check reads one path, the canonical one when the file exists: the one the tab shows.
    let abs = absolute(cwd, &given);
    let shown = abs.display();
    let name = abs.to_string_lossy();
    if IMAGES.contains(&extension(&abs).as_str()) {
        return Err(format!("images are not shown in a pane yet; open it with `open {shown}` (Preview)"));
    }
    if media::kind(&name) == Some(media::Media::Audio) {
        return Err(format!("audio is not shown in a pane yet; play it with `open -a 'QuickTime Player' {shown}`"));
    }
    if media::is_link(&name) {
        return Err(format!("a .webm is not shown in a pane; show it with `open -R {shown}` (Finder)"));
    }
    let is_video = media::kind(&name) == Some(media::Media::Video);
    if abs.is_dir() {
        return Err(format!("{shown} is a folder"));
    }
    if !abs.exists() {
        if wait && is_video {
            return Ok((abs, OpenKind::Video));
        }
        return Err(format!("no such file: {shown}"));
    }
    let kind = if is_video {
        OpenKind::Video
    } else if viewer::is_markdown(&abs) {
        OpenKind::Markdown { raw }
    } else {
        // The viewer's own rule: a NUL in the first bytes is not text.
        let mut head = Vec::new();
        if let Ok(f) = std::fs::File::open(&abs) {
            let _ = f.take(viewer::SNIFF as u64).read_to_end(&mut head);
        }
        if viewer::looks_binary(&head) {
            return Err(format!("{shown} is not a text file; Arkos shows text, markdown and video"));
        }
        OpenKind::Text
    };
    Ok((abs, kind))
}

/// The window a command aims at: `--window`, else the calling session's, else the only one.
pub fn target_from(env: &DirectEnv, window_flag: Option<u32>) -> WindowTarget {
    match (window_flag, env.session) {
        (Some(pid), _) => WindowTarget::Pid(pid),
        (None, Some(s)) => WindowTarget::Session(s),
        (None, None) => WindowTarget::Only,
    }
}

/// Sends one `Direct` and waits for the window's answer: its pid, its project and the outcome.
/// The daemon answers within `wait`, so the socket waits one second longer. No Hello.
pub fn direct(socket: &Path, window: WindowTarget, action: PaneAction, wait: Duration) -> Result<(u32, PathBuf, Result<PaneOutcome, String>), Reach> {
    let id = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
    let timeout = wait + Duration::from_secs(1);
    match hook_cli::send_one_with(socket, Request::Direct { id, window, action }, timeout) {
        Ok(Response::Directed { pid, project, outcome }) => Ok((pid, project, outcome)),
        Ok(Response::Error(e)) => Err(Reach::Io(e)),
        Ok(other) => Err(Reach::Io(format!("arkosd answered something else: {other:?}"))),
        Err(e) => Err(reach_from_io(&e, timeout)),
    }
}

/// Reports a failed reach through `finish_error`.
fn report_reach(reach: &Reach, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let text = reach.line();
    finish_error(json, &text, out, err)
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

/// How long ago, as `HH:MM:SS`.
fn ago(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// The words after the file name in a status or an opened line: where the video is, whether it
/// plays, its loop, its sound and size, and, with `full`, how long ago the file changed. The
/// state word ends the list, always when `full` and otherwise when it is not "ready".
fn detail(v: &VideoState, full: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(d) = v.duration {
        parts.push(format!("{} of {}", fmt_time(v.position), fmt_time(d)));
        parts.push(if v.playing { "playing" } else { "paused" }.into());
    }
    if let Some((a, b)) = v.loop_range {
        parts.push(format!("loop {}-{}", fmt_time(a), fmt_time(b)));
    }
    if v.muted {
        parts.push("muted".into());
    }
    if v.width > 0 && v.height > 0 {
        parts.push(format!("{}x{}", v.width, v.height));
    }
    if full {
        if let Some(m) = v.file_mtime {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(m);
            parts.push(format!("file changed {} ago", ago(now.saturating_sub(m))));
        }
    }
    if full || v.state != "ready" {
        parts.push(match &v.error {
            Some(e) => format!("{} ({e})", v.state),
            None => v.state.clone(),
        });
    } else if let Some(e) = &v.error {
        parts.push(e.clone());
    }
    parts.join(", ")
}

/// A video's line: `x.mp4 in pane 2: 0:12.0 of 1:48.5, paused, loop 0:12.0-0:18.0, 1920x1080, ready`.
pub fn status_line(v: &VideoState, file_name: &str) -> String {
    format!("{file_name} in pane {}: {}", v.pane, detail(v, true))
}

/// What `arkos open` prints; the window says whether it made a pane and whether a document is raw.
pub fn opened_line(o: &PaneOutcome, file_name: &str) -> String {
    let PaneOutcome::Opened { pane, kind, reused, reloaded, new_pane, raw, video, .. } = o else {
        return String::new();
    };
    let verb = if *reused { "shown" } else { "opened" };
    let prefix = if *reloaded { "reloaded, " } else { "" };
    let what = if kind == "video" { format!("video {file_name}") } else { file_name.to_string() };
    let fresh = if *new_pane && !reused { " (new pane)" } else { "" };
    let tail = match (video, kind.as_str()) {
        (Some(v), _) => format!(", {}", detail(v, false)),
        (None, "markdown") => if *raw { ", raw" } else { ", rendered" }.into(),
        _ => String::new(),
    };
    format!("{prefix}{verb} {what} in pane {pane}{fresh}{tail}")
}

/// A report's lines: one per pane, `pane 1 (focused): terminal claude (you), terminal zsh`.
pub fn panes_lines(r: &PanesReport) -> Vec<String> {
    r.panes
        .iter()
        .map(|p| {
            let tabs: Vec<String> = p
                .tabs
                .iter()
                .map(|t| format!("{} {}{}{}", t.kind, t.title, if t.you { " (you)" } else { "" }, if t.active { " (active)" } else { "" }))
                .collect();
            let head = format!("pane {}{}", p.n, if p.focused { " (focused)" } else { "" });
            if tabs.is_empty() {
                head
            } else {
                format!("{head}: {}", tabs.join(", "))
            }
        })
        .collect()
}

/// The video object of spec section 5.
fn video_json(v: &VideoState) -> Json {
    let mut o = json!({
        "duration": v.duration,
        "position": v.position,
        "playing": v.playing,
        "loop": v.loop_range.map(|(a, b)| json!([a, b])),
        "width": v.width,
        "height": v.height,
        "state": v.state,
    });
    if let Some(e) = &v.error {
        o["error"] = json!(e);
    }
    o
}

/// A video's state as `arkos video` prints it in JSON: the video object plus where it is.
fn state_json(v: &VideoState) -> Json {
    let mut o = video_json(v);
    o["pane"] = json!(v.pane);
    o["tab"] = json!(v.tab);
    o["path"] = json!(v.path);
    o["muted"] = json!(v.muted);
    o["file_mtime"] = json!(v.file_mtime);
    o
}

fn panes_json(r: &PanesReport) -> Json {
    let panes: Vec<Json> = r
        .panes
        .iter()
        .map(|p| {
            let tabs: Vec<Json> = p
                .tabs
                .iter()
                .map(|t| {
                    let mut o = json!({ "id": t.id, "kind": t.kind, "title": t.title, "active": t.active });
                    if t.you {
                        o["you"] = json!(true);
                    }
                    if let Some(path) = &t.path {
                        o["path"] = json!(path);
                    }
                    if let Some(s) = t.session {
                        o["session"] = json!(s.0);
                    }
                    o
                })
                .collect();
            json!({ "n": p.n, "id": p.id, "focused": p.focused, "tabs": tabs })
        })
        .collect();
    json!({ "project": r.project, "panes": panes })
}

/// The JSON of an outcome (spec sections 5 to 7), with the window's pid under `window`. One
/// video is the flat object; several are under `videos`.
pub fn outcome_json(pid: u32, o: &PaneOutcome) -> Json {
    let mut v = match o {
        PaneOutcome::Opened { pane, tab, kind, reused, reloaded, video, .. } => json!({
            "pane": pane, "tab": tab, "kind": kind, "reused": reused, "reloaded": reloaded,
            "video": video.as_ref().map(video_json),
        }),
        PaneOutcome::Videos(list) if list.len() == 1 => state_json(&list[0]),
        PaneOutcome::Videos(list) => json!({ "videos": list.iter().map(state_json).collect::<Vec<_>>() }),
        PaneOutcome::Panes(r) => panes_json(r),
    };
    v["window"] = json!(pid);
    v
}

/// Prints `value` as the one JSON value.
fn print_json(out: &mut dyn Write, value: &Json) {
    let _ = writeln!(out, "{value}");
}

/// The `--window PID` flag, or the text refusing it.
fn window_flag(p: &Parsed) -> Result<Option<u32>, String> {
    match p.value("--window") {
        None => Ok(None),
        Some(w) => w.parse::<u32>().map(Some).map_err(|_| format!("not a window pid: {w}")),
    }
}

/// `arkos open <file> ...` against `env`.
pub fn open_cli_to(args: &[String], env: &DirectEnv, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let valued = ["--pane", "--split", "--at", "--loop", "--window"];
    let switches = ["--paused", "--playing", "--raw", "--new", "--wait", "--focus"];
    let Some(p) = parse(args, &valued, &switches) else { return usage(err, OPEN_USAGE) };
    if p.words.len() != 1 || (p.has("--paused") && p.has("--playing")) || (p.value("--pane").is_some() && p.value("--split").is_some()) {
        return usage(err, OPEN_USAGE);
    }
    let json = p.has("--json");
    let flags = (|| -> Result<_, String> {
        let window = window_flag(&p)?;
        let at = p.value("--at").map(parse_time).transpose()?;
        let loop_change = match p.value("--loop") {
            None => LoopChange::Keep,
            Some("off") => LoopChange::Clear,
            Some(r) => parse_range(r).map(|(start, end)| LoopChange::Set { start, end })?,
        };
        let place = if let Some(n) = p.value("--pane") {
            match n.parse::<u32>() {
                Ok(n) if n >= 1 => Placement::Pane(n),
                _ => return Err(format!("not a pane number: {n}")),
            }
        } else if let Some(side) = p.value("--split") {
            Placement::Split(match side {
                "right" => SplitSide::Right,
                "down" => SplitSide::Bottom,
                "left" => SplitSide::Left,
                "up" => SplitSide::Top,
                other => return Err(format!("not a side: {other}; use right, down, left or up")),
            })
        } else {
            Placement::Preview
        };
        Ok((window, at, loop_change, place))
    })();
    let (window, at, loop_change, place) = match flags {
        Ok(f) => f,
        Err(text) => return finish_error(json, &text, out, err),
    };
    let raw = p.has("--raw");
    let (path, kind) = match classify_open(Path::new(&p.words[0]), env.cwd, raw, p.has("--wait")) {
        Ok(c) => c,
        Err(text) => return finish_error(json, &text, out, err),
    };
    let playing = if p.has("--playing") {
        Some(true)
    } else if p.has("--paused") {
        Some(false)
    } else {
        None
    };
    let name = file_name(&path);
    let action = PaneAction::Open { path, kind, place, at, playing, loop_change, reuse: !p.has("--new"), wait_for_file: p.has("--wait"), focus: p.has("--focus") };
    match direct(env.socket, target_from(env, window), action, env.wait) {
        Err(reach) => report_reach(&reach, json, out, err),
        Ok((_, _, Err(text))) => finish_error(json, &text, out, err),
        Ok((pid, _, Ok(o @ PaneOutcome::Opened { .. }))) => {
            if json {
                print_json(out, &outcome_json(pid, &o));
            } else {
                let _ = writeln!(out, "{}", opened_line(&o, &name));
            }
            // A video that cannot be played, or a reused tab whose new cut did not load (the old
            // one still showing): not done, as for `arkos video reload`.
            let failed = matches!(&o, PaneOutcome::Opened { video: Some(v), .. } if v.state == "unplayable" || v.error.as_deref() == Some(crate::video::RELOAD_FAILED));
            i32::from(failed)
        }
        Ok(_) => finish_error(json, "the window answered something else", out, err),
    }
}

/// The op a `video` form means, and the file it names, or a usage (`None`) or a refusal text.
fn video_form(words: &[String], cwd: &Path) -> Option<Result<(VideoOp, Option<PathBuf>), String>> {
    let (sub, rest) = words.split_first()?;
    let file = |w: Option<&String>| w.map(|w| absolute(cwd, w));
    let (op, file_at) = match sub.as_str() {
        "status" => (VideoOp::Status, 0),
        "play" => (VideoOp::Play, 0),
        "pause" => (VideoOp::Pause, 0),
        "reload" => (VideoOp::Reload, 0),
        "close" => (VideoOp::Close, 0),
        "mute" => (VideoOp::Mute(true), 0),
        "unmute" => (VideoOp::Mute(false), 0),
        "seek" => match parse_time(rest.first()?) {
            Ok(t) => (VideoOp::Seek(t), 1),
            Err(e) => return Some(Err(e)),
        },
        "step" => match rest.first().and_then(|w| w.parse::<i32>().ok()) {
            Some(n) => (VideoOp::Step(n), 1),
            None => (VideoOp::Step(1), 0),
        },
        "loop" => match rest.first()?.as_str() {
            "on" => (VideoOp::Loop(LoopChange::WholeFile), 1),
            "off" => (VideoOp::Loop(LoopChange::Clear), 1),
            r => match parse_range(r) {
                Ok((start, end)) => (VideoOp::Loop(LoopChange::Set { start, end }), 1),
                Err(e) => return Some(Err(e)),
            },
        },
        _ => return None,
    };
    if rest.len() > file_at + 1 {
        return None;
    }
    Some(Ok((op, file(rest.get(file_at)))))
}

/// `arkos video <command> ...` against `env`.
pub fn video_cli_to(args: &[String], env: &DirectEnv, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(p) = parse(args, &["--window"], &[]) else { return usage(err, VIDEO_USAGE) };
    let json = p.has("--json");
    let (op, path) = match video_form(&p.words, env.cwd) {
        None => return usage(err, VIDEO_USAGE),
        Some(Err(text)) => return finish_error(json, &text, out, err),
        Some(Ok(form)) => form,
    };
    let window = match window_flag(&p) {
        Ok(w) => w,
        Err(text) => return finish_error(json, &text, out, err),
    };
    match direct(env.socket, target_from(env, window), PaneAction::Video { path, op }, env.wait) {
        Err(reach) => report_reach(&reach, json, out, err),
        Ok((_, _, Err(text))) => finish_error(json, &text, out, err),
        Ok((pid, _, Ok(o @ PaneOutcome::Videos(_)))) => {
            let PaneOutcome::Videos(list) = &o else { return 1 };
            if json {
                print_json(out, &outcome_json(pid, &o));
            } else {
                for v in list {
                    let _ = writeln!(out, "{}", status_line(v, &file_name(&v.path)));
                }
            }
            let failed = matches!(op, VideoOp::Reload) && list.iter().any(|v| v.error.is_some() || v.state == "unplayable");
            i32::from(failed)
        }
        Ok(_) => finish_error(json, "the window answered something else", out, err),
    }
}

/// `arkos panes [--all]` against `env`.
pub fn panes_cli_to(args: &[String], env: &DirectEnv, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(p) = parse(args, &["--window"], &["--all"]) else { return usage(err, PANES_USAGE) };
    // `--all` asks every window, so a window named too is a contradiction, not a filter.
    if !p.words.is_empty() || (p.has("--all") && p.value("--window").is_some()) {
        return usage(err, PANES_USAGE);
    }
    let json = p.has("--json");
    let window = match window_flag(&p) {
        Ok(w) => w,
        Err(text) => return finish_error(json, &text, out, err),
    };
    if !p.has("--all") {
        return match direct(env.socket, target_from(env, window), PaneAction::Panes, env.wait) {
            Err(reach) => report_reach(&reach, json, out, err),
            Ok((_, _, Err(text))) => finish_error(json, &text, out, err),
            Ok((pid, _, Ok(PaneOutcome::Panes(r)))) => {
                if json {
                    print_json(out, &outcome_json(pid, &PaneOutcome::Panes(r)));
                } else {
                    for line in panes_lines(&r) {
                        let _ = writeln!(out, "{line}");
                    }
                }
                0
            }
            Ok(_) => finish_error(json, "the window answered something else", out, err),
        };
    }
    // Every window: ask who is there (the customize relay lists them), then each one.
    let windows = match agent_cli::customize(env.socket, CustomizeAction::ReportState, env.wait) {
        Ok(a) => a.outcomes,
        Err(reach) => return report_reach(&reach, json, out, err),
    };
    if windows.is_empty() {
        return finish_error(json, "no window running", out, err);
    }
    let mut all = Vec::new();
    let mut code = 0;
    for w in &windows {
        match direct(env.socket, WindowTarget::Pid(w.pid), PaneAction::Panes, env.wait) {
            Ok((pid, project, Ok(PaneOutcome::Panes(r)))) => {
                if !json {
                    let _ = writeln!(out, "window {pid} ({})", project.display());
                    for line in panes_lines(&r) {
                        let _ = writeln!(out, "{line}");
                    }
                }
                all.push(outcome_json(pid, &PaneOutcome::Panes(r)));
            }
            other => {
                let text = match other {
                    Ok((_, _, Err(t))) => t,
                    Err(reach) => reach.line(),
                    Ok(_) => "the window answered something else".into(),
                };
                code = 1;
                all.push(json!({ "window": w.pid, "error": text }));
                if !json {
                    let _ = writeln!(err, "window {}: {text}", w.pid);
                }
            }
        }
    }
    if json {
        print_json(out, &json!(all));
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::codec::{read_frame, write_frame};
    use protocol::{ClientMessage, PaneRow, ServerMessage, TabRow};

    /// A daemon stand-in: answers one connection per response, in order, and returns the requests.
    fn fake_daemon(sock: &Path, answers: Vec<Response>) -> std::thread::JoinHandle<Vec<Request>> {
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

    fn state(path: &Path) -> VideoState {
        VideoState {
            pane: 2,
            tab: 9,
            path: path.to_path_buf(),
            duration: Some(108.5),
            position: 12.0,
            playing: false,
            muted: false,
            loop_range: None,
            width: 1920,
            height: 1080,
            state: "ready".into(),
            error: None,
            file_mtime: None,
        }
    }

    fn opened(video: Option<VideoState>, reused: bool) -> Response {
        Response::Directed {
            pid: 7,
            project: "/p".into(),
            outcome: Ok(PaneOutcome::Opened { pane: 2, tab: 9, kind: "video".into(), reused, reloaded: false, new_pane: false, raw: false, video }),
        }
    }

    fn videos(v: Vec<VideoState>) -> Response {
        Response::Directed { pid: 7, project: "/p".into(), outcome: Ok(PaneOutcome::Videos(v)) }
    }

    fn refused(text: &str) -> Response {
        Response::Directed { pid: 7, project: "/p".into(), outcome: Err(text.into()) }
    }

    struct Ran {
        code: i32,
        out: String,
        err: String,
    }

    fn run(f: fn(&[String], &DirectEnv, &mut dyn Write, &mut dyn Write) -> i32, args: &[&str], sock: &Path, cwd: &Path, session: Option<u64>) -> Ran {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let env = DirectEnv { socket: sock, session: session.map(SessionId), cwd, wait: Duration::from_secs(1) };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = f(&args, &env, &mut out, &mut err);
        Ran { code, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
    }

    fn direct_of(r: &Request) -> (WindowTarget, PaneAction) {
        match r {
            Request::Direct { window, action, .. } => (window.clone(), action.clone()),
            other => panic!("not a Direct: {other:?}"),
        }
    }

    #[test]
    fn open_refuses_what_it_cannot_show_before_sending() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("none.sock");
        let d = dir.path().canonicalize().unwrap();
        std::fs::write(d.join("a.png"), b"x").unwrap();
        std::fs::write(d.join("a.mp3"), b"x").unwrap();
        std::fs::write(d.join("a.webm"), b"x").unwrap();
        std::fs::write(d.join("a.mp4"), b"x").unwrap();
        std::fs::create_dir(d.join("folder")).unwrap();
        let cases: Vec<(Vec<&str>, String)> = vec![
            (vec!["nope.mp4"], format!("no such file: {}", d.join("nope.mp4").display())),
            (vec!["a.png"], format!("images are not shown in a pane yet; open it with `open {}` (Preview)", d.join("a.png").display())),
            (vec!["a.mp3"], format!("audio is not shown in a pane yet; play it with `open -a 'QuickTime Player' {}`", d.join("a.mp3").display())),
            (vec!["a.webm"], format!("a .webm is not shown in a pane; show it with `open -R {}` (Finder)", d.join("a.webm").display())),
            (vec!["https://example.com/x.mp4"], "a web address is not a file; open it in the browser pane (Cmd+Shift+B)".into()),
            (vec!["folder"], format!("{} is a folder", d.join("folder").display())),
            (vec!["a.mp4", "--at", "1:60"], "not a time: \"1:60\"".into()),
            (vec!["a.mp4", "--pane", "x"], "not a pane number: x".into()),
        ];
        for (args, text) in cases {
            let ran = run(open_cli_to, &args, &sock, &d, None);
            assert_eq!(ran.code, 1, "{args:?}: {}", ran.err);
            assert_eq!(ran.err.trim(), text, "{args:?}");
            assert_eq!(ran.out, "");
        }
        // --json prints the refusal on stdout and nothing on stderr.
        let ran = run(open_cli_to, &["nope.mp4", "--json"], &sock, &d, None);
        assert_eq!(ran.code, 1);
        assert_eq!(ran.err, "");
        let v: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
        assert_eq!(v["exit"], 1);
        assert!(v["error"].as_str().unwrap().starts_with("no such file: "));
        // --wait lets a missing video through, as an absolute path that is not canonical.
        let sock = dir.path().join("w.sock");
        let server = fake_daemon(&sock, vec![opened(None, false)]);
        let ran = run(open_cli_to, &["later.mp4", "--wait"], &sock, &d, None);
        assert_eq!(ran.code, 0, "{}", ran.err);
        let asked = server.join().unwrap();
        let (_, PaneAction::Open { path, wait_for_file, .. }) = direct_of(&asked[0]) else { panic!() };
        assert_eq!(path, d.join("later.mp4"));
        assert!(wait_for_file);
        // A missing file that is not a video stays refused under --wait.
        let ran = run(open_cli_to, &["later.md", "--wait"], &sock, &d, None);
        assert_eq!(ran.code, 1);
    }

    #[test]
    fn open_refuses_a_binary_file_but_passes_text_and_unsniffed_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().canonicalize().unwrap();
        std::fs::write(d.join("blob.bin"), b"ab\0cd").unwrap();
        std::fs::write(d.join("notes.txt"), b"hello").unwrap();
        std::fs::write(d.join("raw.md"), b"a\0b").unwrap();
        let (_, kind) = classify_open(Path::new("notes.txt"), &d, false, false).unwrap();
        assert_eq!(kind, OpenKind::Text);
        let err = classify_open(Path::new("blob.bin"), &d, false, false).unwrap_err();
        assert_eq!(err, format!("{} is not a text file; Arkos shows text, markdown and video", d.join("blob.bin").display()));
        // Markdown is never sniffed, nor a video; a missing file under --wait is not read.
        assert!(classify_open(Path::new("raw.md"), &d, false, false).is_ok());
        assert!(classify_open(Path::new("later.bin"), &d, false, true).is_err());
    }

    #[test]
    fn open_sends_the_right_action_and_prints_the_line() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().canonicalize().unwrap();
        std::fs::write(d.join("x.mp4"), b"x").unwrap();
        let sock = dir.path().join("d.sock");
        let video = state(&d.join("x.mp4"));
        let server = fake_daemon(&sock, vec![opened(Some(video.clone()), false), opened(Some(video.clone()), true)]);
        let args = ["x.mp4", "--at", "0:12", "--loop", "0:12-0:18", "--playing", "--pane", "2"];
        let ran = run(open_cli_to, &args, &sock, &d, Some(5));
        assert_eq!(ran.code, 0, "{}", ran.err);
        assert_eq!(ran.out, "opened video x.mp4 in pane 2, 0:12.0 of 1:48.5, paused, 1920x1080\n");
        let mut with_json = args.to_vec();
        with_json.push("--json");
        let ran = run(open_cli_to, &with_json, &sock, &d, Some(5));
        assert_eq!(ran.code, 0);
        let v: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
        assert_eq!(v["window"], 7);
        assert_eq!(v["pane"], 2);
        assert_eq!(v["kind"], "video");
        assert_eq!(v["reused"], true);
        assert_eq!(v["video"]["duration"], 108.5);
        assert_eq!(v["video"]["loop"], serde_json::Value::Null);
        assert_eq!(v["video"]["state"], "ready");
        assert!(v["video"].get("error").is_none());
        let asked = server.join().unwrap();
        let (target, action) = direct_of(&asked[0]);
        assert_eq!(target, WindowTarget::Session(SessionId(5)));
        assert_eq!(
            action,
            PaneAction::Open {
                path: d.join("x.mp4"),
                kind: OpenKind::Video,
                place: Placement::Pane(2),
                at: Some(12.0),
                playing: Some(true),
                loop_change: LoopChange::Set { start: 12.0, end: 18.0 },
                reuse: true,
                wait_for_file: false,
                focus: false,
            }
        );
    }

    #[test]
    fn open_says_new_pane_for_a_split_and_reloaded_and_shown_for_a_reuse() {
        let path = Path::new("/v/x.mp4");
        let o = PaneOutcome::Opened { pane: 2, tab: 1, kind: "video".into(), reused: false, reloaded: false, new_pane: true, raw: false, video: Some(state(path)) };
        assert_eq!(opened_line(&o, "x.mp4"), "opened video x.mp4 in pane 2 (new pane), 0:12.0 of 1:48.5, paused, 1920x1080");
        let o = PaneOutcome::Opened { pane: 2, tab: 1, kind: "video".into(), reused: true, reloaded: true, new_pane: false, raw: false, video: Some(state(path)) };
        assert_eq!(opened_line(&o, "x.mp4"), "reloaded, shown video x.mp4 in pane 2, 0:12.0 of 1:48.5, paused, 1920x1080");
        let o = PaneOutcome::Opened { pane: 2, tab: 1, kind: "markdown".into(), reused: false, reloaded: false, new_pane: false, raw: false, video: None };
        assert_eq!(opened_line(&o, "notes.md"), "opened notes.md in pane 2, rendered");
        let o = PaneOutcome::Opened { pane: 3, tab: 1, kind: "markdown".into(), reused: true, reloaded: false, new_pane: false, raw: true, video: None };
        assert_eq!(opened_line(&o, "notes.md"), "shown notes.md in pane 3, raw");
    }

    #[test]
    fn open_exits_1_when_the_reused_tab_could_not_load_the_new_cut_and_status_exits_0() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().canonicalize().unwrap();
        std::fs::write(d.join("x.mp4"), b"x").unwrap();
        let sock = dir.path().join("d.sock");
        let stale = VideoState { error: Some(crate::video::RELOAD_FAILED.into()), ..state(&d.join("x.mp4")) };
        let reloaded = |video: VideoState| Response::Directed {
            pid: 7,
            project: "/p".into(),
            outcome: Ok(PaneOutcome::Opened { pane: 2, tab: 9, kind: "video".into(), reused: true, reloaded: true, new_pane: false, raw: false, video: Some(video) }),
        };
        let server = fake_daemon(&sock, vec![reloaded(stale.clone()), reloaded(stale.clone()), reloaded(state(&d.join("x.mp4"))), videos(vec![stale.clone()])]);
        let ran = run(open_cli_to, &["x.mp4", "--at", "0:12"], &sock, &d, Some(5));
        assert_eq!(ran.code, 1, "{}", ran.out);
        assert_eq!(ran.out, "reloaded, shown video x.mp4 in pane 2, 0:12.0 of 1:48.5, paused, 1920x1080, new file cannot be played yet\n", "the line still prints");
        let ran = run(open_cli_to, &["x.mp4", "--json"], &sock, &d, Some(5));
        assert_eq!(ran.code, 1);
        let v: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
        assert_eq!(v["video"]["error"], crate::video::RELOAD_FAILED);
        assert_eq!(run(open_cli_to, &["x.mp4"], &sock, &d, Some(5)).code, 0, "a reload that loaded is done");
        let ran = run(video_cli_to, &["status"], &sock, &d, Some(5));
        assert_eq!(ran.code, 0, "status reports, it does not fail");
        assert!(ran.out.ends_with("ready (new file cannot be played yet)\n"), "{}", ran.out);
        server.join().unwrap();
    }

    #[test]
    fn open_exits_1_when_the_video_cannot_be_played_as_reload_does() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().canonicalize().unwrap();
        std::fs::write(d.join("x.mp4"), b"x").unwrap();
        let sock = dir.path().join("d.sock");
        let bad = VideoState { duration: None, width: 0, height: 0, state: "unplayable".into(), error: Some("no video track".into()), ..state(&d.join("x.mp4")) };
        let server = fake_daemon(&sock, vec![opened(Some(bad.clone()), false), opened(Some(bad.clone()), true), videos(vec![bad.clone()])]);
        let ran = run(open_cli_to, &["x.mp4"], &sock, &d, Some(5));
        assert_eq!(ran.code, 1, "{}", ran.out);
        assert_eq!(ran.out, "opened video x.mp4 in pane 2, unplayable (no video track)\n", "the line still prints");
        assert_eq!(run(open_cli_to, &["x.mp4", "--json"], &sock, &d, Some(5)).code, 1);
        assert_eq!(run(video_cli_to, &["reload"], &sock, &d, Some(5)).code, 1);
        server.join().unwrap();
    }

    #[test]
    fn open_classifies_a_symlink_by_the_file_it_points_at() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().canonicalize().unwrap();
        std::fs::write(d.join("clip.mp4"), b"x").unwrap();
        std::fs::write(d.join("shot.png"), b"x").unwrap();
        std::os::unix::fs::symlink(d.join("clip.mp4"), d.join("a.png")).unwrap();
        std::os::unix::fs::symlink(d.join("shot.png"), d.join("b.mp4")).unwrap();
        // The tab shows the canonical file, so its name decides the kind, as the image check did.
        assert_eq!(classify_open(Path::new("a.png"), &d, false, false), Ok((d.join("clip.mp4"), OpenKind::Video)));
        assert_eq!(classify_open(Path::new("b.mp4"), &d, false, false), Err(format!("images are not shown in a pane yet; open it with `open {}` (Preview)", d.join("shot.png").display())));
    }

    #[test]
    fn open_targets_the_session_then_the_pid_then_only() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().canonicalize().unwrap();
        std::fs::write(d.join("x.mp4"), b"x").unwrap();
        let sock = dir.path().join("d.sock");
        let server = fake_daemon(&sock, vec![opened(None, false), opened(None, false), opened(None, false)]);
        run(open_cli_to, &["x.mp4"], &sock, &d, Some(3));
        run(open_cli_to, &["x.mp4", "--window", "42"], &sock, &d, Some(3));
        run(open_cli_to, &["x.mp4"], &sock, &d, None);
        let targets: Vec<WindowTarget> = server.join().unwrap().iter().map(|r| direct_of(r).0).collect();
        assert_eq!(targets, vec![WindowTarget::Session(SessionId(3)), WindowTarget::Pid(42), WindowTarget::Only]);
        let env = DirectEnv { socket: &sock, session: None, cwd: &d, wait: Duration::from_secs(1) };
        assert_eq!(target_from(&env, Some(1)), WindowTarget::Pid(1));
    }

    #[test]
    fn open_exits_3_without_a_window_and_1_on_a_refused_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().canonicalize().unwrap();
        std::fs::write(d.join("x.mp4"), b"x").unwrap();
        let none = dir.path().join("none.sock");
        let ran = run(open_cli_to, &["x.mp4"], &none, &d, None);
        assert_eq!((ran.code, ran.err.trim()), (3, "no window running"));
        let sock = dir.path().join("d.sock");
        let server = fake_daemon(
            &sock,
            vec![
                Response::Error("no window running".into()),
                Response::Error("several windows: pids 1, 2; pass --window".into()),
                Response::Error("session 4 is not shown in a window".into()),
                refused("your session is not shown in this window; pass --pane or --window"),
                refused("pane 9 does not exist; this window has 2"),
            ],
        );
        let codes: Vec<i32> = (0..5).map(|_| run(open_cli_to, &["x.mp4"], &sock, &d, None).code).collect();
        server.join().unwrap();
        assert_eq!(codes, vec![3, 1, 3, 3, 1]);
    }

    #[test]
    fn open_rejects_flags_that_cannot_go_together() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("none.sock");
        let d = dir.path();
        for args in [&["x.mp4", "--paused", "--playing"][..], &["x.mp4", "--pane", "1", "--split", "right"], &["x.mp4", "--at"], &["x.mp4", "--bogus"], &[]] {
            let ran = run(open_cli_to, args, &sock, d, None);
            assert_eq!(ran.code, 2, "{args:?}");
            assert!(ran.err.contains("see: arkos guide"));
        }
    }

    #[test]
    fn video_subcommands_map_to_ops() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().canonicalize().unwrap();
        let sock = dir.path().join("d.sock");
        let v = state(&d.join("x.mp4"));
        let forms: Vec<(Vec<&str>, VideoOp)> = vec![
            (vec!["status"], VideoOp::Status),
            (vec!["play"], VideoOp::Play),
            (vec!["pause"], VideoOp::Pause),
            (vec!["seek", "1:05.5"], VideoOp::Seek(65.5)),
            (vec!["step"], VideoOp::Step(1)),
            (vec!["step", "-3"], VideoOp::Step(-3)),
            (vec!["step", "+2"], VideoOp::Step(2)),
            (vec!["loop", "0:02-0:04"], VideoOp::Loop(LoopChange::Set { start: 2.0, end: 4.0 })),
            (vec!["loop", "on"], VideoOp::Loop(LoopChange::WholeFile)),
            (vec!["loop", "off"], VideoOp::Loop(LoopChange::Clear)),
            (vec!["reload"], VideoOp::Reload),
            (vec!["close"], VideoOp::Close),
            (vec!["mute"], VideoOp::Mute(true)),
            (vec!["unmute"], VideoOp::Mute(false)),
        ];
        let server = fake_daemon(&sock, forms.iter().map(|_| videos(vec![v.clone()])).collect());
        for (args, _) in &forms {
            let ran = run(video_cli_to, args, &sock, &d, Some(1));
            assert_eq!(ran.code, 0, "{args:?}: {}", ran.err);
            assert_eq!(ran.out, "x.mp4 in pane 2: 0:12.0 of 1:48.5, paused, 1920x1080, ready\n");
        }
        let asked = server.join().unwrap();
        for ((args, op), r) in forms.iter().zip(&asked) {
            let (_, PaneAction::Video { path, op: got }) = direct_of(r) else { panic!() };
            assert_eq!((&got, path), (op, None), "{args:?}");
        }
        // A file names the tab; a bad time and a bad range are refused; several tabs print a line each.
        let sock = dir.path().join("e.sock");
        let server = fake_daemon(&sock, vec![videos(vec![v.clone(), VideoState { pane: 3, ..v.clone() }])]);
        let ran = run(video_cli_to, &["status", "x.mp4"], &sock, &d, None);
        assert_eq!(ran.out.lines().count(), 2);
        let (_, PaneAction::Video { path, .. }) = direct_of(&server.join().unwrap()[0]) else { panic!() };
        assert_eq!(path, Some(d.join("x.mp4")));
        assert_eq!(run(video_cli_to, &["seek", "abc"], &sock, &d, None).code, 1);
        assert_eq!(run(video_cli_to, &["loop", "5-2"], &sock, &d, None).code, 1);
        assert_eq!(run(video_cli_to, &["seek"], &sock, &d, None).code, 2);
        assert_eq!(run(video_cli_to, &["dance"], &sock, &d, None).code, 2);
        assert_eq!(run(video_cli_to, &[], &sock, &d, None).code, 2);
    }

    #[test]
    fn video_reload_that_fails_exits_1_and_json_has_the_state() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().canonicalize().unwrap();
        let sock = dir.path().join("d.sock");
        let bad = VideoState { error: Some(crate::video::RELOAD_FAILED.into()), ..state(&d.join("x.mp4")) };
        let server = fake_daemon(&sock, vec![videos(vec![bad.clone()]), videos(vec![state(&d.join("x.mp4"))])]);
        let ran = run(video_cli_to, &["reload"], &sock, &d, None);
        assert_eq!(ran.code, 1);
        assert!(ran.out.contains("new file cannot be played yet"), "{}", ran.out);
        let ran = run(video_cli_to, &["status", "--json"], &sock, &d, None);
        server.join().unwrap();
        let v: serde_json::Value = serde_json::from_str(&ran.out).unwrap();
        assert_eq!((v["window"].clone(), v["pane"].clone(), v["muted"].clone()), (7.into(), 2.into(), false.into()));
        assert_eq!(v["position"], 12.0);
    }

    #[test]
    fn status_line_follows_the_spec() {
        let mut v = state(Path::new("/v/x.mp4"));
        v.loop_range = Some((12.0, 18.0));
        v.playing = true;
        assert_eq!(status_line(&v, "x.mp4"), "x.mp4 in pane 2: 0:12.0 of 1:48.5, playing, loop 0:12.0-0:18.0, 1920x1080, ready");
        v.duration = None;
        v.loop_range = None;
        v.state = "loading".into();
        v.width = 0;
        assert_eq!(status_line(&v, "x.mp4"), "x.mp4 in pane 2: loading");
        assert_eq!(ago(4), "00:00:04");
        assert_eq!(ago(3725), "01:02:05");
    }

    #[test]
    fn panes_prints_one_line_per_pane_with_you_active_and_focused() {
        let tab = |id: u64, kind: &str, title: &str, active: bool, you: bool| TabRow { id, kind: kind.into(), title: title.into(), active, you, path: None, session: None };
        let r = PanesReport {
            project: "/p".into(),
            panes: vec![
                PaneRow { n: 1, id: 10, focused: true, tabs: vec![tab(1, "terminal", "claude", false, true), tab(2, "terminal", "zsh", false, false)] },
                PaneRow { n: 2, id: 11, focused: false, tabs: vec![tab(3, "video", "x.mp4", true, false), tab(4, "markdown", "notes.md", false, false)] },
            ],
        };
        assert_eq!(panes_lines(&r), vec!["pane 1 (focused): terminal claude (you), terminal zsh", "pane 2: video x.mp4 (active), markdown notes.md"]);
        let v = outcome_json(7, &PaneOutcome::Panes(r));
        assert_eq!(v["window"], 7);
        assert_eq!(v["panes"][0]["tabs"][0]["you"], true);
        assert!(v["panes"][0]["tabs"][1].get("you").is_none());
        assert_eq!(v["panes"][1]["tabs"][0]["kind"], "video");
        // Through the command, with a fake daemon.
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("d.sock");
        let report = PanesReport { project: "/p".into(), panes: vec![PaneRow { n: 1, id: 10, focused: true, tabs: vec![tab(1, "terminal", "zsh", true, false)] }] };
        let server = fake_daemon(&sock, vec![Response::Directed { pid: 7, project: "/p".into(), outcome: Ok(PaneOutcome::Panes(report)) }]);
        let ran = run(panes_cli_to, &[], &sock, dir.path(), None);
        assert_eq!((ran.code, ran.out.as_str()), (0, "pane 1 (focused): terminal zsh (active)\n"));
        assert!(matches!(direct_of(&server.join().unwrap()[0]).1, PaneAction::Panes));
    }

    #[test]
    fn panes_all_asks_every_window() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("d.sock");
        let report = |n: u32| PanesReport { project: "/p".into(), panes: vec![PaneRow { n, id: 1, focused: false, tabs: vec![] }] };
        let reports = Response::Customized {
            outcomes: vec![
                protocol::CustomizeOutcome { pid: 7, project: "/p".into(), result: Ok(protocol::Report::default()) },
                protocol::CustomizeOutcome { pid: 8, project: "/q".into(), result: Ok(protocol::Report::default()) },
            ],
            missing: vec![],
            too_old: vec![],
        };
        let directed = |pid: u32, n: u32| Response::Directed { pid, project: if pid == 7 { "/p" } else { "/q" }.into(), outcome: Ok(PaneOutcome::Panes(report(n))) };
        let server = fake_daemon(&sock, vec![reports, directed(7, 1), directed(8, 2)]);
        let ran = run(panes_cli_to, &["--all"], &sock, dir.path(), None);
        assert_eq!(ran.code, 0, "{}", ran.err);
        assert_eq!(ran.out, "window 7 (/p)\npane 1\nwindow 8 (/q)\npane 2\n");
        let asked = server.join().unwrap();
        assert_eq!(direct_of(&asked[2]).0, WindowTarget::Pid(8));
    }

    #[test]
    fn panes_all_with_a_window_is_a_usage_error() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("none.sock");
        let ran = run(panes_cli_to, &["--all", "--window", "42"], &sock, dir.path(), None);
        assert_eq!(ran.code, 2);
        assert!(ran.err.contains("see: arkos guide"), "{}", ran.err);
    }

    #[test]
    fn every_usage_text_names_see_arkos_guide() {
        for text in [OPEN_USAGE, VIDEO_USAGE, PANES_USAGE] {
            assert!(text.ends_with("see: arkos guide\n"));
        }
        assert_eq!(VIDEO_USAGE.lines().filter(|l| l.contains("arkos video")).count(), 8);
        assert!(crate::agent_cli::USAGE.contains("arkos open <file>"));
    }
}
