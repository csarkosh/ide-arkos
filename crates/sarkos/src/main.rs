mod client;
mod keys;
mod layout;
mod palette;
mod paths;
mod scroll;
mod sessions;
mod terminal;
mod terminal_element;
mod tree;
mod workspace;

use futures::channel::mpsc::TryRecvError;
use futures::channel::oneshot;
use futures::StreamExt;
use gpui::{
    actions, canvas, div, prelude::*, px, rgb, size, App, Application, AsyncApp, Bounds, Context, Div, Entity, KeyBinding,
    KeyDownEvent, Menu, MenuItem, ScrollDelta, ScrollWheelEvent, Window, WindowBounds, WindowOptions,
};
use protocol::{Event, Request, Response, ServerMessage, SessionId};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use terminal::{Terminal, TerminalCore};
use terminal_element::TerminalElement;

const BG: u32 = 0x11161f;
const FG: u32 = 0xd6dae0;
/// A slightly lighter navy for the one-line status banner above the terminal.
const BANNER_BG: u32 = 0x1a2130;
const SCROLLBACK: usize = 20_000;
/// Output bytes applied in one batch before the event loop yields to the frame loop.
const BATCH_BUDGET: usize = 1 << 20;

actions!(sarkos, [Quit]);

/// Starts every status that reports a `Response::Error`.
const ERROR_PREFIX: &str = "sarkosd error: ";

struct Root {
    client: Arc<client::Client>,
    terminal: Option<Entity<Terminal>>,
    status: String,
    /// The (cols, rows) grid the window last measured while it had no terminal.
    grid: Option<(usize, usize)>,
    /// A restore waiting for the first measurement.
    grid_waiter: Option<oneshot::Sender<(usize, usize)>>,
}

impl Root {
    fn new(client: Arc<client::Client>) -> Self {
        Self { client, terminal: None, status: "connecting to sarkosd".into(), grid: None, grid_waiter: None }
    }

    fn report_grid(&mut self, grid: (usize, usize)) {
        self.grid = Some(grid);
        if let Some(tx) = self.grid_waiter.take() {
            let _ = tx.send(grid);
        }
    }

    /// The grid a new terminal will be shown at: the current terminal's if one is being
    /// replaced, else the last measurement.
    fn current_grid(&self, cx: &App) -> Option<(usize, usize)> {
        match &self.terminal {
            Some(t) => {
                let core = &t.read(cx).core;
                Some((core.columns(), core.screen_lines()))
            }
            None => self.grid,
        }
    }

    /// Applies queued daemon events to the terminal: one grid update and at most one notify.
    fn apply_events(&mut self, pending: &mut Vec<Event>, cx: &mut Context<Self>) {
        if pending.is_empty() {
            return;
        }
        if let Some(t) = self.terminal.clone() {
            t.update(cx, |t, cx| t.handle_events(pending, cx));
        }
        pending.clear();
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(terminal) = self.terminal.clone() else {
            // No terminal yet: measure the grid the terminal will get (the whole window, since the
            // banner clears once it is shown) so a replay can be parsed at that size.
            let root = cx.weak_entity();
            let measure = canvas(
                move |bounds, window, cx| {
                    let grid = terminal_element::grid_for(bounds, window);
                    let _ = root.update(cx, |root, _| root.report_grid(grid));
                },
                |_, _, _, _| {},
            )
            .size_full();
            return div()
                .size_full()
                .relative()
                .bg(rgb(BG))
                .child(measure)
                .child(banner_bar(self.status.clone()).absolute().top_0().left_0());
        };
        let (focus, alive, exit_code) = {
            let t = terminal.read(cx);
            (t.focus.clone(), t.alive, t.exit_code)
        };
        // Connection problems and errors outrank the session's own state.
        let banner = if !self.status.is_empty() {
            Some(self.status.clone())
        } else if !alive {
            Some(match exit_code {
                Some(code) => format!("session ended (exit code {code})"),
                None => "session ended".to_string(),
            })
        } else {
            None
        };
        if !focus.is_focused(window) {
            window.focus(&focus);
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG))
            .track_focus(&focus)
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, _window, cx| {
                let Some(t) = this.terminal.clone() else { return };
                let ks = &ev.keystroke;
                if ks.modifiers.platform && ks.key == "v" {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        t.update(cx, |t, _| t.paste(&text));
                    }
                    return;
                }
                t.update(cx, |t, cx| t.handle_key(ks, cx));
            }))
            .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _window, cx| {
                let Some(t) = this.terminal.clone() else { return };
                t.update(cx, |t, cx| {
                    let cell_h = t.cell.map(|c| c.height).unwrap_or(24.0);
                    // Positive wheel y means the content moves down, which is scrolling up into history.
                    let lines = match ev.delta {
                        ScrollDelta::Lines(p) => p.y,
                        ScrollDelta::Pixels(p) => f32::from(p.y) / cell_h,
                    };
                    // If the manual check in Task 10 step 6 shows inverted scrolling, negate `lines` here.
                    t.scroll_lines(lines, cx);
                });
            }))
            .children(banner.map(banner_bar))
            // The terminal stays painted beneath the banner so scrollback remains readable.
            .child(div().flex_1().min_h_0().child(TerminalElement::new(terminal)))
    }
}

/// Whether a successful response should clear `status`. Only an error is cleared, and only once
/// a terminal is shown: before that the banner is all the window has, so a failed launch stays
/// visible. Connection and "session ended" banners are never cleared here.
fn success_clears(status: &str, has_terminal: bool) -> bool {
    has_terminal && status.starts_with(ERROR_PREFIX)
}

fn banner_bar(text: String) -> Div {
    div()
        .w_full()
        .flex_none()
        .p(px(4.))
        .bg(rgb(BANNER_BG))
        .text_color(rgb(FG))
        .font_family(terminal_element::FONT_FAMILY)
        .text_size(px(terminal_element::FONT_SIZE))
        .child(text)
}

fn output_len(msg: &ServerMessage) -> usize {
    match msg {
        ServerMessage::Event(Event::Output { bytes, .. }) => bytes.len(),
        _ => 0,
    }
}

/// What the launch has asked the daemon for, so its responses can be matched.
struct Launch {
    client: Arc<client::Client>,
    project: PathBuf,
    list_id: u32,
    saved: Option<SessionId>,
    pending_create: Option<u32>,
    pending_attach: Option<(u32, SessionId)>,
    /// Whether the daemon has answered anything, Hello included.
    answered: bool,
}

/// An attach response whose replay still has to be parsed before the session can be shown.
struct Restore {
    session: SessionId,
    replay: Vec<u8>,
    alive: bool,
}

impl Launch {
    /// Handles queued messages in order. Stops at an attach response and returns it, leaving the
    /// messages after it in `queue`: they belong after the replay, so they wait until it is applied.
    fn process(&mut self, root: &mut Root, queue: &mut VecDeque<ServerMessage>, cx: &mut Context<Root>) -> Option<Restore> {
        let mut pending: Vec<Event> = Vec::new();
        while let Some(msg) = queue.pop_front() {
            let (id, response) = match msg {
                ServerMessage::Event(ev) => {
                    pending.push(ev);
                    continue;
                }
                ServerMessage::Response { id, response } => (id, response),
            };
            self.answered = true;
            // Events that came before this response are applied first, keeping order.
            root.apply_events(&mut pending, cx);
            let is_error = matches!(response, Response::Error(_));
            match response {
                Response::Sessions(list) if id == self.list_id => {
                    match self.saved.filter(|s| sessions::reattachable(*s, &list, &self.project)) {
                        Some(sid) => {
                            let aid = self.client.send(Request::Attach { session: sid });
                            self.pending_attach = Some((aid, sid));
                        }
                        None => {
                            let cid = self.client.send(Request::CreateSession {
                                cwd: self.project.clone(),
                                shell: None,
                                env: vec![],
                                nice: 0,
                                rows: 24,
                                cols: 80,
                            });
                            self.pending_create = Some(cid);
                        }
                    }
                }
                Response::SessionCreated { id: sid } if Some(id) == self.pending_create => {
                    self.pending_create = None;
                    let project = self.project.clone();
                    cx.background_spawn(async move {
                        if let Err(e) = workspace::save(&project, &workspace::Workspace { version: workspace::FORMAT_VERSION, project: project.clone(), tree: tree::Tree::new(tree::Tab::Terminal { session: sid }) }) {
                            eprintln!("sarkos: cannot save workspace: {e}");
                        }
                    })
                    .detach();
                    let aid = self.client.send(Request::Attach { session: sid });
                    self.pending_attach = Some((aid, sid));
                }
                Response::Attached { replay, alive } if self.pending_attach.map(|p| p.0) == Some(id) => {
                    let (_, sid) = self.pending_attach.take().unwrap();
                    eprintln!("sarkos: attached to session {} ({} replay bytes)", sid.0, replay.len());
                    root.status = "restoring session".into();
                    cx.notify();
                    return Some(Restore { session: sid, replay, alive });
                }
                Response::Error(e) => {
                    root.status = format!("{ERROR_PREFIX}{e}");
                    eprintln!("sarkos: {}", root.status);
                    cx.notify();
                }
                _ => {}
            }
            // Any later success means the error was transient; show the terminal's state again.
            if !is_error && success_clears(&root.status, root.terminal.is_some()) {
                root.status.clear();
                cx.notify();
            }
        }
        root.apply_events(&mut pending, cx);
        None
    }
}

/// Shows an attached session: waits for the window's grid, parses the replay at that size on the
/// background executor, then builds the terminal on the main thread.
async fn restore(root: &Entity<Root>, r: Restore, cx: &mut AsyncApp) -> anyhow::Result<()> {
    let known = root.update(cx, |root, cx| match root.current_grid(cx) {
        Some(grid) => Ok(grid),
        None => {
            let (tx, rx) = oneshot::channel();
            root.grid_waiter = Some(tx);
            Err(rx)
        }
    })?;
    let (cols, rows) = match known {
        Ok(grid) => grid,
        Err(rx) => rx.await?,
    };
    let Restore { session, replay, alive } = r;
    let started = std::time::Instant::now();
    let (core, title) = cx
        .background_spawn(async move {
            let mut core = TerminalCore::new(cols, rows, SCROLLBACK);
            // The previous UI already answered any queries in the replay and rang its bells, so
            // only the title is kept from this wakeup.
            let title = core.apply_output(&replay).title;
            (core, title)
        })
        .await;
    eprintln!("sarkos: replay parsed at {cols}x{rows} in {:?}", started.elapsed());
    root.update(cx, |root, cx| {
        let client = root.client.clone();
        // The PTY still has the previous window's size. Setting this grid's size is a no-op when
        // they match (the kernel signals SIGWINCH only on a change) and makes the program redraw
        // when they differ; the element will not resend it, since the core is already this size.
        client.send(Request::Resize { session, rows: rows as u16, cols: cols as u16 });
        let terminal = cx.new(|cx| {
            let mut t = Terminal::new(core, session, client, cx);
            t.alive = alive;
            t.title = title.unwrap_or_default();
            t
        });
        root.terminal = Some(terminal);
        root.status = String::new();
        cx.notify();
    })
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
    let saved = workspace::load(&project)
        .and_then(|ws| ws.tree.pane(ws.tree.focused).and_then(|p| p.tabs.first().cloned()))
        .and_then(|t| match t {
            tree::Tab::Terminal { session } => Some(session),
            _ => None,
        });

    Application::new().run(move |cx: &mut App| {
        // gpui installs no menu and no quit binding of its own.
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.set_menus(vec![Menu { name: "Sarkos IDE".into(), items: vec![MenuItem::action("Quit", Quit)] }]);
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let client_for_view = client.clone();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions { title: Some("Sarkos IDE".into()), ..Default::default() }),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Root::new(client_for_view)),
            )
            .expect("open window");
        let root = window.entity(cx).expect("root entity");
        // gpui keeps running without windows; quit with the last one so the heartbeat stops and
        // no windowless process lingers.
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
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

        // Decide which session to show: the workspace's saved one if the daemon still runs it here.
        let list_id = client.send(Request::ListSessions);
        let mut launch = Launch {
            client: client.clone(),
            project: project.clone(),
            list_id,
            saved,
            pending_create: None,
            pending_attach: None,
            answered: false,
        };
        let client_for_task = client.clone();
        cx.spawn(async move |cx| {
            let mut closed = false;
            while !closed {
                let Some(first) = events.next().await else { break };
                // Take everything already queued, up to the byte budget, so a flood becomes one
                // grid update and one notify per batch instead of one per message.
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
                loop {
                    let Ok(step) = root.update(cx, |root, cx| launch.process(root, &mut queue, cx)) else { return };
                    let Some(attached) = step else { break };
                    // The channel is not read while the replay is restored, so live output waits
                    // behind it in order, and past the unread cap the daemon pauses the PTY.
                    if let Err(e) = restore(&root, attached, cx).await {
                        eprintln!("sarkos: restoring the session failed: {e:#}");
                        return;
                    }
                }
                // The batch's output is applied: let the reader thread pull more from the socket.
                client_for_task.consumed_output(bytes);
                yield_now().await;
            }
            let mut reason = client_for_task.disconnect_reason().unwrap_or_else(|| "daemon connection ended".into());
            if !launch.answered {
                // A daemon built before the protocol version existed cannot decode this UI's
                // Hello and hangs up without a word.
                reason.push_str("; if sarkosd is from an older build, restart it (pkill -x sarkosd), which ends every session");
            }
            eprintln!("sarkos: {reason}");
            let _ = root.update(cx, |root, cx| {
                // An error the daemon sent before hanging up (a protocol mismatch) says why; keep it.
                if root.status.starts_with(ERROR_PREFIX) {
                    root.status = format!("{}; {reason}", root.status);
                } else {
                    root.status = reason;
                }
                cx.notify();
            });
        })
        .detach();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_clears_only_an_error_shown_over_a_terminal() {
        assert!(success_clears("sarkosd error: write failed", true));
        // Before a terminal exists the error is all the window shows.
        assert!(!success_clears("sarkosd error: spawn failed", false));
        assert!(!success_clears("daemon closed the connection", true));
        assert!(!success_clears("restoring session", true));
        assert!(!success_clears("", true));
    }
}
