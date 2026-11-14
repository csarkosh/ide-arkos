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
    /// `[[recording]]`: the window through its themes, after the shots, as a loop.
    #[serde(default, rename = "recording")]
    pub recordings: Vec<Recording>,
    /// `[effect]` once, or `[[effect]]` for each of several: a recording through themes with
    /// different effects wants each at the runner's scale.
    #[serde(default, rename = "effect", deserialize_with = "one_or_many")]
    pub effects: Vec<EffectSpec>,
}

fn one_or_many<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<EffectSpec>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(EffectSpec),
        Many(Vec<EffectSpec>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(e) => vec![e],
        OneOrMany::Many(v) => v,
    })
}

/// `[[recording]]`: after the shots, the window switched through `themes` (built-in theme
/// names), each held `hold_secs`, then back to the first, recorded by `scripts/shoot/record.swift`
/// and made a loop by `scripts/shoot/loop.py` as `<scene>-<name>.mp4` beside the stills, its
/// first frame as `<scene>-<name>.jpg`. It waits as a shot does first.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub name: String,
    #[serde(default)]
    pub wait: BTreeMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub after_secs: u64,
    #[serde(default)]
    pub changed: BTreeMap<String, usize>,
    pub themes: Vec<String>,
    /// How long each theme is held, in seconds. The loop shows each for this less the second
    /// after its switch (the config poll and the IDE's fade) and a tenth before the next.
    #[serde(default = "default_hold")]
    pub hold_secs: f64,
    #[serde(default = "default_fps")]
    pub fps: u32,
    /// How one theme becomes the next in the cut, and from the last back into the first.
    #[serde(default)]
    pub transition: Transition,
    /// How long a transition takes, in seconds.
    #[serde(default = "default_fade")]
    pub fade_secs: f64,
    /// The widest the picture is scaled to, in pixels: two for each of the 1000 CSS pixels the
    /// store page shows it at.
    #[serde(default = "default_width")]
    pub width: u32,
    /// Set, the recording is the page's opening clip instead of a loop: the window recorded in
    /// its one theme for `hold_secs`, then `cinema.py` moves a camera over it and the two
    /// scenes' stills behind it, into the key art.
    pub cinema: Option<Cinema>,
}

/// `cinema`: the hero clip's parts (`scripts/shoot/cinema.py`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cinema {
    /// The scenes whose `<scene>-dark.png` stand behind the window, in turn left and right from
    /// the outermost; none named (the default), every persona's but the clip's own scene
    /// (`personas`), so a persona added later is in the clip without a word here.
    #[serde(default)]
    pub backs: Vec<String>,
    /// How long the camera moves, and how long it then holds on the key art, in seconds.
    #[serde(default = "default_move")]
    pub move_secs: f64,
    #[serde(default = "default_rest")]
    pub rest_secs: f64,
}

/// The most stills a hero clip stands behind its window (`scripts/shoot/cinema.py`'s slots).
pub const MAX_BACKS: usize = 6;

/// The persona scenes among the storyboards in `dir` (every story whose `[scene]` does not say
/// `persona = false`), by name, without `except` (the clip's own scene).
pub fn personas(dir: &Path, except: &str) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let story = Story::parse(&std::fs::read_to_string(&path)?).with_context(|| path.display().to_string())?;
        if story.scene.persona && story.scene.name != except {
            names.push(story.scene.name);
        }
    }
    names.sort();
    Ok(names)
}

fn default_move() -> f64 {
    4.6
}

fn default_rest() -> f64 {
    0.9
}

/// `loop.py`'s transitions: a soft-edged sweep from left to right, the new theme washing over
/// the window the way light moves over a product in Apple's videos, or a plain cross-fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transition {
    #[default]
    Sweep,
    Dissolve,
}

impl Transition {
    pub fn name(self) -> &'static str {
        match self {
            Transition::Sweep => "sweep",
            Transition::Dissolve => "dissolve",
        }
    }
}

/// What `loop.py` skips after each switch (the config poll and the IDE's fade) and before the
/// next, so a theme's stretch shows it steady.
pub const SETTLE_SECS: f64 = 1.0;
const GUARD_SECS: f64 = 0.1;

fn default_hold() -> f64 {
    6.0
}

fn default_fps() -> u32 {
    30
}

fn default_fade() -> f64 {
    1.2
}

fn default_width() -> u32 {
    2000
}

/// What a shot or a recording waits for before it happens.
pub struct Waits<'a> {
    pub name: &'a str,
    pub wait: &'a BTreeMap<String, String>,
    pub changed: &'a BTreeMap<String, usize>,
    pub after_secs: u64,
    pub timeout_secs: u64,
}

impl Shot {
    pub fn waits(&self) -> Waits<'_> {
        Waits { name: &self.name, wait: &self.wait, changed: &self.changed, after_secs: self.after_secs, timeout_secs: self.timeout_secs }
    }
}

impl Story {
    /// `--only`: the shots and recordings named are taken, the rest skipped (their waits too).
    /// A name that is neither is refused, so a typo does not quietly shoot nothing.
    pub fn check_only(&self, only: &[String]) -> Result<()> {
        for name in only {
            if !self.shots.iter().any(|s| &s.name == name) && !self.recordings.iter().any(|r| &r.name == name) {
                bail!("--only names {name:?}, which is neither a shot nor a recording of this story (shots: {}; recordings: {})",
                    self.shots.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "),
                    self.recordings.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
        Ok(())
    }

    /// Whether a shot or recording is taken on this run: every one without `--only`.
    pub fn wants(only: Option<&[String]>, name: &str) -> bool {
        only.is_none_or(|names| names.iter().any(|n| n == name))
    }

    /// The recording to re-cut: the one named, or the only one.
    pub fn recording(&self, name: Option<&str>) -> Result<&Recording> {
        match name {
            Some(n) => self.recordings.iter().find(|r| r.name == n).with_context(|| format!("no recording {n:?} in this story")),
            None if self.recordings.len() == 1 => Ok(&self.recordings[0]),
            None if self.recordings.is_empty() => bail!("this story has no recording"),
            None => bail!("this story has {} recordings; name one with --name", self.recordings.len()),
        }
    }
}

impl Recording {
    pub fn waits(&self) -> Waits<'_> {
        Waits { name: &self.name, wait: &self.wait, changed: &self.changed, after_secs: self.after_secs, timeout_secs: self.timeout_secs }
    }

    /// The loop's length: every theme's hold together.
    /// How long each theme shows steady in the loop, before its transition out.
    pub fn steady_secs(&self) -> f64 {
        self.hold_secs - SETTLE_SECS - GUARD_SECS
    }
}

/// `[effect]`: the theme's built-in effect drawn at another wash scale for the picture. A
/// runner's display draws one pixel a point, half of a Retina Mac's two, so a wash at its
/// manifest's 0.5 has half the pixels a point it has on a Retina screen and its fine detail
/// (the runic rain's runes) blurs away; 1.0 there draws as a Retina Mac draws 0.5. The shoot
/// copies `crates/arkos/effects/<name>` into its own config with this scale, which shadows the
/// built-in.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectSpec {
    pub name: String,
    pub scale: f32,
}

/// An effect manifest with its `scale = ...` line set to `scale`, the rest of the line (its
/// comment) and every other line as they were.
pub fn scaled_manifest(manifest: &str, scale: f32) -> Result<String> {
    let mut found = false;
    let mut out = String::with_capacity(manifest.len());
    for line in manifest.split_inclusive('\n') {
        match line.strip_prefix("scale = ") {
            Some(rest) if !found => {
                found = true;
                let tail = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).map_or("", |i| &rest[i..]);
                out.push_str(&format!("scale = {scale:.1}{tail}"));
            }
            _ => out.push_str(line),
        }
    }
    if !found {
        bail!("the effect manifest has no scale line");
    }
    Ok(out)
}

/// The scene: its name is the pictures' (`<name>-dark.png`), its caption the words under it
/// in the composed hero.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub name: String,
    /// Whether the scene is one of the README's personas (all but the probe): a hero clip that
    /// names no backs stands every persona's still behind its window (`personas`).
    #[serde(default = "yes")]
    pub persona: bool,
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
    /// Days back to date a worktree's files, by its name: a worktree untouched that long, so the
    /// Worktrees list shows its age (and dims it past a week). Unnamed ones are left as made.
    #[serde(default)]
    pub aged: BTreeMap<String, u64>,
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
    /// The rendered markdown's `[viewer] page_width` (`full` or `letter`), when the picture shows
    /// a letter page.
    #[serde(default)]
    pub page_width: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemePair {
    pub dark: String,
    pub light: String,
}

impl Default for ThemePair {
    fn default() -> Self {
        ThemePair { dark: "navy".into(), light: "peach-watercolor".into() }
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
    /// The window's layout for the shot: `grid` (the default) or `inbox`, the sessions as a list
    /// with one shown at full size.
    #[serde(default = "default_layout")]
    pub layout: String,
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
    /// A right click at `[x, y]`, fractions of the window, after `click` and before the keys: it
    /// opens a context menu for the picture.
    #[serde(default)]
    pub right_click: Option<[f64; 2]>,
    /// A drag from the first point to the second, fractions of the window, after the clicks and
    /// before the keys: the left button held over a dozen moves.
    #[serde(default)]
    pub drag: Option<[[f64; 2]; 2]>,
    /// The pointer moved to `[x, y]`, fractions of the window, after the keys, and left there: a
    /// hover's tooltip or highlight for the picture.
    #[serde(default)]
    pub hover: Option<[f64; 2]>,
    /// The whole display captured too, as `<picture>.screen.png` beside the picture: what lies
    /// outside the window, such as the Finder window a drag starts in.
    #[serde(default)]
    pub screen: bool,
    /// `false` leaves whichever app is in front there for the input, so a drag can start in
    /// another app's window (an inactive Finder window only takes a press as a click).
    #[serde(default = "yes")]
    pub activate: bool,
    /// Keys sent to the window after the wait and before the capture, in order: `down`,
    /// `right*40`, `cmd+down`, `shift+end`, or a letter.
    #[serde(default)]
    pub keys: Vec<String>,
    /// Take this many captures a tenth of a second apart, without waiting for a still frame, and
    /// keep the one that changed most from the one before: an effect's motion (the runic
    /// decode, 400 ms on changed text) is what a still frame never shows. Every capture is
    /// framed into the progress folder, so a person can choose another.
    #[serde(default)]
    pub burst: Option<usize>,
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

/// The ANSI key code of a lowercase letter or a digit on a US layout.
fn letter_code(c: u8) -> Option<u16> {
    const LETTERS: [u16; 26] = [0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16, 6];
    const DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
    match c {
        b'a'..=b'z' => LETTERS.get((c - b'a') as usize).copied(),
        b'0'..=b'9' => DIGITS.get((c - b'0') as usize).copied(),
        b'=' => Some(24),
        b'-' => Some(27),
        // The brackets, so a story can send Cmd+Option+] and [ (the sheet keys) and Cmd+Shift+].
        b'[' => Some(33),
        b']' => Some(30),
        _ => None,
    }
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
            "return" | "enter" => Some(36), "tab" => Some(48), "escape" | "esc" => Some(53), "delete" | "backspace" => Some(51), "f2" => Some(120),
            // A letter with a modifier goes by its own key code: an app reads Cmd+Z from the key,
            // not from the text, so a letter sent as text on key 0 arrives as Cmd+A.
            letter if !modifiers.is_empty() && letter.len() == 1 && letter_code(letter.as_bytes()[0]).is_some() => letter_code(letter.as_bytes()[0]),
            _ => None,
        };
        let text = if code.is_none() {
            if last.chars().count() != 1 { bail!("key {spec:?}: not a key name or a single character"); }
            Some(last.to_string())
        } else { None };
        Ok(Key { code, text, modifiers, times })
    }
}

fn yes() -> bool {
    true
}

fn default_theme() -> String {
    "dark".into()
}

fn default_layout() -> String {
    "grid".into()
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
        for e in &self.effects {
            if !(0.1..=1.0).contains(&e.scale) {
                bail!("effect scale must be 0.1 to 1.0 (was {})", e.scale);
            }
        }
        for r in &self.recordings {
            if r.name.is_empty() || r.name.contains('/') {
                bail!("a recording's name must be a plain name");
            }
            match &r.cinema {
                None if r.themes.len() < 2 => bail!("recording {:?}: themes must name at least two themes", r.name),
                Some(c) => {
                    if r.themes.len() != 1 {
                        bail!("recording {:?}: a cinema recording shows one theme", r.name);
                    }
                    if c.backs.len() > MAX_BACKS {
                        bail!("recording {:?}: cinema.backs names at most {MAX_BACKS} scenes to stand behind the window", r.name);
                    }
                    if !(1.0..=10.0).contains(&c.move_secs) || !(0.0..=5.0).contains(&c.rest_secs) {
                        bail!("recording {:?}: cinema.move_secs must be 1 to 10 and rest_secs 0 to 5", r.name);
                    }
                    if r.hold_secs < c.move_secs + c.rest_secs + 0.5 {
                        bail!("recording {:?}: hold_secs must cover the move and the rest with half a second over (at least {:.1})", r.name, c.move_secs + c.rest_secs + 0.5);
                    }
                }
                None => {}
            }
            if !(1.0..=15.0).contains(&r.hold_secs) {
                bail!("recording {:?}: hold_secs must be 1 to 15", r.name);
            }
            if !(1..=60).contains(&r.fps) {
                bail!("recording {:?}: fps must be 1 to 60", r.name);
            }
            if !(0.3..=3.0).contains(&r.fade_secs) {
                bail!("recording {:?}: fade_secs must be 0.3 to 3", r.name);
            }
            if r.cinema.is_none() && r.steady_secs() < r.fade_secs + 1.0 {
                bail!("recording {:?}: hold_secs must leave a second of rest beyond the fade (at least {:.1})", r.name, r.fade_secs + SETTLE_SECS + GUARD_SECS + 1.0);
            }
            if !(320..=3840).contains(&r.width) {
                bail!("recording {:?}: width must be 320 to 3840 pixels", r.name);
            }
        }
        for shot in &self.shots {
            for k in &shot.keys {
                Key::parse(k).with_context(|| format!("shot {:?}", shot.name))?;
            }
            if shot.burst.is_some_and(|n| !(2..=60).contains(&n)) {
                bail!("shot {:?}: burst must be 2 to 60 captures", shot.name);
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
        if let Some(name) = self.demo.aged.keys().find(|n| !self.demo.worktrees.contains(n)) {
            bail!("demo.aged names {name:?}, which is not one of demo.worktrees");
        }
        if self.shots.is_empty() {
            bail!("no [[shot]]");
        }
        let waits_ok = |what: &str, w: &Waits| -> Result<()> {
            for (pane, state) in w.wait {
                if !seen.contains(pane.as_str()) {
                    bail!("{what} {:?} waits on pane {pane:?}, which does not exist", w.name);
                }
                if !["working", "waiting", "idle"].contains(&state.as_str()) {
                    bail!("{what} {:?}: state for {pane:?} must be working, waiting or idle", w.name);
                }
            }
            for pane in w.changed.keys() {
                if !seen.contains(pane.as_str()) || !self.pane(pane).is_terminal() {
                    bail!("{what} {:?}: changed names {pane:?}, which is not a shell pane", w.name);
                }
            }
            Ok(())
        };
        for r in &self.recordings {
            waits_ok("recording", &r.waits())?;
        }
        for s in &self.shots {
            if !["grid", "inbox"].contains(&s.layout.as_str()) {
                bail!("shot {:?}: layout must be grid or inbox", s.name);
            }
            if !["dark", "light"].contains(&s.theme.as_str()) {
                bail!("shot {:?}: theme must be dark or light", s.name);
            }
            waits_ok("shot", &s.waits())?;
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
        assert_eq!((s.window.theme.dark.as_str(), s.window.theme.light.as_str()), ("navy", "peach-watercolor"));
        assert_eq!(s.shots[0].crops[0].name, "tabs");
        assert_eq!(s.shots[1].theme, "light");
        assert_eq!(s.window.preset, "compact");
    }

    #[test]
    fn a_shot_is_taken_in_the_grid_unless_it_names_the_inbox_and_no_other_layout() {
        let s = Story::parse(STORY).unwrap();
        assert!(s.shots.iter().all(|shot| shot.layout == "grid"));
        let inbox = STORY.replacen("theme = \"light\"", "theme = \"light\"\nlayout = \"inbox\"", 1);
        assert_eq!(Story::parse(&inbox).unwrap().shots[1].layout, "inbox");
        let err = Story::parse(&STORY.replacen("theme = \"light\"", "theme = \"light\"\nlayout = \"journal\"", 1)).unwrap_err().to_string();
        assert!(err.contains("layout must be grid or inbox"), "{err}");
    }

    #[test]
    fn an_aged_worktree_is_dated_back_and_must_be_one_of_the_demo_s_worktrees() {
        let with = |aged: &str| STORY.replace("worktrees = [\"rate-limit\"]", &format!("worktrees = [\"rate-limit\", \"old\"]\naged = {{ {aged} }}"));
        let s = Story::parse(&with("old = 21")).unwrap();
        assert_eq!(s.demo.aged.get("old"), Some(&21));
        assert!(s.demo.aged.get("rate-limit").is_none(), "unnamed worktrees are left as made");
        let err = Story::parse(&with("missing = 3")).unwrap_err().to_string();
        assert!(err.contains("missing"), "{err}");
    }

    #[test]
    fn every_storyboard_in_the_repository_parses_and_its_fixture_has_what_it_names() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut names = Vec::new();
        for entry in std::fs::read_dir(root.join("scripts/shoot/stories")).unwrap() {
            let path = entry.unwrap().path();
            let story = Story::parse(&std::fs::read_to_string(&path).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(Some(story.scene.name.as_str()), path.file_stem().and_then(|s| s.to_str()), "{}: the scene is named after its file", path.display());
            let fixture = root.join(&story.demo.fixture);
            assert!(fixture.is_dir(), "{}: no fixture {}", path.display(), fixture.display());
            for folder in &story.window.expanded {
                assert!(fixture.join(folder).is_dir(), "{}: the fixture has no folder {folder}", path.display());
            }
            for theme in [&story.window.theme.dark, &story.window.theme.light] {
                assert!(root.join("crates/arkos/themes").join(format!("{theme}.toml")).is_file(), "{}: the window names no built-in theme {theme}", path.display());
            }
            for effect in &story.effects {
                assert!(root.join("crates/arkos/effects").join(&effect.name).join("effect.toml").is_file(), "{}: no built-in effect {}", path.display(), effect.name);
            }
            for recording in &story.recordings {
                for theme in &recording.themes {
                    assert!(root.join("crates/arkos/themes").join(format!("{theme}.toml")).is_file(), "{}: recording {} names no built-in theme {theme}", path.display(), recording.name);
                }
            }
            for pane in &story.panes {
                if let Some(view) = &pane.view {
                    assert!(fixture.join(view).is_file(), "{}: the fixture has no file {view}", path.display());
                }
            }
            names.push(story.scene.name);
        }
        names.sort();
        assert_eq!(names, ["blogger", "data-analyst", "hacker", "indie-game-dev", "multitasker", "probe", "student", "web-developer"]);
    }

    #[test]
    fn a_clip_that_names_no_backs_stands_every_persona_but_its_own_behind_the_window_and_not_the_probe() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let names = personas(&root.join("scripts/shoot/stories"), "multitasker").unwrap();
        assert_eq!(names, ["blogger", "data-analyst", "hacker", "indie-game-dev", "student", "web-developer"]);
        assert!(names.len() <= MAX_BACKS, "cinema.py has a slot for each");
    }

    #[test]
    fn a_story_s_effect_scale_replaces_the_manifest_s_and_keeps_its_comment() {
        let manifest = "name = \"runic\"\nscale = 0.5                   # 0.1 to 1.0; the wash's pixels\nframe_rate = 30\n";
        let out = scaled_manifest(manifest, 1.0).unwrap();
        assert_eq!(out, "name = \"runic\"\nscale = 1.0                   # 0.1 to 1.0; the wash's pixels\nframe_rate = 30\n");
        assert!(scaled_manifest("name = \"x\"\n", 1.0).unwrap_err().to_string().contains("no scale line"));
    }

    #[test]
    fn the_built_in_runic_manifest_takes_a_story_s_scale() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifest = std::fs::read_to_string(root.join("crates/arkos/effects/runic/effect.toml")).unwrap();
        let out = scaled_manifest(&manifest, 1.0).unwrap();
        assert_eq!(out.lines().filter(|l| l.starts_with("scale = 1.0")).count(), 1);
        assert_eq!(out.lines().count(), manifest.lines().count());
    }

    #[test]
    fn a_story_s_effect_and_a_burst_shot_parse_and_bad_ones_are_refused() {
        let text = format!("{STORY}\n[effect]\nname = \"runic\"\nscale = 1.0\n").replace("wait = { work = \"working\", tests = \"waiting\" }", "wait = { work = \"working\", tests = \"waiting\" }\nburst = 8");
        let s = Story::parse(&text).unwrap();
        let e = &s.effects[0];
        assert_eq!((s.effects.len(), e.name.as_str(), e.scale), (1, "runic", 1.0));
        assert_eq!(s.shots[0].burst, Some(8));
        assert_eq!(s.shots[1].burst, None);
        let err = Story::parse(&text.replace("scale = 1.0", "scale = 1.5")).unwrap_err().to_string();
        assert!(err.contains("effect scale must be 0.1 to 1.0"), "{err}");
        let err = Story::parse(&text.replace("burst = 8", "burst = 1")).unwrap_err().to_string();
        assert!(err.contains("burst must be 2 to 60"), "{err}");
    }

    #[test]
    fn several_effects_parse_as_a_list_and_each_scale_is_checked() {
        let text = format!("{STORY}\n[[effect]]\nname = \"runic\"\nscale = 1.0\n\n[[effect]]\nname = \"watercolor\"\nscale = 0.8\n");
        let s = Story::parse(&text).unwrap();
        assert_eq!(s.effects.iter().map(|e| (e.name.as_str(), e.scale)).collect::<Vec<_>>(), [("runic", 1.0), ("watercolor", 0.8)]);
        let err = Story::parse(&text.replace("scale = 0.8", "scale = 0.0")).unwrap_err().to_string();
        assert!(err.contains("effect scale must be 0.1 to 1.0"), "{err}");
        assert!(Story::parse(STORY).unwrap().effects.is_empty(), "no [effect] is an empty list");
    }

    const RECORDING: &str = "\n[[recording]]\nname = \"themes\"\nwait = { tests = \"waiting\" }\nchanged = { work = 1 }\nthemes = [\"navy\", \"peach-watercolor\", \"matrix\"]\nhold_secs = 4\n";

    #[test]
    fn a_recording_parses_with_its_defaults_and_waits_as_a_shot_does() {
        let s = Story::parse(&format!("{STORY}{RECORDING}")).unwrap();
        assert_eq!(s.recordings.len(), 1);
        let r = &s.recordings[0];
        assert_eq!((r.name.as_str(), r.hold_secs, r.fps, r.fade_secs, r.transition, r.width), ("themes", 4.0, 30, 1.2, Transition::Sweep, 2000));
        assert_eq!(r.themes, ["navy", "peach-watercolor", "matrix"]);
        assert!((r.steady_secs() - 2.9).abs() < 1e-9, "a theme shows steady for its hold less the settle and the guard");
        let dissolve = Story::parse(&format!("{STORY}{RECORDING}transition = \"dissolve\"\nfade_secs = 0.8\n")).unwrap();
        assert_eq!((dissolve.recordings[0].transition, dissolve.recordings[0].transition.name(), dissolve.recordings[0].fade_secs), (Transition::Dissolve, "dissolve", 0.8));
        let w = r.waits();
        assert_eq!((w.name, w.wait.get("tests").map(String::as_str), w.changed.get("work"), w.after_secs, w.timeout_secs), ("themes", Some("waiting"), Some(&1), 0, 300));
        assert!(Story::parse(STORY).unwrap().recordings.is_empty());
    }

    #[test]
    fn a_recording_that_cannot_loop_or_waits_on_nothing_real_is_refused() {
        let text = format!("{STORY}{RECORDING}");
        let refused = |from: &str, to: &str, says: &str| {
            let err = format!("{:#}", Story::parse(&text.replace(from, to)).unwrap_err());
            assert!(err.contains(says), "{from} -> {to}: {err}");
        };
        refused("themes = [\"navy\", \"peach-watercolor\", \"matrix\"]", "themes = [\"navy\"]", "at least two themes");
        refused("hold_secs = 4", "hold_secs = 0.5", "hold_secs must be 1 to 15");
        refused("hold_secs = 4", "hold_secs = 4\nfade_secs = 2", "hold_secs must leave a second of rest beyond the fade (at least 4.1)");
        refused("hold_secs = 4", "hold_secs = 4\nfade_secs = 0.1", "fade_secs must be 0.3 to 3");
        refused("hold_secs = 4", "hold_secs = 4\ntransition = \"wipe\"", "expected `sweep` or `dissolve`");
        refused("hold_secs = 4", "hold_secs = 4\nfps = 0", "fps must be 1 to 60");
        refused("hold_secs = 4", "hold_secs = 4\nwidth = 100", "width must be 320 to 3840");
        refused("wait = { tests = \"waiting\" }\nchanged", "wait = { nobody = \"waiting\" }\nchanged", "recording \"themes\" waits on pane \"nobody\"");
        refused("changed = { work = 1 }", "changed = { site = 1 }", "recording \"themes\": changed names \"site\"");
    }

    #[test]
    fn a_cinema_recording_shows_one_theme_over_two_backs_and_its_hold_covers_the_clip() {
        let cinema = "\n[[recording]]\nname = \"hero\"\nwait = { tests = \"waiting\" }\nthemes = [\"navy\"]\nhold_secs = 8\ncinema = { backs = [\"web-developer\", \"blogger\"] }\n";
        let s = Story::parse(&format!("{STORY}{cinema}")).unwrap();
        let r = &s.recordings[0];
        let c = r.cinema.as_ref().unwrap();
        assert_eq!((c.backs.clone(), c.move_secs, c.rest_secs, r.hold_secs), (vec!["web-developer".to_string(), "blogger".to_string()], 4.6, 0.9, 8.0));
        let refused = |from: &str, to: &str, says: &str| {
            let err = format!("{:#}", Story::parse(&format!("{STORY}{}", cinema.replace(from, to))).unwrap_err());
            assert!(err.contains(says), "{from} -> {to}: {err}");
        };
        refused("themes = [\"navy\"]", "themes = [\"navy\", \"matrix\"]", "a cinema recording shows one theme");
        refused("[\"web-developer\", \"blogger\"]", "[\"a\", \"b\", \"c\", \"d\", \"e\", \"f\", \"g\"]", "names at most 6 scenes");
        let every = Story::parse(&format!("{STORY}{}", cinema.replace("cinema = { backs = [\"web-developer\", \"blogger\"] }", "cinema = {}"))).unwrap();
        assert!(every.recordings[0].cinema.as_ref().unwrap().backs.is_empty(), "no backs named: every persona's");
        refused("hold_secs = 8", "hold_secs = 5", "hold_secs must cover the move and the rest with half a second over (at least 6.0)");
        refused("cinema = { backs", "cinema = { move_secs = 12, backs", "cinema.move_secs must be 1 to 10");
        assert!(Story::parse(&format!("{STORY}{}", cinema.replace("themes = [\"navy\"]\nhold_secs = 8\ncinema = { backs = [\"web-developer\", \"blogger\"] }", "themes = [\"navy\"]"))).unwrap_err().to_string().contains("at least two themes"), "without cinema a recording still needs two themes");
    }

    #[test]
    fn a_shot_or_a_recording_is_picked_by_name_and_an_unknown_name_is_refused() {
        let s = Story::parse(&format!("{STORY}{RECORDING}")).unwrap();
        let only = vec!["themes".to_string()];
        s.check_only(&only).unwrap();
        assert!(Story::wants(Some(&only), "themes") && !Story::wants(Some(&only), "dark") && Story::wants(None, "dark"));
        let err = s.check_only(&["darkk".to_string()]).unwrap_err().to_string();
        assert!(err.contains("\"darkk\", which is neither a shot nor a recording") && err.contains("recordings: themes"), "{err}");
        assert_eq!(s.recording(None).unwrap().name, "themes");
        assert_eq!(s.recording(Some("themes")).unwrap().name, "themes");
        assert!(s.recording(Some("other")).unwrap_err().to_string().contains("no recording \"other\""));
        assert!(Story::parse(STORY).unwrap().recording(None).unwrap_err().to_string().contains("no recording"));
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
        assert_eq!(Key::parse("cmd+z").unwrap().code, Some(6), "a modified letter goes by its key, so Cmd+Z is not Cmd+A");
        assert_eq!(Key::parse("cmd+shift+z").unwrap().code, Some(6));
        assert_eq!(Key::parse("cmd+0").unwrap().code, Some(29), "a digit with a modifier too");
        assert_eq!((Key::parse("alt+cmd+]").unwrap().code, Key::parse("cmd+shift+[").unwrap().code), (Some(30), Some(33)), "a modified bracket goes by its key");
        assert_eq!(Key::parse("**2").unwrap().text.as_deref(), Some("*"), "a star is typed as text");
        assert_eq!(Key::parse("f2").unwrap().code, Some(120), "F2 edits a table cell");
        assert_eq!((Key::parse("cmd+=").unwrap().code, Key::parse("cmd+-").unwrap().code), (Some(24), Some(27)), "the font size steps go by their keys");
        assert!(Key::parse("meta+x").is_err());
        assert!(Key::parse("rightt").is_err());
        assert!(Key::parse("right*x").is_err());
        assert!(Key::parse("cmd+").is_err());
    }
}
