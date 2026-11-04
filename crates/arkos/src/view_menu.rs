//! A markdown tab's view toggles behind one `⋯` button at its top right (spec
//! `2026-11-04-outline-rail-and-worktrees-sizing-design.md`, section 5): a click opens a small
//! menu under it listing the raw or rendered view, the inline diff, presentation and the outline
//! rail, each with its shortcut and a check mark when it is on. The menu is drawn like the app's
//! other menus and dismissed like the document's right-click menu (`md_menu`): a press anywhere
//! else, Escape, the tab closing, or the button again.

use crate::motion;
use crate::tree::{PaneId, TabId, Tree, ViewMode};
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, MouseButton, MouseDownEvent, Stateful};
use std::time::Instant;

/// The button's width.
pub const BUTTON: f32 = 26.0;
/// The button's height.
pub const BUTTON_H: f32 = 22.0;
/// The button's and the menu's distance from the tab's right edge.
pub const RIGHT: f32 = 10.0;
/// The menu's key among the menus an effect draws (`chrome::menu_report`).
const MENU_KEY: u64 = 0x16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewAction {
    ToggleMode,
    ToggleInlineDiff,
    TogglePresent,
    ToggleOutline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    pub label: &'static str,
    pub keys: &'static str,
    pub checked: bool,
    pub action: ViewAction,
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

/// Whether an open view menu on `tab` may stay open: its tab is the focused pane's active one,
/// the only place it is drawn. A tab switch or a focus move elsewhere closes it, so it never
/// holds Escape or an update back while unseen.
pub fn stays_open(tab: TabId, tree: &Tree) -> bool {
    tree.pane(tree.focused).is_some_and(|p| p.ids.get(p.active) == Some(&tab))
}

/// The menu's rows for a tab's state: the mode item only when `editable`, the diff item only with
/// `has_changes`, present always, outline only when the document `has_heads` (where the rail
/// shows); the words name what a click turns to.
#[allow(clippy::too_many_arguments)]
pub fn items(mode: ViewMode, delimited: bool, editable: bool, has_changes: bool, inline: bool, presenting: bool, outline: bool, has_heads: bool) -> Vec<MenuItem> {
    let mut m = Vec::new();
    if editable {
        let label = match pill_label_for(mode, delimited) {
            "show raw" => "Show raw",
            "show grid" => "Show grid",
            _ => "Show rendered",
        };
        m.push(MenuItem { label, keys: "⇧⌘M", checked: false, action: ViewAction::ToggleMode });
    }
    if has_changes {
        m.push(MenuItem { label: "Inline diff", keys: "⇧⌘D", checked: inline, action: ViewAction::ToggleInlineDiff });
    }
    m.push(MenuItem { label: if presenting { "Edit" } else { "Present" }, keys: "⌥⌘P", checked: presenting, action: ViewAction::TogglePresent });
    if has_heads {
        m.push(MenuItem { label: "Outline", keys: "⌥⌘O", checked: outline, action: ViewAction::ToggleOutline });
    }
    m
}

/// The button's top: under the find bar when it is open.
pub fn button_top(find_open: bool) -> f32 {
    if find_open {
        44.0
    } else {
        10.0
    }
}

/// Runs a row's toggle through the same paths as its key, so saving and telemetry are the same.
fn run(action: &ViewAction, tab: TabId, root: &mut Root, cx: &mut Context<Root>) {
    match action {
        ViewAction::ToggleMode => root.toggle_markdown_view_for(tab, cx),
        ViewAction::TogglePresent => root.toggle_presentation_for(tab, cx),
        ViewAction::ToggleOutline => root.toggle_outline_for(tab, cx),
        ViewAction::ToggleInlineDiff => {
            if let Some(v) = root.viewers.get(&tab).cloned() {
                v.update(cx, |v, cx| v.toggle_inline_diff(cx));
            }
        }
    }
}

/// The `⋯` button at a markdown tab's top right, a translucent pill like the terminal's "jump to
/// bottom", tinted while its menu is open or under the pointer. A press focuses the pane and
/// closes the other menus (it stops the window's press, which would close this one too); a click
/// opens the menu, or closes it when it is open.
pub fn render_view_button(root: &Root, pane: PaneId, tab: TabId, find_open: bool, bg: gpui::Hsla, border: gpui::Hsla, cx: &mut Context<Root>) -> Stateful<Div> {
    let c = &root.theme.chrome;
    let tint = c.focus_ring.alpha(crate::outline_rail::HOVER_ALPHA);
    let open = root.view_menu.value() == Some(&tab);
    div()
        .id(("view-menu-button", tab))
        .absolute()
        .top(px(button_top(find_open)))
        .right(px(RIGHT))
        .w(px(BUTTON))
        .h(px(BUTTON_H))
        .rounded(px(9.0))
        .bg(bg)
        .border_1()
        .border_color(border)
        .overflow_hidden()
        .cursor_pointer()
        .child(
            div()
                .id(("view-menu-dots", tab))
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .when(open, |d| d.bg(tint))
                .hover(move |s| s.bg(tint))
                .text_color(c.text_dim.hsla())
                .font_family(root.ui_family())
                .text_size(px(root.ui_size(13.0)))
                .child("⋯"),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |root, _ev: &MouseDownEvent, _window, cx| {
                let now = Instant::now();
                root.last_input = now;
                root.context_menu.hide(now);
                root.wt_menu.hide(now);
                root.link_menu.hide(now);
                root.md_menu.hide(now);
                root.close_popovers(now);
                root.focus_pane(pane, cx);
                cx.stop_propagation();
            }),
        )
        .on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
            let now = Instant::now();
            if root.view_menu.value() == Some(&tab) {
                root.view_menu.hide(now);
            } else {
                root.view_menu.show(tab, now);
            }
            cx.notify();
        }))
}

/// The open menu under the button, right-aligned to it, rising into place as the app's other
/// menus do; drawn while it fades out too, when only the live menu takes clicks. A row runs its
/// toggle and closes the menu.
pub fn render_view_menu(root: &mut Root, tab: TabId, items: &[MenuItem], find_open: bool, cx: &mut Context<Root>) -> Option<Div> {
    let (&shown, opacity) = root.view_menu.current(root.frame_now)?;
    if shown != tab {
        return None;
    }
    let live = root.view_menu.value() == Some(&tab);
    let style = root.effect_style();
    let c = &root.theme.chrome;
    let (text, dim, check) = (c.text.hsla(), c.text_dim.hsla(), c.focus_ring.hsla());
    let top = button_top(find_open) + BUTTON_H + 4.0;
    let mut m = div()
        .absolute()
        .right(px(RIGHT))
        .top(px(top - motion::lift(opacity, motion::MENU_LIFT)))
        .opacity(opacity)
        .min_w(px(190.0))
        .flex()
        .flex_col()
        .py(px(4.0))
        .rounded(px(8.0))
        .map(|d| crate::chrome::menu_frame(d, &root.theme, &style))
        .font_family(root.ui_family())
        .text_size(px(root.ui_size(12.0)))
        .text_color(text)
        .when(live, |m| m.occlude().on_any_mouse_down(|_: &MouseDownEvent, _window, cx| cx.stop_propagation()));
    let hover = crate::chrome::menu_colors(&root.theme, style.menus).hover;
    let (effect_rows, hovered) = (style.menu_edges && style.highlights, root.effect_hover);
    if style.menu_edges {
        m = m.child(crate::chrome::menu_report(MENU_KEY, opacity, cx));
    }
    let keys_family = root.family();
    let keys_size = px(root.ui_size(11.0));
    for (i, item) in items.iter().enumerate() {
        let action = item.action.clone();
        let row = div()
            .id(("view-menu-row", i))
            .flex()
            .flex_row()
            .items_center()
            .pl(px(8.0))
            .pr(px(12.0))
            .py(px(4.0))
            .child(div().flex_none().w(px(16.0)).text_color(check).children(item.checked.then_some("✓")))
            .child(div().flex_1().child(item.label))
            .child(div().flex_none().ml(px(18.0)).text_color(dim).font_family(keys_family.clone()).text_size(keys_size).child(item.keys));
        m = m.child(row.when(live, |row| {
            let key = crate::effects::blocks::menu_row_key(MENU_KEY, i);
            crate::chrome::menu_row_hover(row.cursor_pointer(), key, hovered == Some(key), effect_rows, hover, cx).on_click(cx.listener(move |root, _ev: &ClickEvent, _window, cx| {
                root.view_menu.hide(Instant::now());
                run(&action, tab, root, cx);
                cx.notify();
            }))
        }));
    }
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_names_what_a_click_turns_to_with_its_keys() {
        let m = items(ViewMode::Rendered, false, true, true, false, false, false, true);
        assert_eq!(
            m.iter().map(|i| (i.label, i.keys, i.checked)).collect::<Vec<_>>(),
            vec![("Show raw", "⇧⌘M", false), ("Inline diff", "⇧⌘D", false), ("Present", "⌥⌘P", false), ("Outline", "⌥⌘O", false)]
        );
        let m = items(ViewMode::Raw, false, true, true, true, false, true, true);
        assert_eq!(m[0].label, "Show rendered");
        assert!(m[1].checked, "the inline diff is on");
        assert!(m[3].checked, "the outline is open");
        assert_eq!(items(ViewMode::Raw, true, true, false, false, false, false, true)[0].label, "Show grid");
        assert_eq!(items(ViewMode::Rendered, false, true, false, false, true, false, true)[1].label, "Edit", "presenting, without changes Present is second and says Edit");
    }

    #[test]
    fn presenting_checks_present_and_each_row_runs_its_own_toggle() {
        let m = items(ViewMode::Rendered, false, true, true, false, true, false, true);
        let present = m.iter().find(|i| i.action == ViewAction::TogglePresent).unwrap();
        assert!(present.checked);
        assert_eq!(m.iter().map(|i| i.action.clone()).collect::<Vec<_>>(), vec![ViewAction::ToggleMode, ViewAction::ToggleInlineDiff, ViewAction::TogglePresent, ViewAction::ToggleOutline]);
    }

    #[test]
    fn a_read_only_tab_s_menu_has_no_mode_item() {
        let m = items(ViewMode::Rendered, false, false, false, false, false, false, true);
        assert_eq!(m.iter().map(|i| i.label).collect::<Vec<_>>(), vec!["Present", "Outline"]);
        let m = items(ViewMode::Rendered, false, false, true, false, false, false, true);
        assert_eq!(m[0].label, "Inline diff", "with changes the diff item comes first");
    }

    #[test]
    fn a_document_without_headings_has_no_outline_row() {
        let m = items(ViewMode::Rendered, false, true, false, false, false, true, false);
        assert_eq!(m.iter().map(|i| i.label).collect::<Vec<_>>(), vec!["Show raw", "Present"]);
    }

    #[test]
    fn the_menu_stays_open_only_while_its_tab_is_the_focused_pane_s_active_one() {
        use crate::tree::{Side, Tab};
        let viewer = |p: &str| Tab::Viewer { path: p.into(), mode: ViewMode::default(), presenting: false, outline: false };
        let mut tree = Tree::new(viewer("/p/a.md"));
        let left = tree.focused;
        let a = tree.tab_id(left, 0).unwrap();
        assert!(stays_open(a, &tree));
        let idx = tree.add_tab(left, viewer("/p/b.md"));
        tree.activate(left, idx);
        assert!(!stays_open(a, &tree), "a tab switch closes it");
        tree.activate(left, 0);
        let right = tree.split(left, Side::Right, viewer("/p/c.md")).unwrap();
        tree.focus(right);
        assert!(!stays_open(a, &tree), "a focus move to another pane closes it");
        tree.focus(left);
        assert!(stays_open(a, &tree));
    }

    #[test]
    fn the_button_sits_under_an_open_find_bar() {
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
