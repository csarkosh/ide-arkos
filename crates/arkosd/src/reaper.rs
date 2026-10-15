//! Notices shells that exited while something still holds their PTY open, and forgets dead
//! sessions once nobody is attached and the grace period has passed (spec 2.1: the daemon keeps
//! sessions for reattach, not forever).
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
