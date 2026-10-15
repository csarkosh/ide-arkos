//! A client of the shoot's own arkosd: the window's protocol over its socket, one request at a
//! time. It says Hello without a UI pid, so the daemon never takes it for a window (nothing to
//! relaunch, nothing for the watchdog), and ignores the events the daemon pushes.
use anyhow::{bail, Context, Result};
use protocol::codec::{read_frame, write_frame};
use protocol::{ClaudeState, ClientMessage, Request, Response, ServerMessage, SessionId, PROTOCOL_VERSION};
use std::collections::BTreeMap;
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);

pub struct Daemon {
    reader: BufReader<UnixStream>,
    writer: BufWriter<UnixStream>,
    next_id: u32,
}

impl Daemon {
    pub fn connect(socket: &Path) -> Result<Daemon> {
        let stream = UnixStream::connect(socket).with_context(|| format!("connect to {}", socket.display()))?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        Ok(Daemon { reader: BufReader::new(stream.try_clone()?), writer: BufWriter::new(stream), next_id: 1 })
    }

    /// Sends one request and returns its answer; events that arrive meanwhile are skipped.
    pub fn call(&mut self, request: Request) -> Result<Response> {
        let id = self.next_id;
        self.next_id += 1;
        write_frame(&mut self.writer, &ClientMessage { id, request })?;
        self.writer.flush()?;
        loop {
            match read_frame::<_, ServerMessage>(&mut self.reader)? {
                Some(ServerMessage::Response { id: got, response }) if got == id => {
                    return match response {
                        Response::Error(e) => bail!("the daemon answered: {e}"),
                        r => Ok(r),
                    }
                }
                Some(_) => continue,
                None => bail!("the daemon hung up"),
            }
        }
    }

    /// Not a window: pid 0 is what the daemon reads as "no UI", and an empty folder keeps it
    /// from noting one.
    pub fn hello(&mut self) -> Result<String> {
        match self.call(Request::Hello { protocol_version: PROTOCOL_VERSION, ui_pid: 0, ui_exe: None, ui_cwd: PathBuf::new() })? {
            Response::Hello { daemon_version, .. } => Ok(daemon_version),
            other => bail!("unexpected answer to Hello: {other:?}"),
        }
    }

    /// A shell in `cwd`, the daemon's default shell, at a size the window corrects on attach.
    pub fn create(&mut self, cwd: &Path) -> Result<SessionId> {
        match self.call(Request::CreateSession { cwd: cwd.to_path_buf(), shell: None, env: vec![], nice: 0, rows: 40, cols: 120 })? {
            Response::SessionCreated { id } => Ok(id),
            other => bail!("unexpected answer to CreateSession: {other:?}"),
        }
    }

    pub fn input(&mut self, session: SessionId, text: &str) -> Result<()> {
        self.call(Request::Input { session, bytes: text.as_bytes().to_vec() })?;
        Ok(())
    }

    pub fn states(&mut self) -> Result<BTreeMap<u64, ClaudeState>> {
        match self.call(Request::ListSessions)? {
            Response::Sessions(list) => Ok(list.into_iter().map(|s| (s.id.0, s.state)).collect()),
            other => bail!("unexpected answer to ListSessions: {other:?}"),
        }
    }

    pub fn kill(&mut self, session: SessionId) -> Result<()> {
        self.call(Request::Kill { session })?;
        Ok(())
    }
}

/// The story's word for a state.
pub fn state_name(s: ClaudeState) -> &'static str {
    match s {
        ClaudeState::Idle => "idle",
        ClaudeState::Working => "working",
        ClaudeState::Waiting => "waiting",
    }
}
