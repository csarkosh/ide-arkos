//! The command palette (spec 4: Cmd+Shift+P). Every action by name with its keys, the text
//! presets, the themes, and the config commands, filtered by a subsequence match. The scoring
//! and filtering are pure; `render` is the overlay.
use crate::config::Config;
use crate::keymap::{self, Command, ACTIONS};
use crate::line_edit::{self, LineEdit};
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent};

pub const MAX_ROWS: usize = 12;
const WIDTH: f32 = 560.0;
const TEXT: f32 = 13.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    /// Shown dimmed at the right: the keys, or a note.
    pub detail: String,
    pub command: Command,
}

/// Everything the palette offers, in display order.
pub fn items(cfg: &Config, themes: &[String], current_preset: &str, current_theme: &str) -> Vec<Item> {
    let mut out = Vec::new();
    for a in ACTIONS {
        if a.name == "command palette" || a.name == "quit" {
            continue;
        }
        let command = match keymap::command_for(a.name) {
            Some(c) => c,
            None => continue,
        };
        let detail = keymap::keys_for(a.name, &cfg.keybindings).map(|k| keymap::pretty_keys(&k)).unwrap_or_default();
        out.push(Item { label: keymap::label(a.name), detail, command });
    }
    for (name, p) in &cfg.font.presets {
        let mut detail = format!("{} px, line height {}", p.size, p.line_height);
        if name == current_preset {
            detail.push_str(", current");
        } else if name == "compact" {
            detail.push_str(", below the research's comfort floor");
        }
        out.push(Item { label: format!("Text preset: {name}"), detail, command: Command::TextPreset(name.clone()) });
    }
    for name in themes {
        let detail = if name == current_theme { "current".to_string() } else { "until relaunch; set [theme] in config.toml to keep".to_string() };
        out.push(Item { label: format!("Theme: {name}"), detail, command: Command::SetTheme(name.clone()) });
    }
    out.push(Item { label: "Reload config".into(), detail: String::new(), command: Command::ReloadConfig });
    out.push(Item { label: "Extract built-in themes".into(), detail: "to ~/.config/sarkos/themes".into(), command: Command::ExtractThemes });
    out
}

/// Case-insensitive subsequence match. Each matched char scores 1, plus 2 right after the
/// previous match, plus 1 at a word start; `None` when a query char is not found in order.
pub fn score(query: &str, label: &str) -> Option<i32> {
    let label: Vec<char> = label.to_lowercase().chars().collect();
    let mut total = 0;
    let mut pos = 0usize;
    let mut last: Option<usize> = None;
    for q in query.to_lowercase().chars() {
        let found = (pos..label.len()).find(|&i| label[i] == q)?;
        let word_start = found == 0 || label[found - 1] == ' ' || label[found - 1] == ':';
        let adjacent = found > 0 && last == Some(found - 1);
        total += 1 + if adjacent { 2 } else { 0 } + if word_start { 1 } else { 0 };
        last = Some(found);
        pos = found + 1;
    }
    Some(total)
}

/// Indices of the items that match, best first; ties keep the original order.
pub fn filter(query: &str, items: &[Item]) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = items.iter().enumerate().filter_map(|(i, it)| score(query, &it.label).map(|s| (s, i))).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, i)| i).collect()
}

pub struct Palette {
    pub query: LineEdit,
    /// Index into `visible`.
    pub selected: usize,
    pub items: Vec<Item>,
    pub visible: Vec<usize>,
    /// None only in tests.
    pub focus: Option<FocusHandle>,
}

impl Palette {
    pub fn new(items: Vec<Item>, focus: FocusHandle) -> Self {
        let mut p = Self { query: LineEdit::new(), selected: 0, items, visible: vec![], focus: Some(focus) };
        p.refilter();
        p
    }

    pub fn refilter(&mut self) {
        self.visible = filter(&self.query.text, &self.items);
        self.selected = 0;
    }

    pub fn step(&mut self, delta: isize) {
        let n = self.visible.len().min(MAX_ROWS);
        if n == 0 {
            return;
        }
        let n = n as isize;
        self.selected = (((self.selected as isize + delta) % n + n) % n) as usize;
    }

    pub fn selected_command(&self) -> Option<Command> {
        self.visible.get(self.selected).map(|i| self.items[*i].command.clone())
    }
}

/// The overlay: a query line and up to `MAX_ROWS` rows, centred near the top. A click outside
/// closes it; the keys go through `Root::palette_key`.
pub fn render(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let theme = root.theme.clone();
    let family = root.family();
    let p = root.palette.as_ref()?;
    let focus = p.focus.clone()?;
    let mut rows = div().flex().flex_col().mt(px(6.0));
    for (row, idx) in p.visible.iter().take(MAX_ROWS).enumerate() {
        let item = &p.items[*idx];
        let selected = row == p.selected;
        rows = rows.child(
            div()
                .id(("palette-row", row))
                .flex()
                .flex_row()
                .justify_between()
                .px(px(10.0))
                .py(px(5.0))
                .rounded(px(4.0))
                .bg(if selected { theme.chrome.tab_active.hsla() } else { theme.chrome.banner.hsla() })
                .border_1()
                .border_color(if selected { theme.chrome.focus_ring.hsla() } else { theme.chrome.banner.hsla() })
                .cursor_pointer()
                .on_click(cx.listener(move |root, _ev: &ClickEvent, window, cx| {
                    if let Some(p) = root.palette.as_mut() {
                        p.selected = row;
                    }
                    root.run_palette_selection(window, cx);
                }))
                .child(item.label.clone())
                .child(div().text_color(theme.chrome.text_dim.hsla()).child(item.detail.clone())),
        );
    }
    if p.visible.is_empty() {
        rows = rows.child(div().px(px(10.0)).py(px(5.0)).text_color(theme.chrome.text_dim.hsla()).child("no matching command"));
    }
    let panel = div()
        .id("palette")
        .key_context("Palette")
        .track_focus(&focus)
        .w(px(WIDTH))
        .p(px(10.0))
        .rounded(px(8.0))
        .bg(theme.chrome.banner.hsla())
        .border_1()
        .border_color(theme.chrome.border.hsla())
        .text_color(theme.chrome.text.hsla())
        .font_family(family.clone())
        .text_size(px(TEXT))
        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
        .on_key_down(cx.listener(|root, ev: &KeyDownEvent, window, cx| root.palette_key(&ev.keystroke, window, cx)))
        .child(div().px(px(10.0)).py(px(6.0)).child(line_edit::render(&p.query, "type a command", &theme, family, TEXT)))
        .child(rows);
    Some(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(60.0))
            // Hides the panes beneath from the pointer, so a click that closes the palette does
            // not also land on a tab or a terminal.
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|root, _: &MouseDownEvent, _window, cx| root.close_palette(cx)))
            .child(panel),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn labels(items: &[Item], order: &[usize]) -> Vec<String> {
        order.iter().map(|i| items[*i].label.clone()).collect()
    }

    #[test]
    fn items_cover_actions_presets_themes_and_config() {
        let cfg = Config::default();
        let items = items(&cfg, &["navy".into(), "navy-light".into()], "comfortable", "navy");
        let n_actions = crate::keymap::ACTIONS.len() - 2;
        assert_eq!(items.len(), n_actions + 3 + 2 + 2);
        assert!(!items.iter().any(|i| i.label == "Command palette" || i.label == "Quit"));
        let split = items.iter().find(|i| i.label == "Split right").unwrap();
        assert_eq!(split.detail, "Cmd+\\");
        assert_eq!(split.command, Command::Split(crate::tree::Side::Right));
        let preset = items.iter().find(|i| i.label == "Text preset: comfortable").unwrap();
        assert_eq!(preset.detail, "16 px, line height 1.5, current");
        assert_eq!(preset.command, Command::TextPreset("comfortable".into()));
        let compact = items.iter().find(|i| i.label == "Text preset: compact").unwrap();
        assert_eq!(compact.detail, "14 px, line height 1.3, below the research's comfort floor");
        let theme = items.iter().find(|i| i.label == "Theme: navy-light").unwrap();
        assert_eq!(theme.detail, "until relaunch; set [theme] in config.toml to keep");
        assert_eq!(theme.command, Command::SetTheme("navy-light".into()));
        assert_eq!(items.iter().find(|i| i.label == "Theme: navy").unwrap().detail, "current");
        assert!(items.iter().any(|i| i.command == Command::ReloadConfig));
        assert!(items.iter().any(|i| i.command == Command::ExtractThemes));
        let unbound = cfg_with("split right", "");
        let items = self::items(&unbound, &[], "comfortable", "navy");
        assert_eq!(items.iter().find(|i| i.label == "Split right").unwrap().detail, "");
    }

    fn cfg_with(name: &str, keys: &str) -> Config {
        let mut cfg = Config::default();
        cfg.keybindings.insert(name.into(), keys.into());
        cfg
    }

    #[test]
    fn scoring_prefers_word_starts_and_runs() {
        assert_eq!(score("", "anything"), Some(0));
        assert_eq!(score("x", "anything"), None);
        assert!(score("sr", "Split right").unwrap() > score("sr", "Reset ratio sr").unwrap_or(0));
        assert!(score("split", "Split right").unwrap() > score("split", "Spl it right").unwrap());
        assert!(score("SPLIT", "split right").is_some(), "case does not matter");
        assert_eq!(score("tp", "Text preset: relaxed"), Some(4), "both chars at word starts, not adjacent");
        assert_eq!(score("split", "Split right"), Some(14), "2 for the first char, 3 for each adjacent one");
    }

    #[test]
    fn filter_sorts_by_score_then_keeps_order() {
        let items: Vec<Item> = ["Focus pane 1", "Find", "Font bigger", "Split right"]
            .iter()
            .map(|l| Item { label: l.to_string(), detail: String::new(), command: Command::MruTab })
            .collect();
        assert_eq!(filter("", &items).len(), 4);
        assert_eq!(labels(&items, &filter("f", &items)), vec!["Focus pane 1", "Find", "Font bigger"]);
        assert_eq!(labels(&items, &filter("fb", &items)), vec!["Font bigger"]);
        assert_eq!(labels(&items, &filter("zzz", &items)), Vec::<String>::new());
    }

    #[test]
    fn stepping_wraps_within_the_visible_rows() {
        let items: Vec<Item> = (0..3).map(|i| Item { label: format!("Item {i}"), detail: String::new(), command: Command::MruTab }).collect();
        let mut p = Palette { query: crate::line_edit::LineEdit::new(), selected: 0, items, visible: vec![], focus: None };
        p.refilter();
        assert_eq!(p.visible, vec![0, 1, 2]);
        p.step(-1);
        assert_eq!(p.selected, 2);
        p.step(1);
        assert_eq!(p.selected, 0);
        p.query.set("2");
        p.refilter();
        assert_eq!(p.visible, vec![2]);
        assert_eq!(p.selected, 0, "a new filter starts at the top");
        assert_eq!(p.selected_command(), Some(Command::MruTab));
        p.query.set("nothing");
        p.refilter();
        assert_eq!(p.selected_command(), None);
    }
}
