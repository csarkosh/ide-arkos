//! The storyboard (`scripts/shoot/story.toml`): the demo project, the window, its panes and
//! layout, and the shots to take. Also the workspace file the window opens with, built from it.
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Story {
    pub scene: Scene,
    pub demo: Demo,
    pub window: WindowSpec,
    #[serde(rename = "pane")]
    pub panes: Vec<Pane>,
    pub layout: Node,
    #[serde(rename = "shot")]
    pub shots: Vec<Shot>,
}

/// The scene: its name is the pictures' (`<name>-dark.png`), its caption the words under it
/// in the composed hero.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub name: String,
    /// Read by scripts/shoot.sh when it composes the hero.
    #[allow(dead_code)]
    pub caption: String,
}

/// The project the window opens: a copy of `fixture`, each of `repos` (folders of the copy;
/// `.` is the copy itself) made a git repository, with a worktree per name under
/// `.worktrees/` of the copy when it is one, and its `site` folder served on `port`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Demo {
    pub name: String,
    pub fixture: PathBuf,
    /// Only these entries of the fixture are copied, when set.
    #[serde(default)]
    pub only: Vec<String>,
    pub port: Option<u16>,
    #[serde(default = "default_repos")]
    pub repos: Vec<String>,
    #[serde(default)]
    pub worktrees: Vec<String>,
    #[serde(default = "default_site")]
    pub site: String,
    /// Where the copy lives (`~` allowed): the window's title bar shows the path. Under
    /// `~/Library/Caches/arkos-shoot` when unset.
    pub dir: Option<String>,
}

fn default_site() -> String {
    "public".into()
}

fn default_repos() -> Vec<String> {
    vec![".".into()]
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowSpec {
    pub width: f64,
    pub height: f64,
    #[serde(default = "default_preset")]
    pub preset: String,
    #[serde(default = "default_sidebar_width")]
    pub sidebar_width: f32,
    /// Folders the file tree shows open, relative to the demo.
    #[serde(default)]
    pub expanded: Vec<String>,
    #[serde(default)]
    pub sidebar: Sidebar,
    /// The theme names for the two appearances (the config's `[theme]` table); a scene that
    /// shows one look names it for both.
    #[serde(default)]
    pub theme: ThemePair,
    /// Claude Code's own theme in the demo's sessions (its `theme` setting): `light-ansi` or
    /// `dark-ansi` draw it in the terminal's sixteen colours, which Arkos's themes set, so its
    /// code and prompts read on the scene's background. `auto` lets it detect the background.
    #[serde(default = "default_claude_theme")]
    pub claude_theme: String,
}

fn default_claude_theme() -> String {
    "auto".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemePair {
    pub dark: String,
    pub light: String,
}

impl Default for ThemePair {
    fn default() -> Self {
        ThemePair { dark: "navy".into(), light: "navy-light".into() }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sidebar {
    #[default]
    Open,
    Collapsed,
}

fn default_preset() -> String {
    "compact".into()
}

fn default_sidebar_width() -> f32 {
    240.0
}

/// A tab: a shell in the demo (or in a folder of it, `cwd`, a worktree's `.worktrees/<name>`
/// included) that runs `claude` with a prompt or a plain command; a web tab on `browser`; or a
/// file of the demo open in the viewer (`view`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pane {
    pub name: String,
    pub cwd: Option<String>,
    pub claude: Option<String>,
    pub run: Option<String>,
    pub browser: Option<String>,
    pub view: Option<String>,
}

impl Pane {
    /// A shell: a session is created for it.
    pub fn is_terminal(&self) -> bool {
        self.browser.is_none() && self.view.is_none()
    }

    /// The folder its shell starts in, or its file's folder.
    pub fn dir(&self, demo: &Path) -> PathBuf {
        match &self.cwd {
            Some(c) => demo.join(c),
            None => demo.to_path_buf(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Node {
    Leaf {
        tabs: Vec<String>,
        #[serde(default)]
        active: usize,
    },
    Split {
        split: Orientation,
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

/// `horizontal` puts the two side by side, `vertical` one above the other (the tree's words).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Horizontal,
    Vertical,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shot {
    pub name: String,
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Pane name to the Claude Code state it must be in: `working`, `waiting` or `idle`.
    #[serde(default)]
    pub wait: BTreeMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Not before this many seconds after the prompts went in: a session is "working" within
    /// seconds, and the picture wants its work on screen.
    #[serde(default)]
    pub after_secs: u64,
    /// Pane name to how many files its session must have changed or added in its folder (git
    /// status) before the shot: work on screen, whatever the clock says.
    #[serde(default)]
    pub changed: BTreeMap<String, usize>,
    #[serde(default, rename = "crop")]
    pub crops: Vec<Crop>,
    /// A click at `[x, y]`, fractions of the window, before the keys: it gives a pane the
    /// keyboard focus and places the caret, so a picture can show a file being edited.
    #[serde(default)]
    pub click: Option<[f64; 2]>,
    /// Keys sent to the window after the wait and before the capture, in order: `down`,
    /// `right*40`, `cmd+down`, `shift+end`, or a letter.
    #[serde(default)]
    pub keys: Vec<String>,
}

/// One key of a shot's `keys`: a virtual key code or a character, its modifiers, repeated
/// `times`. `right*40` is forty presses; `cmd+down` is the command key held.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub code: Option<u16>,
    pub text: Option<String>,
    pub modifiers: Vec<&'static str>,
    pub times: usize,
}

impl Key {
    pub fn parse(spec: &str) -> Result<Key> {
        let (name, times) = match spec.rsplit_once('*') {
            Some((n, t)) => (n, t.parse::<usize>().with_context(|| format!("key {spec:?}: the count after * is not a number"))?),
            None => (spec, 1),
        };
        let mut parts: Vec<&str> = name.split('+').collect();
        let last = parts.pop().filter(|k| !k.is_empty()).ok_or_else(|| anyhow::anyhow!("key {spec:?}: no key name"))?;
        let mut modifiers = Vec::new();
        for m in parts {
            modifiers.push(match m {
                "cmd" | "command" => "command down",
                "shift" => "shift down",
                "alt" | "option" => "option down",
                "ctrl" | "control" => "control down",
                other => bail!("key {spec:?}: unknown modifier {other:?}"),
            });
        }
        let code = match last {
            "left" => Some(123), "right" => Some(124), "down" => Some(125), "up" => Some(126),
            "home" => Some(115), "end" => Some(119), "pageup" => Some(116), "pagedown" => Some(121),
            "return" | "enter" => Some(36), "tab" => Some(48), "escape" | "esc" => Some(53), "delete" | "backspace" => Some(51),
            _ => None,
        };
        let text = if code.is_none() {
            if last.chars().count() != 1 { bail!("key {spec:?}: not a key name or a single character"); }
            Some(last.to_string())
        } else { None };
        Ok(Key { code, text, modifiers, times })
    }
}

fn default_theme() -> String {
    "dark".into()
}

fn default_timeout() -> u64 {
    300
}

/// A detail cut from the shot: `rect` is `[x, y, w, h]` as fractions of the window.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crop {
    pub name: String,
    pub rect: [f64; 4],
}

impl Story {
    pub fn parse(text: &str) -> Result<Story> {
        let story: Story = toml::from_str(text).context("story.toml")?;
        story.check()?;
        Ok(story)
    }

    fn check(&self) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        if !["auto", "dark", "light", "dark-ansi", "light-ansi", "dark-daltonized", "light-daltonized"].contains(&self.window.claude_theme.as_str()) {
            bail!("window.claude_theme must be one of Claude Code's themes (auto, dark, light, dark-ansi, light-ansi, dark-daltonized, light-daltonized)");
        }
        for shot in &self.shots {
            for k in &shot.keys {
                Key::parse(k).with_context(|| format!("shot {:?}", shot.name))?;
            }
            if let Some([x, y]) = shot.click {
                if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
                    bail!("shot {:?}: click is [x, y] as fractions of the window", shot.name);
                }
            }
        }
        for p in &self.panes {
            if !seen.insert(p.name.as_str()) {
                bail!("pane {:?} is listed twice", p.name);
            }
            let kinds = [p.claude.is_some(), p.run.is_some(), p.browser.is_some(), p.view.is_some()].iter().filter(|b| **b).count();
            if kinds > 1 {
                bail!("pane {:?}: one of claude, run, browser or view, not several", p.name);
            }
            if !p.is_terminal() && p.cwd.is_some() {
                bail!("pane {:?}: only a shell has a cwd", p.name);
            }
            for rel in p.cwd.iter().chain(p.view.iter()) {
                let path = Path::new(rel);
                if path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                    bail!("pane {:?}: {rel:?} must be a folder or file inside the demo", p.name);
                }
            }
        }
        let mut placed = Vec::new();
        self.layout.names(&mut placed);
        for name in &placed {
            if !seen.contains(name.as_str()) {
                bail!("layout names pane {name:?}, which [[pane]] does not define");
            }
        }
        for name in &seen {
            if placed.iter().filter(|n| n == name).count() != 1 {
                bail!("pane {name:?} must appear exactly once in the layout");
            }
        }
        if self.scene.name.is_empty() || self.scene.name.contains('/') {
            bail!("scene.name must be a plain name");
        }
        if self.demo.repos.is_empty() {
            bail!("demo.repos must name at least one folder (\".\" for the demo itself)");
        }
        if !self.demo.worktrees.is_empty() && !self.demo.repos.iter().any(|r| r == ".") {
            bail!("demo.worktrees needs the demo itself among demo.repos");
        }
        if self.shots.is_empty() {
            bail!("no [[shot]]");
        }
        for s in &self.shots {
            if !["dark", "light"].contains(&s.theme.as_str()) {
                bail!("shot {:?}: theme must be dark or light", s.name);
            }
            for (pane, state) in &s.wait {
                if !seen.contains(pane.as_str()) {
                    bail!("shot {:?} waits on pane {pane:?}, which does not exist", s.name);
                }
                if !["working", "waiting", "idle"].contains(&state.as_str()) {
                    bail!("shot {:?}: state for {pane:?} must be working, waiting or idle", s.name);
                }
            }
            for pane in s.changed.keys() {
                if !seen.contains(pane.as_str()) || !self.pane(pane).is_terminal() {
                    bail!("shot {:?}: changed names {pane:?}, which is not a shell pane", s.name);
                }
            }
            for c in &s.crops {
                let [x, y, w, h] = c.rect;
                if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) || w <= 0.0 || h <= 0.0 || x + w > 1.0 + 1e-9 || y + h > 1.0 + 1e-9 {
                    bail!("shot {:?}: crop {:?} must lie within the window (fractions)", s.name, c.name);
                }
            }
        }
        Ok(())
    }

    pub fn pane(&self, name: &str) -> &Pane {
        self.panes.iter().find(|p| p.name == name).expect("checked by Story::check")
    }
}

impl Node {
    fn names(&self, out: &mut Vec<String>) {
        match self {
            Node::Leaf { tabs, .. } => out.extend(tabs.iter().cloned()),
            Node::Split { first, second, .. } => {
                first.names(out);
                second.names(out);
            }
        }
    }
}

/// FNV-1a over the path's bytes: how `arkos` names a project's workspace file (`paths.rs`).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub fn workspace_file(workspaces_dir: &Path, project: &Path) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    workspaces_dir.join(format!("{:016x}.json", fnv1a(project.as_os_str().as_bytes())))
}

/// The window's frame on the display, in points from its top-left: the size asked for,
/// centred, below the menu bar. The same arithmetic as `arkos`'s `fit_window` when the saved
/// display is unknown, which is how the file below places the window.
pub fn centred(w: f64, h: f64, screen_w: f64, screen_h: f64, menu_bar: f64) -> (f64, f64, f64, f64) {
    let (w, h) = (w.min(screen_w), h.min(screen_h - menu_bar));
    let x = ((screen_w - w) / 2.0).clamp(0.0, screen_w - w);
    let y = ((screen_h - h) / 2.0).clamp(menu_bar, screen_h - h);
    (x.round(), y.round(), w, h)
}

/// The workspace file (`workspace.rs`, version 3) the window opens with: the layout's panes
/// and tabs over the sessions already created, the browser tabs, the sidebar open on the
/// demo's folders and worktrees, and the window's frame.
pub fn workspace_json(story: &Story, project: &Path, sessions: &BTreeMap<String, u64>, worktree_dirs: &BTreeMap<String, PathBuf>, frame: (f64, f64, f64, f64)) -> serde_json::Value {
    let mut ids = 1u64;
    let mut names = Vec::new();
    let mut focused = None;
    let root = node_json(&story.layout, story, project, sessions, &mut ids, &mut names, &mut focused);
    let expanded: Vec<_> = story.window.expanded.iter().map(|d| project.join(d)).collect();
    let (x, y, w, h) = frame;
    serde_json::json!({
        "version": 3,
        "project": project,
        "tree": { "root": root, "next_id": ids, "focused": focused.unwrap_or(1), "maximized": null, "names": names },
        "text_preset": story.window.preset,
        "sidebar": {
            "width": story.window.sidebar_width, "collapsed": story.window.sidebar == Sidebar::Collapsed, "show_ignored": false,
            "expanded": expanded, "worktrees_expanded": worktree_dirs.values().collect::<Vec<_>>(),
            "explorer_open": true, "worktrees_open": true
        },
        // No display has this id, so the window centres itself at this size (`open_bounds`).
        "window": { "display": "shoot", "x": x, "y": y, "w": w, "h": h }
    })
}

fn node_json(node: &Node, story: &Story, project: &Path, sessions: &BTreeMap<String, u64>, ids: &mut u64, names: &mut Vec<(u64, String)>, focused: &mut Option<u64>) -> serde_json::Value {
    let mut next = || {
        let id = *ids;
        *ids += 1;
        id
    };
    match node {
        Node::Leaf { tabs, active } => {
            let pane_id = next();
            focused.get_or_insert(pane_id);
            let mut tab_ids = Vec::new();
            let mut tab_json = Vec::new();
            for name in tabs {
                let pane = story.pane(name);
                let tab_id = next();
                tab_ids.push(tab_id);
                names.push((tab_id, name.clone()));
                tab_json.push(if let Some(url) = &pane.browser {
                    serde_json::json!({ "Browser": { "url": url } })
                } else if let Some(file) = &pane.view {
                    serde_json::json!({ "Viewer": { "path": project.join(file) } })
                } else {
                    // A shell elsewhere than the project names its folder, so a restart
                    // reattaches it only where it lives.
                    let cwd = pane.cwd.as_ref().map(|c| project.join(c));
                    serde_json::json!({ "Terminal": { "session": sessions[name], "cwd": cwd } })
                });
            }
            let n = tabs.len();
            let active = (*active).min(n.saturating_sub(1));
            let mut mru: Vec<usize> = (0..n).collect();
            mru.retain(|i| *i != active);
            mru.insert(0, active);
            serde_json::json!({ "Leaf": { "id": pane_id, "tabs": tab_json, "active": active, "mru": mru, "ids": tab_ids } })
        }
        Node::Split { split, ratio, first, second } => {
            let id = next();
            let first = node_json(first, story, project, sessions, ids, names, focused);
            let second = node_json(second, story, project, sessions, ids, names, focused);
            let orientation = match split {
                Orientation::Horizontal => "Horizontal",
                Orientation::Vertical => "Vertical",
            };
            serde_json::json!({ "Split": { "id": id, "orientation": orientation, "ratio": ratio, "first": first, "second": second } })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STORY: &str = r#"
[scene]
name = "web-developer"
caption = "The web developer"

[demo]
name = "lantern"
fixture = "scripts/shoot/demo"
port = 4173
worktrees = ["rate-limit"]

[window]
width = 1360
height = 860
expanded = ["src"]

[[pane]]
name = "work"
cwd = ".worktrees/rate-limit"
claude = "do the thing"

[[pane]]
name = "tests"
claude = "test the thing"

[[pane]]
name = "shell"
run = "npm test"

[[pane]]
name = "site"
browser = "http://localhost:4173/"

[[pane]]
name = "readme"
view = "README.md"

[layout]
split = "horizontal"
ratio = 0.6
first = { tabs = ["work", "tests", "readme"] }
second = { split = "vertical", ratio = 0.55, first = { tabs = ["site"] }, second = { tabs = ["shell"] } }

[[shot]]
name = "hero"
wait = { work = "working", tests = "waiting" }

[[shot.crop]]
name = "tabs"
rect = [0.0, 0.0, 0.5, 0.2]

[[shot]]
name = "hero-light"
theme = "light"
"#;

    fn sessions() -> BTreeMap<String, u64> {
        [("work", 11), ("tests", 12), ("shell", 13)].into_iter().map(|(n, id)| (n.to_string(), id)).collect()
    }

    #[test]
    fn a_story_parses_and_checks() {
        let s = Story::parse(STORY).unwrap();
        assert_eq!(s.panes.len(), 5);
        assert_eq!(s.scene.name, "web-developer");
        assert_eq!(s.window.sidebar, Sidebar::Open);
        assert_eq!((s.window.theme.dark.as_str(), s.window.theme.light.as_str()), ("navy", "navy-light"));
        assert_eq!(s.shots[0].crops[0].name, "tabs");
        assert_eq!(s.shots[1].theme, "light");
        assert_eq!(s.window.preset, "compact");
    }

    #[test]
    fn a_pane_missing_from_the_layout_is_refused() {
        let text = STORY.replace(r#"first = { tabs = ["work", "tests", "readme"] }"#, r#"first = { tabs = ["work", "readme"] }"#);
        let err = Story::parse(&text).unwrap_err().to_string();
        assert!(err.contains("\"tests\" must appear exactly once"), "{err}");
    }

    #[test]
    fn a_folder_outside_the_demo_is_refused() {
        let text = STORY.replace(r#"cwd = ".worktrees/rate-limit""#, r#"cwd = "../elsewhere""#);
        let err = Story::parse(&text).unwrap_err().to_string();
        assert!(err.contains("inside the demo"), "{err}");
    }

    #[test]
    fn a_wait_on_an_unknown_state_is_refused() {
        let text = STORY.replace(r#"tests = "waiting""#, r#"tests = "done""#);
        let err = Story::parse(&text).unwrap_err().to_string();
        assert!(err.contains("must be working, waiting or idle"), "{err}");
    }

    #[test]
    fn the_workspace_file_matches_the_windows_format() {
        let s = Story::parse(STORY).unwrap();
        let project = Path::new("/tmp/lantern");
        let worktrees: BTreeMap<String, PathBuf> = [("rate-limit".to_string(), project.join(".worktrees/rate-limit"))].into_iter().collect();
        let ws = workspace_json(&s, project, &sessions(), &worktrees, (55.0, 48.0, 1360.0, 860.0));
        assert_eq!(ws["version"], 3);
        assert_eq!(ws["project"], "/tmp/lantern");
        let root = &ws["tree"]["root"]["Split"];
        assert_eq!(root["orientation"], "Horizontal");
        let left = &root["first"]["Leaf"];
        assert_eq!(left["tabs"][0]["Terminal"]["session"], 11);
        assert_eq!(left["tabs"][0]["Terminal"]["cwd"], "/tmp/lantern/.worktrees/rate-limit");
        assert_eq!(left["tabs"][1]["Terminal"]["cwd"], serde_json::Value::Null);
        assert_eq!(left["tabs"][2]["Viewer"]["path"], "/tmp/lantern/README.md");
        assert_eq!(left["mru"], serde_json::json!([0, 1, 2]));
        assert_eq!(ws["sidebar"]["collapsed"], false);
        let right = &root["second"]["Split"];
        assert_eq!(right["orientation"], "Vertical");
        assert_eq!(right["first"]["Leaf"]["tabs"][0]["Browser"], serde_json::json!({ "url": "http://localhost:4173/" }));
        assert_eq!(right["second"]["Leaf"]["tabs"][0]["Terminal"]["session"], 13);
        // Every pane, split and tab id is distinct and below next_id; the names follow the tab ids.
        let next = ws["tree"]["next_id"].as_u64().unwrap();
        let names = ws["tree"]["names"].as_array().unwrap();
        assert_eq!(names.len(), 5);
        for n in names {
            assert!(n[0].as_u64().unwrap() < next);
        }
        assert_eq!(names[0][1], "work");
        assert_eq!(ws["tree"]["focused"], 2, "the first leaf (after the root split took id 1)");
        assert_eq!(ws["sidebar"]["expanded"][0], "/tmp/lantern/src");
        assert_eq!(ws["sidebar"]["worktrees_expanded"][0], "/tmp/lantern/.worktrees/rate-limit");
        assert_eq!(ws["window"]["w"], 1360.0);
        assert_eq!(ws["text_preset"], "compact");
    }

    #[test]
    fn fnv1a_matches_the_reference_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(workspace_file(Path::new("/w"), Path::new("a")).file_name().unwrap(), "af63dc4c8601ec8c.json");
    }

    #[test]
    fn the_window_is_centred_below_the_menu_bar() {
        assert_eq!(centred(1360.0, 860.0, 1470.0, 956.0, 38.0), (55.0, 48.0, 1360.0, 860.0));
        // Too tall for the screen: as tall as the space under the menu bar, starting there.
        assert_eq!(centred(1360.0, 1000.0, 1470.0, 956.0, 38.0), (55.0, 38.0, 1360.0, 918.0));
    }

    #[test]
    fn keys_parse_to_codes_and_modifiers() {
        let k = Key::parse("right*40").unwrap();
        assert_eq!((k.code, k.times, k.modifiers.len()), (Some(124), 40, 0));
        let k = Key::parse("cmd+shift+down").unwrap();
        assert_eq!((k.code, k.modifiers.clone()), (Some(125), vec!["command down", "shift down"]));
        let k = Key::parse("a").unwrap();
        assert_eq!((k.code, k.text.as_deref()), (None, Some("a")));
        assert!(Key::parse("meta+x").is_err());
        assert!(Key::parse("rightt").is_err());
        assert!(Key::parse("right*x").is_err());
        assert!(Key::parse("cmd+").is_err());
    }
}
