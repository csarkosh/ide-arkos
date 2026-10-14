//! The hook side of session awareness (spec 5 and 2.3) and of recovery (update-in-place spec,
//! section 11): `sarkos notify`, run by Claude Code's hooks with `SARKOS_SESSION_ID` in its
//! environment, sends the daemon one request and exits 0 whatever happens; `sarkos hooks` prints
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
}

/// `notify --state <working|waiting|idle>` or `notify --event <session-start|session-end>`.
pub fn parse_notify(args: &[String]) -> Result<Notify, String> {
    let usage = "usage: sarkos notify --state <working|waiting|idle> | --event <session-start|session-end>";
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
/// `cwd` and `hook_event_name` on every event (its hooks reference); only the id is kept.
pub fn parse_hook_input(stdin: &str) -> Option<HookInput> {
    let v: serde_json::Value = serde_json::from_str(stdin).ok()?;
    let id = v.get("session_id")?.as_str()?.trim();
    (!id.is_empty()).then(|| HookInput { session_id: id.to_string() })
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
        Notify::Event(HookEvent::SessionEnd) => Some(Request::ClearAgent { session, id: input?.session_id.clone() }),
    }
}

/// One request on a fresh connection, with no Hello: the daemon must not take a hook for a
/// window. Events the daemon pushes meanwhile are skipped until the answer arrives.
pub fn send_one(socket: &Path, request: Request) -> std::io::Result<Response> {
    let stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
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

/// `sarkos notify ...` with its inputs explicit, for tests. Always 0 for `notify`: a hook
/// that fails would show up in Claude Code as a hook error, for something that is only a tab
/// icon. Every reason is on stderr, which Claude Code only shows for a non-zero exit.
pub fn run_with(args: &[String], socket: &Path, session_env: Option<&str>, stdin: &mut dyn Read) -> i32 {
    let notify = match parse_notify(args) {
        Ok(n) => n,
        Err(usage) => {
            eprintln!("sarkos: {usage}");
            return 0;
        }
    };
    let Some(session) = session_env.and_then(|s| s.trim().parse::<u64>().ok()).map(SessionId) else {
        // Started outside Sarkos IDE: nothing to tell anyone.
        return 0;
    };
    let input = match notify {
        Notify::Event(_) => {
            let mut text = String::new();
            let _ = stdin.take(STDIN_CAP).read_to_string(&mut text);
            parse_hook_input(&text)
        }
        Notify::State(_) => None,
    };
    let Some(request) = request_for(&notify, session, input.as_ref()) else {
        eprintln!("sarkos notify: the hook's stdin had no session_id; nothing recorded");
        return 0;
    };
    match send_one(socket, request) {
        Ok(Response::Ok) => {}
        Ok(Response::Error(e)) => eprintln!("sarkos notify: sarkosd answered: {e}"),
        Ok(other) => eprintln!("sarkos notify: unexpected answer {other:?}"),
        // Includes a daemon too old to decode SetAgent: it drops the client without an answer.
        Err(e) => eprintln!("sarkos notify: cannot reach sarkosd: {e}"),
    }
    0
}

/// Quoted for `sh` when the path needs it.
fn shell_quote(path: &Path) -> String {
    let s = path.to_string_lossy();
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c)) {
        s.into_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The settings block Claude Code needs (spec 5's table, plus the two lifecycle events the agent
/// record comes from). `exe` is this binary's launch path: through Homebrew's link it survives
/// upgrades; a build in a worktree needs `sarkos hooks install` again after moving.
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

/// A matcher group that runs `sarkos notify`, whatever path it was installed from.
fn is_ours(group: &serde_json::Value) -> bool {
    group["hooks"].as_array().is_some_and(|hooks| {
        hooks.iter().any(|h| h["command"].as_str().is_some_and(|c| c.contains("sarkos notify") || c.contains("sarkos' notify")))
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

/// Merges the hook entries into Claude Code's settings file, writing `<file>.bak` first when
/// the file exists. A file that does not parse is left untouched, with the error returned.
pub fn install_hooks_at(path: &Path, exe: &Path) -> Result<(PathBuf, usize, usize), String> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => {
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("cannot parse {}: {e}; fix it by hand first", path.display()))?;
            std::fs::write(path.with_extension("json.bak"), &text).map_err(|e| format!("cannot write the backup: {e}"))?;
            v
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let (merged, added, replaced) = merge_hooks(existing, &hooks_json(exe));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())? + "\n";
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok((path.to_path_buf(), added, replaced))
}

pub fn install_hooks(exe: &Path) -> Result<(PathBuf, usize, usize), String> {
    install_hooks_at(&claude_settings_path(), exe)
}

fn this_exe() -> PathBuf {
    crate::update::launch_path().unwrap_or_else(|| PathBuf::from("sarkos"))
}

/// `sarkos notify ...` and `sarkos hooks print|install`. The exit code for the process.
pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("notify") => run_with(args, &crate::paths::socket_path(), std::env::var("SARKOS_SESSION_ID").ok().as_deref(), &mut std::io::stdin()),
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
                    eprintln!("sarkos hooks: {e}");
                    1
                }
            },
            _ => {
                eprintln!("usage: sarkos hooks print | install");
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
        assert_eq!(parse_hook_input(json), Some(HookInput { session_id: "abc123".into() }));
        assert_eq!(parse_hook_input("not json"), None);
        assert_eq!(parse_hook_input(r#"{"cwd":"/p"}"#), None);
        assert_eq!(parse_hook_input(r#"{"session_id":""}"#), None);
    }

    #[test]
    fn the_resume_command_is_claude_resume_with_the_id() {
        assert_eq!(resume_command("claude", "abc"), vec!["claude", "--resume", "abc"]);
        let r = request_for(&Notify::Event(HookEvent::SessionStart), SessionId(7), Some(&HookInput { session_id: "abc".into() }));
        assert_eq!(r, Some(Request::SetAgent { session: SessionId(7), agent: AgentRecord { kind: "claude".into(), id: "abc".into(), resume: resume_command("claude", "abc") } }));
        assert_eq!(request_for(&Notify::Event(HookEvent::SessionEnd), SessionId(7), Some(&HookInput { session_id: "abc".into() })), Some(Request::ClearAgent { session: SessionId(7), id: "abc".into() }));
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

    #[test]
    fn notify_sends_one_request_for_the_session_in_the_environment() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        let code = run_with(&args("notify --state waiting"), &socket, Some("7"), &mut std::io::empty());
        assert_eq!(code, 0);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Request::SetState { session: SessionId(7), state: ClaudeState::Waiting });
    }

    #[test]
    fn a_session_start_hook_records_the_agent_from_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        let mut stdin = r#"{"session_id":"abc","cwd":"/p","hook_event_name":"SessionStart","source":"startup"}"#.as_bytes();
        assert_eq!(run_with(&args("notify --event session-start"), &socket, Some("7"), &mut stdin), 0);
        let agent = AgentRecord { kind: "claude".into(), id: "abc".into(), resume: vec!["claude".into(), "--resume".into(), "abc".into()] };
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), Request::SetAgent { session: SessionId(7), agent });
    }

    #[test]
    fn notify_without_a_session_id_exits_zero_and_sends_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (socket, rx) = fake_daemon(dir.path());
        assert_eq!(run_with(&args("notify --state working"), &socket, None, &mut std::io::empty()), 0);
        assert_eq!(run_with(&args("notify --state working"), &socket, Some("not a number"), &mut std::io::empty()), 0);
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err(), "nothing reached the daemon");
    }

    #[test]
    fn notify_without_a_daemon_or_with_bad_arguments_exits_zero_quickly() {
        let dir = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        assert_eq!(run_with(&args("notify --state working"), &dir.path().join("missing.sock"), Some("7"), &mut std::io::empty()), 0);
        assert_eq!(run_with(&args("notify --state nonsense"), &dir.path().join("missing.sock"), Some("7"), &mut std::io::empty()), 0);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_hook_entries_cover_the_six_events_with_the_notification_matcher() {
        let v = hooks_json(Path::new("/opt/homebrew/bin/sarkos"));
        let hooks = v["hooks"].as_object().unwrap();
        let mut events: Vec<&String> = hooks.keys().collect();
        events.sort();
        assert_eq!(events, ["Notification", "PreToolUse", "SessionEnd", "SessionStart", "Stop", "UserPromptSubmit"]);
        assert_eq!(hooks["Notification"][0]["matcher"], "permission_prompt|idle_prompt");
        assert_eq!(hooks["Notification"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --state waiting");
        assert_eq!(hooks["Stop"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --state waiting");
        assert_eq!(hooks["UserPromptSubmit"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --state working");
        assert_eq!(hooks["PreToolUse"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --state working");
        assert!(hooks["PreToolUse"][0].get("matcher").is_none(), "every tool");
        assert_eq!(hooks["SessionStart"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --event session-start");
        assert_eq!(hooks["SessionEnd"][0]["hooks"][0]["command"], "/opt/homebrew/bin/sarkos notify --event session-end");
        assert_eq!(hooks["Stop"][0]["hooks"][0]["timeout"], 5);
        let spaced = hooks_json(Path::new("/Users/me/My Apps/sarkos"));
        assert_eq!(spaced["hooks"]["Stop"][0]["hooks"][0]["command"], "'/Users/me/My Apps/sarkos' notify --state waiting");
    }

    #[test]
    fn merging_is_idempotent_replaces_an_old_path_and_keeps_other_hooks() {
        let ours = hooks_json(Path::new("/new/sarkos"));
        let existing: serde_json::Value = serde_json::from_str(
            r#"{"model":"opus","hooks":{"Stop":[
                {"hooks":[{"type":"command","command":"/old/sarkos notify --state waiting"}]},
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
        assert_eq!(stop[1]["hooks"][0]["command"], "/new/sarkos notify --state waiting");
        let (again, added, replaced) = merge_hooks(merged.clone(), &ours);
        assert_eq!((added, replaced), (6, 6), "a second install replaces its own entries");
        assert_eq!(again, merged);
    }

    #[test]
    fn install_writes_a_backup_and_the_merged_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"model":"opus"}"#).unwrap();
        let (p, added, replaced) = install_hooks_at(&path, Path::new("/x/sarkos")).unwrap();
        assert_eq!((p, added, replaced), (path.clone(), 6, 0));
        assert_eq!(std::fs::read_to_string(dir.path().join("settings.json.bak")).unwrap(), r#"{"model":"opus"}"#);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "/x/sarkos notify --state waiting");
        // A missing file is created; a broken one is left alone.
        let fresh = dir.path().join("sub").join("settings.json");
        assert_eq!(install_hooks_at(&fresh, Path::new("/x/sarkos")).unwrap().1, 6);
        std::fs::write(&path, "{ not json").unwrap();
        assert!(install_hooks_at(&path, Path::new("/x/sarkos")).unwrap_err().contains("cannot parse"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }
}
