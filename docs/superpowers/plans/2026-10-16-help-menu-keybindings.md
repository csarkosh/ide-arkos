# Help menu and the Keybindings window

Status: approved by Cyrus in chat on 2026-10-16 ("yes, cmd-/ and add the Documentation item") after an in-chat design; a bounded follow-up to milestone 8a (spec `docs/superpowers/specs/2026-10-15-menubar-about-settings-design.md`, whose window pattern and registry this reuses). Ships as 0.3.2.

Goal: a second menu, "Help", with "Keybindings" (Cmd+/) and "Documentation"; a read-only Keybindings window that lists every action with its current key, grouped under sections by function so a binding is easy to find.

## Global Constraints

- Machine rules: every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's `.cargo/config.toml` stays; no `--release`; never two cargo processes at once; focused filters while iterating, the workspace suite once per task; `cargo check --workspace --all-targets` prints no warning from our crates, in the test build too. Known timing flakes under load: `concurrent_spawns_give_each_shell_only_its_own_terminal`, `the_grace_counts_from_the_relaunch_so_a_slow_relaunch_keeps_the_old_bundle`, `recovery::tests::a_death_rewrites_the_manifest_without_the_dirty_mark`; rerun alone and report both results.
- Dead code between tasks: an item Task 2 first uses, reached by a test in Task 1, gets `#[cfg_attr(not(test), allow(dead_code))] // Used from Task 2 on.`; unreached items get `#[allow(dead_code)] // Used from Task 2 on.`; Task 2 removes them; after Task 2 `grep -rn "Used from Task" crates/arkos/src` prints nothing.
- Menus (exact labels, in order): the "Arkos IDE" menu is unchanged; a second menu "Help" with "Keybindings" and "Documentation". The `menus()` test pins both menus.
- Keymap: new actions `Keybindings` (row `"keybindings"`, default `cmd-/`) and `Documentation` (row `"documentation"`, no key), inserted immediately after `row("settings", "cmd-,")` and before `row("open config", "")`; SPEC_ROWS gains `("cmd-/", "keybindings", "Keybindings")` and `("", "documentation", "Documentation")`; the count assertions move by the rows added (two rows, one bound). `Command::{Keybindings, Documentation}` with doc comments; `command_for` maps both; the palette lists "Keybindings" with detail "Cmd+/" and "Documentation" with no detail. DEFAULT_CONFIG's keybinding comments gain `# "keybindings" = "cmd-/"` and `# "documentation" = ""`.
- Sections: `ActionSpec` gains `pub section: Section`; `row(section, name, keys)`; `pub enum Section { Window, Panes, Tabs, Terminal, Sidebar, Browser, Markdown, Maintenance }` in that display order, `Section::title()` giving "Window and app", "Panes", "Tabs", "Terminal", "Sidebar and files", "Browser", "Markdown editing", "Updates and maintenance". Membership: Window: new window, open folder, settings, about, keybindings, documentation, open config, command palette, quit. Panes: focus pane 1 to 9, focus left, focus right, focus up, focus down, split right, split down, maximize pane. Tabs: new terminal tab, next tab, previous tab, next tab alt, previous tab alt, most recently used tab, close tab, reopen closed tab, close all in pane, move tab left, move tab right, move tab to previous pane, move tab to next pane. Terminal: clear to start, font bigger, font smaller, find. Sidebar: toggle sidebar, focus file tree, focus worktrees, show ignored files. Browser: new browser tab, new single-page browser tab, dock a chrome window, undock chrome window. Maintenance: update now, check for updates, retry update, notifications, doctor, start at login, install claude code hooks. Markdown has no ACTIONS rows; its rows come from `MARKDOWN_KEYS`.
- `pub const MARKDOWN_KEYS: &[(&str, &str)]` (label, keys already in the Mac spelling), the viewer's fixed keys from `viewer::KeyAction` and `key_action`: ("Save", "Cmd+S"), ("Leave the block", "Escape"), ("Continue a list", "Enter"), ("Undo", "Cmd+Z"), ("Redo", "Shift+Cmd+Z"), ("Select all", "Cmd+A"), ("Copy", "Cmd+C"), ("Cut", "Cmd+X"), ("Paste", "Cmd+V"), ("Indent a list item", "Tab"), ("Outdent a list item", "Shift+Tab"), ("Open a link", "Cmd+click"), ("Scroll to the top or end", "Home, End"). The implementer checks each against `key_action` in viewer.rs and drops or corrects any that the code does not do, recording the change.
- `pub struct KeyRow { pub label: String, pub keys: Option<String> }` and `pub fn grouped(overrides: &BTreeMap<String, String>) -> Vec<(Section, Vec<KeyRow>)>`: every section in display order (a section with no rows is omitted); rows in ACTIONS order within a section; `label` through `keymap::label`, `keys` through `keys_for` then `pretty_keys` (`None` when unbound); the nine "focus pane N" rows collapse into one row ("Focus pane 1 to 9", "Cmd+1 to Cmd+9") when every one of them keeps its default, else they stay nine rows; the Markdown section's rows are `MARKDOWN_KEYS` with `Some(keys)`; "clear to start" keeps its label and key. A test asserts every ACTIONS name appears exactly once across the sections (the nine focus rows counting through the collapsed row), the section order, the collapsed row, that an override `"find" = "cmd-shift-f"` shows "Shift+Cmd+F", and that an unbound "doctor" has `None`.
- The window: `crates/arkos/src/keybindings.rs`, title "Keybindings", 520 by 640 px, resizable, minimum 420 by 400, centred, single instance through `Aux.keybindings: Option<WindowHandle<KeybindingsWindow>>`, a tracked `FocusHandle`; Escape and `CloseTab` (Cmd+W) close it; `NewWindow` forwards through `aux::forward_new_window`; it follows live theme changes through `aux::push_theme`; `aux_open` counts it and `close_all` closes it; `aux::open_keybindings(cx)` runs inside `cx.defer`, rebuilds the rows from the Root's `config.keybindings` (else `LaunchState.config.keybindings`) and either refreshes the open window (`set_rows`, `set_theme`, `activate_window`) or opens one. The Root's `reload_config` pushes fresh rows through `aux::push_keybindings(cx, &config.keybindings)` so a saved override shows at once. Drawing: a scrolling column; each section a header in `chrome.text` with a hairline under it; each row two columns, the label in `chrome.text`, the keys right-aligned in the pane's monospace family (as the About window's values are) in `chrome.text`, "none" in `chrome.text_dim`; a footer "Change them under [keybindings] in config.toml" with an "Open config" pill that runs `open -t` on the config path as Settings' Keyboard section does.
- Documentation: `Documentation` opens `https://github.com/csarkosh/ide-arkos#readme` through `cx.open_url`, from the global `cx.on_action` in `main()` and the Root's `run` arm alike (`aux::open_documentation(cx)` holds the URL as `pub const DOCS_URL`).
- Settings' Keyboard section gains, after the existing line, a second line "Every binding is listed under Help, Keybindings" with a "Keybindings" pill that calls `aux::open_keybindings` (deferred).
- README: a "Help" paragraph in the menu section (the two items; Cmd+/; what the window shows and how it is grouped; that overrides show as bound); the keybinding table gains "keybindings" Cmd+/ and "documentation" unbound; Status rows: the Help menu's two items; Cmd+/ opens the Keybindings window with eight sections; an override in `config.toml` shows in the window within a second; "Documentation" opens the README in the browser; the Settings Keyboard pill opens the window.
- No em-dashes in any file. Comments are full sentences. Headless tests only.

## File structure

- Create `crates/arkos/src/keybindings.rs`.
- Modify `crates/arkos/src/keymap.rs` (`Section`, `ActionSpec.section`, `row`, `MARKDOWN_KEYS`, `KeyRow`, `grouped`, the actions, SPEC_ROWS), `command_palette.rs`, `config.rs` (DEFAULT_CONFIG comments), `main.rs` (`menus()` and its test, `run` arms, global `on_action`s, `reload_config` push), `aux.rs` (`keybindings` handle, `open_keybindings`, `open_documentation`, `push_keybindings`, `aux_open`, `close_all`, `push_theme`), `settings.rs` (the Keyboard line), `README.md`.

---

### Task 1: Sections, the grouped rows, the two actions, the Help menu

**Files:**
- Modify: `crates/arkos/src/keymap.rs`, `crates/arkos/src/command_palette.rs`, `crates/arkos/src/config.rs`, `crates/arkos/src/main.rs` (`menus()` and its test; `run` arms; the global `on_action`s next to About's), `crates/arkos/src/aux.rs` (the `keybindings` handle typed on a stub, `open_keybindings` as a stub that logs, `open_documentation`, `aux_open` and `close_all` counting the new handle)
- Create: `crates/arkos/src/keybindings.rs` as a stub (`pub struct KeybindingsWindow;` rendering an empty div, "Used from Task 2 on.") so the handle type compiles

**Interfaces:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Section { Window, Panes, Tabs, Terminal, Sidebar, Browser, Markdown, Maintenance }
impl Section { pub const ALL: [Section; 8]; pub fn title(self) -> &'static str; }
pub struct ActionSpec { pub name, pub keys, pub context, pub section: Section }
pub const MARKDOWN_KEYS: &[(&str, &str)];
#[derive(Debug, Clone, PartialEq, Eq)] pub struct KeyRow { pub label: String, pub keys: Option<String> }
pub fn grouped(overrides: &BTreeMap<String, String>) -> Vec<(Section, Vec<KeyRow>)>; // "Used from Task 2 on."
// aux.rs
pub const DOCS_URL: &str = "https://github.com/csarkosh/ide-arkos#readme";
pub fn open_documentation(cx: &mut App);
pub fn open_keybindings(cx: &mut App); // stub: logs until Task 2
```

- [ ] **Step 1: Failing tests**

```rust
// keymap.rs
#[test]
fn every_action_sits_in_one_section_and_the_rows_read_as_the_mac_writes_them() {
    let g = grouped(&BTreeMap::new());
    let order: Vec<Section> = g.iter().map(|(s, _)| *s).collect();
    assert_eq!(order, Section::ALL.to_vec(), "every section, in display order");
    let mut seen = 0;
    for (section, rows) in &g {
        for row in rows {
            if *section == Section::Markdown { continue; }
            seen += if row.label == "Focus pane 1 to 9" { 9 } else { 1 };
        }
    }
    assert_eq!(seen, ACTIONS.len(), "every action once; the nine focus rows through the collapsed row");
    let panes = &g.iter().find(|(s, _)| *s == Section::Panes).unwrap().1;
    assert_eq!(panes[0], KeyRow { label: "Focus pane 1 to 9".into(), keys: Some("Cmd+1 to Cmd+9".into()) });
    let window = &g.iter().find(|(s, _)| *s == Section::Window).unwrap().1;
    assert!(window.iter().any(|r| r.label == "Keybindings" && r.keys.as_deref() == Some("Cmd+/")));
    assert!(window.iter().any(|r| r.label == "Documentation" && r.keys.is_none()));
    let maint = &g.iter().find(|(s, _)| *s == Section::Maintenance).unwrap().1;
    assert!(maint.iter().any(|r| r.label == "Doctor" && r.keys.is_none()));
    let md = &g.iter().find(|(s, _)| *s == Section::Markdown).unwrap().1;
    assert_eq!(md.len(), MARKDOWN_KEYS.len());
    assert_eq!(md[0], KeyRow { label: "Save".into(), keys: Some("Cmd+S".into()) });
    let mut o = BTreeMap::new();
    o.insert("find".to_string(), "cmd-shift-f".to_string());
    o.insert("focus pane 3".to_string(), "cmd-shift-3".to_string());
    let g = grouped(&o);
    let term = &g.iter().find(|(s, _)| *s == Section::Terminal).unwrap().1;
    assert!(term.iter().any(|r| r.label == "Find" && r.keys.as_deref() == Some("Shift+Cmd+F")));
    let panes = &g.iter().find(|(s, _)| *s == Section::Panes).unwrap().1;
    assert_eq!(panes.iter().filter(|r| r.label.starts_with("Focus pane ")).count(), 9, "an override on one of the nine spreads them out");
    assert_eq!(keys_for("keybindings", &BTreeMap::new()), Some("cmd-/".to_string()));
    assert_eq!(command_for("documentation"), Some(Command::Documentation));
}
```

The `menus()` test in main.rs gains the second menu: name "Help", items Action "Keybindings", Action "Documentation", in that order. The palette test finds "Keybindings" with detail "Cmd+/" and "Documentation" with detail "".

- [ ] **Step 2: Run to see them fail**

`nice -n 15 ~/.cargo/bin/cargo test -p arkos --bin arkos -- keymap:: command_palette:: tests::menus`

- [ ] **Step 3: Implement**

The enum and `title`; `row(section, name, keys)` and every existing row given its section per the constraint (the `ActionSpec { .. }` literal for "clear to start" gets `section: Section::Terminal`); `MARKDOWN_KEYS` checked against `viewer::key_action`; `grouped` with the collapse rule; the two actions through `actions!`, `Command`, `binding()`, `command_for`, `route!`, SPEC_ROWS and the counts; palette label arms; DEFAULT_CONFIG comments; `menus()` with the Help menu; `main()`'s global `cx.on_action` for `Keybindings` and `Documentation` next to About's; Root `run` arms; `aux.rs` additions with the stub.

- [ ] **Step 4: Run, check, commit**

```bash
git commit -m "Keymap sections and grouped rows; the Help menu with Keybindings and Documentation"
```

---

### Task 2: The Keybindings window, the Settings link, README

**Files:**
- Create: `crates/arkos/src/keybindings.rs` (replace the stub)
- Modify: `crates/arkos/src/aux.rs` (`open_keybindings` opens or refreshes; `push_keybindings`; `push_theme` reaches the new window), `crates/arkos/src/main.rs` (`reload_config` and `apply_config` push the rows; `mod keybindings;`), `crates/arkos/src/settings.rs` (the Keyboard line), `crates/arkos/src/keymap.rs` (remove the Task 2 attribute), `README.md`

**Interfaces:**
```rust
pub struct KeybindingsWindow { rows: Vec<(keymap::Section, Vec<keymap::KeyRow>)>, theme: Theme, mono: SharedString, focus: FocusHandle }
pub fn open(cx: &mut App, rows: Vec<(Section, Vec<KeyRow>)>, theme: Theme, mono: SharedString) -> Option<WindowHandle<KeybindingsWindow>>;
impl KeybindingsWindow { pub fn set_rows(&mut self, rows: .., cx: &mut Context<Self>); pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>); }
pub fn footer_text() -> &'static str; // "Change them under [keybindings] in config.toml"
// aux.rs
pub fn push_keybindings(cx: &mut App, overrides: &BTreeMap<String, String>);
```

- [ ] **Step 1: Failing test**

A small pure test in keybindings.rs: `footer_text()` is the exact string, and a helper `keys_text(&KeyRow) -> (String, bool)` gives ("Cmd+S", false) for a bound row and ("none", true) for an unbound one (the bool meaning dimmed).

- [ ] **Step 2: Implement**

The window per the constraint, modelled on `about.rs` (`open`, the key wiring) and `settings.rs` (a resizable window, the pill, the deferred callbacks). `aux::open_keybindings` per the constraint; `push_keybindings` from `Root::apply_config` (which `reload_config` calls); `push_theme` updates the new window too; `aux_open` and `close_all` already count it from Task 1. The Settings Keyboard line. README per the constraint.

- [ ] **Step 3: Gates and commit**

`grep -rn "Used from Task" crates/arkos/src` empty; no em-dash in README.md or `crates/arkos/src/*.rs`; the warning check; the suite.

```bash
git commit -m "The Keybindings window under Help; Settings links to it; README"
```

---

## Deferred

- A search field in the Keybindings window; a keybinding editor (click a row to rebind); conflict detection between overrides; printing the table from the CLI (`arkos keys`).
