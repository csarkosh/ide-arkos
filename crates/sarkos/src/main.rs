mod client;
mod paths;

use futures::StreamExt;
use gpui::{
    div, prelude::*, px, rgb, size, App, Application, Bounds, Context, Window, WindowBounds, WindowOptions,
};
use protocol::{Request, Response, ServerMessage};
use std::sync::Arc;

const BG: u32 = 0x11161f;

struct Root {
    #[allow(dead_code)]
    client: Arc<client::Client>,
    status: String,
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(BG)).text_color(rgb(0xd6dae0)).p_4().child(self.status.clone())
    }
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
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        let client_for_view = client.clone();
        let root = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions { title: Some("sarkos".into()), ..Default::default() }),
                    ..Default::default()
                },
                move |_, cx| cx.new(|_| Root { client: client_for_view, status: "connecting".into() }),
            )
            .expect("open window");
        let client_for_task = client.clone();
        let list_id = client.send(Request::ListSessions);
        let root_entity = root.entity(cx).expect("root entity");
        cx.spawn(async move |cx| {
            while let Some(msg) = events.next().await {
                let ok = root_entity.update(cx, |root, cx| {
                    if let ServerMessage::Response { id, response: Response::Sessions(list) } = &msg {
                        if *id == list_id {
                            root.status = format!("{} session(s) in sarkosd", list.len());
                            eprintln!("sarkos: {}", root.status);
                            cx.notify();
                        }
                    }
                });
                if ok.is_err() {
                    break;
                }
            }
            let reason = client_for_task.disconnect_reason().unwrap_or_else(|| "daemon connection ended".into());
            eprintln!("sarkos: {reason}");
            let _ = root_entity.update(cx, |root, cx| {
                root.status = reason;
                cx.notify();
            });
        })
        .detach();
    });
}
