//! The hook side of session awareness (spec 5 and 2.3) and of recovery (update-in-place spec,
//! section 11): `arkos notify`, run by Claude Code's hooks with `ARKOS_SESSION_ID` in its
//! environment, sends the daemon one request and exits 0 whatever happens; `arkos hooks` prints
//! or installs the hook entries. No gpui here: this runs before the window exists.

use protocol::codec::{read_frame, write_frame};
use protocol::{AgentRecord, ClaudeState, ClientMessage, Request, Response, ServerMessage, SessionId};
use std::io::{BufReader, BufWriter, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long one request to the daemon may take; a hook must never hold Claude Code up.
const TIMEOUT: Duration = Duration::from_secs(2);
/// A hook's stdin is a small JSON object; more than this is not one.
const STDIN_CAP: u64 = 64 * 1024;

/// What the SessionStart hook prints inside Arkos when `arkos` resolves on its PATH. Claude Code
/// adds a SessionStart hook's standard output to the agent's context, so the agent learns where
/// the guide is.
pub const CONTEXT_LINES: &str = "This session runs inside Arkos IDE.\nTo change its theme, fonts or settings, run `arkos guide` first and follow it.\n";

/// The SessionStart hook's two lines: `CONTEXT_LINES` when `on_path`, else the second line names
/// `exe`, this binary, by its full path, quoted for `sh`, so the command it gives runs as written.
pub fn context_lines(on_path: bool, exe: &Path) -> String {
    if on_path {
        CONTEXT_LINES.to_string()
    } else {
        format!("This session runs inside Arkos IDE.\nTo change its theme, fonts or settings, run `{} guide` first and follow it.\n", shell_quote(exe))
    }
}

/// Whether `name` resolves through `path_var` (a `PATH` value): some entry holds a file of that
/// name with an execute bit. Empty entries are skipped rather than read as the current folder.
pub fn resolves_on_path(path_var: &str, name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path_var
        .split(':')
        .filter(|dir| !dir.is_empty())
        .any(|dir| std::fs::metadata(Path::new(dir).join(name)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notify {
    State(ClaudeState),
    Event(HookEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookInput {
    pub session_id: String,
    /// SessionEnd's `reason`: `clear`, `resume`, `logout`, `prompt_input_exit` or `other`.
    pub reason: Option<String>,
}

/// `notify --state <working|waiting|idle>` or `notify --event <session-start|session-end>`.
pub fn parse_notify(args: &[String]) -> Result<Notify, String> {
    let usage = "usage: arkos notify --state <working|waiting|idle> | --event <session-start|session-end>";
    match (args.first().map(String::as_str), args.get(1).map(String::as_str), args.get(2).map(String::as_str)) {
        (Some("notify"), Some("--state"), Some("working")) => Ok(Notify::State(ClaudeState::Working)),
        (Some("notify"), Some("--state"), Some("waiting")) => Ok(Notify::State(ClaudeState::Waiting)),
        (Some("notify"), Some("--state"), Some("idle")) => Ok(Notify::State(ClaudeState::Idle)),
        (Some("notify"), Some("--event"), Some("session-start")) => Ok(Notify::Event(HookEvent::SessionStart)),
        (Some("notify"), Some("--event"), Some("session-end")) => Ok(Notify::Event(HookEvent::SessionEnd)),
        _ => Err(usage.to_string()),
    }
}

/// The fields of a hook's stdin this needs: Claude Code sends `session_id`, `transcript_path`,
/// `cwd` and `hook_event_name` on every event (its hooks reference), and SessionEnd a `reason`;
/// only the id and the reason are kept.
pub fn parse_hook_input(stdin: &str) -> Option<HookInput> {
    let v: serde_json::Value = serde_json::from_str(stdin).ok()?;
    let id = v.get("session_id")?.as_str()?.trim();
    let reason = v.get("reason").and_then(|r| r.as_str()).map(str::to_string);
    (!id.is_empty()).then(|| HookInput { session_id: id.to_string(), reason })
}

/// The command that resumes an agent's session, as argv. Claude Code: `claude --resume <id>`
/// works from any directory and never sends a message.
pub fn resume_command(kind: &str, id: &str) -> Vec<String> {
    match kind {
        "claude" => vec!["claude".into(), "--resume".into(), id.into()],
        other => vec![other.into(), "--resume".into(), id.into()],
    }
}

pub fn request_for(notify: &Notify, session: SessionId, input: Option<&HookInput>) -> Option<Request> {
    match notify {
        Notify::State(state) => Some(Request::SetState { session, state: *state }),
        Notify::Event(HookEvent::SessionStart) => {
            let id = input?.session_id.clone();
            Some(Request::SetAgent { session, agent: AgentRecord { kind: "claude".into(), resume: resume_command("claude", &id), id } })
        }
        // "other" is a Claude Code that was killed, as a shutdown kills it: the record stays, so
        // the recovery that follows resumes it. Any other end (or none given) clears it.
        Notify::Event(HookEvent::SessionEnd) => {
            let input = input?;
            (input.reason.as_deref() != Some("other")).then(|| Request::ClearAgent { session, id: input.session_id.clone() })
        }
    }
}

/// One request on a fresh connection, with no Hello: the daemon must not take a hook for a
/// window. Events the daemon pushes meanwhile are skipped until the answer arrives.
pub fn send_one(socket: &Path, request: Request) -> std::io::Result<Response> {
    send_one_with(socket, request, TIMEOUT)
}

/// `send_one` with its own read and write timeout, for a request whose answer takes longer than
/// a hook may wait (`arkos theme` and friends wait for the windows' answers).
pub fn send_one_with(socket: &Path, request: Request, timeout: Duration) -> std::io::Result<Response> {
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut w = BufWriter::new(stream.try_clone()?);
    let mut r = BufReader::new(stream);
    write_frame(&mut w, &ClientMessage { id: 1, request })?;
    w.flush()?;
    loop {
        match read_frame::<_, ServerMessage>(&mut r)? {
            Some(ServerMessage::Response { id: 1, response }) => return Ok(response),
            Some(_) => continue,
            None => return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the daemon hung up without answering")),
        }
    }
}

/// `arkos notify ...` with its inputs explicit, for tests. Always 0 for `notify`: a hook
/// that fails would show up in Claude Code as a hook error, for something that is only a tab
/// icon. Every reason is on stderr, which Claude Code only shows for a non-zero exit. A session
/// start inside Arkos, and only that, calls `start` for the skills folder (the starting Claude
/// Code's own, `None` when the environment names none) and the context lines; it installs or
/// refreshes the agent guide skill there and prints the lines to `out`; nothing else goes to `out`.
pub fn run_with(args: &[String], socket: &Path, session_env: Option<&str>, stdin: &mut dyn Read, out: &mut dyn Write, start: &dyn Fn() -> (Option<PathBuf>, String)) -> i32 {
    let notify = match parse_notify(args) {
        Ok(n) => n,
        Err(usage) => {
            eprintln!("arkos: {usage}");
            return 0;
        }
    };
    let Some(session) = session_env.and_then(|s| s.trim().parse::<u64>().ok()).map(SessionId) else {
        // Started outside Arkos IDE: nothing to tell anyone.
        return 0;
    };
    if notify == Notify::Event(HookEvent::SessionStart) {
        // The skill reaches this Claude Code at its next start; the lines cover this one.
        let (skills, lines) = start();
        match skills {
            Some(dir) => {
                if let Err(e) = crate::guide::ensure_skill(&dir) {
                    eprintln!("arkos notify: the agent guide skill was not installed: {e}");
                }
            }
            None => eprintln!("arkos notify: the agent guide skill was not installed: neither CLAUDE_CONFIG_DIR nor HOME is set"),
        }
        let _ = out.write_all(lines.as_bytes());
        let _ = out.flush();
    }
    let input = match notify {
        Notify::Event(_) => {
            let mut text = String::new();
            let _ = stdin.take(STDIN_CAP).read_to_string(&mut text);
            parse_hook_input(&text)
        }
        Notify::State(_) => None,
    };
    let Some(request) = request_for(&notify, session, input.as_ref()) else {
        if input.is_none() {
            eprintln!("arkos notify: the hook's stdin had no session_id; nothing recorded");
        }
        return 0;
    };
    match send_one(socket, request) {
        Ok(Response::Ok) => {}
        Ok(Response::Error(e)) => eprintln!("arkos notify: arkosd answered: {e}"),
        Ok(other) => eprintln!("arkos notify: unexpected answer {other:?}"),
        // Includes a daemon too old to decode SetAgent: it drops the client without an answer.
        Err(e) => eprintln!("arkos notify: cannot reach arkosd: {e}"),
    }
    0
}

/// Quoted for `sh` when the path needs it.
pub(crate) fn shell_quote(path: &Path) -> String {
    let s = path.to_string_lossy();
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c)) {
        s.into_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The settings block Claude Code needs (spec 5's table, plus the two lifecycle events the agent
/// record comes from). `exe` is this binary's launch path: through Homebrew's link it survives
/// upgrades; a build in a worktree needs `arkos hooks install` again after moving.
pub fn hooks_json(exe: &Path) -> serde_json::Value {
    let cmd = |tail: &str| serde_json::json!({ "type": "command", "command": format!("{} notify {tail}", shell_quote(exe)), "timeout": 5 });
    serde_json::json!({
        "hooks": {
            "SessionStart": [{ "hooks": [cmd("--event session-start")] }],
            "SessionEnd": [{ "hooks": [cmd("--event session-end")] }],
            "UserPromptSubmit": [{ "hooks": [cmd("--state working")] }],
            "PreToolUse": [{ "hooks": [cmd("--state working")] }],
            "Notification": [{ "matcher": "permission_prompt|idle_prompt", "hooks": [cmd("--state waiting")] }],
            "Stop": [{ "hooks": [cmd("--state waiting")] }],
        }
    })
}

/// A matcher group that runs `arkos notify`, whatever path it was installed from.
fn is_ours(group: &serde_json::Value) -> bool {
    group["hooks"].as_array().is_some_and(|hooks| {
        hooks.iter().any(|h| h["command"].as_str().is_some_and(|c| c.contains("arkos notify") || c.contains("arkos' notify")))
    })
}

/// Adds `ours` to `settings`, replacing every group of ours already there (an older path) and
/// leaving everyone else's groups first. Returns (settings, added, replaced).
pub fn merge_hooks(mut settings: serde_json::Value, ours: &serde_json::Value) -> (serde_json::Value, usize, usize) {
    if !settings.is_object() {
        settings = serde_json::json!({});
    }
    let (mut added, mut replaced) = (0, 0);
    let root = settings.as_object_mut().unwrap();
    let hooks = root.entry("hooks").or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        *hooks = serde_json::json!({});
    }
    for (event, groups) in ours["hooks"].as_object().unwrap() {
        let slot = hooks.as_object_mut().unwrap().entry(event.clone()).or_insert_with(|| serde_json::json!([]));
        if !slot.is_array() {
            *slot = serde_json::json!([]);
        }
        let arr = slot.as_array_mut().unwrap();
        let before = arr.len();
        arr.retain(|g| !is_ours(g));
        replaced += before - arr.len();
        for g in groups.as_array().unwrap() {
            arr.push(g.clone());
            added += 1;
        }
    }
    (settings, added, replaced)
}

/// `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json` (Claude Code's user settings).
pub fn claude_settings_path() -> PathBuf {
    match std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("settings.json"),
        None => PathBuf::from(std::env::var("HOME").expect("HOME is set")).join(".claude").join("settings.json"),
    }
}

/// Reads and parses Claude Code's settings file. A missing file is `Ok(None)`.
fn read_settings(path: &Path) -> Result<Option<serde_json::Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| format!("cannot parse {}: {e}; fix it by hand first", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// Reads Claude Code's settings file (a missing one is `{}`), lets `edit` change the value, and
/// writes it back pretty with a trailing newline. When the edit changes nothing the file is not
/// touched; otherwise `settings.json.bak` is written first, from the text already read, when the
/// file existed. A file that does not parse is left untouched (no backup either) and the error
/// returned.
pub fn edit_settings(path: &Path, edit: impl FnOnce(serde_json::Value) -> serde_json::Value) -> Result<(), String> {
    let (parsed, original) = match std::fs::read_to_string(path) {
        Ok(text) => {
            let v = serde_json::from_str(&text).map_err(|e| format!("cannot parse {}: {e}; fix it by hand first", path.display()))?;
            (v, Some(text))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (serde_json::json!({}), None),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let edited = edit(parsed.clone());
    if edited == parsed {
        return Ok(());
    }
    if let Some(text) = &original {
        std::fs::write(path.with_extension("json.bak"), text).map_err(|e| format!("cannot write the backup: {e}"))?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&edited).map_err(|e| e.to_string())? + "\n";
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Sets Claude Code's `wheelScrollAccelerationEnabled` in the settings file at `path`, keeping every
/// other key; a file that already holds the value is left untouched.
pub fn set_wheel_scroll_acceleration_at(path: &Path, enabled: bool) -> Result<(), String> {
    edit_settings(path, |mut v| {
        if !v.is_object() {
            v = serde_json::json!({});
        }
        v["wheelScrollAccelerationEnabled"] = serde_json::json!(enabled);
        v
    })
}

/// Ok(Some(value)) when the key is a bool, Ok(None) when absent or the file is missing, Err("cannot read ..." or "cannot parse ...") otherwise.
pub fn wheel_scroll_acceleration_at(path: &Path) -> Result<Option<bool>, String> {
    Ok(read_settings(path)?.and_then(|v| v.get("wheelScrollAccelerationEnabled").and_then(|b| b.as_bool())))
}

/// Claude Code's `theme` key: Ok(Some(name)) when it is a string, Ok(None) when absent or the
/// file is missing, the `read_settings` error otherwise.
pub fn claude_theme_at(path: &Path) -> Result<Option<String>, String> {
    Ok(read_settings(path)?.and_then(|v| v.get("theme").and_then(|t| t.as_str()).map(str::to_string)))
}

/// Sets Claude Code's theme to `auto` (Auto, match terminal): it asks the terminal for its
/// background and listens for its theme pushes, both of which Arkos answers from its own theme,
/// so Claude Code's colours are readable on every Arkos theme and follow each switch.
pub fn set_claude_theme_auto_at(path: &Path) -> Result<(), String> {
    edit_settings(path, |mut v| {
        if !v.is_object() {
            v = serde_json::json!({});
        }
        v["theme"] = serde_json::json!("auto");
        v
    })
}

/// Whether Arkos sets Claude Code's theme to `auto` without asking: when it is unset or one of the
/// two stock themes, which match one kind of terminal background and are what onboarding
/// leaves. A colourblind-friendly, ANSI-only or custom theme is a deliberate choice; the doctor
/// offers the change for those and leaves the choice to the person.
pub fn claude_theme_is_stock(theme: Option<&str>) -> bool {
    matches!(theme, None | Some("dark") | Some("light"))
}

/// Whether every event in `hooks_json(exe)` has, in the file, a group whose commands equal ours for
/// that event (so hooks installed from an old or moved binary count as missing, which is what an
/// install would replace). A missing file is `Ok(false)`; one that cannot be read or parsed is the
/// `read_settings` error, since an install could not succeed there either.
pub fn hooks_installed_at(path: &Path, exe: &Path) -> Result<bool, String> {
    let Some(settings) = read_settings(path)? else { return Ok(false) };
    let ours = hooks_json(exe);
    Ok(ours["hooks"].as_object().is_some_and(|events| {
        events.iter().all(|(event, our_groups)| {
            let wanted: Vec<&serde_json::Value> = our_groups.as_array().into_iter().flatten().map(|g| &g["hooks"]).collect();
            settings["hooks"][event].as_array().is_some_and(|groups| wanted.iter().all(|w| groups.iter().any(|g| g.get("hooks") == Some(*w))))
        })
    }))
}

/// Merges the hook entries into Claude Code's settings file, writing `<file>.bak` first when
/// the file exists. A file that does not parse is left untouched, with the error returned.
pub fn install_hooks_at(path: &Path, exe: &Path) -> Result<(PathBuf, usize, usize), String> {
    let mut counts = (0, 0);
    edit_settings(path, |existing| {
        let (merged, added, replaced) = merge_hooks(existing, &hooks_json(exe));
        counts = (added, replaced);
        merged
    })?;
    Ok((path.to_path_buf(), counts.0, counts.1))
}

pub fn install_hooks(exe: &Path) -> Result<(PathBuf, usize, usize), String> {
    install_hooks_at(&claude_settings_path(), exe)
}

fn this_exe() -> PathBuf {
    crate::update::launch_path().unwrap_or_else(|| PathBuf::from("arkos"))
}

/// `arkos notify ...` and `arkos hooks print|install`. The exit code for the process.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("notify") => run_with(
            args,
            &crate::paths::socket_path(),
            std::env::var("ARKOS_SESSION_ID").ok().as_deref(),
            &mut std::io::stdin(),
            &mut std::io::stdout(),
            &|| {
                let on_path = std::env::var("PATH").is_ok_and(|p| resolves_on_path(&p, "arkos"));
                (crate::guide::find_skills_dir(), context_lines(on_path, &this_exe()))
            },
        ),
        Some("hooks") => match args.get(1).map(String::as_str) {
            Some("print") => {
                println!("{}", serde_json::to_string_pretty(&hooks_json(&this_exe())).unwrap());
                0
            }
            Some("install") => match install_hooks(&this_exe()) {
                Ok((path, added, replaced)) => {
                    println!("installed {added} hook entries in {} (replaced {replaced}); restart claude for them to take effect", path.display());
                    0
                }
                Err(e) => {
                    eprintln!("arkos hooks: {e}");
                    1
                }
            },
            _ => {
                eprintln!("usage: arkos hooks print | install");
                2
            }
        },
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn notify_arguments_name_a_state_or_an_event() {
        assert_eq!(parse_notify(&args("notify --state working")), Ok(Notify::State(ClaudeState::Working)));
        assert_eq!(parse_notify(&args("notify --state waiting")), Ok(Notify::State(ClaudeState::Waiting)));
        assert_eq!(parse_notify(&args("notify --state idle")), Ok(Notify::State(ClaudeState::Idle)));
        assert_eq!(parse_notify(&args("notify --event session-start")), Ok(Notify::Event(HookEvent::SessionStart)));
        assert_eq!(parse_notify(&args("notify --event session-end")), Ok(Notify::Event(HookEvent::SessionEnd)));
        assert!(parse_notify(&args("notify --state busy")).is_err());
        assert!(parse_notify(&args("notify")).is_err());
        assert!(parse_notify(&args("notify --event")).is_err());
    }

    #[test]
    fn hook_stdin_gives_the_agents_session_id() {
        let json = r#"{"session_id":"abc123","transcript_path":"/t.jsonl","cwd":"/p","hook_event_name":"SessionStart","source":"resume"}"#;
        assert_eq!(parse_hook_input(json), Some(HookInput { session_id: "abc123".into(), reason: None }));
        let end = r#"{"session_id":"abc123","hook_event_name":"SessionEnd","reason":"other"}"#;
        assert_eq!(parse_hook_input(end), Some(HookInput { session_id: "abc123".into(), reason: Some("other".into()) }));
        assert_eq!(parse_hook_input("not json"), None);
        assert_eq!(parse_hook_input(r#"{"cwd":"/p"}"#), None);
        assert_eq!(parse_hook_input(r#"{"session_id":""}"#), None);
    }

    #[test]
    fn the_resume_command_is_claude_resume_with_the_id() {
        assert_eq!(resume_command("claude", "abc"), vec!["claude", "--resume", "abc"]);
        let input = |reason: Option<&str>| HookInput { session_id: "abc".into(), reason: reason.map(str::to_string) };
        let r = request_for(&Notify::Event(HookEvent::SessionStart), SessionId(7), Some(&input(None)));
        assert_eq!(r, Some(Request::SetAgent { session: SessionId(7), agent: AgentRecord { kind: "claude".into(), id: "abc".into(), resume: resume_command("claude", "abc") } }));
        let clear = Some(Request::ClearAgent { session: SessionId(7), id: "abc".into() });
        for reason in [None, Some("clear"), Some("resume"), Some("logout"), Some("prompt_input_exit")] {
            assert_eq!(request_for(&Notify::Event(HookEvent::SessionEnd), SessionId(7), Some(&input(reason))), clear, "{reason:?}");
        }
        assert_eq!(request_for(&Notify::Event(HookEvent::SessionEnd), SessionId(7), Some(&input(Some("other")))), None, "killed, as at a shutdown: kept so it is resumed");
        assert_eq!(request_for(&Notify::Event(HookEvent::SessionStart), SessionId(7), None), None, "no stdin, nothing to record");
        assert_eq!(request_for(&Notify::State(ClaudeState::Waiting), SessionId(7), None), Some(Request::SetState { session: SessionId(7), state: ClaudeState::Waiting }));
    }

    /// A daemon stand-in: accepts one client, records its first request, answers Ok.
    fn fake_daemon(dir: &Path) -> (PathBuf, mpsc::Receiver<Request>) {
        let socket = dir.join("d.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut w = BufWriter::new(stream);
            if let Ok(Some(msg)) = read_frame::<_, ClientMessage>(&mut r) {
                tx.send(msg.request).unwrap();
                write_frame(&mut w, &ServerMessage::Response { id: msg.id, response: Response::Ok }).unwrap();
                w.flush().unwrap();
            }
        });
        (socket, rx)
    }

    /// A session start's inputs for a test: the skills folder given, and the on-PATH lines.
    fn start_in(skills: &Path) -> impl Fn() -> (Option<PathBuf>, String) + '_ {
        move || (Some(skills.to_path_buf()), CONTEXT_LINES.to_string())
    }

    #[test]
    fn notify_sends_one_request_for_the_session_in_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        let skills = dir.path().join("skills");
        let mut out = Vec::new();
        let unresolved = || -> (Option<PathBuf>, String) { panic!("a state notify resolves no skills folder") };
        let code = run_with(&args("notify --state waiting"), &socket, Some("7"), &mut std::io::empty(), &mut out, &unresolved);
        assert_eq!(code, 0);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Request::SetState { session: SessionId(7), state: ClaudeState::Waiting });
        assert!(out.is_empty(), "a state hook prints nothing");
        assert!(!skills.exists(), "only a session start installs the skill");
    }

    #[test]
    fn a_session_start_hook_records_the_agent_from_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        let skills = dir.path().join("skills");
        let start = r#"{"session_id":"abc","cwd":"/p","hook_event_name":"SessionStart","source":"startup"}"#;
        let mut out = Vec::new();
        assert_eq!(run_with(&args("notify --event session-start"), &socket, Some("7"), &mut start.as_bytes(), &mut out, &start_in(&skills)), 0);
        let agent = AgentRecord { kind: "claude".into(), id: "abc".into(), resume: vec!["claude".into(), "--resume".into(), "abc".into()] };
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Request::SetAgent { session: SessionId(7), agent });
        assert_eq!(String::from_utf8(out).unwrap(), CONTEXT_LINES);
        let skill = skills.join("arkos-ide").join("SKILL.md");
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), crate::guide::skill_text(), "the starting Claude Code's folder gets the skill");
        let mtime = std::fs::metadata(&skill).unwrap().modified().unwrap();
        // A second start finds the skill current and leaves the file alone.
        let again = tempfile::tempdir().unwrap();
        let (socket, _rx) = fake_daemon(again.path());
        let mut out = Vec::new();
        assert_eq!(run_with(&args("notify --event session-start"), &socket, Some("7"), &mut start.as_bytes(), &mut out, &start_in(&skills)), 0);
        assert_eq!(String::from_utf8(out).unwrap(), CONTEXT_LINES);
        assert_eq!(std::fs::metadata(&skill).unwrap().modified().unwrap(), mtime);
        // A skill someone else wrote under that name stays as it was, and the hook still succeeds.
        std::fs::write(&skill, "---\nname: theirs\n---\nhi\n").unwrap();
        let third = tempfile::tempdir().unwrap();
        let (socket, _rx) = fake_daemon(third.path());
        let mut out = Vec::new();
        assert_eq!(run_with(&args("notify --event session-start"), &socket, Some("7"), &mut start.as_bytes(), &mut out, &start_in(&skills)), 0);
        assert_eq!(String::from_utf8(out).unwrap(), CONTEXT_LINES);
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), "---\nname: theirs\n---\nhi\n");
    }

    #[test]
    fn a_session_end_hook_clears_the_agent_unless_the_process_was_killed() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        let skills = dir.path().join("skills");
        let mut out = Vec::new();
        let mut stdin = r#"{"session_id":"abc","hook_event_name":"SessionEnd","reason":"other"}"#.as_bytes();
        assert_eq!(run_with(&args("notify --event session-end"), &socket, Some("7"), &mut stdin, &mut out, &start_in(&skills)), 0);
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "nothing reached the daemon");
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        let mut stdin = r#"{"session_id":"abc","hook_event_name":"SessionEnd","reason":"clear"}"#.as_bytes();
        assert_eq!(run_with(&args("notify --event session-end"), &socket, Some("7"), &mut stdin, &mut out, &start_in(&skills)), 0);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Request::ClearAgent { session: SessionId(7), id: "abc".into() });
        assert!(out.is_empty(), "a session end prints nothing");
        assert!(!skills.exists());
    }

    #[test]
    fn without_home_a_state_notify_still_reports_and_a_session_start_skips_only_the_skill() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        // The environment as the hook would see it with neither CLAUDE_CONFIG_DIR nor HOME set.
        let homeless = || (crate::guide::skills_dir_from(None, None), CONTEXT_LINES.to_string());
        let mut out = Vec::new();
        assert_eq!(run_with(&args("notify --state working"), &socket, Some("7"), &mut std::io::empty(), &mut out, &homeless), 0);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Request::SetState { session: SessionId(7), state: ClaudeState::Working });
        assert!(out.is_empty());
        let again = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(again.path());
        let start = r#"{"session_id":"abc","hook_event_name":"SessionStart"}"#;
        assert_eq!(run_with(&args("notify --event session-start"), &socket, Some("7"), &mut start.as_bytes(), &mut out, &homeless), 0);
        assert!(matches!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Request::SetAgent { .. }), "the agent is still recorded");
        assert_eq!(String::from_utf8(out).unwrap(), CONTEXT_LINES, "and the lines still print");
    }

    #[test]
    fn the_context_lines_name_the_full_path_when_arkos_is_not_on_the_path() {
        assert_eq!(context_lines(true, Path::new("/anywhere/arkos")), CONTEXT_LINES);
        assert_eq!(
            context_lines(false, Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkos")),
            "This session runs inside Arkos IDE.\nTo change its theme, fonts or settings, run `'/Applications/Arkos IDE.app/Contents/MacOS/arkos' guide` first and follow it.\n"
        );
        assert!(context_lines(false, Path::new("/opt/arkos/bin/arkos")).contains("run `/opt/arkos/bin/arkos guide` first"));
    }

    #[test]
    fn arkos_resolves_only_through_an_executable_file_on_the_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (bin, plain, folder) = (dir.path().join("bin"), dir.path().join("plain"), dir.path().join("folder"));
        for d in [&bin, &plain, &folder] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(bin.join("arkos"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(bin.join("arkos"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(plain.join("arkos"), "not executable").unwrap();
        std::fs::set_permissions(plain.join("arkos"), std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::create_dir_all(folder.join("arkos")).unwrap();
        let joined = |dirs: &[&Path]| dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(":");
        assert!(resolves_on_path(&joined(&[&plain, &folder, &bin]), "arkos"), "the last entry has it");
        assert!(!resolves_on_path(&joined(&[&plain, &folder]), "arkos"), "a plain file and a folder do not count");
        assert!(!resolves_on_path(&joined(&[&bin]), "arkosd"));
        assert!(!resolves_on_path("", "arkos"));
        assert!(!resolves_on_path(&format!("::{}", dir.path().join("missing").display()), "arkos"));
    }

    #[test]
    fn notify_without_a_session_id_exits_zero_and_sends_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        let skills = dir.path().join("skills");
        let mut out = Vec::new();
        assert_eq!(run_with(&args("notify --state working"), &socket, None, &mut std::io::empty(), &mut out, &start_in(&skills)), 0);
        assert_eq!(run_with(&args("notify --state working"), &socket, Some("not a number"), &mut std::io::empty(), &mut out, &start_in(&skills)), 0);
        let mut stdin = r#"{"session_id":"abc","hook_event_name":"SessionStart"}"#.as_bytes();
        assert_eq!(run_with(&args("notify --event session-start"), &socket, None, &mut stdin, &mut out, &start_in(&skills)), 0);
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "nothing reached the daemon");
        assert!(out.is_empty(), "outside Arkos a session start prints nothing");
        assert!(!skills.exists(), "and installs nothing");
    }

    #[test]
    fn notify_without_a_daemon_or_with_bad_arguments_exits_zero_quickly() {
        let dir = tempfile::tempdir().unwrap();
        let skills = dir.path().join("skills");
        let started = std::time::Instant::now();
        assert_eq!(run_with(&args("notify --state working"), &dir.path().join("missing.sock"), Some("7"), &mut std::io::empty(), &mut Vec::new(), &start_in(&skills)), 0);
        assert_eq!(run_with(&args("notify --state nonsense"), &dir.path().join("missing.sock"), Some("7"), &mut std::io::empty(), &mut Vec::new(), &start_in(&skills)), 0);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_hook_entries_cover_the_six_events_with_the_notification_matcher() {
        let v = hooks_json(Path::new("/opt/homebrew/bin/arkos"));
        let hooks = v["hooks"].as_object().unwrap();
        let mut events: Vec<&String> = hooks.keys().collect();
        events.sort();
        assert_eq!(events, ["Notification", "PreToolUse", "SessionEnd", "SessionStart", "Stop", "UserPromptSubmit"]);
        assert_eq!(hooks["Notification"][0]["matcher"], "permission_prompt|idle_prompt");
        assert_eq!(hooks["Notification"][0]["hooks"][0]["command"], "/opt/homebrew/bin/arkos notify --state waiting");
        assert_eq!(hooks["Stop"][0]["hooks"][0]["command"], "/opt/homebrew/bin/arkos notify --state waiting");
        assert_eq!(hooks["UserPromptSubmit"][0]["hooks"][0]["command"], "/opt/homebrew/bin/arkos notify --state working");
        assert_eq!(hooks["PreToolUse"][0]["hooks"][0]["command"], "/opt/homebrew/bin/arkos notify --state working");
        assert!(hooks["PreToolUse"][0].get("matcher").is_none(), "every tool");
        assert_eq!(hooks["SessionStart"][0]["hooks"][0]["command"], "/opt/homebrew/bin/arkos notify --event session-start");
        assert_eq!(hooks["SessionEnd"][0]["hooks"][0]["command"], "/opt/homebrew/bin/arkos notify --event session-end");
        assert_eq!(hooks["Stop"][0]["hooks"][0]["timeout"], 5);
        let spaced = hooks_json(Path::new("/Users/me/My Apps/arkos"));
        assert_eq!(spaced["hooks"]["Stop"][0]["hooks"][0]["command"], "'/Users/me/My Apps/arkos' notify --state waiting");
    }

    #[test]
    fn merging_is_idempotent_replaces_an_old_path_and_keeps_other_hooks() {
        let ours = hooks_json(Path::new("/new/arkos"));
        let existing: serde_json::Value = serde_json::from_str(
            r#"{"model":"opus","hooks":{"Stop":[
                {"hooks":[{"type":"command","command":"/old/arkos notify --state waiting"}]},
                {"hooks":[{"type":"command","command":"say done"}]}
            ]}}"#,
        )
        .unwrap();
        let (merged, added, replaced) = merge_hooks(existing, &ours);
        assert_eq!((added, replaced), (6, 1));
        assert_eq!(merged["model"], "opus");
        let stop = merged["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "say done", "the other hook stays, first");
        assert_eq!(stop[1]["hooks"][0]["command"], "/new/arkos notify --state waiting");
        let (again, added, replaced) = merge_hooks(merged.clone(), &ours);
        assert_eq!((added, replaced), (6, 6), "a second install replaces its own entries");
        assert_eq!(again, merged);
    }

    #[test]
    fn install_writes_a_backup_and_the_merged_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"model":"opus"}"#).unwrap();
        let (p, added, replaced) = install_hooks_at(&path, Path::new("/x/arkos")).unwrap();
        assert_eq!((p, added, replaced), (path.clone(), 6, 0));
        assert_eq!(std::fs::read_to_string(dir.path().join("settings.json.bak")).unwrap(), r#"{"model":"opus"}"#);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "/x/arkos notify --state waiting");
        // A missing file is created; a broken one is left alone.
        let fresh = dir.path().join("sub").join("settings.json");
        assert_eq!(install_hooks_at(&fresh, Path::new("/x/arkos")).unwrap().1, 6);
        std::fs::write(&path, "{ not json").unwrap();
        assert!(install_hooks_at(&path, Path::new("/x/arkos")).unwrap_err().contains("cannot parse"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[test]
    fn the_scroll_key_is_set_and_everything_else_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(wheel_scroll_acceleration_at(&path), Ok(None), "no file");
        set_wheel_scroll_acceleration_at(&path, false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\n  \"wheelScrollAccelerationEnabled\": false\n}\n");
        assert!(!path.with_extension("json.bak").exists(), "nothing to back up the first time");
        std::fs::write(&path, r#"{"theme":"dark","hooks":{"Stop":[{"hooks":[]}]},"wheelScrollAccelerationEnabled":true}"#).unwrap();
        assert_eq!(wheel_scroll_acceleration_at(&path), Ok(Some(true)));
        set_wheel_scroll_acceleration_at(&path, false).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["wheelScrollAccelerationEnabled"], false);
        assert_eq!(v["theme"], "dark");
        assert!(v["hooks"]["Stop"].is_array(), "other keys and nesting kept");
        assert!(path.with_extension("json.bak").exists());
        assert_eq!(wheel_scroll_acceleration_at(&path), Ok(Some(false)));
    }

    #[test]
    fn the_claude_theme_is_set_to_auto_and_everything_else_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(claude_theme_at(&path), Ok(None), "no file");
        std::fs::write(&path, r#"{"theme":"dark","wheelScrollAccelerationEnabled":false}"#).unwrap();
        assert_eq!(claude_theme_at(&path), Ok(Some("dark".into())));
        set_claude_theme_auto_at(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!((v["theme"].as_str(), v["wheelScrollAccelerationEnabled"].as_bool()), (Some("auto"), Some(false)));
        assert!(path.with_extension("json.bak").exists());
        assert!(claude_theme_is_stock(None) && claude_theme_is_stock(Some("dark")) && claude_theme_is_stock(Some("light")));
        assert!(!claude_theme_is_stock(Some("auto")) && !claude_theme_is_stock(Some("dark-daltonized")) && !claude_theme_is_stock(Some("light-ansi")));
    }

    #[test]
    fn an_edit_that_changes_nothing_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let bak = path.with_extension("json.bak");
        let original = r#"{"theme":"dark"}"#;
        std::fs::write(&path, original).unwrap();
        set_wheel_scroll_acceleration_at(&path, false).unwrap();
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), original);
        let after_first = std::fs::read_to_string(&path).unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        set_wheel_scroll_acceleration_at(&path, false).unwrap();
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), original, "the backup still holds the pre-edit text");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), after_first);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    }

    #[test]
    fn an_unparseable_settings_file_is_reported_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "\u{feff}{ \"a\": 1 } // comment").unwrap();
        let before = std::fs::read(&path).unwrap();
        let err = set_wheel_scroll_acceleration_at(&path, false).unwrap_err();
        assert!(err.starts_with(&format!("cannot parse {}", path.display())) && err.ends_with("; fix it by hand first"), "{err}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(!path.with_extension("json.bak").exists(), "no backup of a file we did not change");
        assert!(wheel_scroll_acceleration_at(&path).is_err());
        assert!(hooks_installed_at(&path, Path::new("/usr/local/bin/arkos")).is_err());
    }

    #[test]
    fn hooks_count_as_installed_only_when_every_event_has_ours_from_this_binary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let exe = Path::new("/Applications/Arkos IDE.app/Contents/MacOS/arkos");
        assert_eq!(hooks_installed_at(&path, exe), Ok(false), "no file");
        install_hooks_at(&path, Path::new("/old/place/arkos")).unwrap();
        assert_eq!(hooks_installed_at(&path, exe), Ok(false), "hooks from another binary are what an install would replace");
        install_hooks_at(&path, exe).unwrap();
        assert_eq!(hooks_installed_at(&path, exe), Ok(true));
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        v["hooks"].as_object_mut().unwrap().remove("Stop");
        std::fs::write(&path, v.to_string()).unwrap();
        assert_eq!(hooks_installed_at(&path, exe), Ok(false), "one event's group removed by hand");
    }
}
