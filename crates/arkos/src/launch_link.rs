//! The chooser's line to the daemon while no project window exists. The process connects as the
//! chooser opens (see `launch::Plan`), and the chooser gets the link once the connection exists
//! (`Chooser::attach_link`). The link owns the connection's receiver from then on, lists the
//! sessions for the chooser, stops a folder's, and hands the receiver back when a folder is
//! chosen, with every message that was not its own (the Hello's answer among them) kept in order
//! for the project window's connection loop. It beats the daemon's heartbeat on the current
//! client, and when the connection ends it reconnects with the window loop's policy
//! (`connection::Reconnect`): the new client replaces the old one (`client()` is always the
//! current), and the sessions are listed again.
//!
//! A folder chosen while the link is reconnecting: `finish` stops the attempts and returns the
//! dead receiver (closed), which the project window's loop treats as a lost connection and
//! reconnects on its own; `client()` is then the last client, also dead.
use crate::chooser_sessions::{LinkEvent, Tracker};
use crate::client::Client;
use crate::connection::{self, Reconnect};
use futures::channel::mpsc::{unbounded, UnboundedReceiver};
use futures::channel::oneshot;
use futures::future::{select, Either};
use futures::StreamExt;
use gpui::{App, AsyncApp, Task};
use protocol::{Request, ServerMessage, SessionId};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub type OnEvent = Box<dyn Fn(LinkEvent, &mut App)>;

type Receiver = UnboundedReceiver<ServerMessage>;
/// What `finish` gives back: the messages that were not the chooser's, in order, the receiver
/// they came from, and the client of that same connection (the window sends its Hello and its
/// requests on this one, never on a client read earlier).
pub type Handback = (Vec<ServerMessage>, Receiver, Arc<Client>);

/// A connection made by an attempt that finished after the link was stopped is dropped, so no
/// stray connection stays open and the handback's client and receiver stay one pair.
fn adopt_unless_stopped<T>(stopped: bool, connection: T) -> Option<T> {
    if stopped {
        None
    } else {
        Some(connection)
    }
}

/// Cheap to clone: every copy is the same link.
#[derive(Clone)]
pub struct Link {
    client: Rc<RefCell<Arc<Client>>>,
    socket: PathBuf,
    daemon_exe: PathBuf,
    daemon_args: Vec<String>,
    /// Whether the heartbeat goes on; `finish` ends it.
    beating: Rc<Cell<bool>>,
    tracker: Rc<RefCell<Tracker>>,
    /// The receiver until `start` moves it into the reading task.
    events: Rc<RefCell<Option<Receiver>>>,
    /// The reading task and what stops it.
    running: Rc<RefCell<Option<(oneshot::Sender<()>, Task<Handback>)>>>,
}

impl Link {
    pub fn new(client: Arc<Client>, events: Receiver, socket: PathBuf, daemon_exe: PathBuf, daemon_args: Vec<String>) -> Self {
        Self {
            client: Rc::new(RefCell::new(client)),
            socket,
            daemon_exe,
            daemon_args,
            beating: Rc::new(Cell::new(true)),
            tracker: Rc::default(),
            events: Rc::new(RefCell::new(Some(events))),
            running: Rc::default(),
        }
    }

    /// The current connection's client, for the project window that takes the link's place.
    pub fn client(&self) -> Arc<Client> {
        self.client.borrow().clone()
    }

    /// Reads the daemon's messages, delivering the chooser's answers to `on_event` and keeping
    /// everything else for the window, and asks for the session list.
    pub fn start(&self, on_event: OnEvent, cx: &mut App) {
        let Some(mut events) = self.events.borrow_mut().take() else { return };
        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let tracker = self.tracker.clone();
        let (client, socket, daemon_exe, daemon_args) = (self.client.clone(), self.socket.clone(), self.daemon_exe.clone(), self.daemon_args.clone());
        // The chooser's Hello named this pid, so the watchdog expects heartbeats: they run on
        // the main thread, on whichever client is current, until `finish`.
        let (beat_client, beating) = (self.client.clone(), self.beating.clone());
        cx.spawn(async move |cx: &mut AsyncApp| {
            let mut seq = 0u64;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if !beating.get() {
                    return;
                }
                beat_client.borrow().send(Request::Heartbeat { seq });
                seq += 1;
            }
        })
        .detach();
        let task = cx.spawn(async move |cx: &mut AsyncApp| {
            let mut backlog: Vec<ServerMessage> = Vec::new();
            loop {
                match select(events.next(), &mut stop_rx).await {
                    // The connection ended: reconnect, as the window's loop would.
                    Either::Left((None, _)) => {
                        tracker.borrow_mut().clear();
                        // What was kept belongs to the dead connection.
                        backlog.clear();
                        if cx.update(|cx| on_event(LinkEvent::Lost, cx)).is_err() {
                            break;
                        }
                        let lost_at = Instant::now();
                        let mut policy = Reconnect::new(lost_at);
                        let next = loop {
                            let wait = cx.background_executor().timer(policy.next_delay());
                            if let Either::Right(_) = select(wait, &mut stop_rx).await {
                                break None;
                            }
                            policy.next_attempt();
                            let start = policy.should_start();
                            match connection::attempt(&socket, &daemon_exe, &daemon_args, start, cx).await {
                                Ok(c) => {
                                    let stopped = !matches!(stop_rx.try_recv(), Ok(None));
                                    break adopt_unless_stopped(stopped, c);
                                }
                                Err(e) => {
                                    if start {
                                        policy.failed_start();
                                        if policy.gave_up() {
                                            eprintln!("arkos: arkosd failed to start {} times in a row; only connecting from now on: {e}", connection::MAX_START_FAILURES);
                                        }
                                    }
                                }
                            }
                        };
                        let Some((new_client, new_events)) = next else { break };
                        eprintln!("arkos: the chooser reconnected to arkosd after {:.1}s", lost_at.elapsed().as_secs_f32());
                        // The new client said Hello at connect, naming no folder.
                        let new_client = Arc::new(new_client);
                        *client.borrow_mut() = new_client.clone();
                        events = new_events;
                        let id = new_client.send(Request::ListSessions);
                        tracker.borrow_mut().listed(id);
                    }
                    Either::Left((Some(ServerMessage::Response { id, response }), _)) => {
                        let answer = tracker.borrow_mut().on_response(id, response);
                        match answer {
                            Ok(Some(ev)) => {
                                if cx.update(|cx| on_event(ev, cx)).is_err() {
                                    break;
                                }
                            }
                            Ok(None) => {}
                            Err(response) => backlog.push(ServerMessage::Response { id, response }),
                        }
                    }
                    Either::Left((Some(event), _)) => backlog.push(event),
                    Either::Right(_) => break,
                }
            }
            let current = client.borrow().clone();
            (backlog, events, current)
        });
        *self.running.borrow_mut() = Some((stop_tx, task));
        self.list();
    }

    pub fn list(&self) {
        let id = self.client().send(Request::ListSessions);
        self.tracker.borrow_mut().listed(id);
    }

    /// Kills every one of `sessions`; `LinkEvent::Stopped` for `folder` follows once each is
    /// answered. Nothing happens for an empty list.
    pub fn stop(&self, folder: PathBuf, sessions: &[SessionId]) {
        if sessions.is_empty() {
            return;
        }
        let ids: Vec<u32> = sessions.iter().map(|s| self.client().send(Request::Kill { session: *s })).collect();
        self.tracker.borrow_mut().stopping(folder, &ids);
    }

    /// Whether a Kill is still unanswered: it would otherwise answer into the window's loop,
    /// which did not ask.
    pub fn kills_pending(&self) -> bool {
        self.tracker.borrow().kills_pending()
    }

    /// Ends the link: the receiver and the kept messages come back for the project window.
    /// Ready at once when the link never started.
    pub fn finish(&self) -> Task<Handback> {
        self.beating.set(false);
        if let Some((stop, task)) = self.running.borrow_mut().take() {
            let _ = stop.send(());
            return task;
        }
        match self.events.borrow_mut().take() {
            Some(events) => Task::ready((Vec::new(), events, self.client())),
            // Finished twice: a closed receiver, which the window's loop treats as a lost
            // connection and reconnects.
            None => Task::ready((Vec::new(), unbounded().1, self.client())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_late_success_after_stop_is_dropped_and_a_live_one_kept() {
        // The pair (client, receiver) is adopted whole or not at all.
        assert_eq!(adopt_unless_stopped(true, ("client", "receiver")), None);
        assert_eq!(adopt_unless_stopped(false, ("client", "receiver")), Some(("client", "receiver")));
    }

    #[test]
    fn the_stop_signal_is_seen_after_an_attempt() {
        let (tx, mut rx) = oneshot::channel::<()>();
        assert!(matches!(rx.try_recv(), Ok(None)), "not stopped yet");
        tx.send(()).unwrap();
        assert!(!matches!(rx.try_recv(), Ok(None)), "stopped");
        let (tx, mut rx) = oneshot::channel::<()>();
        drop(tx);
        assert!(!matches!(rx.try_recv(), Ok(None)), "a dropped link counts as stopped");
    }
}
