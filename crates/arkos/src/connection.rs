//! The window's link to arkosd for the life of the window (update-in-place spec, section 3):
//! one client at a time, its heartbeat (also the launch chooser's) and event loop, and a reconnect with backoff when the
//! daemon's connection ends, which an upgrade's exec does on purpose. After ten seconds without
//! a daemon the window starts one, since the old one is gone for good.
use crate::client::{self, Client};
use crate::{Root, ERROR_PREFIX};
use futures::channel::mpsc::{TryRecvError, UnboundedReceiver};
use futures::StreamExt;
use gpui::{App, AsyncApp, Entity};
use protocol::{Event, Request, Response, ServerMessage};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant};

pub const FIRST_RETRY: Duration = Duration::from_millis(250);
pub const MAX_RETRY: Duration = Duration::from_secs(2);
pub const START_DAEMON_AFTER: Duration = Duration::from_secs(10);
pub const BANNER_AFTER: Duration = Duration::from_secs(10);
/// Daemon starts that fail in a row before the window stops starting one and only connects.
pub const MAX_START_FAILURES: u32 = 5;
/// Output bytes applied in one batch before the event loop yields to the frame loop.
const BATCH_BUDGET: usize = 1 << 20;

pub fn backoff(attempt: u32) -> Duration {
    let factor = 1u32 << attempt.min(3);
    (FIRST_RETRY * factor).min(MAX_RETRY)
}

/// After `START_DAEMON_AFTER` without a daemon the window starts one, until `MAX_START_FAILURES`
/// starts in a row have failed: a daemon that keeps exiting is not started again and again.
pub fn should_start_daemon(lost_for: Duration, start_failures: u32) -> bool {
    lost_for >= START_DAEMON_AFTER && start_failures < MAX_START_FAILURES
}

/// Whether a disconnect ends the reconnecting: the daemon refused this window's protocol, and
/// would refuse every retry the same way.
pub fn is_final(status: &str) -> bool {
    status.starts_with(ERROR_PREFIX) && status.contains("protocol version mismatch")
}

/// When to try again and when to start the daemon, for a lost connection: the window's loop and
/// the launch chooser's link share it.
pub struct Reconnect {
    lost_at: Instant,
    attempt: u32,
    start_failures: u32,
}

impl Reconnect {
    pub fn new(lost_at: Instant) -> Self {
        Reconnect { lost_at, attempt: 0, start_failures: 0 }
    }
    pub fn next_delay(&self) -> Duration {
        backoff(self.attempt)
    }
    pub fn should_start(&self) -> bool {
        should_start_daemon(self.lost_at.elapsed(), self.start_failures)
    }
    /// The failed starts reached `MAX_START_FAILURES`: only connecting from here on.
    pub fn gave_up(&self) -> bool {
        self.start_failures == MAX_START_FAILURES
    }
    pub fn next_attempt(&mut self) {
        self.attempt += 1;
    }
    pub fn failed_start(&mut self) {
        self.start_failures += 1;
    }
}

/// One connect on the background executor, starting the daemon too when `start`. The session id
/// it says is the running journal's, or a fresh one's when the daemon sealed it after the socket
/// dropped (`telemetry::connect_sid`).
pub async fn attempt(socket: &std::path::Path, daemon_exe: &std::path::Path, daemon_args: &[String], start: bool, cx: &AsyncApp) -> Result<(Client, UnboundedReceiver<ServerMessage>), String> {
    let (s, d, a) = (socket.to_path_buf(), daemon_exe.to_path_buf(), daemon_args.to_vec());
    cx.background_executor()
        .spawn(async move {
            let sid = crate::telemetry::connect_sid();
            if start { Client::connect_or_start(&s, &d, &a, sid) } else { Client::connect(&s, sid) }
        })
        .await
        .map_err(|e| format!("{e:#}"))
}

/// Binds `first`, then keeps the window connected until it closes.
pub fn start(root: Entity<Root>, socket: PathBuf, daemon_exe: PathBuf, daemon_args: Vec<String>, first: (Arc<Client>, UnboundedReceiver<ServerMessage>, Vec<ServerMessage>), cx: &mut App) {
    cx.spawn(async move |cx| {
        // `backlog`: what the launch chooser read from this connection before the window
        // existed and was not its own (the Hello's answer); applied before anything newer.
        let (mut client, mut events, backlog) = first;
        let mut backlog: VecDeque<ServerMessage> = backlog.into();
        loop {
            let generation = match root.update(cx, |root, cx| {
                root.connected(client.clone(), cx);
                root.connection_generation
            }) {
                Ok(g) => g,
                Err(_) => return,
            };
            let (beat_root, beat_client) = (root.clone(), client.clone());
            cx.spawn(async move |cx| {
                let alive = move |cx: &mut AsyncApp| beat_root.read_with(cx, |root, _| root.connection_generation).ok() == Some(generation);
                heartbeat(beat_client, alive, cx).await
            })
            .detach();
            let Some(reason) = pump(&root, &client, &mut events, &mut backlog, cx).await else { return };
            let Ok(status) = root.update(cx, |root, cx| {
                root.disconnected(reason, cx);
                root.status.clone()
            }) else {
                return;
            };
            if is_final(&status) {
                eprintln!("arkos: not reconnecting: {status}");
                return;
            }
            let lost_at = Instant::now();
            let mut policy = Reconnect::new(lost_at);
            let next = loop {
                cx.background_executor().timer(policy.next_delay()).await;
                policy.next_attempt();
                let start = policy.should_start();
                match attempt(&socket, &daemon_exe, &daemon_args, start, cx).await {
                    Ok(c) => break (c, start),
                    Err(e) => {
                        if start {
                            policy.failed_start();
                        }
                        let gave_up = start && policy.gave_up();
                        if gave_up {
                            eprintln!("arkos: arkosd failed to start {MAX_START_FAILURES} times in a row; only connecting from now on: {e}");
                        }
                        let log = client::daemon_log(&socket);
                        if root
                            .update(cx, |root, cx| {
                                if gave_up {
                                    root.status = format!("arkosd keeps exiting; see the log at {}", log.display());
                                } else if start {
                                    root.status = format!("arkosd is not running and could not be started: {e}");
                                }
                                cx.notify();
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            };
            let ((next_client, next_events), started) = next;
            let how = if started { ", starting it if it was not running" } else { "" };
            eprintln!("arkos: reconnected to arkosd after {:.1}s{how}", lost_at.elapsed().as_secs_f32());
            // Another process answers: the daemon went and a new one came up (an upgrade's exec
            // keeps the pid).
            if next_client.peer_pid() != client.peer_pid() {
                crate::telemetry::error(crate::telemetry::Failure::DaemonRestarted);
            }
            client = Arc::new(next_client);
            events = next_events;
        }
    })
    .detach();
}

/// Sends a heartbeat on `client` each second while `alive` holds: for the project window, while
/// its connection is the current one; for the launch chooser, until the project window takes
/// the connection over. Run on the main thread (a task of the app's foreground executor), so a
/// main thread that hangs stops the heartbeat and the daemon's watchdog relaunches the window.
pub async fn heartbeat(client: Arc<Client>, alive: impl Fn(&mut AsyncApp) -> bool, cx: &mut AsyncApp) {
    let mut seq = 0u64;
    loop {
        cx.background_executor().timer(Duration::from_secs(1)).await;
        if !alive(cx) {
            return;
        }
        client.send(Request::Heartbeat { seq });
        seq += 1;
    }
}

/// Applies everything `client` delivers until its connection ends, and returns why it ended.
/// `None` when the window is gone.
async fn pump(root: &Entity<Root>, client: &Client, events: &mut UnboundedReceiver<ServerMessage>, backlog: &mut VecDeque<ServerMessage>, cx: &mut AsyncApp) -> Option<String> {
    let mut closed = false;
    // Whether the daemon has answered anything, for the hint below.
    let mut answered = false;
    while !closed {
        let first = match backlog.pop_front() {
            Some(first) => first,
            None => {
                let Some(first) = events.next().await else { break };
                first
            }
        };
        // Take everything already queued, up to the byte budget, so a flood becomes one
        // grid update and one notify per terminal per batch instead of one per message.
        let mut bytes = output_len(&first);
        let mut queue = VecDeque::from([first]);
        while bytes <= BATCH_BUDGET {
            let next = match backlog.pop_front() {
                Some(msg) => Ok(msg),
                None => events.try_recv(),
            };
            match next {
                Ok(msg) => {
                    bytes += output_len(&msg);
                    queue.push_back(msg);
                }
                Err(TryRecvError::Closed) => {
                    closed = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        root.update(cx, |root, cx| {
            let mut pending: Vec<Event> = Vec::new();
            for msg in queue.drain(..) {
                match msg {
                    ServerMessage::Event(ev) => pending.push(ev),
                    ServerMessage::Response { id, response } => {
                        answered = true;
                        // Events that came before this response are applied first, keeping order.
                        if !pending.is_empty() {
                            root.route_events(&pending, cx);
                            pending.clear();
                        }
                        let is_error = matches!(response, Response::Error(_));
                        let outcome = root.sessions.handle_response(id, response);
                        root.handle_outcome(outcome, cx);
                        // A later success means the error was transient.
                        if !is_error && root.status.starts_with(ERROR_PREFIX) {
                            root.status.clear();
                            cx.notify();
                        }
                    }
                }
            }
            if !pending.is_empty() {
                root.route_events(&pending, cx);
            }
        })
        .ok()?;
        // The batch's output is applied: let the reader thread pull more from the socket.
        client.consumed_output(bytes);
        yield_now().await;
    }
    let mut reason = client.disconnect_reason().unwrap_or_else(|| "daemon connection ended".into());
    if !answered {
        // A daemon built before the protocol version existed cannot decode this UI's
        // Hello and hangs up without a word.
        reason.push_str("; if arkosd is from an older build, restart it (pkill -x arkosd), which ends every session");
    }
    Some(reason)
}

fn output_len(msg: &ServerMessage) -> usize {
    match msg {
        ServerMessage::Event(Event::Output { bytes, .. }) => bytes.len(),
        _ => 0,
    }
}

/// Returns Pending once. A channel with queued messages resolves `next()` immediately, so without
/// this a flood would keep the main thread in the event loop and starve painting and input.
async fn yield_now() {
    let mut yielded = false;
    futures::future::poll_fn(|cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_from_a_quarter_second_to_two_seconds() {
        assert_eq!(backoff(0), Duration::from_millis(250));
        assert_eq!(backoff(1), Duration::from_millis(500));
        assert_eq!(backoff(2), Duration::from_millis(1000));
        assert_eq!(backoff(3), Duration::from_millis(2000));
        assert_eq!(backoff(4), Duration::from_millis(2000));
        assert_eq!(backoff(40), Duration::from_millis(2000), "no overflow");
    }

    #[test]
    fn only_a_protocol_refusal_is_final() {
        let refused = format!("{ERROR_PREFIX}protocol version mismatch: daemon speaks 1 to 2, client 3; this arkosd is older than the window: run pkill -x arkosd (ends every session) and open Arkos IDE again; daemon closed the connection");
        assert!(is_final(&refused));
        assert!(!is_final("daemon closed the connection"));
        assert!(!is_final(&format!("{ERROR_PREFIX}no session 4; daemon closed the connection")));
        assert!(!is_final("protocol version mismatch"), "only as an error the daemon sent");
    }

    #[test]
    fn after_ten_seconds_the_window_starts_a_daemon() {
        assert!(!should_start_daemon(Duration::from_secs(0), 0));
        assert!(!should_start_daemon(Duration::from_millis(9_999), 0));
        assert!(should_start_daemon(Duration::from_secs(10), 0));
    }

    #[test]
    fn after_five_failed_starts_the_window_only_connects() {
        assert!(should_start_daemon(Duration::from_secs(60), 4));
        assert!(!should_start_daemon(Duration::from_secs(60), 5));
        assert!(!should_start_daemon(Duration::from_secs(60), 6));
    }

    #[test]
    fn the_reconnect_policy_matches_the_window_loop() {
        let mut r = Reconnect::new(Instant::now() - Duration::from_secs(11));
        assert_eq!(r.next_delay(), backoff(0));
        assert!(r.should_start(), "ten seconds lost: start the daemon");
        r.failed_start();
        r.failed_start();
        r.failed_start();
        r.failed_start();
        r.failed_start();
        assert!(!r.should_start(), "five failed starts: only connect");
        r.next_attempt();
        assert_eq!(r.next_delay(), backoff(1));
    }
}
