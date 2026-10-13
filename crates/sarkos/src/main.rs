mod client;
mod keys;
mod layout;
mod palette;
mod paths;
mod scroll;
mod terminal;
mod terminal_element;
mod workspace;

use futures::channel::mpsc::TryRecvError;
use futures::StreamExt;
use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Entity, KeyDownEvent, ScrollDelta,
    ScrollWheelEvent, Window, WindowBounds, WindowOptions,
};
use protocol::{Event, Request, Response, ServerMessage, SessionId};
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

struct Root {
    client: Arc<client::Client>,
    terminal: Option<Entity<Terminal>>,
    status: String,
}

impl Root {
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
            return div().size_full().bg(rgb(BG)).text_color(rgb(FG)).p_4().child(self.status.clone());
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
            .children(banner.map(|text| {
                div()
                    .w_full()
                    .flex_none()
                    .p(px(4.))
                    .bg(rgb(BANNER_BG))
                    .text_color(rgb(FG))
                    .font_family(terminal_element::FONT_FAMILY)
                    .text_size(px(terminal_element::FONT_SIZE))
                    .child(text)
            }))
            // The terminal stays painted beneath the banner so scrollback remains readable.
            .child(div().flex_1().min_h_0().child(TerminalElement::new(terminal)))
    }
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

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let client_for_view = client.clone();
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions { title: Some("sarkos".into()), ..Default::default() }),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Root { client: client_for_view, terminal: None, status: "connecting to sarkosd".into() }),
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

        // Decide which session to show: the workspace's saved one if the daemon still has it.
        let list_id = client.send(Request::ListSessions);
        let mut pending_create: Option<u32> = None;
        let mut pending_attach: Option<(u32, SessionId)> = None;
        let project_for_task = project.clone();
        let client_for_task = client.clone();
        cx.spawn(async move |cx| {
            let mut closed = false;
            while !closed {
                let Some(first) = events.next().await else { break };
                // Take everything already queued, up to the byte budget, so a flood becomes one
                // grid update and one notify per batch instead of one per message.
                let mut bytes = output_len(&first);
                let mut batch = vec![first];
                while bytes <= BATCH_BUDGET {
                    match events.try_recv() {
                        Ok(msg) => {
                            bytes += output_len(&msg);
                            batch.push(msg);
                        }
                        Err(TryRecvError::Closed) => {
                            closed = true;
                            break;
                        }
                        Err(TryRecvError::Empty) => break,
                    }
                }
                let step = root.update(cx, |root, cx| {
                    let mut pending: Vec<Event> = Vec::new();
                    for msg in batch {
                        let (id, response) = match msg {
                            ServerMessage::Event(ev) => {
                                pending.push(ev);
                                continue;
                            }
                            ServerMessage::Response { id, response } => (id, response),
                        };
                        // Events that came before this response are applied first, keeping order.
                        root.apply_events(&mut pending, cx);
                        match response {
                            Response::Sessions(list) if id == list_id => {
                                match saved.filter(|s| list.iter().any(|i| i.id == *s)) {
                                    Some(sid) => {
                                        let aid = client_for_task.send(Request::Attach { session: sid });
                                        pending_attach = Some((aid, sid));
                                    }
                                    None => {
                                        let cid = client_for_task.send(Request::CreateSession {
                                            cwd: project_for_task.clone(),
                                            shell: None,
                                            env: vec![],
                                            nice: 0,
                                            rows: 24,
                                            cols: 80,
                                        });
                                        pending_create = Some(cid);
                                    }
                                }
                            }
                            Response::SessionCreated { id: sid } if Some(id) == pending_create => {
                                pending_create = None;
                                let project = project_for_task.clone();
                                cx.background_spawn(async move {
                                    if let Err(e) = workspace::save(&project, sid) {
                                        eprintln!("sarkos: cannot save workspace: {e}");
                                    }
                                })
                                .detach();
                                let aid = client_for_task.send(Request::Attach { session: sid });
                                pending_attach = Some((aid, sid));
                            }
                            Response::Attached { replay, alive } if pending_attach.map(|p| p.0) == Some(id) => {
                                let (_, sid) = pending_attach.take().unwrap();
                                let mut core = TerminalCore::new(80, 24, SCROLLBACK);
                                // The previous UI already answered any queries in the replay and
                                // rang its bells, so only the title is kept from this wakeup.
                                let title = core.apply_output(&replay).title;
                                let client = root.client.clone();
                                let terminal = cx.new(|cx| {
                                    let mut t = Terminal::new(core, sid, client, cx);
                                    t.alive = alive;
                                    t.title = title.unwrap_or_default();
                                    t
                                });
                                root.terminal = Some(terminal);
                                root.status = String::new();
                                cx.notify();
                            }
                            Response::Error(e) => {
                                root.status = format!("sarkosd error: {e}");
                                eprintln!("sarkos: {}", root.status);
                                cx.notify();
                            }
                            _ => {}
                        }
                    }
                    root.apply_events(&mut pending, cx);
                });
                if step.is_err() {
                    return;
                }
                // The batch's output is applied: let the reader thread pull more from the socket.
                client_for_task.consumed_output(bytes);
                yield_now().await;
            }
            let reason = client_for_task.disconnect_reason().unwrap_or_else(|| "daemon connection ended".into());
            eprintln!("sarkos: {reason}");
            let _ = root.update(cx, |root, cx| {
                root.status = reason;
                cx.notify();
            });
        })
        .detach();
    });
}
