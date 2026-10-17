//! The crash dialog (telemetry spec, section 2.3): at the launch after a panic, the incidents the
//! panic hooks wrote (`incidents/<inc>.json` in the telemetry folder) are claimed, so a second
//! window launched at the same time never shows them too, and then shown, queued or deleted as
//! the person's `telemetry.crashes` says. Under `ask` one window of the About window's family
//! says what happened in one sentence, shows the exact bytes under Show report, and waits for
//! Send or Don't send; nothing is queued while it is open, and closing it without a choice puts
//! the incidents back for the next launch. Every decision writes one line per report to
//! `receipts`, which Settings' Reports list reads.
//!
//! A claim is an atomic rename to `incidents/claimed/<inc>.<pid>.json`: of two processes
//! renaming one file, one finds it gone. A claim whose process ended (a window that crashed or
//! quit with the dialog open) is put back at the next launch. An incident that `Crash::from_bytes`
//! refuses is deleted and counted, never shown.
use crate::chrome::{self, TITLE_BAR_H, UI_FONT};
use crate::theme::Theme;
use crate::{about, aux, config, keymap, settings};
use ::telemetry::visits_app::{Crash, Crashes, Mode, Queue};
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, FocusHandle, KeyDownEvent, SharedString, TitlebarOptions, Window, WindowBounds, WindowHandle,
    WindowOptions,
};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

/// The dialog's controls, named as `docs/telemetry.md` names them.
pub const SHOW_REPORT: &str = "Show report";
pub const DONT_SEND: &str = "Don't send";
pub const SEND: &str = "Send";
pub const ALWAYS_SEND: &str = "Always send crash reports";

/// The file in the telemetry folder that keeps one line per decided report: `<inc> <date>
/// <outcome>`, the date the incident's local day and the outcome `queued` or `declined`.
pub const RECEIPTS: &str = "receipts";

/// The window's title, which its own title bar draws.
const TITLE: &str = "Crash Report";
/// The window's width, and its height with the report hidden and shown. Each height is the
/// content's plus the title bar.
const WIDTH: f32 = 480.0;
const HEIGHT: f32 = 168.0 + TITLE_BAR_H;
const HEIGHT_SHOWN: f32 = 520.0 + TITLE_BAR_H;
/// The largest incident read: a real report, with its 64 frames and the one image, is a few
/// kilobytes, so anything this large is not the hook's and is refused unread.
const MAX_INCIDENT: u64 = 64 * 1024;

/// What the launch does with the incidents it claimed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    /// The dialog, for this many incidents.
    Show(usize),
    /// Queued without asking: the person chose `always`.
    QueueAll,
    /// Deleted: the person chose `never`.
    DeleteAll,
    /// Put back for a later launch.
    Wait,
}

/// The decision for `incidents` valid incidents under the person's `crashes` setting and the
/// mode in force. Any rule that names a reason (a source build, `DO_NOT_TRACK`,
/// `ARKOS_TELEMETRY`, `CI`, a run that is not interactive, a config error) keeps them waiting,
/// even one that forces crashes to `never`: only the person's own `never` deletes them. With
/// nothing to show there is nothing to do.
pub fn decide(crashes: Crashes, mode: &Mode, incidents: usize) -> Decision {
    if mode.reason.is_some() || incidents == 0 {
        return Decision::Wait;
    }
    match crashes {
        Crashes::Ask => Decision::Show(incidents),
        Crashes::Always => Decision::QueueAll,
        Crashes::Never => Decision::DeleteAll,
    }
}

/// Whether `s` is 32 lowercase hex characters, an incident id as the hook names it.
fn is_inc(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Whether `name` is an incident the hook finished: `<inc>.json`. Its temporary `.<inc>.tmp`,
/// and anything else in the folder, is not.
pub fn is_incident_name(name: &str) -> bool {
    name.strip_suffix(".json").is_some_and(is_inc)
}

/// The `inc` and pid of a claim's name, `<inc>.<pid>.json`.
fn claim_of(name: &str) -> Option<(&str, u32)> {
    let (inc, pid) = name.strip_suffix(".json")?.split_once('.')?;
    let digits = !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit());
    (is_inc(inc) && digits).then(|| pid.parse().ok().map(|pid| (inc, pid))).flatten()
}

/// Whether process `pid` is running (`cache::alive`). No process of ours is pid 0, or above
/// what a pid can be.
pub fn alive(pid: u32) -> bool {
    pid != 0 && libc::pid_t::try_from(pid).is_ok_and(crate::cache::alive)
}

fn incidents_dir(dir: &Path) -> PathBuf {
    dir.join("incidents")
}

fn claimed_dir(dir: &Path) -> PathBuf {
    dir.join("incidents").join("claimed")
}

/// The plain files in `folder` whose names pass `keep`, sorted by name; none when it is missing.
fn files_named(folder: &Path, keep: impl Fn(&str) -> bool) -> Vec<String> {
    let Ok(entries) = fs::read_dir(folder) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| keep(name))
        .collect();
    names.sort();
    names
}

/// Claims incident `inc` of the telemetry folder `dir` for process `pid`: renames it to
/// `incidents/claimed/<inc>.<pid>.json` and returns that path, or `None` when it is gone, which
/// is another process's claim landing first.
pub fn claim(dir: &Path, inc: &str, pid: u32) -> io::Result<Option<PathBuf>> {
    let claimed = claimed_dir(dir);
    // Only the person may read the reports, as the hook made the folder above.
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&claimed)?;
    let to = claimed.join(format!("{inc}.{pid}.json"));
    match fs::rename(incidents_dir(dir).join(format!("{inc}.json")), &to) {
        Ok(()) => Ok(Some(to)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Puts every claim whose process `alive` says ended back in the incidents folder, and returns
/// how many it put back. A live process's claim is its dialog, open now.
pub fn put_back_dead(dir: &Path, alive: &dyn Fn(u32) -> bool) -> usize {
    let claimed = claimed_dir(dir);
    let mut back = 0;
    for name in files_named(&claimed, |n| claim_of(n).is_some()) {
        let Some((inc, pid)) = claim_of(&name) else { continue };
        if !alive(pid) && fs::rename(claimed.join(&name), incidents_dir(dir).join(format!("{inc}.json"))).is_ok() {
            back += 1;
        }
    }
    back
}

/// One claimed incident: its id, the exact bytes the hook wrote (what Show report shows and Send
/// queues), the report they parse to, and when the hook wrote it.
pub struct Claim {
    pub inc: String,
    pub bytes: Vec<u8>,
    pub crash: Crash,
    pub at: SystemTime,
    /// The claimed file, `incidents/claimed/<inc>.<pid>.json`.
    file: PathBuf,
}

/// The incidents this process claimed, until one decision takes them all. Dropped undecided (the
/// dialog closed without a choice, or the launch waits), they go back to the incidents folder.
pub struct Claims {
    dir: PathBuf,
    held: Vec<Claim>,
    /// How many incidents were refused by `Crash::from_bytes` and deleted.
    pub invalid: usize,
}

/// What became of a report, as `receipts` and the Reports list spell it.
#[derive(Clone, Copy)]
enum Outcome {
    Queued,
    Declined,
}

impl Claims {
    /// Claims every incident in the telemetry folder `dir` for process `pid`, after putting back
    /// the claims of processes `alive` says ended. Each is read and parsed; one that does not
    /// parse is deleted and counted in `invalid`.
    pub fn take(dir: &Path, pid: u32, alive: &dyn Fn(u32) -> bool) -> Claims {
        let back = put_back_dead(dir, alive);
        if back > 0 {
            eprintln!("arkos: {back} crash report(s) of a window that ended are waiting again");
        }
        let mut claims = Claims { dir: dir.to_path_buf(), held: Vec::new(), invalid: 0 };
        for name in files_named(&incidents_dir(dir), is_incident_name) {
            let inc = &name[..name.len() - ".json".len()];
            let file = match claim(dir, inc, pid) {
                Ok(Some(file)) => file,
                Ok(None) => continue,
                Err(e) => {
                    eprintln!("arkos: cannot claim a crash report: {e}");
                    continue;
                }
            };
            match read_incident(&file) {
                Some((bytes, crash, at)) => claims.held.push(Claim { inc: inc.to_string(), bytes, crash, at, file }),
                None => {
                    let _ = fs::remove_file(&file);
                    claims.invalid += 1;
                }
            }
        }
        claims
    }

    /// The incidents claimed, sorted by their ids.
    pub fn held(&self) -> &[Claim] {
        &self.held
    }

    /// Send: queues each report with `Queue::enqueue_crash` as `now`'s, deletes its claim and
    /// writes its receipt, and returns how many it queued. One the queue refuses as invalid is
    /// deleted; one it could not write for another reason waits for the next launch.
    pub fn send(mut self, now: u64) -> usize {
        let queue = Queue::new(&self.dir);
        let mut queued = 0;
        for c in std::mem::take(&mut self.held) {
            match queue.enqueue_crash(&c.crash, now) {
                Ok(_) => {
                    let _ = fs::remove_file(&c.file);
                    receipt(&self.dir, &c, Outcome::Queued);
                    queued += 1;
                }
                Err(e) if matches!(e.kind(), io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData) => {
                    eprintln!("arkos: a crash report was refused by the queue and deleted: {e}");
                    let _ = fs::remove_file(&c.file);
                }
                Err(e) => {
                    eprintln!("arkos: cannot queue a crash report, it waits for the next launch: {e}");
                    self.held.push(c);
                }
            }
        }
        queued
    }

    /// Don't send, and the setting `never`: deletes each claimed report and writes its receipt.
    pub fn decline(mut self) {
        for c in std::mem::take(&mut self.held) {
            match fs::remove_file(&c.file) {
                Ok(()) => receipt(&self.dir, &c, Outcome::Declined),
                Err(e) => eprintln!("arkos: cannot delete a crash report: {e}"),
            }
        }
    }

    /// Puts every claimed report back in the incidents folder, undecided.
    pub fn put_back(self) {
        drop(self);
    }
}

impl Drop for Claims {
    fn drop(&mut self) {
        for c in self.held.drain(..) {
            if let Err(e) = fs::rename(&c.file, incidents_dir(&self.dir).join(format!("{}.json", c.inc))) {
                eprintln!("arkos: cannot put a crash report back: {e}");
            }
        }
    }
}

/// A claimed incident's bytes, the report they parse to and when it was written; `None` for one
/// that is too large, unreadable or not a crash report.
fn read_incident(file: &Path) -> Option<(Vec<u8>, Crash, SystemTime)> {
    let f = fs::File::open(file).ok()?;
    let meta = f.metadata().ok()?;
    if meta.len() > MAX_INCIDENT {
        return None;
    }
    let mut bytes = Vec::new();
    f.take(MAX_INCIDENT).read_to_end(&mut bytes).ok()?;
    let crash = Crash::from_bytes(&bytes).ok()?;
    Some((bytes, crash, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)))
}

/// Appends `c`'s line to `receipts`. A receipt that cannot be written is logged: the decision
/// itself stands.
fn receipt(dir: &Path, c: &Claim, outcome: Outcome) {
    let word = match outcome {
        Outcome::Queued => "queued",
        Outcome::Declined => "declined",
    };
    let line = format!("{} {} {word}\n", c.inc, about::format_date(c.at));
    let written = fs::OpenOptions::new().create(true).append(true).open(dir.join(RECEIPTS)).and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = written {
        eprintln!("arkos: cannot write the crash report's receipt: {e}");
    }
}

/// The dialog's sentence: which binary ran into an error, on which day, and how many more
/// incidents the one decision covers. "Ran into an error", not "crashed": the daemon catches
/// some panics and carries on, and those are reported too.
pub fn sentence(bin: &str, date: &str, more: usize) -> String {
    let who = if bin == ::telemetry::Bin::Daemon.as_str() { "The background daemon" } else { "Arkos IDE" };
    match more {
        0 => format!("{who} ran into an error on {date}."),
        n => format!("{who} ran into an error on {date}, and {n} more."),
    }
}

/// "Always send crash reports" checked at Send: writes `telemetry.crashes = "always"` with the
/// writer `arkos settings set` uses, `config::set_values`, after the same checks: a missing file
/// is written with the defaults first, and nothing is written unless the edited file would load.
pub fn always_send(path: &Path) -> Result<(), String> {
    let edit: &[(&[&str], toml_edit::Value)] = &[(&["telemetry", "crashes"], "always".into())];
    // Writes the defaults when the file is missing.
    let _ = config::load(path);
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    config::apply_edits(&text, edit).and_then(|t| config::parse(&t).map_err(|e| e.to_string()))?;
    config::set_values(path, edit)
}

/// One line of Settings' Reports list.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    /// The incident's local day, "YYYY-MM-DD".
    pub date: String,
    pub inc: String,
    /// `waiting` (not decided yet), `queued`, `left the queue` (queued, and its file is gone:
    /// sent, or refused or expired and deleted, which this window cannot tell apart) or
    /// `declined`.
    pub status: &'static str,
}

/// The Reports list for the telemetry folder `dir`: every receipt, a queued one read as `left the
/// queue` once `queue/crash-<inc>.json` is gone (the crate deletes a refused or expired report
/// too, so it never says `sent`), and every incident still waiting (a dialog may be
/// showing it now), newest first. A line of `receipts` that is not a receipt is skipped.
pub fn reports(dir: &Path) -> Vec<Row> {
    let mut rows = Vec::new();
    for line in fs::read_to_string(dir.join(RECEIPTS)).unwrap_or_default().lines() {
        let mut fields = line.split_whitespace();
        let (Some(inc), Some(date), Some(outcome), None) = (fields.next(), fields.next(), fields.next(), fields.next()) else { continue };
        let status = match outcome {
            "queued" if dir.join("queue").join(format!("crash-{inc}.json")).exists() => "queued",
            "queued" => "left the queue",
            "declined" => "declined",
            _ => continue,
        };
        if is_inc(inc) && date.len() == 10 {
            rows.push(Row { date: date.to_string(), inc: inc.to_string(), status });
        }
    }
    let waiting = |folder: PathBuf, inc_of: fn(&str) -> Option<&str>| {
        files_named(&folder, |n| inc_of(n).is_some()).into_iter().filter_map(move |name| {
            let at = fs::metadata(folder.join(&name)).and_then(|m| m.modified()).ok()?;
            Some(Row { date: about::format_date(at), inc: inc_of(&name)?.to_string(), status: "waiting" })
        })
    };
    rows.extend(waiting(incidents_dir(dir), |n| n.strip_suffix(".json").filter(|inc| is_inc(inc))));
    rows.extend(waiting(claimed_dir(dir), |n| claim_of(n).map(|(inc, _)| inc)));
    // Later lines are later decisions: reversed first, so the sort keeps the newest first within
    // a day.
    rows.reverse();
    rows.sort_by(|a, b| b.date.cmp(&a.date));
    rows
}

/// Set once this process has looked at its incidents, so a second project window (there is one
/// per process) never claims again.
static LOOKED: AtomicBool = AtomicBool::new(false);

/// At the launch of the project window, never in a command-line run: claims the incidents of
/// the telemetry folder and acts on `decide` under the person's `crashes` setting and `mode`.
/// `on_always` applies the config after "Always send crash reports" wrote it.
pub fn at_launch(crashes: Crashes, mode: &Mode, theme: Theme, mono: SharedString, on_always: Rc<dyn Fn(&mut App)>, cx: &mut App) {
    if LOOKED.swap(true, Ordering::SeqCst) {
        return;
    }
    let dir = crate::paths::telemetry_dir();
    // This process holds no claim yet, so a claim under its own pid is an earlier run's whose
    // pid came round again: that one ended too.
    let pid = std::process::id();
    let claims = Claims::take(&dir, pid, &|p| p != pid && alive(p));
    if claims.invalid > 0 {
        eprintln!("arkos: deleted {} crash report(s) that were not valid", claims.invalid);
    }
    match decide(crashes, mode, claims.held().len()) {
        Decision::Show(_) => {
            let handle = open(cx, claims, theme, mono, on_always);
            if cx.has_global::<aux::Aux>() {
                cx.global_mut::<aux::Aux>().crash = handle;
            }
        }
        Decision::QueueAll => {
            claims.send(crate::telemetry::unix_now());
            aux::reports_changed(cx);
        }
        Decision::DeleteAll => {
            claims.decline();
            aux::reports_changed(cx);
        }
        Decision::Wait => claims.put_back(),
    }
}

/// What the dialog shows and the state of its two controls.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DialogState {
    pub sentence: String,
    /// Every claimed report's bytes, as text, in the order shown.
    pub report: String,
    /// Whether the Show report disclosure is open.
    pub show_report: bool,
    /// "Always send crash reports": checked only by the person.
    pub always: bool,
}

impl DialogState {
    /// The dialog's first state over `claims`: the sentence names the newest incident and counts
    /// the rest, the report is every claimed report's bytes, the disclosure is closed and the
    /// Always send box unchecked, so Send is consent for these reports alone.
    pub fn of(claims: &Claims) -> DialogState {
        let held = claims.held();
        let newest = held.iter().max_by_key(|c| c.at);
        let sentence = newest.map_or_else(String::new, |c| sentence(&c.crash.env.bin, &about::format_date(c.at), held.len() - 1));
        let report = held.iter().map(|c| String::from_utf8_lossy(&c.bytes).into_owned()).collect::<Vec<_>>().join("\n\n");
        DialogState { sentence, report, show_report: false, always: false }
    }
}

/// The crash dialog's view: the claims until Send or Don't send takes them; dropped with the
/// window, it puts back whatever it still holds.
pub struct CrashDialog {
    claims: Option<Claims>,
    state: DialogState,
    theme: Theme,
    mono: SharedString,
    on_always: Rc<dyn Fn(&mut App)>,
    focus: FocusHandle,
}

impl CrashDialog {
    fn new(claims: Claims, theme: Theme, mono: SharedString, on_always: Rc<dyn Fn(&mut App)>, focus: FocusHandle) -> CrashDialog {
        CrashDialog { state: DialogState::of(&claims), claims: Some(claims), theme, mono, on_always, focus }
    }

    /// Redraws the window in the project window's new theme.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Send: the reports go to the queue, and with the box checked, `crashes` becomes `always`.
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(claims) = self.claims.take() {
            claims.send(crate::telemetry::unix_now());
        }
        if self.state.always {
            match always_send(&crate::paths::config_file()) {
                Ok(()) => {
                    let on_always = self.on_always.clone();
                    cx.defer(move |cx| on_always(cx));
                }
                Err(e) => eprintln!("arkos: cannot turn crash reports to always: {e}"),
            }
        }
        cx.defer(aux::reports_changed);
        window.remove_window();
    }

    /// Don't send: the reports are deleted.
    fn dont_send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(claims) = self.claims.take() {
            claims.decline();
        }
        cx.defer(aux::reports_changed);
        window.remove_window();
    }
}

/// Opens the dialog centred on the main display over `claims`. `None` when gpui could not open
/// it, which the log records; the claims then go back for the next launch.
fn open(cx: &mut App, claims: Claims, theme: Theme, mono: SharedString, on_always: Rc<dyn Fn(&mut App)>) -> Option<WindowHandle<CrashDialog>> {
    let fixed = size(px(WIDTH), px(HEIGHT));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, fixed, cx))),
        // The window draws the IDE's own title bar in the native one's place.
        titlebar: Some(TitlebarOptions { title: Some(TITLE.into()), appears_transparent: true, traffic_light_position: None }),
        focus: true,
        is_resizable: false,
        window_min_size: Some(fixed),
        ..Default::default()
    };
    let opened = cx.open_window(options, move |window, cx| {
        let view = cx.new(|cx| CrashDialog::new(claims, theme, mono, on_always, cx.focus_handle()));
        window.focus(&view.read(cx).focus);
        view
    });
    match opened {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("arkos: the crash dialog did not open: {e:#}");
            None
        }
    }
}

impl Render for CrashDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = settings::Colors::of(&self.theme);
        let (bg, field) = (self.theme.chrome.window_bg.hsla(), self.theme.chrome.sidebar.hsla());
        let disclosure = div()
            .id("crash-show-report")
            .flex_none()
            .flex()
            .gap(px(6.0))
            .text_color(c.accent)
            .cursor_pointer()
            .hover(|s| s.underline())
            .child(if self.state.show_report { "▾" } else { "▸" })
            .child(SHOW_REPORT)
            .on_click(cx.listener(|d, _, window, cx| {
                d.state.show_report = !d.state.show_report;
                window.resize(size(px(WIDTH), px(if d.state.show_report { HEIGHT_SHOWN } else { HEIGHT })));
                cx.notify();
            }));
        let report = self.state.show_report.then(|| {
            div()
                .id("crash-report")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(8.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(c.border)
                .bg(field)
                .font_family(self.mono.clone())
                .text_size(px(11.0))
                .child(self.state.report.clone())
        });
        let always = div()
            .id("crash-always")
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_pointer()
            .child(
                div()
                    .w(px(14.0))
                    .h(px(14.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(3.0))
                    .border_1()
                    .border_color(if self.state.always { c.accent } else { c.border })
                    .text_size(px(11.0))
                    .child(if self.state.always { "✓" } else { "" }),
            )
            .child(ALWAYS_SEND)
            .on_click(cx.listener(|d, _, _, cx| {
                d.state.always = !d.state.always;
                cx.notify();
            }));
        let dont = settings::pill("crash-dont-send", DONT_SEND, false, true, c).on_click(cx.listener(|d, _, window, cx| d.dont_send(window, cx)));
        let send = settings::pill("crash-send", SEND, true, true, c).on_click(cx.listener(|d, _, window, cx| d.send(window, cx)));
        div()
            .id("crash-dialog")
            .key_context("CrashDialog")
            .track_focus(&self.focus)
            .on_key_down(|ev: &KeyDownEvent, window, _| {
                // Escape closes without a choice: the reports wait for the next launch.
                if ev.keystroke.key == "escape" && !ev.keystroke.modifiers.modified() {
                    window.remove_window();
                }
            })
            // Cmd+W arrives as the close-tab action.
            .on_action(|_: &keymap::CloseTab, window, _| window.remove_window())
            .on_action(|_: &keymap::NewWindow, _, cx| aux::forward_new_window(cx))
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .text_color(c.text)
            .text_size(px(13.0))
            .font_family(SharedString::from(UI_FONT))
            .child(chrome::aux_title_bar(TITLE, &self.theme))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .p(px(20.0))
                    .child(div().flex_none().font_weight(gpui::FontWeight::SEMIBOLD).child(self.state.sentence.clone()))
                    .child(div().flex_none().text_color(c.dim).text_size(px(12.0)).child("A crash report says where it happened and in which build. Nothing in it names you, this Mac or your files."))
                    .child(disclosure)
                    .children(report)
                    .when(!self.state.show_report, |d| d.child(div().flex_1()))
                    .child(div().flex_none().flex().items_center().gap(px(8.0)).child(always).child(div().flex_1()).child(dont).child(send)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::telemetry::visits_app::{new_id, Channel, Crash, CrashKind, Crashes, Envelope, Uptime, Usage};
    use ::telemetry::Bin;
    use std::time::Duration;

    /// The mode a build of `channel` gives `crashes` under usage `local`, with one environment
    /// variable set (or none), interactive or not.
    fn mode(crashes: Crashes, var: Option<(&'static str, &'static str)>, channel: Channel, interactive: bool) -> Mode {
        crate::telemetry::mode_of(Usage::Local, crashes, channel, var, interactive)
    }

    /// Review Focus 6: the dialog shows only under `ask` with no rule of the environment and
    /// something to show; `always` queues and the setting `never` deletes, both only with no
    /// rule; every rule of the environment, and a config error, keeps the incidents waiting,
    /// even when it forces crashes to `never`.
    #[test]
    fn the_decision_for_every_setting_rule_and_count() {
        use Decision::*;
        for n in [1, 3] {
            assert_eq!(decide(Crashes::Ask, &mode(Crashes::Ask, None, Channel::Release, true), n), Show(n));
            assert_eq!(decide(Crashes::Always, &mode(Crashes::Always, None, Channel::Release, true), n), QueueAll);
            assert_eq!(decide(Crashes::Never, &mode(Crashes::Never, None, Channel::Release, true), n), DeleteAll);
        }
        for crashes in [Crashes::Never, Crashes::Ask, Crashes::Always] {
            assert_eq!(decide(crashes, &mode(crashes, None, Channel::Release, true), 0), Wait, "nothing to show under {crashes:?}");
            let ruled = [
                mode(crashes, None, Channel::Source, true),
                mode(crashes, Some(("DO_NOT_TRACK", "1")), Channel::Release, true),
                mode(crashes, Some(("ARKOS_TELEMETRY", "off")), Channel::Release, true),
                mode(crashes, Some(("ARKOS_TELEMETRY", "log")), Channel::Release, true),
                mode(crashes, Some(("CI", "true")), Channel::Release, true),
                mode(crashes, None, Channel::Release, false),
                crate::telemetry::mode_of_broken_config(),
            ];
            for m in &ruled {
                assert!(m.reason.is_some());
                for n in [0, 1, 3] {
                    assert_eq!(decide(crashes, m, n), Wait, "{crashes:?} under {:?} with {n}", m.reason);
                }
            }
        }
        // An environment that forces crashes to `never` is not the person's `never`: they wait.
        let dnt = mode(Crashes::Ask, Some(("DO_NOT_TRACK", "1")), Channel::Release, true);
        assert_eq!(dnt.crashes, Crashes::Never);
        assert_eq!(decide(Crashes::Ask, &dnt, 2), Wait);
        let off = mode(Crashes::Always, Some(("ARKOS_TELEMETRY", "off")), Channel::Release, true);
        assert_eq!(off.crashes, Crashes::Never);
        assert_eq!(decide(Crashes::Always, &off, 2), Wait);
    }

    #[test]
    fn incidents_are_listed_by_the_hooks_names_only() {
        assert!(is_incident_name(&format!("{}.json", new_id())));
        assert!(is_incident_name("0123456789abcdef0123456789abcdef.json"));
        for bad in [
            ".0123456789abcdef0123456789abcdef.tmp",
            "0123456789abcdef0123456789abcde.json",
            "0123456789abcdef0123456789abcdef0.json",
            "0123456789ABCDEF0123456789abcdef.json",
            "0123456789abcdef0123456789abcdef.json.tmp",
            "0123456789abcdef0123456789abcdef.4242.json",
            "claimed",
            "README",
        ] {
            assert!(!is_incident_name(bad), "{bad}");
        }
    }

    /// A valid crash report of `bin` under a fresh `inc`.
    fn crash(bin: Bin) -> Crash {
        Crash {
            env: Envelope { ch: Channel::Release, ..::telemetry::envelope(bin) },
            inc: new_id(),
            ck: CrashKind::Panic,
            commit: "4f2a9c1".into(),
            pk: "other".into(),
            mh: Some("a".repeat(64)),
            file: "/src/crates/arkos/src/md_view.rs".into(),
            line: 812,
            frames: vec![(0, 1_843_200)],
            images: vec![("arkos".into(), "0".repeat(32), 48_234_496)],
            sig: None,
            up: Uptime::Lt10m,
            mem: None,
        }
    }

    /// 2026-10-15 12:00 UTC, when the test incidents happened.
    const AT: u64 = 1_792_065_600;

    /// Writes `crash` as the panic hook does, `incidents/<inc>.json`, dated `AT`, and returns its
    /// bytes.
    fn incident(dir: &Path, crash: &Crash) -> Vec<u8> {
        let bytes = crash.to_bytes().unwrap();
        write_incident(dir, &format!("{}.json", crash.inc), &bytes);
        bytes
    }

    fn write_incident(dir: &Path, name: &str, bytes: &[u8]) {
        let incidents = dir.join("incidents");
        std::fs::create_dir_all(&incidents).unwrap();
        let file = incidents.join(name);
        std::fs::write(&file, bytes).unwrap();
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(at()).unwrap();
    }

    fn at() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(AT)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir).map_or(Vec::new(), |d| d.map(|e| e.unwrap().file_name().into_string().unwrap()).collect());
        names.sort();
        names
    }

    fn receipts(dir: &Path) -> String {
        std::fs::read_to_string(dir.join(RECEIPTS)).unwrap_or_default()
    }

    /// A pid no process has: above the kernel's limit.
    const DEAD: u32 = 999_999_999;

    #[test]
    fn a_claim_is_one_process_s_and_the_other_finds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let c = crash(Bin::App);
        incident(dir.path(), &c);
        let first = claim(dir.path(), &c.inc, 100).unwrap();
        assert_eq!(first, Some(dir.path().join("incidents/claimed").join(format!("{}.100.json", c.inc))));
        assert_eq!(claim(dir.path(), &c.inc, 200).unwrap(), None, "the second claimer finds it gone");
        assert_eq!(names(&dir.path().join("incidents/claimed")), vec![format!("{}.100.json", c.inc)]);
        // Raced: of two threads claiming one file at once, exactly one has it.
        for _ in 0..50 {
            let c = crash(Bin::App);
            incident(dir.path(), &c);
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let racers: Vec<_> = [300, 400]
                .map(|pid| {
                    let (dir, inc, barrier) = (dir.path().to_path_buf(), c.inc.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        barrier.wait();
                        claim(&dir, &inc, pid).unwrap()
                    })
                })
                .into_iter()
                .collect();
            let won: Vec<_> = racers.into_iter().filter_map(|t| t.join().unwrap()).collect();
            assert_eq!(won.len(), 1, "{won:?}");
        }
    }

    #[test]
    fn a_claim_whose_process_ended_is_put_back_and_a_live_one_kept() {
        let dir = tempfile::tempdir().unwrap();
        let (dead, live) = (crash(Bin::App), crash(Bin::Daemon));
        let claimed = dir.path().join("incidents/claimed");
        std::fs::create_dir_all(&claimed).unwrap();
        std::fs::write(claimed.join(format!("{}.{DEAD}.json", dead.inc)), dead.to_bytes().unwrap()).unwrap();
        std::fs::write(claimed.join(format!("{}.{}.json", live.inc, std::process::id())), live.to_bytes().unwrap()).unwrap();
        std::fs::write(claimed.join("stray.json"), "{}").unwrap();
        assert_eq!(put_back_dead(dir.path(), &|pid| pid != DEAD), 1);
        assert_eq!(std::fs::read(dir.path().join("incidents").join(format!("{}.json", dead.inc))).unwrap(), dead.to_bytes().unwrap());
        assert_eq!(names(&claimed), vec![format!("{}.{}.json", live.inc, std::process::id()), "stray.json".to_string()]);
        // The real check: this process is alive, a pid above the limit is not.
        assert!(alive(std::process::id()));
        assert!(!alive(DEAD));
    }

    #[test]
    fn taking_claims_every_incident_and_deletes_and_counts_an_invalid_one() {
        let dir = tempfile::tempdir().unwrap();
        let (app, daemon) = (crash(Bin::App), crash(Bin::Daemon));
        let (app_bytes, daemon_bytes) = (incident(dir.path(), &app), incident(dir.path(), &daemon));
        let bad = new_id();
        write_incident(dir.path(), &format!("{bad}.json"), b"{\"k\":\"crash\"}");
        write_incident(dir.path(), &format!(".{}.tmp", new_id()), b"half");
        // Another window's claim, alive, is not this one's to show.
        let theirs = crash(Bin::App);
        std::fs::create_dir_all(dir.path().join("incidents/claimed")).unwrap();
        std::fs::write(dir.path().join("incidents/claimed").join(format!("{}.{}.json", theirs.inc, std::process::id() + 1)), theirs.to_bytes().unwrap()).unwrap();
        let claims = Claims::take(dir.path(), 4242, &|_| true);
        assert_eq!(claims.invalid, 1);
        let mut held: Vec<(&str, &[u8])> = claims.held().iter().map(|c| (c.inc.as_str(), c.bytes.as_slice())).collect();
        held.sort();
        let mut want = vec![(app.inc.as_str(), app_bytes.as_slice()), (daemon.inc.as_str(), daemon_bytes.as_slice())];
        want.sort();
        assert_eq!(held, want);
        assert!(claims.held().iter().all(|c| c.at == at()));
        assert_eq!(names(&dir.path().join("incidents")).len(), 2, "the hidden temporary file and the claimed folder: {:?}", names(&dir.path().join("incidents")));
        assert!(!dir.path().join("incidents").join(format!("{bad}.json")).exists(), "the invalid one is deleted");
        let claimed = names(&dir.path().join("incidents/claimed"));
        assert_eq!(claimed.len(), 3, "two of ours and theirs: {claimed:?}");
        assert!(claimed.contains(&format!("{}.4242.json", app.inc)) && claimed.contains(&format!("{}.4242.json", daemon.inc)));
        assert_eq!(receipts(dir.path()), "", "nothing decided yet");
        claims.put_back();
    }

    #[test]
    fn send_queues_the_exact_bytes_and_writes_a_queued_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (crash(Bin::App), crash(Bin::Daemon));
        let (a_bytes, b_bytes) = (incident(dir.path(), &a), incident(dir.path(), &b));
        let claims = Claims::take(dir.path(), 4242, &|_| true);
        assert_eq!(names(&dir.path().join("queue")), Vec::<String>::new(), "nothing is queued while the dialog is open");
        assert_eq!(claims.send(AT + 3600), 2);
        assert_eq!(std::fs::read(dir.path().join("queue").join(format!("crash-{}.json", a.inc))).unwrap(), a_bytes);
        assert_eq!(std::fs::read(dir.path().join("queue").join(format!("crash-{}.json", b.inc))).unwrap(), b_bytes);
        assert_eq!(names(&dir.path().join("incidents/claimed")), Vec::<String>::new());
        assert_eq!(names(&dir.path().join("incidents")), vec!["claimed".to_string()]);
        let date = crate::about::format_date(at());
        let mut lines: Vec<String> = receipts(dir.path()).lines().map(str::to_string).collect();
        lines.sort();
        let mut want = vec![format!("{} {date} queued", a.inc), format!("{} {date} queued", b.inc)];
        want.sort();
        assert_eq!(lines, want);
    }

    #[test]
    fn dont_send_deletes_and_writes_a_declined_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let c = crash(Bin::App);
        incident(dir.path(), &c);
        let claims = Claims::take(dir.path(), 4242, &|_| true);
        claims.decline();
        assert_eq!(names(&dir.path().join("queue")), Vec::<String>::new());
        assert_eq!(names(&dir.path().join("incidents/claimed")), Vec::<String>::new());
        assert_eq!(names(&dir.path().join("incidents")), vec!["claimed".to_string()]);
        assert_eq!(receipts(dir.path()), format!("{} {} declined\n", c.inc, crate::about::format_date(at())));
    }

    #[test]
    fn closing_without_a_choice_puts_the_claims_back_for_next_time() {
        let dir = tempfile::tempdir().unwrap();
        let c = crash(Bin::App);
        let bytes = incident(dir.path(), &c);
        let claims = Claims::take(dir.path(), 4242, &|_| true);
        assert!(!dir.path().join("incidents").join(format!("{}.json", c.inc)).exists(), "claimed");
        drop(claims);
        assert_eq!(std::fs::read(dir.path().join("incidents").join(format!("{}.json", c.inc))).unwrap(), bytes);
        assert_eq!(names(&dir.path().join("incidents/claimed")), Vec::<String>::new());
        assert_eq!(receipts(dir.path()), "", "no decision, no receipt");
        // The next launch finds it again, with its date.
        let again = Claims::take(dir.path(), 4343, &|_| true);
        assert_eq!(again.held().len(), 1);
        assert_eq!(again.held()[0].at, at());
        again.put_back();
        assert!(dir.path().join("incidents").join(format!("{}.json", c.inc)).exists());
    }

    /// Review Focus 6: the dialog opens with Always send unchecked and the report hidden; the
    /// sentence names the newest incident and counts the other, and the report is every claimed
    /// report's exact bytes.
    #[test]
    fn the_dialog_opens_unchecked_and_names_the_newest_incident() {
        let dir = tempfile::tempdir().unwrap();
        let (older, newer) = (crash(Bin::App), crash(Bin::Daemon));
        incident(dir.path(), &older);
        let newer_bytes = newer.to_bytes().unwrap();
        let file = dir.path().join("incidents").join(format!("{}.json", newer.inc));
        std::fs::write(&file, &newer_bytes).unwrap();
        let later = at() + Duration::from_secs(2 * 86_400);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(later).unwrap();
        let claims = Claims::take(dir.path(), 4242, &|_| true);
        let state = DialogState::of(&claims);
        assert!(!state.always, "Always send starts unchecked");
        assert!(!state.show_report, "the report starts hidden");
        assert_eq!(state.sentence, format!("The background daemon ran into an error on {}, and 1 more.", crate::about::format_date(later)));
        let joined = claims.held().iter().map(|c| String::from_utf8(c.bytes.clone()).unwrap()).collect::<Vec<_>>().join("\n\n");
        assert_eq!(state.report, joined);
        assert!(state.report.contains(&String::from_utf8(newer_bytes).unwrap()) && state.report.contains(&String::from_utf8(older.to_bytes().unwrap()).unwrap()));
        claims.put_back();
    }

    #[test]
    fn the_sentence_names_the_binary_the_date_and_the_rest() {
        assert_eq!(sentence("app", "2026-10-15", 0), "Arkos IDE ran into an error on 2026-10-15.");
        assert_eq!(sentence("daemon", "2026-10-15", 0), "The background daemon ran into an error on 2026-10-15.");
        assert_eq!(sentence("app", "2026-10-15", 2), "Arkos IDE ran into an error on 2026-10-15, and 2 more.");
        assert_eq!(sentence("daemon", "2026-10-14", 1), "The background daemon ran into an error on 2026-10-14, and 1 more.");
    }

    #[test]
    fn the_dialog_s_words_are_the_disclosure_page_s() {
        assert_eq!([SHOW_REPORT, DONT_SEND, SEND, ALWAYS_SEND], ["Show report", "Don't send", "Send", "Always send crash reports"]);
        let page = include_str!("../../../docs/telemetry.md").split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(page.contains(&format!("The crash dialog's **{SHOW_REPORT}** shows the exact bytes")));
        assert!(page.contains("shown under Reports in Settings"));
        assert!(!sentence("app", "x", 1).contains("crashed"), "the daemon recovers from some panics");
    }

    #[test]
    fn always_send_writes_the_crashes_key_through_the_config_writer() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        // A missing file is written with the defaults first, then edited.
        always_send(&config).unwrap();
        let text = std::fs::read_to_string(&config).unwrap();
        assert_eq!(crate::config::parse(&text).unwrap().telemetry.crashes, Crashes::Always);
        assert!(text.contains("[telemetry]"));
        // An existing file keeps its comments and other keys.
        std::fs::write(&config, "# mine\n[update]\nauto = false\n\n[telemetry]\ncrashes = \"ask\" # asked\n").unwrap();
        always_send(&config).unwrap();
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.contains("# mine") && text.contains("# asked"), "{text}");
        let parsed = crate::config::parse(&text).unwrap();
        assert_eq!(parsed.telemetry.crashes, Crashes::Always);
        assert!(!parsed.update.auto);
        // A file that would not load after the edit is left alone.
        std::fs::write(&config, "[telemetry]\nusage = \"maybe\"\n").unwrap();
        assert!(always_send(&config).is_err());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "[telemetry]\nusage = \"maybe\"\n");
    }

    /// A queued report whose file is gone has left the queue: sent, or refused or expired and
    /// deleted by the crate, which this window cannot tell apart, so it never says `sent`.
    #[test]
    fn the_reports_list_reads_the_receipts_and_the_queue_and_adds_what_waits() {
        let dir = tempfile::tempdir().unwrap();
        let (sent, queued, declined, waiting, shown) = (new_id(), new_id(), new_id(), crash(Bin::App), crash(Bin::Daemon));
        std::fs::write(
            dir.path().join(RECEIPTS),
            format!("{sent} 2026-10-01 queued\n{declined} 2026-10-03 declined\nnot a receipt\n{queued} 2026-10-02 queued\n"),
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("queue")).unwrap();
        std::fs::write(dir.path().join("queue").join(format!("crash-{queued}.json")), "{}").unwrap();
        incident(dir.path(), &waiting);
        // One a dialog shows right now waits too.
        std::fs::create_dir_all(dir.path().join("incidents/claimed")).unwrap();
        let file = dir.path().join("incidents/claimed").join(format!("{}.{}.json", shown.inc, std::process::id()));
        std::fs::write(&file, shown.to_bytes().unwrap()).unwrap();
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(at()).unwrap();
        let date = crate::about::format_date(at());
        let rows: Vec<(String, String, &str)> = reports(dir.path()).into_iter().map(|r| (r.date, r.inc, r.status)).collect();
        let mut waiting_rows = vec![(date.clone(), waiting.inc.clone(), "waiting"), (date.clone(), shown.inc.clone(), "waiting")];
        waiting_rows.sort();
        assert_eq!(&rows[2..], &[
            ("2026-10-03".to_string(), declined.clone(), "declined"),
            ("2026-10-02".to_string(), queued.clone(), "queued"),
            ("2026-10-01".to_string(), sent.clone(), "left the queue"),
        ], "newest first: {rows:?}");
        let mut first: Vec<_> = rows[..2].to_vec();
        first.sort();
        assert_eq!(first, waiting_rows);
        assert_eq!(reports(&dir.path().join("none")), Vec::new(), "no folder, no reports");
    }
}
