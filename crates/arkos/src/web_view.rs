//! The web tab's view (web pane spec, sections 3 to 6): one per web tab, holding the model, the
//! web view behind it (`Web`: the real `WKWebView` host or nothing), the address field and the
//! bar's notice. `web_element` draws it; this file has the state and the actions the bar, the
//! keys and an agent's command run, each through the model and then a repaint. The model is
//! tested headless in `web.rs`; nothing here is tested through gpui.

use crate::line_edit::{Edit, LineEdit};
use crate::screen::Frame;
use crate::web::{self, LoadState, ModelEvent, Web, WebModel, NOTICE_FOR};
use gpui::{Context, EventEmitter, FocusHandle};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How often a local page's file is looked at (spec 6).
const FILE_POLL: Duration = Duration::from_secs(1);

/// What `Root` hears from a view.
pub enum WebEvent {
    /// The tab's URL changed: write it into the tab and save.
    Save(String),
    /// The tab's title changed (the page's title, or the fallback).
    Title(String),
}

impl EventEmitter<WebEvent> for WebView {}

pub struct WebView {
    pub model: WebModel,
    web: Box<dyn Web>,
    /// The content's focus: the `Browser` key context lives on it.
    pub focus: FocusHandle,
    /// The address field and its focus; `editing` while the user types in it, so a navigation
    /// committing meanwhile does not replace the text.
    pub address: LineEdit,
    pub address_focus: FocusHandle,
    pub editing: bool,
    /// The bar's notice and when it was set (`NOTICE_FOR`).
    pub notice: Option<(String, Instant)>,
    /// The title last reported, so `Root` hears each change once.
    title_sent: Option<String>,
    /// The notice a repaint has been scheduled for.
    notice_timer: Option<Instant>,
}

impl WebView {
    /// A view on `url` (empty: blank) that starts loading it now. `project` is what a local page
    /// in it may read.
    pub fn new(url: String, project: PathBuf, mut web: Box<dyn Web>, cx: &mut Context<Self>) -> Self {
        let mut model = WebModel::new(url, project);
        model.open(&mut *web);
        let mut address = LineEdit::new();
        address.set(&model.url);
        // The once a second poll of a local page's file (spec 6), for as long as the view lives.
        cx.spawn(async move |view, cx| loop {
            cx.background_executor().timer(FILE_POLL).await;
            if view.update(cx, |v, cx| v.tick(cx)).is_err() {
                break;
            }
        })
        .detach();
        WebView { model, web, focus: cx.focus_handle(), address, address_focus: cx.focus_handle(), editing: false, notice: None, title_sent: None, notice_timer: None }
    }

    /// The tab's title (spec 3.1).
    pub fn title(&self) -> String {
        web::title_for(self.model.title.as_deref(), &self.model.url)
    }

    /// The page it is on.
    pub fn url(&self) -> &str {
        &self.model.url
    }

    /// What `arkos open` and `arkos panes` report for this tab.
    pub fn status(&self) -> protocol::WebState {
        let (state, error) = match &self.model.state {
            LoadState::Blank => ("blank", None),
            LoadState::Loading { .. } => ("loading", None),
            LoadState::Ready => ("ready", None),
            LoadState::Failed { reason } => ("failed", Some(reason.clone())),
        };
        protocol::WebState { url: self.model.shown_address().to_string(), state: state.to_string(), error }
    }

    /// The page has the keyboard (the window's first responder is inside the web view).
    pub fn has_keys(&self) -> bool {
        self.web.has_keys()
    }

    /// Called by the element on every draw: takes what the view reports, feeds the title and
    /// the address field, tells `Root` what to keep. True when the next display frame should be
    /// drawn too: while loading (the progress hairline moves).
    pub fn frame_step(&mut self, now: Instant, cx: &mut Context<Self>) -> bool {
        for event in self.model.apply(&mut *self.web, now) {
            match event {
                ModelEvent::Save(url) => cx.emit(WebEvent::Save(url)),
                ModelEvent::Notice(text) => self.set_notice(text, now),
                ModelEvent::Loaded { .. } => {}
            }
        }
        let title = self.title();
        if self.title_sent.as_deref() != Some(&title) {
            self.title_sent = Some(title.clone());
            cx.emit(WebEvent::Title(title));
        }
        if !self.editing && self.address.text != self.model.shown_address() {
            self.address.set(self.model.shown_address());
        }
        self.schedule_notice_end(cx);
        self.model.loading()
    }

    fn set_notice(&mut self, text: String, now: Instant) {
        self.notice = Some((text, now));
    }

    /// A notice leaves the bar after `NOTICE_FOR`; an idle view asks for no frames, so one
    /// repaint is scheduled for then.
    fn schedule_notice_end(&mut self, cx: &mut Context<Self>) {
        let Some((_, at)) = self.notice.as_ref() else { return };
        let at = *at;
        if self.notice_timer == Some(at) {
            return;
        }
        self.notice_timer = Some(at);
        let left = NOTICE_FOR.saturating_sub(at.elapsed());
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(left).await;
            let _ = view.update(cx, |v, cx| {
                if v.notice.as_ref().is_some_and(|(_, t)| *t == at) {
                    v.notice = None;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The once a second poll (spec 6): a local page follows its file.
    pub fn tick(&mut self, cx: &mut Context<Self>) {
        let fp = web::file_path(&self.model.url).and_then(|p| crate::config_watch::fingerprint(&p));
        if self.model.tick_file(&mut *self.web, fp) {
            cx.notify();
        }
    }

    /// Goes to what the address field (or an agent) says, under `parse_address`'s rules; the
    /// reason when it is not an address.
    pub fn navigate_text(&mut self, text: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let url = web::parse_address(text, &self.model.project)?;
        self.navigate(url, cx);
        Ok(())
    }

    /// Goes to `url`, an absolute URL.
    pub fn navigate(&mut self, url: String, cx: &mut Context<Self>) {
        self.model.navigate(&mut *self.web, url);
        self.editing = false;
        self.address.set(self.model.shown_address());
        cx.notify();
    }

    /// Reload, or Retry on the failed screen; a reload of a page the user typed a new address
    /// over goes to the page, not the typing.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.model.reload(&mut *self.web);
        cx.notify();
    }

    /// Reloads only when the file behind a local page is newer than the one shown: an agent's
    /// second `arkos open` of a page it rewrote. True when it reloaded.
    pub fn reload_if_changed(&mut self, cx: &mut Context<Self>) -> bool {
        let fp = web::file_path(&self.model.url).and_then(|p| crate::config_watch::fingerprint(&p));
        if web::file_path(&self.model.url).is_some() && fp == self.model.shown_fp && !matches!(self.model.state, LoadState::Failed { .. }) {
            return false;
        }
        self.reload(cx);
        true
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        self.model.stop(&mut *self.web);
        cx.notify();
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        self.web.back();
        cx.notify();
    }

    pub fn forward(&mut self, cx: &mut Context<Self>) {
        self.web.forward();
        cx.notify();
    }

    /// Zoom a step up (positive), down (negative) or back to 1 (zero).
    pub fn zoom(&mut self, dir: i32, cx: &mut Context<Self>) {
        self.model.zoom_by(&mut *self.web, dir);
        cx.notify();
    }

    /// The page in the default browser.
    pub fn open_outside(&self, cx: &mut Context<Self>) {
        let url = self.model.shown_address();
        if !url.is_empty() {
            cx.open_url(url);
        }
    }

    /// Where the view goes this frame, or `None` to hide it.
    pub fn place(&mut self, frame: Option<Frame>) {
        self.web.place(frame);
    }

    /// The colour behind the page, from the final theme (`Root::finish_theme`).
    pub fn set_under_color(&mut self, rgb: (u8, u8, u8)) {
        self.web.set_under_color(rgb);
    }

    /// The page is drawn by the web view (hidden otherwise, and Arkos draws a screen).
    pub fn shows_page(&self) -> bool {
        match self.model.state {
            LoadState::Blank | LoadState::Failed { .. } => false,
            LoadState::Loading { .. } => self.model.url.is_empty() == false || self.model.requested.is_some(),
            LoadState::Ready => true,
        }
    }

    /// The user clicked into the address field: it edits what it shows, cursor at the end.
    pub fn begin_edit(&mut self, cx: &mut Context<Self>) {
        self.editing = true;
        let text = self.address.text.clone();
        self.address.set(&text);
        cx.notify();
    }

    /// A key in the address field: Enter goes, Escape puts the page's address back. True when the
    /// field is done with the keys (Enter or Escape) and the pane should have them.
    pub fn address_key(&mut self, ks: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        if ks.modifiers.platform && ks.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                self.address.insert_str(&text);
                self.editing = true;
                cx.notify();
            }
            return false;
        }
        match self.address.handle(ks) {
            Edit::Submit | Edit::SubmitBack => {
                let text = self.address.text.clone();
                if let Err(e) = self.navigate_text(&text, cx) {
                    self.set_notice(e, Instant::now());
                    self.schedule_notice_end(cx);
                    cx.notify();
                }
                self.editing = false;
                true
            }
            Edit::Cancel => {
                self.editing = false;
                self.address.set(self.model.shown_address());
                cx.notify();
                true
            }
            Edit::Changed => {
                self.editing = true;
                cx.notify();
                false
            }
            Edit::Unchanged | Edit::Up | Edit::Down => {
                cx.notify();
                false
            }
        }
    }
}
