//! The command palette (spec 4: Cmd+Shift+P). Every action by name with its keys, the text
//! presets, the themes, and the config commands, filtered by a subsequence match. The scoring
//! and filtering are pure; `render` is the overlay.
use crate::config::{Config, StartAtLogin};
use crate::keymap::{self, Command, ACTIONS};
use crate::line_edit::{self, LineEdit};
use crate::motion;
use crate::Root;
use gpui::{div, prelude::*, px, ClickEvent, Context, Div, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent};
use std::path::Path;

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

/// Everything the palette offers, in display order. `themes_dir` is where "Extract built-in
/// themes" writes; `update_ready` is the status bar's "update ready" text, when there is one;
/// `update_error` is whether the last update failed, which offers "Retry update".
/// `show_ignored` is whether the file tree lists ignored entries.
pub fn items(cfg: &Config, themes: &[String], current_preset: &str, current_theme: &str, themes_dir: &Path, update_ready: Option<&str>, update_error: bool, start_at_login: StartAtLogin, login_item_installed: bool, claude_settings: &Path, show_ignored: bool, unread: usize, to_fix: usize) -> Vec<Item> {
    let mut out = Vec::new();
    for a in ACTIONS {
        if a.name == "command palette" || a.name == "quit" {
            continue;
        }
        let command = match keymap::command_for(a.name) {
            Some(c) => c,
            None => continue,
        };
        // Only an update that failed has anything to retry.
        if command == Command::RetryUpdate && !update_error {
            continue;
        }
        let detail = match command {
            Command::UpdateNow => update_ready.unwrap_or("relaunch on the installed binaries").to_string(),
            Command::StartAtLogin => match (start_at_login, login_item_installed) {
                (StartAtLogin::Ask, false) => "off: turn on so sessions come back after a restart".to_string(),
                (StartAtLogin::Ask, true) => "on: run to turn off".to_string(),
                (StartAtLogin::On, _) => "on, set in config.toml ([recovery] start_at_login)".to_string(),
                (StartAtLogin::Off, _) => "off, set in config.toml ([recovery] start_at_login)".to_string(),
            },
            Command::Notifications => if unread == 0 { "nothing new".to_string() } else { format!("{unread} unread") },
            Command::Doctor => if to_fix == 0 { "all good".to_string() } else { format!("{to_fix} to fix") },
            Command::InstallHooks => format!("into {}", claude_settings.display()),
            // The skills folder sits beside the settings file: both follow CLAUDE_CONFIG_DIR.
            Command::InstallAgentGuide => format!("into {}", claude_settings.with_file_name("skills").display()),
            Command::ShowIgnoredFiles => if show_ignored { "on" } else { "off" }.to_string(),
            _ => keymap::keys_for(a.name, &cfg.keybindings).map(|k| keymap::pretty_keys(&k)).unwrap_or_default(),
        };
        // The Keybindings window labels its rows the same way.
        out.push(Item { label: keymap::label_for(a.name), detail, command });
    }
    for (name, p) in &cfg.font.presets {
        let mut detail = format!("{} px, line height {}", p.size, p.line_height);
        if name == "compact" {
            detail.push_str(", below the research's comfort floor");
        }
        if name == current_preset {
            detail.push_str(", current");
        }
        out.push(Item { label: format!("Text preset: {name}"), detail, command: Command::TextPreset(name.clone()) });
    }
    for name in themes {
        let detail = if name == current_theme { "current".to_string() } else { "until the config reloads; set [theme] in config.toml to keep".to_string() };
        out.push(Item { label: format!("Theme: {name}"), detail, command: Command::SetTheme(name.clone()) });
    }
    out.push(Item { label: "Reload config".into(), detail: String::new(), command: Command::ReloadConfig });
    out.push(Item { label: "Extract built-in themes".into(), detail: format!("to {}", themes_dir.display()), command: Command::ExtractThemes });
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
    /// The query line's hint: what the rows are.
    pub placeholder: &'static str,
}

impl Palette {
    pub fn new(items: Vec<Item>, focus: FocusHandle) -> Self {
        Self::list(items, "type a command", focus)
    }

    /// The palette over a list of choices of one kind (a list a command offers), each row
    /// running its own command; filtered and picked like the commands.
    pub fn list(items: Vec<Item>, placeholder: &'static str, focus: FocusHandle) -> Self {
        let mut p = Self { query: LineEdit::new(), selected: 0, items, visible: vec![], focus: Some(focus), placeholder };
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
/// closes it; the keys go through `Root::palette_key`. While it fades out it is only drawn: no
/// focus, no keys, no clicks, nothing hidden from the pointer.
pub fn render(root: &mut Root, cx: &mut Context<Root>) -> Option<Div> {
    let theme = root.theme.clone();
    let family = root.ui_family();
    let (p, opacity) = root.palette.current(root.frame_now)?;
    let live = root.palette.value().is_some();
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
                .when(live, |d| {
                    d.cursor_pointer().on_click(cx.listener(move |root, _ev: &ClickEvent, window, cx| {
                        if let Some(p) = root.palette.value_mut() {
                            p.selected = row;
                        }
                        root.run_palette_selection(window, cx);
                    }))
                })
                .child(item.label.clone())
                .child(div().text_color(theme.chrome.text_dim.hsla()).child(item.detail.clone())),
        );
    }
    if p.visible.is_empty() {
        rows = rows.child(div().px(px(10.0)).py(px(5.0)).text_color(theme.chrome.text_dim.hsla()).child("nothing matches"));
    }
    let panel = div()
        .id("palette")
        .key_context("Palette")
        .when(live, |d| d.track_focus(&focus))
        .relative()
        .top(px(-motion::lift(opacity, motion::PANEL_LIFT)))
        .opacity(opacity)
        .w(px(WIDTH))
        .p(px(10.0))
        .rounded(px(8.0))
        .bg(theme.chrome.banner.hsla())
        .border_1()
        .border_color(theme.chrome.border.hsla())
        .text_color(theme.chrome.text.hsla())
        .font_family(family.clone())
        .text_size(px(TEXT))
        .when(live, |d| {
            d.on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _window, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|root, ev: &KeyDownEvent, window, cx| root.palette_key(&ev.keystroke, window, cx)))
        })
        .child(div().px(px(10.0)).py(px(6.0)).child(line_edit::render(&p.query, p.placeholder, &theme, family, TEXT)))
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
            .when(live, |d| d.occlude().on_mouse_down(MouseButton::Left, cx.listener(|root, _: &MouseDownEvent, _window, cx| root.close_palette(cx))))
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
        let dir = std::path::Path::new("/u/.config/arkos/themes");
        let settings = Path::new("/Users/me/.claude/settings.json");
        let items = items(&cfg, &["navy".into(), "navy-light".into()], "comfortable", "navy", dir, None, false, StartAtLogin::Ask, false, settings, false, 0, 0);
        // The palette and quit are not offered, nor "Retry update" with no update error.
        let n_actions = crate::keymap::ACTIONS.len() - 3;
        assert_eq!(items.len(), n_actions + 3 + 2 + 2);
        assert_eq!(items.iter().find(|i| i.label == "Check for updates").unwrap().command, Command::CheckForUpdates);
        assert!(!items.iter().any(|i| i.command == Command::RetryUpdate));
        let failed = self::items(&cfg, &[], "comfortable", "navy", dir, None, true, StartAtLogin::Ask, false, settings, false, 0, 0);
        assert_eq!(failed.iter().find(|i| i.label == "Retry update").unwrap().command, Command::RetryUpdate);
        assert!(!items.iter().any(|i| i.label == "Command palette" || i.label == "Quit"));
        let update = items.iter().find(|i| i.label == "Update now").unwrap();
        assert_eq!(update.command, Command::UpdateNow);
        assert_eq!(update.detail, "relaunch on the installed binaries");
        let ready = self::items(&cfg, &[], "comfortable", "navy", dir, Some("update ready: 0.2.0 (def5678)"), false, StartAtLogin::Ask, false, settings, false, 0, 0);
        assert_eq!(ready.iter().find(|i| i.label == "Update now").unwrap().detail, "update ready: 0.2.0 (def5678)");
        let split = items.iter().find(|i| i.label == "Split right").unwrap();
        assert_eq!(split.detail, "Cmd+\\");
        assert_eq!(split.command, Command::Split(crate::tree::Side::Right));
        let preset = items.iter().find(|i| i.label == "Text preset: comfortable").unwrap();
        assert_eq!(preset.detail, "16 px, line height 1.5, current");
        assert_eq!(preset.command, Command::TextPreset("comfortable".into()));
        let compact = items.iter().find(|i| i.label == "Text preset: compact").unwrap();
        assert_eq!(compact.detail, "14 px, line height 1.3, below the research's comfort floor");
        let theme = items.iter().find(|i| i.label == "Theme: navy-light").unwrap();
        assert_eq!(theme.detail, "until the config reloads; set [theme] in config.toml to keep");
        assert_eq!(theme.command, Command::SetTheme("navy-light".into()));
        assert_eq!(items.iter().find(|i| i.label == "Theme: navy").unwrap().detail, "current");
        assert!(items.iter().any(|i| i.command == Command::ReloadConfig));
        let extract = items.iter().find(|i| i.command == Command::ExtractThemes).unwrap();
        assert_eq!(extract.detail, "to /u/.config/arkos/themes");
        let items = self::items(&cfg, &[], "compact", "navy", dir, None, false, StartAtLogin::Ask, false, settings, false, 0, 0);
        let compact = items.iter().find(|i| i.label == "Text preset: compact").unwrap();
        assert_eq!(compact.detail, "14 px, line height 1.3, below the research's comfort floor, current", "the floor note stays");
        let sidebar = items.iter().find(|i| i.label == "Toggle sidebar").unwrap();
        assert_eq!((sidebar.detail.as_str(), &sidebar.command), ("Cmd+B", &Command::ToggleSidebar));
        let tree = items.iter().find(|i| i.label == "Focus file tree").unwrap();
        assert_eq!((tree.detail.as_str(), &tree.command), ("Shift+Cmd+E", &Command::FocusFileTree));
        let open = items.iter().find(|i| i.label == "Open folder\u{2026}").unwrap();
        assert_eq!((open.detail.as_str(), &open.command), ("Cmd+O", &Command::OpenFolder));
        let about = items.iter().find(|i| i.label == "About Arkos IDE").unwrap();
        assert_eq!((about.detail.as_str(), &about.command), ("", &Command::About));
        let keys = items.iter().find(|i| i.label == "Keybindings").unwrap();
        assert_eq!((keys.detail.as_str(), &keys.command), ("Cmd+/", &Command::Keybindings));
        let docs = items.iter().find(|i| i.label == "Documentation").unwrap();
        assert_eq!((docs.detail.as_str(), &docs.command), ("", &Command::Documentation));
        let prefs = items.iter().find(|i| i.label == "Settings").unwrap();
        assert_eq!((prefs.detail.as_str(), &prefs.command), ("Cmd+,", &Command::OpenSettings));
        let config = items.iter().find(|i| i.label == "Open config").unwrap();
        assert_eq!((config.detail.as_str(), &config.command), ("", &Command::OpenConfig), "Cmd+, is Settings now");
        let window = items.iter().find(|i| i.label == "New window").unwrap();
        assert_eq!((window.detail.as_str(), &window.command), ("Cmd+N", &Command::NewWindow));
        let browser = items.iter().find(|i| i.label == "New browser tab\u{2026}").unwrap();
        assert_eq!((browser.detail.as_str(), &browser.command), ("Shift+Cmd+B", &Command::NewBrowserTab));
        assert!(items.iter().all(|i| !i.label.contains("Chrome")), "the docking commands are gone");
        let markdown = items.iter().find(|i| i.label == "Toggle markdown view").unwrap();
        assert_eq!((markdown.detail.as_str(), &markdown.command), ("Shift+Cmd+M", &Command::ToggleMarkdownView));
        let save = items.iter().find(|i| i.label == "Save file").unwrap();
        assert_eq!((save.detail.as_str(), &save.command), ("Cmd+S", &Command::SaveFile));
        let open_file = items.iter().find(|i| i.label == "Open file in pane\u{2026}").unwrap();
        assert_eq!((open_file.detail.as_str(), &open_file.command), ("", &Command::OpenFileInPane), "no key: the palette asks for a path");
        for a in crate::keymap::ACTIONS.iter().filter(|a| a.name.starts_with("video ")) {
            let label = crate::keymap::label_for(a.name);
            assert!(label.starts_with("Video: "), "{label}");
            let item = items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label}"));
            assert_eq!(Some(item.command.clone()), crate::keymap::command_for(a.name), "{label}");
        }
        let play = items.iter().find(|i| i.label == "Video: play or pause").unwrap();
        assert_eq!((play.detail.as_str(), &play.command), ("Space", &Command::VideoPlayPause));
        let ignored = items.iter().find(|i| i.label == "Show ignored files").unwrap();
        assert_eq!((ignored.detail.as_str(), &ignored.command), ("off", &Command::ShowIgnoredFiles));
        let shown = self::items(&cfg, &[], "comfortable", "navy", dir, None, false, StartAtLogin::Ask, false, settings, true, 0, 0);
        assert_eq!(shown.iter().find(|i| i.label == "Show ignored files").unwrap().detail, "on");
        let unbound = cfg_with("split right", "");
        let items = self::items(&unbound, &[], "comfortable", "navy", dir, None, false, StartAtLogin::Ask, false, settings, false, 0, 0);
        assert_eq!(items.iter().find(|i| i.label == "Split right").unwrap().detail, "");
    }

    #[test]
    fn the_recovery_items_say_what_running_them_does() {
        let cfg = Config::default();
        let settings = Path::new("/Users/me/.claude/settings.json");
        let find = |items: &[Item], label: &str| items.iter().find(|i| i.label == label).cloned().unwrap_or_else(|| panic!("no {label}"));
        let ask_off = items(&cfg, &[], "comfortable", "navy", Path::new("/t"), None, false, StartAtLogin::Ask, false, settings, false, 2, 1);
        let quiet = items(&cfg, &[], "comfortable", "navy", Path::new("/t"), None, false, StartAtLogin::Ask, false, settings, false, 0, 0);
        assert_eq!(find(&ask_off, "Start at login").detail, "off: turn on so sessions come back after a restart");
        assert_eq!(find(&ask_off, "Start at login").command, Command::StartAtLogin);
        let ask_on = items(&cfg, &[], "comfortable", "navy", Path::new("/t"), None, false, StartAtLogin::Ask, true, settings, false, 0, 0);
        assert_eq!(find(&ask_on, "Start at login").detail, "on: run to turn off");
        let pinned = items(&cfg, &[], "comfortable", "navy", Path::new("/t"), None, false, StartAtLogin::On, true, settings, false, 0, 0);
        assert_eq!(find(&pinned, "Start at login").detail, "on, set in config.toml ([recovery] start_at_login)");
        let off = items(&cfg, &[], "comfortable", "navy", Path::new("/t"), None, false, StartAtLogin::Off, false, settings, false, 0, 0);
        assert_eq!(find(&off, "Start at login").detail, "off, set in config.toml ([recovery] start_at_login)");
        let hooks = find(&ask_off, "Install Claude Code hooks");
        assert_eq!(hooks.detail, "into /Users/me/.claude/settings.json");
        assert_eq!(hooks.command, Command::InstallHooks);
        let guide = find(&ask_off, "Install agent guide");
        assert_eq!((guide.detail.as_str(), guide.command), ("into /Users/me/.claude/skills", Command::InstallAgentGuide));
        let bell = find(&ask_off, "Notifications");
        assert_eq!((bell.detail.as_str(), bell.command), ("2 unread", Command::Notifications));
        let doc = find(&ask_off, "Doctor");
        assert_eq!((doc.detail.as_str(), doc.command), ("1 to fix", Command::Doctor));
        assert_eq!(find(&quiet, "Notifications").detail, "nothing new");
        assert_eq!(find(&quiet, "Doctor").detail, "all good");
        let labels: Vec<&str> = ask_off.iter().map(|i| i.label.as_str()).collect();
        let check = labels.iter().position(|l| *l == "Check for updates").unwrap();
        assert_eq!(&labels[check + 1..check + 3], &["Notifications", "Doctor"]);
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
        let mut p = Palette { query: crate::line_edit::LineEdit::new(), selected: 0, items, visible: vec![], focus: None, placeholder: "type a command" };
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
