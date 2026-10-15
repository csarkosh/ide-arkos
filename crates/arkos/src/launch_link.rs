//! The chooser's line to the daemon while no project window exists. The process connects before
//! any window opens (see `launch::Plan`); the link owns the connection's receiver meanwhile,
//! lists the sessions for the chooser, stops a folder's, and hands the receiver back when a
//! folder is chosen, for the project window's own connection loop.
use crate::chooser_sessions::{LinkEvent, Tracker};
use crate::client::Client;
use futures::channel::mpsc::{unbounded, UnboundedReceiver};
use futures::channel::oneshot;
use futures::future::{select, Either};
use futures::StreamExt;
use gpui::{App, AsyncApp, Task};
use protocol::{Request, ServerMessage, SessionId};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub type OnEvent = Box<dyn Fn(LinkEvent, &mut App)>;

type Receiver = UnboundedReceiver<ServerMessage>;

/// Cheap to clone: every copy is the same link.
#[derive(Clone)]
pub struct Link {
    client: Arc<Client>,
    tracker: Rc<RefCell<Tracker>>,
    /// The receiver until `start` moves it into the reading task.
    events: Rc<RefCell<Option<Receiver>>>,
    /// The reading task and what stops it.
    running: Rc<RefCell<Option<(oneshot::Sender<()>, Task<Receiver>)>>>,
}

impl Link {
    pub fn new(client: Arc<Client>, events: Receiver) -> Self {
        Self { client, tracker: Rc::default(), events: Rc::new(RefCell::new(Some(events))), running: Rc::default() }
    }

    /// Reads the daemon's answers, delivering the chooser's to `on_event`, and asks for the
    /// session list. Answers to the heartbeat are nobody's; an event that arrives now (an update
    /// staged) is dropped, since the project window asks for the update status when it connects.
    pub fn start(&self, on_event: OnEvent, cx: &mut App) {
        let Some(mut events) = self.events.borrow_mut().take() else { return };
        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let tracker = self.tracker.clone();
        let task = cx.spawn(async move |cx: &mut AsyncApp| {
            let mut dropped_events = 0usize;
            loop {
                match select(events.next(), &mut stop_rx).await {
                    // The connection ended: the window's loop reconnects once it has the receiver.
                    Either::Left((None, _)) => break,
                    Either::Left((Some(ServerMessage::Response { id, response }), _)) => {
                        let event = tracker.borrow_mut().on_response(id, response);
                        if let Some(ev) = event {
                            if cx.update(|cx| on_event(ev, cx)).is_err() {
                                break;
                            }
                        }
                    }
                    Either::Left((Some(ServerMessage::Event(_)), _)) => dropped_events += 1,
                    Either::Right(_) => break,
                }
            }
            if dropped_events > 0 {
                eprintln!("arkos: chooser: {dropped_events} daemon events arrived before a project was chosen; the window asks again");
            }
            events
        });
        *self.running.borrow_mut() = Some((stop_tx, task));
        self.list();
    }

    pub fn list(&self) {
        let id = self.client.send(Request::ListSessions);
        self.tracker.borrow_mut().listed(id);
    }

    /// Kills every one of `sessions`; `LinkEvent::Stopped` for `folder` follows once each is
    /// answered. Nothing happens for an empty list.
    pub fn stop(&self, folder: PathBuf, sessions: &[SessionId]) {
        if sessions.is_empty() {
            return;
        }
        let ids: Vec<u32> = sessions.iter().map(|s| self.client.send(Request::Kill { session: *s })).collect();
        self.tracker.borrow_mut().stopping(folder, &ids);
    }

    /// Ends the link: the receiver comes back for the project window. Ready at once when the
    /// link never started.
    pub fn finish(&self) -> Task<Receiver> {
        if let Some((stop, task)) = self.running.borrow_mut().take() {
            let _ = stop.send(());
            return task;
        }
        match self.events.borrow_mut().take() {
            Some(events) => Task::ready(events),
            // Finished twice: a closed receiver, which the window's loop treats as a lost
            // connection and reconnects.
            None => Task::ready(unbounded().1),
        }
    }
}
