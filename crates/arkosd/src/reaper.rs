//! Notices shells that exited while something still holds their PTY open, ends closed tabs'
//! sessions nobody reopened within their grace, and forgets dead sessions once nobody is
//! attached and the TTL has passed (spec 2.1: the daemon keeps sessions for reattach, not
//! forever).
use crate::server::Shared;
use std::time::Duration;

pub fn start(state: Shared, every: Duration, ttl: Duration) {
    std::thread::Builder::new()
        .name("reaper".into())
        .spawn(move || loop {
            std::thread::sleep(every);
            scan(&state, ttl);
        })
        .expect("spawn reaper");
}

fn scan(state: &Shared, ttl: Duration) {
    // Clone the handles out so no session lock is taken under the global lock.
    let sessions: Vec<_> = {
        let st = state.lock().unwrap();
        // During an upgrade a reap after the snapshot would lose the exit code to the new image,
        // which finds the pid already gone; it reaps on the next pass after the resume instead.
        if st.upgrading {
            return;
        }
        st.sessions.values().cloned().collect()
    };
    for s in &sessions {
        s.poll_exit();
        s.escalate_kill();
    }
    // A closed tab nobody reopened (milestone 6a spec, section 14): ended the way `Kill` ends
    // a session, so the escalation to SIGKILL follows a second later from `escalate_kill`.
    for s in &sessions {
        // Taken once: a shell that ended by itself has nothing to end.
        let Some(secs) = s.take_due_grace() else { continue };
        if s.info().alive {
            crate::journal::note(format_args!("session {} in {}: not reopened within {secs} s, ended", s.id.0, s.cwd().display()));
            if let Err(e) = s.kill() {
                eprintln!("arkosd: session {}: cannot end it after its grace: {e:#}", s.id.0);
            }
        }
    }
    let mut dead = Vec::new();
    for s in &sessions {
        if let Some(at) = s.exited_at() {
            if at.elapsed() >= ttl && !s.has_subscribers() {
                dead.push(s.id);
            }
        }
    }
    if dead.is_empty() {
        return;
    }
    let mut st = state.lock().unwrap();
    for id in dead {
        // Re-check under the lock: a client may have attached meanwhile.
        let still_dead = st.sessions.get(&id).map(|s| !s.has_subscribers()).unwrap_or(false);
        if still_dead {
            st.sessions.remove(&id);
            st.manifest_dirty = true;
            eprintln!("arkosd: session {} reaped {}s after it ended", id.0, ttl.as_secs());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::State;
    use crate::session::{Backpressure, ClientSink, Session, SpawnSpec};
    use protocol::SessionId;
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    fn session(id: u64) -> Arc<Session> {
        let spec = SpawnSpec {
            id: SessionId(id),
            cwd: std::env::temp_dir(),
            shell: Some("/bin/sh".into()),
            env: vec![],
            nice: 0,
            rows: 24,
            cols: 80,
            replay_capacity: 64,
            backpressure: Backpressure::default(),
            seed: Vec::new(),
            recovered: false,
            agent: None,
        };
        // No reader: the scan's `poll_exit` is what records the exit.
        Session::spawn_for_test(spec, false).unwrap()
    }

    /// The reaper's decision is one step under the session lock: an attach is never killed
    /// between "due" and "nobody attached".
    #[test]
    fn a_due_grace_is_taken_once_and_one_within_it_or_attached_is_not() {
        let s = session(34);
        assert_eq!(s.take_due_grace(), None, "no grace");
        s.set_grace(60);
        assert_eq!(s.take_due_grace(), None, "within it");
        assert_eq!(s.grace_secs(), Some(60), "kept while within it");
        s.set_grace_left(Duration::ZERO, 60);
        assert_eq!(s.take_due_grace(), Some(60));
        assert_eq!((s.take_due_grace(), s.grace_secs()), (None, None), "taken once");
        // An attach clears the grace: nothing is due to a session a window shows.
        s.set_grace_left(Duration::ZERO, 60);
        let (a, _b) = UnixStream::pair().unwrap();
        s.subscribe(1, ClientSink::new(1, a, 1 << 20), 1);
        assert_eq!(s.take_due_grace(), None);
        let _ = s.kill();
    }

    #[test]
    fn a_session_past_its_grace_is_ended_and_one_attached_or_within_it_is_not() {
        let (due, within, attached) = (session(31), session(32), session(33));
        let state: Shared = Arc::new(Mutex::new(State::default()));
        state.lock().unwrap().sessions.extend([(due.id, due.clone()), (within.id, within.clone()), (attached.id, attached.clone())]);
        due.set_grace(1);
        within.set_grace(60);
        attached.set_grace(1);
        let (a, _b) = UnixStream::pair().unwrap();
        attached.subscribe(1, ClientSink::new(1, a, 1 << 20), 1);
        std::thread::sleep(Duration::from_millis(1050));
        scan(&state, Duration::from_secs(600));
        assert_eq!(due.grace_secs(), None, "ended once, not again every pass");
        let deadline = Instant::now() + Duration::from_secs(5);
        while due.info().alive {
            assert!(Instant::now() < deadline, "the session past its grace was never ended");
            scan(&state, Duration::from_secs(600));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(within.info().alive, "still within its grace");
        assert!(attached.info().alive, "a window shows it");
        assert_eq!(within.grace_secs(), Some(60));
        assert!(state.lock().unwrap().sessions.contains_key(&due.id), "an ended session stays listed for the usual TTL");
        let _ = within.kill();
        let _ = attached.kill();
    }
}
