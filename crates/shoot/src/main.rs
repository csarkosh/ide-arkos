//! `arkos-shoot`: Arkos IDE's screenshots, taken of the real thing. It copies a demo project,
//! starts a second arkosd of its own (its state under `~/Library/Caches/arkos-shoot`, so the
//! Arkos in use is untouched), opens a dev-build window on the demo with the storyboard's
//! layout, types `claude` and a prompt into each pane, waits for the tab states the shot asks
//! for, and captures the window with `screencapture`. `scripts/shoot.sh` builds and runs it;
//! `scripts/shoot/story.toml` is the storyboard.
mod daemon;
mod story;

use anyhow::{bail, Context, Result};
use daemon::{state_name, Daemon};
use protocol::{ClaudeState, SessionId};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use story::{Shot, Story};

const USAGE: &str = "usage: arkos-shoot run [--story scripts/shoot/story.toml] [--out docs/images] [--bin target/debug] [--app <an Arkos IDE.app to start instead of one made from --bin>]
       arkos-shoot bundle [--story ...] [--bin target/debug]   (only makes and signs the bundle, to grant it Accessibility)
       arkos-shoot clean";

/// Where the shoot keeps everything: the daemon's state (socket, sessions, workspaces, logs),
/// the window's config, and the demo project.
struct Dirs {
    cache: PathBuf,
    state: PathBuf,
    config_home: PathBuf,
}

impl Dirs {
    fn new() -> Result<Dirs> {
        let home = PathBuf::from(std::env::var("HOME").context("HOME")?);
        let cache = home.join("Library/Caches/arkos-shoot");
        Ok(Dirs { state: cache.join("state"), config_home: cache.join("config"), cache })
    }

    fn config_file(&self) -> PathBuf {
        self.config_home.join("arkos/config.toml")
    }

    fn socket(&self) -> PathBuf {
        self.state.join("arkosd.sock")
    }
}

struct Opts {
    story: PathBuf,
    out: PathBuf,
    bin: PathBuf,
    /// The app bundle to start through Launch Services (`open -a`, which is what lets the
    /// window come to the front) instead of the one made from the binaries in `bin`.
    app: Option<PathBuf>,
}

fn parse_opts(args: &[String]) -> Result<Opts> {
    let mut opts = Opts { story: "scripts/shoot/story.toml".into(), out: "docs/images".into(), bin: "target/debug".into(), app: None };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let value = || it.clone().next().cloned().with_context(|| format!("{a} needs a value"));
        match a.as_str() {
            "--story" => opts.story = value()?.into(),
            "--out" => opts.out = value()?.into(),
            "--bin" => opts.bin = value()?.into(),
            "--app" => opts.app = Some(value()?.into()),
            other => bail!("unknown argument {other}\n{USAGE}"),
        }
        it.next();
    }
    Ok(opts)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => parse_opts(&args[1..]).and_then(run),
        Some("bundle") => parse_opts(&args[1..]).and_then(bundle_only),
        Some("clean") => Dirs::new().and_then(|d| clean(&d)),
        _ => {
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("arkos-shoot: {e:#}");
        std::process::exit(1);
    }
}

/// Makes (and signs, when the identity exists) the bundle a run would, and names it, so it
/// can be added under Accessibility before any run.
fn bundle_only(opts: Opts) -> Result<()> {
    let dirs = Dirs::new()?;
    let story_path = opts.story.canonicalize().with_context(|| format!("story {}", opts.story.display()))?;
    let repo = repo_of(&story_path)?;
    let bin = opts.bin.canonicalize().with_context(|| format!("binaries in {}", opts.bin.display()))?;
    let app = make_bundle(&repo, &bin, &dirs.cache.join("app"))?;
    println!("{}", app.display());
    Ok(())
}

/// The repository a story belongs to: the nearest folder above it with a Cargo.toml.
fn repo_of(story: &Path) -> Result<PathBuf> {
    story.ancestors().skip(1).find(|d| d.join("Cargo.toml").is_file()).map(Path::to_path_buf).with_context(|| format!("no Cargo.toml above {}", story.display()))
}

fn say(msg: &str) {
    eprintln!("arkos-shoot: {msg}");
}

/// Set by SIGINT or SIGTERM: the run stops at its next step and ends everything it started
/// (the window, the daemon, the sessions, the site, the Chrome window), as it would at the end.
static STOPPING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOPPING.store(true, std::sync::atomic::Ordering::SeqCst);
}

fn install_signal_handlers() {
    unsafe {
        libc::signal(libc::SIGINT, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
}

/// A pause that ends early, with an error, when a signal asked the run to stop.
fn pause(d: Duration) -> Result<()> {
    let until = Instant::now() + d;
    while Instant::now() < until {
        if STOPPING.load(std::sync::atomic::Ordering::SeqCst) {
            bail!("stopped by a signal");
        }
        std::thread::sleep(Duration::from_millis(100).min(until - Instant::now()));
    }
    Ok(())
}

/// What the run started and must end, whatever happens on the way.
#[derive(Default)]
struct Running {
    daemon: Option<Child>,
    window_pid: Option<u32>,
    site: Option<Child>,
    client: Option<Daemon>,
    sessions: Vec<SessionId>,
    chrome_url: Option<String>,
    /// The browser panes' pages, closed in Chrome at the end whatever happened.
    pages: Vec<String>,
    /// The demo's folder once it exists: processes still working in it are ended at the end.
    demo: Option<PathBuf>,
    /// Chrome's windows before the shoot opened its page: the docked one is the new one.
    chrome_before: Option<Vec<u32>>,
}

fn run(opts: Opts) -> Result<()> {
    let dirs = Dirs::new()?;
    let story_path = opts.story.canonicalize().with_context(|| format!("story {}", opts.story.display()))?;
    let story = Story::parse(&std::fs::read_to_string(&story_path)?)?;
    let repo = repo_of(&story_path)?;
    let bin = opts.bin.canonicalize().with_context(|| format!("binaries in {}", opts.bin.display()))?;
    for name in ["arkos", "arkosd"] {
        if !bin.join(name).is_file() {
            bail!("{} is missing; build it first (scripts/shoot.sh does)", bin.join(name).display());
        }
    }
    std::fs::create_dir_all(&opts.out)?;
    let out = opts.out.canonicalize()?;
    clean(&dirs)?;
    // A previous run that was killed may have left its page open in Chrome, or a process in
    // the demo folder (which is about to be remade).
    for url in story.panes.iter().filter_map(|p| p.browser.as_deref()) {
        close_chrome_windows(url);
    }
    let demo_dir = match &story.demo.dir {
        Some(d) => PathBuf::from(expand_home(d)),
        None => dirs.cache.join(&story.demo.name),
    };
    if let Ok(existing) = demo_dir.canonicalize() {
        end_processes_in(&existing);
    }
    std::fs::create_dir_all(&dirs.state)?;
    std::fs::create_dir_all(dirs.config_home.join("arkos"))?;

    let app = match &opts.app {
        Some(a) => a.canonicalize().with_context(|| format!("app bundle {}", a.display()))?,
        None => make_bundle(&repo, &bin, &dirs.cache.join("app"))?,
    };
    install_signal_handlers();
    let mut running = Running { pages: story.panes.iter().filter_map(|p| p.browser.clone()).collect(), ..Running::default() };
    let result = shoot(&story, &repo, &bin, Some(&app), &dirs, &out, &mut running);
    stop(&mut running, &dirs);
    // The verdict as a file too: started through Launch Services (scripts/shoot.sh), the
    // process has no exit code anyone sees.
    let progress = out.join("progress").join(&story.scene.name);
    let _ = std::fs::create_dir_all(&progress);
    let _ = std::fs::write(progress.join("result.txt"), match &result {
        Ok(()) => "ok\n".to_string(),
        Err(e) => format!("failed: {e:#}\n"),
    });
    result
}

fn shoot(story: &Story, repo: &Path, bin: &Path, app: Option<&Path>, dirs: &Dirs, out: &Path, running: &mut Running) -> Result<()> {
    // 1. The demo project, fresh.
    let demo = match &story.demo.dir {
        Some(d) => PathBuf::from(expand_home(d)),
        None => dirs.cache.join(&story.demo.name),
    };
    let worktrees = prepare_demo(&repo.join(&story.demo.fixture), &demo, story, &bin.join("arkos"))?;
    let demo = demo.canonicalize()?;
    running.demo = Some(demo.clone());
    say(&format!("demo project at {}", demo.display()));

    // 2. Its site, for the browser pane.
    let site = demo.join(&story.demo.site);
    if let (Some(port), true) = (story.demo.port, site.is_dir()) {
        running.site = Some(
            Command::new("python3")
                .args(["-m", "http.server", &port.to_string(), "--bind", "127.0.0.1", "--directory"])
                .arg(&site)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .context("python3 -m http.server")?,
        );
        std::fs::write(dirs.state.join("site.pid"), running.site.as_ref().map(|s| s.id()).unwrap_or(0).to_string())?;
    }

    // 3. The window's config: the first shot's theme, the storyboard's text size and sidebar,
    //    no login item and no update checks.
    write_config(&dirs.config_file(), story, &story.shots[0].theme)?;

    // 4. The shoot's own daemon.
    let log = std::fs::File::create(dirs.state.join("arkosd.log"))?;
    let mut daemon = Command::new(bin.join("arkosd"));
    // The daemon's environment is every shell's, and so every demo Claude Code's: nothing of
    // the session this runs from may reach them (a CLAUDE_CODE_* marker makes Claude Code
    // treat the demo as a child session; ARKOS_SESSION_ID would aim its hooks at the
    // wrong tab).
    for (key, _) in std::env::vars_os() {
        let k = key.to_string_lossy();
        if k.starts_with("CLAUDE") || k.starts_with("ARKOS_") {
            daemon.env_remove(&key);
        }
    }
    let daemon = daemon
        .arg("--socket")
        .arg(dirs.socket())
        .args(["--check-every-secs", "0"])
        .env("ARKOS_APP_SUPPORT", &dirs.state)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .context("start arkosd")?;
    std::fs::write(dirs.state.join("daemon.pid"), daemon.id().to_string())?;
    running.daemon = Some(daemon);
    let mut client = wait_for_daemon(&dirs.socket())?;
    say(&format!("daemon {} up", client.hello()?));

    // 5. A shell per terminal pane, before the window exists, so the layout can name them.
    let mut sessions = BTreeMap::new();
    for pane in story.panes.iter().filter(|p| p.is_terminal()) {
        let cwd = pane.dir(&demo);
        let id = client.create(&cwd)?;
        running.sessions.push(id);
        sessions.insert(pane.name.clone(), id.0);
    }
    running.client = Some(client);
    let client = running.client.as_mut().expect("set above");

    // 6. The workspace file the window opens with. The window centres itself on the display
    //    (the file names no display it knows); a frame that would run under the Dock or the menu
    //    bar is narrowed or shortened first, since the Dock draws over any window.
    let screen = Screen::main().unwrap_or(Screen { w: 1470.0, h: 956.0, left: 0.0, top: 38.0, right: 1470.0, bottom: 956.0 });
    let (w, h) = screen.fit_centred(story.window.width, story.window.height);
    if (w, h) != (story.window.width, story.window.height) {
        say(&format!("window {}x{} would run under the Dock or the menu bar; using {w}x{h}", story.window.width, story.window.height));
    }
    let mut frame = story::centred(w, h, screen.w, screen.h, screen.top);
    let ws_file = story::workspace_file(&dirs.state.join("workspaces"), &demo);
    std::fs::create_dir_all(ws_file.parent().expect("workspaces dir"))?;
    std::fs::write(&ws_file, serde_json::to_string_pretty(&story::workspace_json(story, &demo, &sessions, &worktrees, frame))?)?;

    // 7. The window, from the dev build, on the demo.
    let window_pid = start_window(bin, app, &demo, dirs)?;
    std::fs::write(dirs.state.join("window.pid"), window_pid.to_string())?;
    running.window_pid = Some(window_pid);
    // The window notes where it opened; that frame is the one to capture.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        pause(Duration::from_millis(500))?;
        if let Some(f) = saved_frame(&ws_file) {
            frame = f;
            break;
        }
        if Instant::now() > deadline {
            break;
        }
    }
    say(&format!("window at {:?}", frame));
    if !activate(window_pid) && app.is_none() {
        say("could not bring the window to the front; it may be behind the app in use");
    }
    pause(Duration::from_secs(2))?;
    let window_id = arkos_window_id(repo, window_pid)?;
    let progress = Progress { dir: out.join("progress").join(&story.scene.name), window_id, started: Instant::now(), last: Some(std::cell::Cell::new(Instant::now() - Duration::from_secs(60))) };
    let _ = std::fs::remove_dir_all(&progress.dir);
    std::fs::create_dir_all(&progress.dir)?;

    // 8. Type into the panes: plain commands at once; `claude`, then the prompt once it has
    //    drawn. A folder with pre-approved permissions in .claude/settings.json gets Claude
    //    Code's trust question with "No, exit" selected: Down then Enter says yes to it. Without
    //    the question (the folder was trusted on an earlier run) the keys land on an empty
    //    prompt and do nothing.
    for pane in &story.panes {
        if let Some(cmd) = &pane.run {
            client.input(SessionId(sessions[&pane.name]), &format!("{cmd}\r"))?;
        }
    }
    let claude: Vec<_> = story.panes.iter().filter(|p| p.claude.is_some()).collect();
    for pane in &claude {
        client.input(SessionId(sessions[&pane.name]), "claude\r")?;
    }
    if !claude.is_empty() {
        pause(Duration::from_secs(6))?;
        for pane in &claude {
            client.input(SessionId(sessions[&pane.name]), "\x1b[B")?;
            pause(Duration::from_millis(300))?;
            client.input(SessionId(sessions[&pane.name]), "\r")?;
        }
        pause(Duration::from_secs(4))?;
        for pane in &claude {
            let id = SessionId(sessions[&pane.name]);
            client.input(id, pane.claude.as_deref().expect("filtered"))?;
            pause(Duration::from_millis(400))?;
            client.input(id, "\r")?;
            say(&format!("{}: prompt sent", pane.name));
        }
    }

    // 9. The shots.
    let mut theme = story.shots[0].theme.clone();
    let mut failures = Vec::new();
    for shot in &story.shots {
        if shot.theme != theme {
            write_config(&dirs.config_file(), story, &shot.theme)?;
            theme = shot.theme.clone();
            pause(Duration::from_millis(1500))?;
        }
        let unmet = wait_for(shot, story, &sessions, client, &mut running.chrome_url, &mut running.chrome_before, repo, &progress, &demo)?;
        if !unmet.is_empty() {
            say(&format!("{}: after {} s these were not as asked: {}", shot.name, shot.timeout_secs, unmet.join(", ")));
            failures.push(format!("{} ({})", shot.name, unmet.join(", ")));
        }
        // The captures themselves stay with the progress files; `out` gets the framed pictures.
        let picture = format!("{}-{}", story.scene.name, shot.name);
        let raw = progress.dir.join(format!("{picture}.raw.png"));
        capture_steady(repo, window_id, &raw)?;
        // The Chrome window Arkos docked is another app's window: captured on its own and laid
        // over the pane it sits on.
        let overlay = match running.chrome_before.as_deref().map(|before| docked_chrome(repo, frame, before)).transpose()?.flatten() {
            Some((id, x, y)) => {
                let path = progress.dir.join(format!("{picture}.chrome.png"));
                capture_window(id, &path)?;
                Some((path, x - frame.0, y - frame.1))
            }
            None => None,
        };
        frame_image(repo, &raw, &out.join(format!("{picture}.png")), shot, overlay.as_ref())?;
        say(&format!("{}: {}", shot.name, out.join(format!("{picture}.png")).display()));
    }
    if !failures.is_empty() {
        bail!("taken, but not as the story asked: {}", failures.join("; "));
    }
    Ok(())
}

/// Polls the sessions' states until the shot's are all as asked (or its timeout passes), and
/// opens the browser pane's page in Chrome the first time a session works, which is when
/// Arkos docks a new Chrome window. Returns what was not as asked.
#[allow(clippy::too_many_arguments)]
fn wait_for(shot: &Shot, story: &Story, sessions: &BTreeMap<String, u64>, client: &mut Daemon, chrome_url: &mut Option<String>, chrome_before: &mut Option<Vec<u32>>, repo: &Path, progress: &Progress, demo: &Path) -> Result<Vec<String>> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(shot.timeout_secs);
    let mut settle_until = None;
    let claude_panes: Vec<&str> = story.panes.iter().filter(|p| p.claude.is_some()).map(|p| p.name.as_str()).collect();
    loop {
        let states = client.states()?;
        let state_of = |name: &str| sessions.get(name).and_then(|id| states.get(id)).copied().unwrap_or(ClaudeState::Idle);
        progress.note(&shot.name, &story.panes.iter().filter(|p| p.is_terminal()).map(|p| format!("{}={}", p.name, state_name(state_of(&p.name)))).collect::<Vec<_>>().join(" "), chrome_url.is_some());
        // Claude Code says it is working within seconds of a prompt (its UserPromptSubmit
        // hook). Every pane still idle a minute on: it never started, and waiting is waste.
        if !claude_panes.is_empty() && started.elapsed() > Duration::from_secs(60) && claude_panes.iter().all(|p| state_of(p) == ClaudeState::Idle) {
            bail!("{}: no Claude Code pane has reported working after a minute; the latest capture in {} shows what happened", shot.name, progress.dir.display());
        }
        if shot.chrome && chrome_url.is_none() && states.values().any(|s| *s == ClaudeState::Working) {
            if let Some(url) = story.panes.iter().find_map(|p| p.browser.clone()) {
                *chrome_before = Some(window_list(repo)?.into_iter().filter(|w| w.owner == "Google Chrome").map(|w| w.id).collect());
                // -g: Chrome does not come to the front, so the Arkos window keeps its place.
                Command::new("open").args(["-g", "-na", "Google Chrome", "--args", &format!("--app={url}")]).status().context("open Chrome")?;
                *chrome_url = Some(url);
                settle_until = Some(Instant::now() + Duration::from_secs(4));
                say("opened the page in Chrome for Arkos to dock");
            }
        }
        let mut unmet: Vec<String> = shot.wait.iter().filter(|(pane, want)| state_name(state_of(pane)) != want.as_str()).map(|(pane, want)| format!("{pane} is {}, not {want}", state_name(state_of(pane)))).collect();
        for (pane, want) in &shot.changed {
            let n = changed_files(&story.pane(pane).dir(demo));
            if n < *want {
                unmet.push(format!("{pane} has changed {n} files, not {want}"));
            }
        }
        let chrome_pending = shot.chrome && chrome_url.is_none();
        let settling = settle_until.is_some_and(|t| Instant::now() < t);
        let too_early = progress.started.elapsed() < Duration::from_secs(shot.after_secs);
        if unmet.is_empty() && !chrome_pending && !settling && !too_early {
            return Ok(Vec::new());
        }
        if Instant::now() > deadline {
            let mut all = unmet;
            if chrome_pending {
                all.push("no session worked, so Chrome was never opened".into());
            }
            return Ok(all);
        }
        pause(Duration::from_millis(500))?;
    }
}

/// Makes the window's process the active app. A process started from a shell opens its window
/// behind the app in use (macOS activates only what the user, the Dock or Launch Services
/// started), and the capture needs it in front. A few tries: the app registers with the window
/// server a moment after it starts.
fn activate(pid: u32) -> bool {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    const IGNORING_OTHER_APPS: NSApplicationActivationOptions = NSApplicationActivationOptions(1 << 1);
    for _ in 0..10 {
        if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32) {
            if app.activateWithOptions(IGNORING_OTHER_APPS) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    false
}

/// How many files `git status` lists as changed or new in `dir` (0 outside a repository).
fn changed_files(dir: &Path) -> usize {
    Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(dir)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

/// `screencapture` of one window by its id, without its shadow: the window as it is, in front
/// or behind whatever else is open. It needs the Screen Recording permission for the app this
/// runs in (a terminal, or Arkos itself); without it the file shows nothing of the window.
fn capture_window(id: u32, out: &Path) -> Result<()> {
    for attempt in 1..=4 {
        let status = Command::new("screencapture").args(["-x", "-o", "-l", &id.to_string()]).arg(out).status().context("screencapture")?;
        if status.success() && out.is_file() {
            return Ok(());
        }
        say(&format!("screencapture of window {id} failed (try {attempt})"));
        std::thread::sleep(Duration::from_secs(1));
    }
    bail!("screencapture failed for {}", out.display())
}

/// A capture of a frame that holds still: two captures 400 ms apart, kept when under 0.4% of
/// the pixels differ (a spinner and a clock tick pass; a pane mid-repaint, which Claude Code
/// does whole lines at a time, does not), tried up to eight times before the last one is kept
/// with a note.
fn capture_steady(repo: &Path, id: u32, out: &Path) -> Result<()> {
    let other = out.with_extension("b.png");
    for attempt in 1..=8 {
        capture_window(id, out)?;
        std::thread::sleep(Duration::from_millis(400));
        capture_window(id, &other)?;
        let diff = Command::new("python3").arg("-I").arg(repo.join("scripts/shoot/diff.py")).arg(out).arg(&other).output().context("diff.py")?;
        let fraction: f64 = String::from_utf8_lossy(&diff.stdout).trim().parse().unwrap_or(1.0);
        if fraction < 0.004 {
            let _ = std::fs::rename(&other, out);
            return Ok(());
        }
        say(&format!("capture {attempt}: {:.1}% of the window changed between two frames; again", fraction * 100.0));
    }
    say("no still frame in eight tries; keeping the last");
    let _ = std::fs::rename(&other, out);
    Ok(())
}

/// A window the window server lists: `scripts/shoot/windows.swift`'s line.
struct Win {
    id: u32,
    pid: u32,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    owner: String,
}

fn window_list(repo: &Path) -> Result<Vec<Win>> {
    let out = Command::new("swift").arg(repo.join("scripts/shoot/windows.swift")).output().context("swift (the Xcode command line tools)")?;
    if !out.status.success() {
        bail!("windows.swift failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(8, ' ').collect();
            if f.len() < 8 {
                return None;
            }
            Some(Win { id: f[0].parse().ok()?, pid: f[1].parse().ok()?, x: f[2].parse().ok()?, y: f[3].parse().ok()?, w: f[4].parse().ok()?, h: f[5].parse().ok()?, owner: f[7].to_string() })
        })
        .collect())
}

/// The project window of the shoot's arkos: its one window wider than a pane.
fn arkos_window_id(repo: &Path, pid: u32) -> Result<u32> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        // Not its menu bar's window (full width, 33 points high): the biggest one of some height.
        if let Some(w) = window_list(repo)?.into_iter().filter(|w| w.pid == pid && w.w > 400.0 && w.h > 200.0).max_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h))) {
            return Ok(w.id);
        }
        if Instant::now() > deadline {
            bail!("the window server lists no window for arkos (pid {pid})");
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The Chrome window Arkos docked, if any: a Chrome window that was not there before the
/// shoot opened its page (`before`), lying inside the window's frame. Its id and top-left
/// corner in points.
fn docked_chrome(repo: &Path, frame: (f64, f64, f64, f64), before: &[u32]) -> Result<Option<(u32, f64, f64)>> {
    let (fx, fy, fw, fh) = frame;
    let inside = |w: &Win| w.owner == "Google Chrome" && !before.contains(&w.id) && w.x >= fx - 2.0 && w.y >= fy - 2.0 && w.x + w.w <= fx + fw + 2.0 && w.y + w.h <= fy + fh + 2.0;
    Ok(window_list(repo)?.into_iter().find(inside).map(|w| (w.id, w.x, w.y)))
}

/// Progress for whoever watches the run: every 20 s a capture of the window into
/// `<out>/progress/` and a line of the panes' states in `log.txt` there, so a run that has gone
/// wrong shows it long before its timeout.
struct Progress {
    dir: PathBuf,
    window_id: u32,
    started: Instant,
    last: Option<std::cell::Cell<Instant>>,
}

impl Progress {
    const EVERY: Duration = Duration::from_secs(20);

    fn note(&self, shot: &str, states: &str, chrome_opened: bool) {
        let due = match &self.last {
            Some(last) => last.get().elapsed() >= Self::EVERY,
            None => true,
        };
        if !due {
            return;
        }
        let secs = self.started.elapsed().as_secs();
        let file = self.dir.join(format!("{shot}-{secs:04}s.png"));
        let captured = capture_window(self.window_id, &file).is_ok();
        let line = format!("{secs:4}s {shot}: {states}{}{}", if chrome_opened { " chrome=opened" } else { "" }, if captured { "" } else { " (capture failed)" });
        if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("log.txt")) {
            use std::io::Write;
            let _ = writeln!(log, "{line}");
        }
        say(&line);
        match &self.last {
            Some(last) => last.set(Instant::now()),
            None => {}
        }
    }
}

/// `scripts/shoot/frame.py`: the raw capture framed as a macOS window on a transparent
/// background, plus the shot's crops.
fn frame_image(repo: &Path, raw: &Path, out: &Path, shot: &Shot, overlay: Option<&(PathBuf, f64, f64)>) -> Result<()> {
    let mut cmd = Command::new("python3");
    cmd.arg("-I").arg(repo.join("scripts/shoot/frame.py")).arg(raw).arg(out);
    if let Some((path, x, y)) = overlay {
        // Points to the capture's pixels: Retina, 2 to 1.
        cmd.arg("--overlay").arg(format!("{}:{},{}", path.display(), (x * 2.0).round(), (y * 2.0).round()));
    }
    for c in &shot.crops {
        let [x, y, w, h] = c.rect;
        cmd.arg("--crop").arg(format!("{}:{x},{y},{w},{h}", c.name));
    }
    let status = cmd.status().context("frame.py")?;
    if !status.success() {
        bail!("frame.py failed for {}", raw.display());
    }
    Ok(())
}

/// The window's frame as it noted it (`WindowSaved`), once the file carries one.
fn saved_frame(ws_file: &Path) -> Option<(f64, f64, f64, f64)> {
    let text = std::fs::read_to_string(ws_file).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let w = v.get("window")?;
    if w.get("display")?.as_str()? == "shoot" {
        return None;
    }
    Some((w["x"].as_f64()?, w["y"].as_f64()?, w["w"].as_f64()?, w["h"].as_f64()?))
}

/// The main display in points, top-left origin: its size, and the edges of the area the menu
/// bar and the Dock leave (AppKit's visible frame).
#[derive(Debug, Clone, Copy)]
struct Screen {
    w: f64,
    h: f64,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Screen {
    fn main() -> Option<Screen> {
        use objc2_app_kit::NSScreen;
        let mtm = objc2::MainThreadMarker::new()?;
        let screen = NSScreen::mainScreen(mtm).or_else(|| NSScreen::screens(mtm).firstObject())?;
        let (f, v) = (screen.frame(), screen.visibleFrame());
        // AppKit's origin is the bottom-left corner.
        let top = (f.origin.y + f.size.height) - (v.origin.y + v.size.height);
        Some(Screen { w: f.size.width, h: f.size.height, left: v.origin.x - f.origin.x, top, right: v.origin.x - f.origin.x + v.size.width, bottom: top + v.size.height })
    }

    /// The largest `w` by `h` (or smaller) that, centred on the display as the window will
    /// be, stays inside the visible area.
    fn fit_centred(&self, w: f64, h: f64) -> (f64, f64) {
        let side = self.left.max(self.w - self.right);
        let max_w = (self.w - 2.0 * side).floor();
        // Vertically the window is centred too, then pushed down from under the menu bar, so
        // only the Dock's side (if it is at the bottom) and the menu bar together bound it.
        let max_h = (self.h - self.top - (self.h - self.bottom)).floor();
        (w.min(max_w).max(200.0), h.min(max_h).max(200.0))
    }
}

fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => path.to_string(),
    }
}

fn wait_for_daemon(socket: &Path) -> Result<Daemon> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if socket.exists() {
            if let Ok(d) = Daemon::connect(socket) {
                return Ok(d);
            }
        }
        if Instant::now() > deadline {
            bail!("arkosd did not come up on {}", socket.display());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The window's config for the shoot: the storyboard's text size and sidebar, the shot's
/// theme, and nothing that reaches outside (no login item, no update checks).
fn write_config(path: &Path, story: &Story, theme: &str) -> Result<()> {
    let text = format!(
        "# Written by arkos-shoot for each run; edit scripts/shoot/story.toml instead.\n\
         [font]\npreset = \"{preset}\"\n\n\
         [sidebar]\nwidth = {sidebar:.1}\ncollapsed = {collapsed}\n\n\
         [theme]\ndark = \"{dark}\"\nlight = \"{light}\"\n\n\
         [appearance]\nmode = \"{theme}\"\n\n\
         [recovery]\nstart_at_login = \"off\"\n\n\
         [update]\nauto = false\ncheck_every_secs = 0\n",
        preset = story.window.preset,
        sidebar = story.window.sidebar_width,
        collapsed = story.window.sidebar == story::Sidebar::Collapsed,
        dark = story.window.theme.dark,
        light = story.window.theme.light,
    );
    std::fs::write(path, text).with_context(|| format!("write {}", path.display()))
}

/// The demo: the fixture copied and made a repository with a couple of commits, a worktree per
/// name, and Claude Code's project settings: Arkos's hooks aimed at the dev binary (the user's
/// own, aimed at the installed app, reach the other daemon and do nothing) and the permissions
/// a session needs to work without asking.
fn prepare_demo(fixture: &Path, demo: &Path, story: &Story, arkos_exe: &Path) -> Result<BTreeMap<String, PathBuf>> {
    if !fixture.is_dir() {
        bail!("fixture {} is not a folder", fixture.display());
    }
    // The copy is remade every run; a folder that is not ours is never touched.
    if demo.exists() && !demo.join(".arkos-shoot").exists() && demo.read_dir().map(|mut d| d.next().is_some()).unwrap_or(true) {
        bail!("{} exists and was not made by the shoot; remove it or point demo.dir elsewhere", demo.display());
    }
    if demo.exists() {
        std::fs::remove_dir_all(demo)?;
    }
    if story.demo.only.is_empty() {
        copy_dir(fixture, demo)?;
    } else {
        std::fs::create_dir_all(demo)?;
        for entry in &story.demo.only {
            let from = fixture.join(entry);
            if !from.is_dir() {
                bail!("demo.only names {entry:?}, which the fixture does not have");
            }
            copy_dir(&from, &demo.join(entry))?;
        }
    }
    std::fs::write(demo.join(".arkos-shoot"), "made by arkos-shoot; deleted and remade on each run\n")?;
    let git = |dir: &Path, args: &[&str]| -> Result<()> {
        let status = Command::new("git")
            .args(["-c", "user.name=Demo", "-c", "user.email=demo@example.com", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(dir)
            .stdout(Stdio::null())
            .status()
            .context("git")?;
        if !status.success() {
            bail!("git {} failed in {}", args.join(" "), dir.display());
        }
        Ok(())
    };
    for repo in &story.demo.repos {
        let dir = demo.join(repo);
        if !dir.is_dir() {
            bail!("demo.repos names {repo:?}, which the fixture does not have");
        }
        git(&dir, &["init", "-q"])?;
        // Two commits when there is a site to add, so the history reads as a project's
        // rather than an import's; one otherwise.
        let site = &story.demo.site;
        let title = first_heading(&dir.join("README.md")).unwrap_or_else(|| "Initial import".into());
        if dir.join(site).is_dir() {
            git(&dir, &["add", "-A", "--", ".", &format!(":!{site}")])?;
            git(&dir, &["commit", "-q", "-m", &title])?;
            git(&dir, &["add", "-A"])?;
            git(&dir, &["commit", "-q", "-m", "Serve the landing page"])?;
        } else {
            git(&dir, &["add", "-A"])?;
            git(&dir, &["commit", "-q", "-m", &title])?;
        }
    }
    let mut worktrees = BTreeMap::new();
    for name in &story.demo.worktrees {
        let dir = demo.join(".worktrees").join(name);
        git(demo, &["worktree", "add", "-q", "-b", name, dir.to_str().context("worktree path")?])?;
        worktrees.insert(name.clone(), dir.canonicalize()?);
    }
    // Claude Code reads .claude/settings.json in the folder it runs in: every pane's folder
    // gets one (the worktree's is its own; a worktree's checkout does not carry the main one's).
    let settings = claude_settings(arkos_exe);
    let mut dirs: Vec<PathBuf> = story.panes.iter().filter(|p| p.is_terminal()).map(|p| p.dir(demo)).collect();
    dirs.push(demo.to_path_buf());
    dirs.sort();
    dirs.dedup();
    for dir in dirs {
        std::fs::create_dir_all(dir.join(".claude"))?;
        std::fs::write(dir.join(".claude/settings.json"), serde_json::to_string_pretty(&settings)?)?;
    }
    Ok(worktrees)
}

/// A README's first heading, for the first commit's message.
fn first_heading(readme: &Path) -> Option<String> {
    std::fs::read_to_string(readme).ok()?.lines().find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
}

/// The hooks `arkos hooks install` writes (hook_cli.rs), for the dev binary, and permissions.
fn claude_settings(arkos_exe: &Path) -> serde_json::Value {
    let exe = arkos_exe.display().to_string().replace('\'', "'\\''");
    let cmd = |tail: &str| serde_json::json!({ "type": "command", "command": format!("'{exe}' notify {tail}"), "timeout": 5 });
    serde_json::json!({
        "permissions": {
            // Every shell command: a pattern list still stops at one with a variable in it, and
            // a session stopped at a permission question is not working. The demo is a copy.
            "allow": ["Read", "Edit", "Write", "MultiEdit", "Glob", "Grep", "Bash", "WebSearch", "WebFetch"],
            "defaultMode": "acceptEdits"
        },
        "hooks": {
            "SessionStart": [{ "hooks": [cmd("--event session-start")] }],
            "SessionEnd": [{ "hooks": [cmd("--event session-end")] }],
            "UserPromptSubmit": [{ "hooks": [cmd("--state working")] }],
            "PreToolUse": [{ "hooks": [cmd("--state working")] }],
            "Notification": [{ "matcher": "permission_prompt|idle_prompt", "hooks": [cmd("--state waiting")] }],
            "Stop": [{ "hooks": [cmd("--state waiting")] }]
        }
    })
}

/// The dev binaries in the release's bundle shape (`packaging/bundle.sh` without the zip), at a
/// fixed place, with its own bundle id so macOS keeps its permissions apart from the installed
/// app's: Launch Services starts a bundle, and the browser pane needs Accessibility.
fn make_bundle(repo: &Path, bin: &Path, dir: &Path) -> Result<PathBuf> {
    let app = dir.join("Arkos IDE.app");
    let cargo = std::fs::read_to_string(repo.join("Cargo.toml")).with_context(|| format!("read {}", repo.join("Cargo.toml").display()))?;
    let version = cargo.lines().find_map(|l| l.strip_prefix("version = \"").and_then(|v| v.strip_suffix('"'))).context("version in Cargo.toml")?;
    let template = repo.join("packaging/Info.plist.in");
    // Its own bundle id and name: macOS keeps permissions (Accessibility, for the browser pane)
    // by app, and the installed Arkos IDE's must not be mistaken for this bundle's.
    let plist = std::fs::read_to_string(&template)
        .with_context(|| format!("read {}", template.display()))?
        .replace("@VERSION@", version)
        .replace("<string>sh.csarko.arkos</string>", "<string>sh.csarko.arkos.shoot</string>")
        .replace("<string>Arkos IDE</string>", "<string>Arkos IDE (shoot)</string>");
    std::fs::create_dir_all(app.join("Contents/MacOS")).with_context(|| format!("create {}", app.display()))?;
    std::fs::create_dir_all(app.join("Contents/Resources"))?;
    std::fs::write(app.join("Contents/Info.plist"), plist)?;
    for name in ["arkos", "arkosd"] {
        let to = app.join("Contents/MacOS").join(name);
        // A fresh file, not an overwrite: the running copy of a previous run may still be mapped.
        let _ = std::fs::remove_file(&to);
        std::fs::copy(bin.join(name), &to).with_context(|| format!("copy {name} into the bundle"))?;
    }
    let icon = repo.join("packaging/AppIcon.icns");
    if icon.is_file() {
        std::fs::copy(icon, app.join("Contents/Resources/AppIcon.icns"))?;
    }
    // Signed with the Developer ID (the release's identity, hardened runtime and all) when the
    // keychain holds one: then the bundle's identity is the certificate, not the binary's hash,
    // and macOS keeps the Accessibility grant across rebuilds. Unsigned otherwise, with the
    // grant to redo each time.
    let identities = Command::new("security").args(["find-identity", "-v", "-p", "codesigning"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let identity = identities.lines().find_map(|l| l.split('"').nth(1).filter(|n| n.starts_with("Developer ID Application:")).map(str::to_string));
    match identity {
        Some(identity) => {
            for target in [app.join("Contents/MacOS/arkosd"), app.join("Contents/MacOS/arkos"), app.clone()] {
                let status = Command::new("codesign").args(["--force", "--options", "runtime", "--timestamp", "--sign", &identity, "--identifier", "sh.csarko.arkos.shoot"]).arg(&target).stdout(Stdio::null()).stderr(Stdio::null()).status().context("codesign")?;
                if !status.success() {
                    bail!("codesign as {identity:?} failed for {}", target.display());
                }
            }
        }
        None => say("no Developer ID Application identity in the keychain: the bundle is unsigned, and macOS forgets its Accessibility grant at every rebuild"),
    }
    Ok(app)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Ends what the run started: the sessions, the window, the daemon, the site, and the Chrome
/// window Arkos docked (an Apple event to Chrome; when macOS refuses it, the window stays).
fn stop(running: &mut Running, dirs: &Dirs) {
    if let Some(client) = running.client.as_mut() {
        for id in &running.sessions {
            if let Err(e) = client.kill(*id) {
                say(&format!("session {}: {e}", id.0));
            }
        }
        std::thread::sleep(Duration::from_millis(1500));
    }
    if let Some(pid) = running.window_pid.take() {
        // SIGTERM: the window quits without asking.
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        let gone = Instant::now() + Duration::from_secs(5);
        while unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 && Instant::now() < gone {
            std::thread::sleep(Duration::from_millis(100));
        }
        if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
    if let Some(mut d) = running.daemon.take() {
        // arkosd ignores SIGTERM by design; its sessions are already gone.
        let _ = d.kill();
        let _ = d.wait();
    }
    if let Some(mut s) = running.site.take() {
        let _ = s.kill();
        let _ = s.wait();
    }
    // Every page the run may have opened, whether it got to the capture or not.
    for url in running.pages.drain(..) {
        close_chrome_windows(&url);
    }
    // Whatever a session left running in the demo (a server a Claude Code smoke test started in
    // the background outlives its shell).
    if let Some(demo) = running.demo.take() {
        end_processes_in(&demo);
    }
    for f in ["daemon.pid", "window.pid", "site.pid"] {
        let _ = std::fs::remove_file(dirs.state.join(f));
    }
}

/// Starts the window on `demo` and returns its pid. Through Launch Services when there is an
/// app bundle (`open -n -a`, with the shoot's environment and the folder as the argument:
/// an app started that way comes to the front, one started from a shell opens behind the app
/// in use); the bare binary otherwise.
fn start_window(bin: &Path, app: Option<&Path>, demo: &Path, dirs: &Dirs) -> Result<u32> {
    let log = std::fs::File::create(dirs.state.join("window.log"))?;
    let Some(app) = app else {
        let child = Command::new(bin.join("arkos"))
            .current_dir(demo)
            .env("ARKOS_APP_SUPPORT", &dirs.state)
            .env("XDG_CONFIG_HOME", &dirs.config_home)
            .env_remove("ARKOS_RELAUNCH")
            .env_remove("ARKOS_SESSION_ID")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .context("start arkos")?;
        return Ok(child.id());
    };
    let exe = app.join("Contents/MacOS/arkos");
    if !exe.is_file() {
        bail!("{} has no arkos binary", app.display());
    }
    let status = Command::new("open")
        .arg("-n")
        .arg("--env")
        .arg(format!("ARKOS_APP_SUPPORT={}", dirs.state.display()))
        .arg("--env")
        .arg(format!("XDG_CONFIG_HOME={}", dirs.config_home.display()))
        // Above the other windows for the scene: a window covered entirely is not drawn, and
        // its captures would repeat its last frame.
        .args(["--env", "ARKOS_WINDOW_LEVEL=floating"])
        .arg("--stderr")
        .arg(dirs.state.join("window.log"))
        .arg("-a")
        .arg(app)
        .arg("--args")
        .arg(demo)
        .status()
        .context("open the app bundle")?;
    if !status.success() {
        bail!("open {} failed", app.display());
    }
    // `open` returns at once; the process shows up a moment later.
    let pattern = exe.display().to_string();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let out = Command::new("pgrep").args(["-f", &pattern]).output().context("pgrep")?;
        let pids: Vec<u32> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect();
        if let Some(pid) = pids.into_iter().min() {
            return Ok(pid);
        }
        if Instant::now() > deadline {
            bail!("the window did not start from {}", app.display());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Ends every process whose working directory is inside `dir` (lsof lists them): the demo's
/// own shells are gone by now, so these are what a session started and left behind.
fn end_processes_in(dir: &Path) {
    let Ok(out) = Command::new("lsof").args(["-d", "cwd", "-Fpn"]).output() else { return };
    let text = String::from_utf8_lossy(&out.stdout);
    let (mut pid, mut ended) = (None, 0);
    let me = std::process::id();
    for line in text.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse::<u32>().ok();
        } else if let Some(cwd) = line.strip_prefix('n') {
            if let Some(p) = pid.filter(|p| *p != me && *p > 1) {
                if Path::new(cwd).starts_with(dir) {
                    unsafe { libc::kill(p as libc::pid_t, libc::SIGTERM) };
                    ended += 1;
                }
            }
        }
    }
    if ended > 0 {
        say(&format!("ended {ended} process(es) a session left running in {}", dir.display()));
    }
}

/// Closes Chrome's windows on `url` (the page the shoot opened, docked or not): an Apple
/// event to Chrome, which macOS may refuse until the Automation permission is granted; then
/// the windows stay and the run says so.
fn close_chrome_windows(url: &str) {
    let script = format!("tell application \"Google Chrome\" to close (every window whose URL of active tab starts with \"{url}\")");
    let ok = Command::new("osascript").args(["-e", &script]).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        say(&format!("could not close Chrome's {url} windows (Automation permission for Chrome?); close them yourself"));
    }
}

/// Ends a previous run's daemon and window if they are still there, and clears the state.
fn clean(dirs: &Dirs) -> Result<()> {
    for f in ["window.pid", "daemon.pid", "site.pid"] {
        if let Ok(text) = std::fs::read_to_string(dirs.state.join(f)) {
            if let Ok(pid) = text.trim().parse::<i32>() {
                if pid > 1 {
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                    say(&format!("ended the previous run's {} ({pid})", f.trim_end_matches(".pid")));
                }
            }
        }
    }
    if dirs.state.exists() {
        std::fs::remove_dir_all(&dirs.state)?;
    }
    Ok(())
}
