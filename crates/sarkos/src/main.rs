mod client;
mod keys;
mod layout;
mod palette;
mod pane_view;
mod paths;
mod scroll;
mod split_view;
mod sessions;
mod terminal;
mod terminal_element;
mod tree;
mod workspace;

use futures::channel::mpsc::TryRecvError;
use futures::channel::oneshot;
use futures::StreamExt;
use gpui::{
    actions, div, prelude::*, px, rgb, size, App, Application, AsyncApp, Bounds, Context, Div, Entity, FocusHandle, KeyBinding,
    Menu, MenuItem, MouseButton, MouseMoveEvent, MouseUpEvent, Window, WindowBounds, WindowOptions,
};
use protocol::{Event, Request, Response, ServerMessage, SessionId};
use sessions::{reconcile, Outcome, Reconcile, SessionManager, SCROLLBACK};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use terminal::{Terminal, TerminalCore};
use tree::{Orientation, PaneId, Rect, Side, SplitId, Tab, Tree};
use workspace::{request_save, SharedSaveQueue, Workspace, FORMAT_VERSION};

const BG: u32 = 0x11161f;
const FG: u32 = 0xd6dae0;
/// A slightly lighter navy for the one-line status banner above the panes.
const BANNER_BG: u32 = 0x1a2130;
const BANNER_H: f32 = 28.0;
/// Output bytes applied in one batch before the event loop yields to the frame loop.
const BATCH_BUDGET: usize = 1 << 20;

actions!(sarkos, [Quit]);

/// Starts every status that reports a `Response::Error`.
const ERROR_PREFIX: &str = "sarkosd error: ";
/// The status until the daemon answers the session list.
const CONNECTING: &str = "connecting to sarkosd";

pub struct Root {
    pub tree: Tree,
    pub sessions: SessionManager,
    pub save_queue: SharedSaveQueue,
    pub project: PathBuf,
    /// Connection-level banner (errors, disconnects). Empty when all is well.
    pub status: String,
    /// The grid each pane last measured while it had no terminal to measure itself.
    pub grids: HashMap<PaneId, (usize, usize)>,
    pub grid_waiters: HashMap<PaneId, Vec<oneshot::Sender<(usize, usize)>>>,
    /// Focus handles for panes that show no terminal (busy or empty).
    pub pane_focus: HashMap<PaneId, FocusHandle>,
    /// A split handle being dragged: the split, its orientation, and the split's bounds.
    pub resizing: Option<(SplitId, Orientation, Rect)>,
    pub font_size: f32,
    /// The area the tree is laid out in, in window pixels, from the last render.
    pub content: Rect,
    /// Pane ids in reading order (the tab bar numbers), computed once per render.
    pub pane_order: Vec<PaneId>,
}

impl Root {
    fn new(client: Arc<client::Client>, project: PathBuf, saved: Option<Workspace>) -> Self {
        // A fresh workspace starts with one pane holding a placeholder session that the launch
        // replaces with a real shell (reconcile sees it is not in the daemon's list).
        let tree = saved.map(|ws| ws.tree).unwrap_or_else(|| Tree::new(Tab::Terminal { session: SessionId(0) }));
        Self {
            tree,
            sessions: SessionManager::new(client.clone(), Some(client), project.clone()),
            save_queue: SharedSaveQueue::default(),
            project,
            status: CONNECTING.into(),
            grids: HashMap::new(),
            grid_waiters: HashMap::new(),
            pane_focus: HashMap::new(),
            resizing: None,
            font_size: terminal_element::DEFAULT_FONT_SIZE,
            content: Rect { x: 0.0, y: 0.0, w: 1200.0, h: 800.0 },
            pane_order: Vec::new(),
        }
    }

    pub fn layout(&self) -> tree::Layout {
        self.tree.layout(self.content)
    }

    pub fn save(&mut self, cx: &mut App) {
        let ws = Workspace { version: FORMAT_VERSION, project: self.project.clone(), tree: self.tree.clone() };
        request_save(&self.save_queue, ws, cx);
    }

    pub fn report_grid(&mut self, pane: PaneId, grid: (usize, usize)) {
        self.grids.insert(pane, grid);
        for tx in self.grid_waiters.remove(&pane).unwrap_or_default() {
            let _ = tx.send(grid);
        }
    }

    /// The grid a terminal in `pane` would get: the pane's current terminal's, else the last
    /// measurement of the pane's placeholder.
    fn pane_grid(&self, pane: PaneId, cx: &App) -> Option<(usize, usize)> {
        let p = self.tree.pane(pane)?;
        if let Some(Tab::Terminal { session }) = p.tabs.get(p.active) {
            if let Some(t) = self.sessions.terminals.get(session) {
                let core = &t.read(cx).core;
                return Some((core.columns(), core.screen_lines()));
            }
        }
        self.grids.get(&pane).copied()
    }

    pub fn active_terminal(&self, _cx: &App) -> Option<Entity<Terminal>> {
        let p = self.tree.pane(self.tree.focused)?;
        match p.tabs.get(p.active)? {
            Tab::Terminal { session } => self.sessions.terminals.get(session).cloned(),
            Tab::Viewer { .. } => None,
        }
    }

    pub fn focus_pane(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if self.tree.pane(pane).is_none() {
            return;
        }
        self.tree.focused = pane;
        cx.notify();
    }

    pub fn activate_tab(&mut self, pane: PaneId, idx: usize, cx: &mut Context<Self>) {
        self.tree.activate(pane, idx);
        self.save(cx);
        cx.notify();
    }

    /// Closes a tab. A terminal's session is killed and detached; the pane stays (spec 4).
    pub fn close_tab(&mut self, pane: PaneId, idx: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tree.close_tab(pane, idx) else { return };
        if let Tab::Terminal { session } = tab {
            if session != SessionId(0) {
                self.sessions.kill(session);
                self.sessions.detach(session);
            }
            self.sessions.ready(pane, idx);
        }
        self.save(cx);
        cx.notify();
    }

    pub fn new_terminal_tab(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        let idx = self.tree.add_tab(pane, Tab::Terminal { session: SessionId(0) });
        self.tree.focused = pane;
        self.sessions.create(pane, idx, false);
        self.save(cx);
        cx.notify();
    }

    /// Splits the focused pane and starts a fresh shell in the new one.
    pub fn split_focused(&mut self, side: Side, cx: &mut Context<Self>) {
        let pane = self.tree.focused;
        if let Some(new_pane) = self.tree.split(pane, side, Tab::Terminal { session: SessionId(0) }) {
            self.sessions.create(new_pane, 0, false);
            self.save(cx);
            cx.notify();
        }
    }

    /// Applies what the daemon answered.
    pub fn handle_outcome(&mut self, outcome: Outcome, saved_layout: bool, cx: &mut Context<Self>) {
        match outcome {
            Outcome::None => {}
            Outcome::Sessions(list) => {
                if self.status == CONNECTING {
                    // The banner goes away, so every pane grows: forget measurements taken under
                    // it so restores wait for the next frame's grid.
                    self.status.clear();
                    self.grids.clear();
                }
                for step in reconcile(&self.tree, &list, &self.project) {
                    match step {
                        Reconcile::Attach { pane, idx, session } => self.sessions.attach(session, pane, idx, false),
                        Reconcile::Create { pane, idx } => self.sessions.create(pane, idx, saved_layout),
                    }
                }
                cx.notify();
            }
            Outcome::Created { pane, idx, session, note } => {
                self.tree.replace_tab(pane, idx, Tab::Terminal { session });
                if note {
                    self.sessions.notes.insert(session);
                }
                self.save(cx);
                self.sessions.attach(session, pane, idx, note);
                cx.notify();
            }
            Outcome::Attached { session, pane, idx, replay, alive, note: _ } => {
                eprintln!("sarkos: attached to session {} ({} replay bytes)", session.0, replay.len());
                self.sessions.begin_restore(session);
                let root = cx.entity();
                cx.spawn(async move |_, cx| {
                    if let Err(e) = restore(&root, session, pane, idx, replay, alive, cx).await {
                        eprintln!("sarkos: restoring session {} failed: {e:#}", session.0);
                        let _ = root.update(cx, |root, cx| {
                            root.sessions.abort_restore(session, pane, idx);
                            cx.notify();
                        });
                    }
                })
                .detach();
            }
            Outcome::Killed { .. } => {}
            Outcome::Error(e) => {
                self.status = format!("{ERROR_PREFIX}{e}");
                eprintln!("sarkos: {}", self.status);
                cx.notify();
            }
        }
    }
}

/// Parses a replay off the main thread at the pane's grid, then installs the terminal.
async fn restore(root: &Entity<Root>, session: SessionId, pane: PaneId, idx: usize, replay: Vec<u8>, alive: bool, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let known = root.update(cx, |root, cx| match root.pane_grid(pane, cx) {
        Some(grid) => Ok(grid),
        None => {
            let (tx, rx) = oneshot::channel();
            root.grid_waiters.entry(pane).or_default().push(tx);
            cx.notify();
            Err(rx)
        }
    })?;
    let (cols, rows) = match known {
        Ok(grid) => grid,
        Err(rx) => rx.await?,
    };
    let started = std::time::Instant::now();
    let (core, title) = cx
        .background_spawn(async move {
            let mut core = TerminalCore::new(cols, rows, SCROLLBACK);
            // The previous UI already answered any queries in the replay; keep only the title.
            let title = core.apply_output(&replay).title;
            (core, title)
        })
        .await;
    eprintln!("sarkos: session {} replay parsed at {cols}x{rows} in {:?}", session.0, started.elapsed());
    root.update(cx, |root, cx| {
        // The PTY still has the previous window's size. Setting this grid's size is a no-op when
        // they match and makes the program redraw when they differ.
        let client = root.sessions.transport.clone();
        client.send(Request::Resize { session, rows: rows as u16, cols: cols as u16 });
        let terminal = cx.new(|cx| {
            let mut t = Terminal::new(core, session, root.sessions.client.clone().expect("client"), cx);
            t.alive = alive;
            t.title = title.unwrap_or_default();
            t
        });
        root.sessions.finish_restore(session, terminal, cx);
        root.sessions.ready(pane, idx);
        cx.notify();
    })
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        let banner_h = if self.status.is_empty() { 0.0 } else { BANNER_H };
        let content = Rect { x: 0.0, y: banner_h, w: f32::from(size.width), h: f32::from(size.height) - banner_h };
        if content != self.content {
            // Measurements taken at the old size would parse a replay at the wrong grid.
            self.grids.clear();
            self.content = content;
        }
        // Built first: it creates the focus handles of panes that show no terminal.
        let body = split_view::render_tree(self, window, cx);
        // Focus follows the tree: the focused pane's terminal, or the pane's own handle.
        let wanted = match self.active_terminal(cx) {
            Some(t) => Some(t.read(cx).focus.clone()),
            None => self.pane_focus.get(&self.tree.focused).cloned(),
        };
        if let Some(f) = wanted {
            if !f.is_focused(window) {
                window.focus(&f);
            }
        }
        div()
            .size_full()
            .bg(rgb(BG))
            .flex()
            .flex_col()
            .on_mouse_move(cx.listener(|root, ev: &MouseMoveEvent, _window, cx| {
                if let Some((split, orientation, area)) = root.resizing {
                    // `area` and the pointer are both in window pixels.
                    let ratio = split_view::ratio_from_pointer(area, orientation, f32::from(ev.position.x), f32::from(ev.position.y));
                    root.tree.set_ratio(split, ratio);
                    root.grids.clear();
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|root, _ev: &MouseUpEvent, _window, cx| {
                    if root.resizing.take().is_some() {
                        root.save(cx);
                    }
                }),
            )
            .children((!self.status.is_empty()).then(|| banner_bar(self.status.clone())))
            .child(div().flex_1().min_h_0().child(body))
    }
}

fn banner_bar(text: String) -> Div {
    div()
        .w_full()
        .h(px(BANNER_H))
        .flex_none()
        .px(px(8.0))
        .flex()
        .items_center()
        .bg(rgb(BANNER_BG))
        .text_color(rgb(FG))
        .font_family(terminal_element::FONT_FAMILY)
        .text_size(px(13.0))
        .child(text)
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("sarkos {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    // Spec 2.5: the UI thread is user-interactive so macOS schedules it ahead of the heavy work
    // Claude Code's children do.
    let qos = unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0) };
    if qos != 0 {
        eprintln!("sarkos: could not set user-interactive QoS (error {qos})");
    }
    let (client, mut events) = match client::Client::connect_or_start(&paths::socket_path(), &paths::daemon_exe()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sarkos: cannot reach sarkosd: {e:#}");
            std::process::exit(1);
        }
    };
    let client = Arc::new(client);
    let project = std::env::current_dir().expect("cwd");
    // Synchronous on purpose: one small file read, done before the window opens, so it cannot
    // stall painting or input. Saves happen on the background executor.
    let saved = workspace::load(&project);
    let save_queue = SharedSaveQueue::default();

    Application::new().run(move |cx: &mut App| {
        // gpui installs no menu and no quit binding of its own.
        // A pending debounced save is written before quitting, so the last layout change survives.
        let quit_queue = save_queue.clone();
        cx.on_action(move |_: &Quit, cx| {
            workspace::flush_now(&quit_queue);
            cx.quit();
        });
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.set_menus(vec![Menu { name: "Sarkos IDE".into(), items: vec![MenuItem::action("Quit", Quit)] }]);
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let client_for_view = client.clone();
        let view_queue = save_queue.clone();
        let saved_layout = saved.is_some();
        let view_project = project.clone();
        let view_saved = saved.clone();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions { title: Some("Sarkos IDE".into()), ..Default::default() }),
                    ..Default::default()
                },
                move |_, cx| {
                    cx.new(|_| {
                        let mut root = Root::new(client_for_view, view_project, view_saved);
                        root.save_queue = view_queue;
                        root
                    })
                },
            )
            .expect("open window");
        let root = window.entity(cx).expect("root entity");
        // gpui keeps running without windows; quit with the last one so the heartbeat stops and
        // no windowless process lingers.
        let close_queue = save_queue.clone();
        cx.on_window_closed(move |cx| {
            if cx.windows().is_empty() {
                workspace::flush_now(&close_queue);
                cx.quit();
            }
        })
        .detach();

        // Heartbeat from the main thread, once a second (spec 2.4).
        let hb_client = client.clone();
        cx.spawn(async move |cx| {
            let mut seq = 0u64;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                hb_client.send(Request::Heartbeat { seq });
                seq += 1;
            }
        })
        .detach();

        // Ask for the daemon's sessions; the answer reconciles the saved layout against them.
        root.update(cx, |root, _| root.sessions.list());
        let client_for_task = client.clone();
        cx.spawn(async move |cx| {
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
                let Ok(()) = root.update(cx, |root, cx| {
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
                                root.handle_outcome(outcome, saved_layout, cx);
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
                }) else {
                    return;
                };
                // The batch's output is applied: let the reader thread pull more from the socket.
                client_for_task.consumed_output(bytes);
                yield_now().await;
            }
            let mut reason = client_for_task.disconnect_reason().unwrap_or_else(|| "daemon connection ended".into());
            if !answered {
                // A daemon built before the protocol version existed cannot decode this UI's
                // Hello and hangs up without a word.
                reason.push_str("; if sarkosd is from an older build, restart it (pkill -x sarkosd), which ends every session");
            }
            eprintln!("sarkos: {reason}");
            let _ = root.update(cx, |root, cx| {
                // An error the daemon sent before hanging up (a protocol mismatch) says why; keep it.
                root.status = if root.status.starts_with(ERROR_PREFIX) { format!("{}; {reason}", root.status) } else { reason };
                cx.notify();
            });
        })
        .detach();
    });
}
