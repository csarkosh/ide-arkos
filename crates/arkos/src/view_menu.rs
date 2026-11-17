//! A markdown tab's view: three icon buttons in one group at its top right, Present, Edit and
//! Raw (markdown view group spec, section 1). The one in force is lit; a click on another
//! switches to it through the same toggles the keys run (Option+Cmd+P, Shift+Cmd+M), and a
//! button held under the pointer shows a tooltip saying what it does. The inline diff has its own
//! chip beside the group and the outline its ☰ on the rail, so nothing else hides here.

use crate::tree::{PaneId, TabId, ViewMode};
use crate::Root;
use gpui::{div, prelude::*, px, svg, AnyView, ClickEvent, Context, Div, Hsla, MouseButton, MouseDownEvent, SharedString, Stateful, Window};
use std::time::Instant;

/// One button's width.
pub const SEGMENT: f32 = 28.0;
/// The group's height.
pub const BUTTON_H: f32 = 22.0;
/// The group's width: three buttons.
pub const GROUP_W: f32 = 3.0 * SEGMENT;
/// The group's distance from the tab's right edge.
pub const RIGHT: f32 = 10.0;

/// How a markdown tab shows its document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    /// Rendered, read without editing: no caret, a click follows a link.
    Present,
    /// Rendered, edited where it reads.
    Edit,
    /// The file's own text.
    Raw,
}

/// One of the group's buttons: what it shows, its icon, and its tooltip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub kind: ViewKind,
    pub icon: &'static str,
    pub title: &'static str,
    pub tip: &'static str,
    pub keys: &'static str,
}

/// The view a tab is in.
pub fn current(mode: ViewMode, presenting: bool) -> ViewKind {
    match (mode, presenting) {
        (ViewMode::Raw, _) => ViewKind::Raw,
        (ViewMode::Rendered, true) => ViewKind::Present,
        (ViewMode::Rendered, false) => ViewKind::Edit,
    }
}

/// The group's three buttons in order; a document that cannot be written reads rather than edits
/// in the middle one.
pub fn segments(editable: bool) -> [Segment; 3] {
    [
        Segment { kind: ViewKind::Present, icon: "icons/view-present.svg", title: "Present", tip: "Read the document without editing: no caret, and a click follows a link.", keys: "⌥⌘P" },
        if editable {
            Segment { kind: ViewKind::Edit, icon: "icons/view-edit.svg", title: "Edit", tip: "Edit the document as it reads: the block under the caret shows its markdown.", keys: "⇧⌘M" }
        } else {
            Segment { kind: ViewKind::Edit, icon: "icons/view-edit.svg", title: "Read", tip: "The document as it reads. This file cannot be written.", keys: "⇧⌘M" }
        },
        Segment { kind: ViewKind::Raw, icon: "icons/view-raw.svg", title: "Raw", tip: "The file's own text, markdown and all.", keys: "⇧⌘M" },
    ]
}

/// A toggle a switch runs, as its key does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    /// Rendered and raw (Shift+Cmd+M); to raw also ends presenting.
    Mode,
    /// Presenting on and off (Option+Cmd+P); from raw it renders too.
    Present,
}

/// The toggle that takes a tab from `from` to `to`: none when it is there already.
pub fn step(from: ViewKind, to: ViewKind) -> Option<Toggle> {
    if from == to {
        return None;
    }
    Some(match to {
        ViewKind::Present => Toggle::Present,
        ViewKind::Raw => Toggle::Mode,
        ViewKind::Edit if from == ViewKind::Present => Toggle::Present,
        ViewKind::Edit => Toggle::Mode,
    })
}

/// The group's top: under the find bar when it is open.
pub fn button_top(find_open: bool) -> f32 {
    if find_open {
        44.0
    } else {
        10.0
    }
}

/// A button's tooltip: its name and keys, and what it does.
struct Tip {
    title: SharedString,
    keys: SharedString,
    text: SharedString,
    fg: Hsla,
    dim: Hsla,
    bg: Hsla,
    border: Hsla,
    family: SharedString,
    size: f32,
}

impl gpui::Render for Tip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // A set width: gpui sizes a tooltip from its content unwrapped, so a wrapped line would
        // run out of its box.
        div()
            .w(px(240.0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .bg(self.bg)
            .border_1()
            .border_color(self.border)
            .text_color(self.fg)
            .font_family(self.family.clone())
            .text_size(px(self.size))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(div().flex().gap(px(8.0)).child(div().font_weight(gpui::FontWeight::BOLD).child(self.title.clone())).child(div().text_color(self.dim).child(self.keys.clone())))
            .child(div().text_color(self.dim).child(self.text.clone()))
    }
}

/// The group at a markdown tab's top right, a translucent pill like the terminal's "jump to
/// bottom": the view in force lit in the accent, the others tinted under the pointer. A press
/// focuses the pane and closes the open menus (it stops the window's press); a click switches.
#[allow(clippy::too_many_arguments)]
pub fn render_view_group(root: &Root, pane: PaneId, tab: TabId, now: ViewKind, editable: bool, find_open: bool, bg: Hsla, border: Hsla, cx: &mut Context<Root>) -> Stateful<Div> {
    let c = &root.theme.chrome;
    let tint = c.focus_ring.alpha(crate::outline_rail::HOVER_ALPHA);
    let lit = c.focus_ring.alpha(0.28);
    let (fg, dim) = (c.text.hsla(), c.text_dim.hsla());
    let (tip_bg, tip_border, family, size) = (c.banner.hsla(), c.border.hsla(), root.ui_family(), root.ui_size(12.0));
    let icon = px(root.ui_size(14.0)).round();
    let mut group = div()
        .id(("view-group", tab))
        .absolute()
        .top(px(button_top(find_open)))
        .right(px(RIGHT))
        .w(px(GROUP_W))
        .h(px(BUTTON_H))
        .flex()
        .rounded(px(9.0))
        .bg(bg)
        .border_1()
        .border_color(border)
        .overflow_hidden()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                let now = Instant::now();
                root.last_input = now;
                root.context_menu.hide(now);
                root.wt_menu.hide(now);
                root.session_menu.hide(now);
                root.link_menu.hide(now);
                root.new_tab_menu.hide(now);
                root.md_menu.hide(now);
                root.close_popovers(now);
                root.focus_pane(pane, cx);
                cx.stop_propagation();
            }),
        );
    for (i, seg) in segments(editable).into_iter().enumerate() {
        let on = seg.kind == now;
        let kind = seg.kind;
        let (title, keys, text) = (SharedString::from(seg.title), SharedString::from(seg.keys), SharedString::from(seg.tip));
        let family = family.clone();
        group = group.child(
            div()
                .id(("view-group-button", tab * 4 + i as u64))
                .w(px(SEGMENT))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .when(i > 0, |d| d.border_l_1().border_color(border))
                .when(on, |d| d.bg(lit))
                .when(!on, |d| d.cursor_pointer().hover(move |s| s.bg(tint)))
                .child(svg().path(seg.icon).size(icon).flex_none().text_color(if on { fg } else { dim }))
                .tooltip(move |_window, cx| {
                    let tip = Tip { title: title.clone(), keys: keys.clone(), text: text.clone(), fg, dim, bg: tip_bg, border: tip_border, family: family.clone(), size };
                    AnyView::from(cx.new(|_| tip))
                })
                .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                    select(kind, tab, root, cx);
                    cx.notify();
                })),
        );
    }
    group
}

/// Switches `tab` to `to` through the toggles its keys run, so saving and telemetry are theirs.
fn select(to: ViewKind, tab: TabId, root: &mut Root, cx: &mut Context<Root>) {
    let Some((pane, idx)) = root.tree.locate(tab) else { return };
    let Some(crate::tree::Tab::Viewer { mode, presenting, .. }) = root.tree.pane(pane).and_then(|p| p.tabs.get(idx)).cloned() else { return };
    match step(current(mode, presenting), to) {
        Some(Toggle::Mode) => root.toggle_markdown_view_for(tab, cx),
        Some(Toggle::Present) => root.toggle_presentation_for(tab, cx),
        None => {}
    }
}

/// What a double click on `path` in `tab` leaves zoomed (markdown view group spec, section 2):
/// that image, unless it is the one zoomed already, which the double click puts back.
pub fn zoom_after(zoomed: Option<&(TabId, std::path::PathBuf)>, tab: TabId, path: std::path::PathBuf) -> Option<(TabId, std::path::PathBuf)> {
    match zoomed {
        Some((t, p)) if *t == tab && *p == path => None,
        _ => Some((tab, path)),
    }
}

/// Whether `tab` is the focused pane's active one, where a zoomed image is drawn: elsewhere the
/// zoom ends, so it never holds Escape while unseen.
pub fn in_front(tab: TabId, tree: &crate::tree::Tree) -> bool {
    tree.pane(tree.focused).is_some_and(|p| p.ids.get(p.active) == Some(&tab))
}

/// The grid strip's mode pill text: it names the mode a click switches to, the grid for a
/// delimited file.
pub fn pill_label_for(mode: ViewMode, delimited: bool) -> &'static str {
    match mode {
        ViewMode::Rendered => "show raw",
        ViewMode::Raw if delimited => "show grid",
        ViewMode::Raw => "show rendered",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_in_force_is_read_from_the_mode_and_presenting() {
        assert_eq!(current(ViewMode::Rendered, true), ViewKind::Present);
        assert_eq!(current(ViewMode::Rendered, false), ViewKind::Edit);
        assert_eq!(current(ViewMode::Raw, false), ViewKind::Raw);
        assert_eq!(current(ViewMode::Raw, true), ViewKind::Raw, "raw wins: only the rendered view presents");
    }

    #[test]
    fn each_switch_runs_the_one_toggle_its_key_would() {
        use ViewKind::*;
        assert_eq!(step(Edit, Present), Some(Toggle::Present));
        assert_eq!(step(Present, Edit), Some(Toggle::Present));
        assert_eq!(step(Edit, Raw), Some(Toggle::Mode));
        assert_eq!(step(Present, Raw), Some(Toggle::Mode), "to raw ends presenting");
        assert_eq!(step(Raw, Edit), Some(Toggle::Mode));
        assert_eq!(step(Raw, Present), Some(Toggle::Present), "presenting from raw renders too");
        for k in [Present, Edit, Raw] {
            assert_eq!(step(k, k), None);
        }
    }

    #[test]
    fn the_group_is_present_edit_raw_each_with_a_tip_and_a_read_only_file_reads() {
        let s = segments(true);
        assert_eq!(s.iter().map(|s| (s.kind, s.title)).collect::<Vec<_>>(), vec![(ViewKind::Present, "Present"), (ViewKind::Edit, "Edit"), (ViewKind::Raw, "Raw")]);
        assert!(s.iter().all(|s| !s.tip.is_empty() && s.icon.starts_with("icons/view-")));
        assert_eq!(segments(false)[1].title, "Read");
    }

    #[test]
    fn a_double_click_zooms_an_image_and_another_on_the_same_one_puts_it_back() {
        let a = std::path::PathBuf::from("/p/a.png");
        let b = std::path::PathBuf::from("/p/b.png");
        assert_eq!(zoom_after(None, 3, a.clone()), Some((3, a.clone())));
        assert_eq!(zoom_after(Some(&(3, a.clone())), 3, a.clone()), None);
        assert_eq!(zoom_after(Some(&(3, a.clone())), 3, b.clone()), Some((3, b.clone())), "another image takes its place");
        assert_eq!(zoom_after(Some(&(3, a.clone())), 4, a.clone()), Some((4, a)), "the same file in another tab");
    }

    #[test]
    fn the_group_sits_under_an_open_find_bar() {
        assert_eq!(button_top(true), 44.0);
        assert_eq!(button_top(false), 10.0);
    }

    #[test]
    fn the_grid_strip_s_pill_names_the_other_mode() {
        assert_eq!(pill_label_for(ViewMode::Rendered, false), "show raw");
        assert_eq!(pill_label_for(ViewMode::Raw, false), "show rendered");
        assert_eq!(pill_label_for(ViewMode::Raw, true), "show grid");
    }
}
