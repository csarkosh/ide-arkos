//! The window's link to sarkosd for the life of the window (update-in-place spec, section 3):
//! one client at a time, its heartbeat and event loop, and a reconnect with backoff when the
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

/// Binds `first`, then keeps the window connected until it closes.
pub fn start(root: Entity<Root>, socket: PathBuf, daemon_exe: PathBuf, daemon_args: Vec<String>, first: (Arc<Client>, UnboundedReceiver<ServerMessage>), cx: &mut App) {
    cx.spawn(async move |cx| {
        let (mut client, mut events) = first;
        loop {
            let generation = match root.update(cx, |root, cx| {
                root.connected(client.clone(), cx);
                root.connection_generation
            }) {
                Ok(g) => g,
                Err(_) => return,
            };
            heartbeat(root.clone(), client.clone(), generation, cx);
            let Some(reason) = pump(&root, &client, &mut events, cx).await else { return };
            let Ok(status) = root.update(cx, |root, cx| {
                root.disconnected(reason, cx);
                root.status.clone()
            }) else {
                return;
            };
            if is_final(&status) {
                eprintln!("sarkos: not reconnecting: {status}");
                return;
            }
            let lost_at = Instant::now();
            let mut attempt = 0u32;
            let mut start_failures = 0u32;
            let next = loop {
                cx.background_executor().timer(backoff(attempt)).await;
                attempt += 1;
                let (s, d, a) = (socket.clone(), daemon_exe.clone(), daemon_args.clone());
                let start = should_start_daemon(lost_at.elapsed(), start_failures);
                let attempt_result = cx
                    .background_executor()
                    .spawn(async move { if start { Client::connect_or_start(&s, &d, &a) } else { Client::connect(&s) } })
                    .await;
                match attempt_result {
                    Ok(c) => break (c, start),
                    Err(e) => {
                        if start {
                            start_failures += 1;
                        }
                        let gave_up = start && start_failures == MAX_START_FAILURES;
                        if gave_up {
                            eprintln!("sarkos: sarkosd failed to start {MAX_START_FAILURES} times in a row; only connecting from now on: {e:#}");
                        }
                        let log = client::daemon_log(&socket);
                        if root
                            .update(cx, |root, cx| {
                                if gave_up {
                                    root.status = format!("sarkosd keeps exiting; see the log at {}", log.display());
                                } else if start {
                                    root.status = format!("sarkosd is not running and could not be started: {e:#}");
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
            eprintln!("sarkos: reconnected to sarkosd after {:.1}s{how}", lost_at.elapsed().as_secs_f32());
            client = Arc::new(next_client);
            events = next_events;
        }
    })
    .detach();
}

/// Sends a heartbeat each second while `generation` is the window's current connection.
fn heartbeat(root: Entity<Root>, client: Arc<Client>, generation: u64, cx: &mut AsyncApp) {
    cx.spawn(async move |cx| {
        let mut seq = 0u64;
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            match root.read_with(cx, |root, _| root.connection_generation) {
                Ok(g) if g == generation => {}
                _ => return,
            }
            client.send(Request::Heartbeat { seq });
            seq += 1;
        }
    })
    .detach();
}

/// Applies everything `client` delivers until its connection ends, and returns why it ended.
/// `None` when the window is gone.
async fn pump(root: &Entity<Root>, client: &Client, events: &mut UnboundedReceiver<ServerMessage>, cx: &mut AsyncApp) -> Option<String> {
    let mut closed = false;
    // Whether the daemon has answered anything, for the hint below.
    let mut answered = false;
    while !closed {
        let Some(first) = events.next().await else { break };
        // Take everything already queued, up to the byte budget, so a flood becomes one
        // grid update and one notify per terminal per batch instead of one per message.
        let mut bytes = output_len(&first);
        let mut queue = VecDeque::from([first]);
        while bytes <= BATCH_BUDGET {
            match events.try_recv() {
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
                            root.sessions.route_events(&pending, cx);
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
                root.sessions.route_events(&pending, cx);
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
        reason.push_str("; if sarkosd is from an older build, restart it (pkill -x sarkosd), which ends every session");
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
        let refused = format!("{ERROR_PREFIX}protocol version mismatch: daemon speaks 1 to 2, client 3; update the daemon: run Update Sarkos IDE now; daemon closed the connection");
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
}
