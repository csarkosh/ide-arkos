# Video Pane and `arkos open` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A video is a tab in Arkos (drawn in process, with controls, following its file on disk), and an agent in an Arkos terminal can open a video or a document in a pane beside itself and drive the video with `arkos open`, `arkos video` and `arkos panes`.

**Architecture:** `Tab::Video` joins `Tab` and is persisted with its position and loop. A pure `video.rs` holds the playback model behind a `Player` trait (the real `AvPlayer` wraps AVFoundation and hands frames to gpui's `surface()` as `'420f'` pixel buffers); `video_element.rs` draws the picture and the control strip. The daemon gains a targeted relay (`Request::Direct`) that reaches one window, resolved from the agent's `ARKOS_SESSION_ID`; `direct.rs` in the window plans the placement and answers; `open_cli.rs` is the agent's command line. The guide, hook line and skill description announce it.

**Tech Stack:** Rust 2021, gpui 0.2.2 (unchanged; its `surface` element), `objc2-av-foundation` 0.3.2, `objc2-core-media` 0.3.2, `objc2-core-video` 0.3.2, `core-video` 0.4.3 (gpui's own CoreVideo crate, for the `surface()` type), `block2` (the objc2 completion handlers). AVFoundation plays the audio.

**Spec:** `docs/superpowers/specs/2026-10-20-video-pane-design.md` (every section number below is that document's).

## Global Constraints

- Machine rules (the user's MacBook Air locked up from build load): every cargo call is `nice -n 15 ~/.cargo/bin/cargo ...`; the worktree's local `.cargo/config.toml` (`jobs = 3`, shared `target-dir = /Users/csarko/Projects/ide-arkos/target`) stays, copied from the main checkout before the first cargo call; no `--release` builds; never two cargo processes at once (check `ps aux | grep -c '[c]argo\|[r]ustc'` first); focused filters while iterating, the workspace suite once per task; the warning check is `cargo check --workspace --all-targets`.
- New dependencies, only these, in `crates/arkos/Cargo.toml`: `objc2-av-foundation = { version = "0.3.2", default-features = false, features = ["std", "AVPlayer", "AVPlayerItem", "AVPlayerItemOutput", "AVAsset", "AVAsynchronousKeyValueLoading", "objc2-core-media", "objc2-core-video", "block2"] }` plus whatever feature the compiler names for a method used, `objc2-core-media = { version = "0.3.2", default-features = false, features = ["std", "CMTime"] }`, `objc2-core-video = { version = "0.3.2", default-features = false, features = ["std", "CVPixelBuffer", "CVBuffer", "CVImageBuffer"] }`, `core-video = "0.4.3"`, `block2 = "0.6"` (the version objc2 0.6 pairs with; take the one already in `Cargo.lock`). No gpui bump.
- AVFoundation is touched only in `av_player.rs`, only on the UI thread, behind the `Player` trait; `cargo test` never constructs an `AVPlayer`. The pixel buffer attributes `kCVPixelBufferPixelFormatTypeKey = kCVPixelFormatType_420YpCbCr8BiPlanarFullRange`, `kCVPixelBufferIOSurfacePropertiesKey = {}`, `kCVPixelBufferMetalCompatibilityKey = true` are mandatory (gpui unwraps the texture; another format panics the window).
- Protocol: `PROTOCOL_VERSION` 7 to 8, `DIRECT_FROM: u32 = 8`; enum variants appended, never inserted; existing structs never change shape.
- Time on the wire and in JSON is seconds as `f64`; human lines use `m:ss.f` (`h:mm:ss.f` over an hour). Exit codes: 0 done, 1 refused or failed, 2 usage, 3 no window to aim at.
- Nothing is sent anywhere by telemetry that names a file: the new events carry kinds and commands only, listed in `telemetry/events.toml` and `docs/telemetry.md`.
- No em-dashes in any file. Warning-free. Doc comments in plain prose, tests named as sentences, as the tree does. Every `arkos` subcommand and flag the parser accepts is named in `docs/agent-guide.md` (Task 10's test enforces it).

## Review Focus

1. A file ffmpeg is still writing is never loaded: a changing fingerprint waits, a stable one reloads, and a reload whose item fails to load keeps the old item playing (Task 4 `a_changing_file_waits_and_a_stable_one_reloads`, `a_failed_reload_keeps_the_old_item_and_says_so`).
2. The agent's `arkos open` while the user types in the agent's terminal must not move keyboard focus; `--focus` must (Task 8 `an_open_leaves_the_focus_where_it_was_unless_asked`).
3. A session shown in two windows, a session not shown anywhere (closed in its grace), and a plain Terminal with several windows each produce the right window or the right error, never a tab in the wrong window (Task 2 `direct_target_picks_the_newest_window_of_a_session_and_names_the_rest`).
4. A reload to a shorter cut clamps the position and the loop; a loop whose start is past the new end is cleared and said (Task 4 `a_shorter_cut_clamps_the_position_and_clears_a_loop_past_its_end`).
5. A `.mp4` in an old workspace file comes up as a video tab, not a "binary" viewer; a `Tab::Video` saved by this build loads on this build with its position (Task 3 `a_viewer_on_a_video_is_promoted_when_the_workspace_loads`).

## File structure

- `crates/protocol/src/lib.rs`: the targeted channel's types (Task 1).
- `crates/arkosd/src/server.rs`: `direct_target`, `DirectWait`, the `Direct` and `DirectDone` arms (Task 2); `crates/arkosd/tests/daemon.rs`: the end to end relay (Task 2).
- `crates/arkos/src/tree.rs`, `closed_stack.rs`, `workspace.rs`, `telemetry.rs`, `telemetry/events.toml`, `pane_view.rs` (title and a placeholder body): the model (Task 3).
- `crates/arkos/src/video.rs` (new): time parsing and formatting, the follow decision, `Player` trait with `FakePlayer`, `VideoModel` (state machine) (Task 4).
- `crates/arkos/src/av_player.rs` (new), `app_nap.rs` (the display token), `Cargo.toml`: the real player (Task 5).
- `crates/arkos/src/video_view.rs` (new): the `VideoView` entity (model plus player plus frame), and `video_element.rs` (new): the picture, strip, scrubber; `keymap.rs` (Section::Video and actions), `command_palette.rs`, `pane_view.rs` (`render_video`) (Task 6).
- `crates/arkos/src/main.rs`, `sidebar.rs`, `md_view.rs`, `viewer.rs`, `explorer_watch.rs` (none, the poll is reused): the window integrations (Task 7).
- `crates/arkos/src/direct.rs` (new): `plan_open`, the panes report, `Root::handle_direct` (Task 8).
- `crates/arkos/src/open_cli.rs` (new), `agent_cli.rs` (`USAGE`), `main.rs` (dispatch): the command line (Task 9).
- `crates/arkos/src/hook_cli.rs`, `guide.rs`, `doctor.rs`, `docs/agent-guide.md`, `docs/guide.md`, `README.md`, `docs/development.md`, `docs/telemetry.md`: discovery and docs (Task 10).

## Rulings taken while planning

1. `Event::Direct` carries the calling session (`Option<SessionId>`) so the window can anchor the preview pane and mark "(you)" without a second lookup; the daemon fills it from `WindowTarget::Session`.
2. `protocol::SplitSide` mirrors `tree::Side` (the protocol crate cannot depend on the window); `direct.rs` converts.
3. Reuse looks for any tab on the same canonical path, whatever its kind, in the target window (a markdown file open as a viewer is reused too).
4. The "preview pane" map is keyed by `Option<SessionId>`: `None` is the no-session anchor (spec section 5, call 14).
5. The scrubber's loop selection (Shift+drag) is in Task 6; `L` without a selection loops the whole file.
6. `VideoState.pane` is the one-based Cmd+1 number, as `arkos panes` prints; `tab` is the `TabId`.
7. A `Video` op on a window with no video tab answers `Err("no video tab open")`; with several and no path, `Err` listing `pane {n}: {file name}` per line, except `Status`, which returns them all.
8. The real player's frame pull happens in `VideoView`'s render through `window.request_animation_frame()`, matching how `Root::any_motion` keeps other animations alive; the view requests its own frame while playing, so `any_motion` is untouched.

---

### Task 1: The protocol's targeted channel

**Files:**
- Modify: `crates/protocol/src/lib.rs` (constants, new types after `CustomizeOutcome`, appended variants of `Request`, `Response`, `Event`, the ordering test)

**Interfaces:**
- Produces:
  ```rust
  pub const PROTOCOL_VERSION: u32 = 8;
  pub const DIRECT_FROM: u32 = 8;
  pub enum WindowTarget { Session(SessionId), Pid(u32), Only }
  pub enum SplitSide { Left, Right, Top, Bottom }
  pub enum OpenKind { Video, Markdown { raw: bool }, Text }
  pub enum Placement { Preview, Pane(u32), Split(SplitSide) }
  pub enum LoopChange { Keep, Set { start: f64, end: f64 }, WholeFile, Clear }
  pub enum VideoOp { Status, Play, Pause, Seek(f64), Step(i32), Loop(LoopChange), Reload, Close, Mute(bool) }
  pub enum PaneAction {
      Open { path: PathBuf, kind: OpenKind, place: Placement, at: Option<f64>, playing: Option<bool>, loop_change: LoopChange, reuse: bool, wait_for_file: bool, focus: bool },
      Video { path: Option<PathBuf>, op: VideoOp },
      Panes,
  }
  pub struct VideoState { pub pane: u32, pub tab: u64, pub path: PathBuf, pub duration: Option<f64>, pub position: f64, pub playing: bool, pub muted: bool, pub loop_range: Option<(f64, f64)>, pub width: u32, pub height: u32, pub state: String, pub error: Option<String>, pub file_mtime: Option<u64> }
  pub struct TabRow { pub id: u64, pub kind: String, pub title: String, pub active: bool, pub you: bool, pub path: Option<PathBuf>, pub session: Option<SessionId> }
  pub struct PaneRow { pub n: u32, pub id: u64, pub focused: bool, pub tabs: Vec<TabRow> }
  pub struct PanesReport { pub project: PathBuf, pub panes: Vec<PaneRow> }
  pub enum PaneOutcome { Opened { pane: u32, tab: u64, kind: String, reused: bool, reloaded: bool, video: Option<VideoState> }, Videos(Vec<VideoState>), Panes(PanesReport) }
  // appended:
  Request::Direct { id: u64, window: WindowTarget, action: PaneAction }
  Request::DirectDone { id: u64, outcome: Result<PaneOutcome, String> }
  Response::Directed { pid: u32, project: PathBuf, outcome: Result<PaneOutcome, String> }
  Event::Direct { id: u64, action: PaneAction, session: Option<SessionId> }
  ```
  All derive `Debug, Clone, PartialEq, Serialize, Deserialize` (`f64` fields forbid `Eq`; `PaneAction`, `PaneOutcome`, `VideoState` and the enums holding them drop `Eq`; `Request`, `Response` and `Event` drop `Eq` too, so check the few `assert_eq!` sites still compile, which they do since `PartialEq` is enough). `VideoState.state` is one of `"ready" | "loading" | "reloading" | "waiting" | "unplayable"`.

- [ ] **Step 1: Extend the ordering test** in `lib.rs`'s `new_variants_are_appended_after_the_old_ones`: `PROTOCOL_VERSION == 8`; `first_byte(&Request::Direct { .. })` is the index after `SessionQuit`'s, `DirectDone` one more; `Response::Directed` after `TelemetryStatus`; `Event::Direct` after `Event::Customize`. Add `direct_messages_round_trip_through_postcard_and_json` building one `PaneAction::Open` with every field set, one `PaneOutcome::Opened` with a `VideoState`, one `PanesReport` with two panes, and asserting postcard and `serde_json` round trips are equal.
- [ ] **Step 2: Run** `nice -n 15 ~/.cargo/bin/cargo test -p protocol` and see the new tests fail to compile.
- [ ] **Step 3: Add the constants, types and variants** as in Interfaces, each with a one-line doc comment saying who sends it and what answers it (model them on `Customize`'s). Bump `PROTOCOL_VERSION`.
- [ ] **Step 4: Run** `nice -n 15 ~/.cargo/bin/cargo test -p protocol`; expect PASS. Then `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets`: the daemon and window compile (a `match` on `Request` or `Event` with a wildcard arm needs nothing; one without needs `_ => {}`; fix any `Eq` derive that no longer holds).
- [ ] **Step 5: Commit** `protocol: a targeted Direct request for one window (protocol 8)`.

### Task 2: The daemon relays `Direct` to one window

**Files:**
- Modify: `crates/arkosd/src/server.rs` (`State.direct: HashMap<u64, DirectWait>`, `direct_target`, the two `dispatch` arms, `flush_direct`), `crates/arkosd/tests/daemon.rs`

**Interfaces:**
- Consumes: Task 1's types.
- Produces (in `server.rs`):
  ```rust
  pub struct DirectWait { pub client: ClientId, pub msg_id: u32, pub tx: ClientSink, pub pid: u32, pub project: PathBuf, pub started: Instant }
  /// The one project window a Direct goes to, or why none does.
  pub fn direct_target(st: &State, target: &WindowTarget) -> Result<(ClientId, u32, PathBuf), String>
  ```
  Error texts, exact: `"no window running"` (no connected project window at all); `"several windows: pids {a}, {b}; pass --window"` (`Only` with more than one, pids ascending); `"session {id} is not shown in a window"` (`Session` with no window attached to it); `"no window with pid {pid}"`; `"window {pid}: too old for this command"` (protocol below `DIRECT_FROM`); after the wait, `"window {pid}: no answer"`.
  `Session` with several windows attached picks the one with the highest `ClientId` (connection order is creation order: the newest). A project window is one with `connected && ui_cwd.is_some() && ui_pid.is_some()`, as `customize_targets` defines it.

- [ ] **Step 1: Write the unit test** `direct_target_picks_the_newest_window_of_a_session_and_names_the_rest` in `server.rs`'s tests: build a `State` with three client records through the existing test helpers (`dispatch` with `Hello`, `Attach`), then assert: `Only` with two windows errs with the exact pids text; `Only` with one gives it; `Session(s)` attached to windows 1 and 2 gives 2; `Session(s)` attached nowhere errs `session {id} is not shown in a window`; `Pid(9)` errs `no window with pid 9`; a window with `protocol: 7` errs `too old`; no window at all errs `no window running`.
- [ ] **Step 2: Write the integration test** `a_direct_request_reaches_one_window_and_the_tool_gets_its_answer` in `tests/daemon.rs`, modelled on `a_customize_request_is_relayed_...`: two windows via `connect_as`, the first creates a session and attaches it; a raw tool sends `Request::Direct { id: 1, window: WindowTarget::Session(sid), action: PaneAction::Panes }`; window 1 gets `Event::Direct { id: 1, action: PaneAction::Panes, session: Some(sid) }` and window 2 gets no event (use the harness's non-blocking read or a short timeout read); window 1 answers `DirectDone { id: 1, outcome: Ok(PaneOutcome::Panes(PanesReport { project: "/p".into(), panes: vec![] })) }`; the tool's response is `Response::Directed { pid, project, outcome: Ok(..) }`. A second test `a_direct_to_a_window_that_never_answers_is_an_error_after_the_wait`: no `DirectDone`; after `CUSTOMIZE_WAIT` the tool gets `Response::Error("window {pid}: no answer")`.
- [ ] **Step 3: Run** `nice -n 15 ~/.cargo/bin/cargo test -p arkosd direct` and see them fail.
- [ ] **Step 4: Implement** `direct_target`, the `Request::Direct` arm (resolve; on `Err(text)` answer `Response::Error(text)` at once; else send `Event::Direct { id, action, session }` to that one client where `session` is `Some(s)` for `WindowTarget::Session(s)`; insert the `DirectWait`; spawn the deadline thread as `Customize` does, calling `flush_direct`), the `Request::DirectDone` arm (pop the wait and answer `Response::Directed { pid, project, outcome }`; a late or unknown id answers `Ok`), and `flush_direct` (answer `Response::Error("window {pid}: no answer")` when still waiting after `CUSTOMIZE_WAIT`). `id in flight` is refused as `Customize` does.
- [ ] **Step 5: Run** the two filters again; expect PASS. Run `nice -n 15 ~/.cargo/bin/cargo test -p arkosd` once.
- [ ] **Step 6: Commit** `arkosd: Direct reaches the window that shows the agent's session`.

### Task 3: `Tab::Video` in the model, the workspace and the tab bar

**Files:**
- Modify: `crates/arkos/src/tree.rs`, `closed_stack.rs`, `workspace.rs`, `telemetry.rs` (`PaneKind::Video`, `Surface::Video`, `kind_of`, `Opened::{Agent, Poster}`, `Failure::VideoUnplayable`), `telemetry/events.toml`, `pane_view.rs` (title arm, bar height stays `TAB_BAR_H`, body arm calls a placeholder `render_video` that Task 6 replaces), `main.rs` (the `match tab` sites the compiler names: `close_tab`'s `Closed`, the reopen arm, the `Tab::Viewer { .. } | Tab::Browser { .. } => None` arms)

**Interfaces:**
- Produces:
  ```rust
  // tree.rs
  Tab::Video { path: PathBuf, #[serde(default)] at: Option<f64>, #[serde(default)] loop_range: Option<(f64, f64)> }
  impl Tree {
      /// Writes a video tab's saved position and loop; any other tab is left alone.
      pub fn set_video_state(&mut self, pane: PaneId, idx: usize, at: Option<f64>, loop_range: Option<(f64, f64)>);
      /// The first tab in `pane` on `path`, whatever its kind.
      pub fn find_path(&self, pane: PaneId, path: &Path) -> Option<usize>;
      /// Every tab on `path` in the tree, in reading order.
      pub fn tabs_on(&self, path: &Path) -> Vec<(PaneId, usize)>;
  }
  // closed_stack.rs
  Closed::Video { path: PathBuf, at: Option<f64>, loop_range: Option<(f64, f64)> }
  // workspace.rs
  /// A viewer tab on a video file becomes a video tab (files from before 0.6.0).
  pub fn promote_videos(tree: &mut Tree);
  // telemetry.rs
  PaneKind::Video => "video"; Surface::Video => "video"; Opened::Agent => "agent", Opened::Poster => "poster"; Failure::VideoUnplayable => "video_unplayable"
  // pane_view.rs
  fn render_video(root: &mut Root, pane: PaneId, tab: TabId, path: &Path, cx: &mut Context<Root>) -> Div  // placeholder until Task 6
  ```
  `Tab` and `Closed` derive `PartialEq` but no longer `Eq` (the `f64`). `promote_videos` uses `media::kind(path.to_string_lossy())` is `Some(Media::Video)`; it keeps the tab id (`replace_tab`).

- [ ] **Step 1: Write the tests.** `tree.rs`: `set_video_state_writes_only_a_video_tab` (a Video tab gets `at` and `loop_range`; a Viewer tab in the same pane is unchanged); `find_path_and_tabs_on_see_every_kind` (a Viewer and a Video on the same path are both found). `workspace.rs`: `a_video_tab_round_trips_with_its_position_and_loop` (save and load `Tab::Video { at: Some(12.0), loop_range: Some((12.0, 18.0)) }` and one with `None`s); `a_viewer_on_a_video_is_promoted_when_the_workspace_loads` (a version 3 JSON with `{ "Viewer": { "path": "/p/cut.mp4" } }` loads as `Tab::Video { path, at: None, loop_range: None }` with the same `ids`). `closed_stack.rs`: `a_closed_video_has_no_session` (`session()` is `None`). `telemetry.rs`: extend `kind_of` test with `Tab::Video`; the allowlist test `every_helper_writes_only_values_the_allowlist_lists` covers the new values once `events.toml` lists them.
- [ ] **Step 2: Run** `nice -n 15 ~/.cargo/bin/cargo test -p arkos tree:: workspace:: closed_stack:: telemetry::` and see them fail.
- [ ] **Step 3: Implement** the variant, the three `Tree` methods, `Closed::Video`, `promote_videos` (called in `load_from` after `repair_ids`), the telemetry values, and in `events.toml`: `pane_open.kind` and `pane_close.kind` gain `"video"`; `pane_open.how` gains `"agent"`, `"poster"`; `[events.error] kind` gains `"video_unplayable"`; `[focus]` gains `"video"`; a new table `[events.agent_pane]` with `command = ["open", "status", "play", "pause", "seek", "step", "loop", "reload", "close", "mute", "panes"]` and `ok = "bool"` (Task 8 records it; the `values!` enum `AgentCommand` and `Recorder::agent_pane(command: AgentCommand, ok: bool, now)` plus the free function `telemetry::agent_pane(command, ok)` are written here so the allowlist test sees them). Then the `match` sites: `pane_view.rs` title arm `Tab::Video { path, .. } => (file name, TabMark::None)`, body arm to `render_video` (a `div()` with the file name in `text_dim`, centred); `main.rs` `close_tab` records `Closed::Video { path, at, loop_range }` reading the tab's fields; the reopen arm adds `Tab::Video` back with them; other arms treat `Video` like `Viewer`.
- [ ] **Step 4: Run** the filters; expect PASS. `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets`: warning-free (the `telemetry` crate's `the_render_lists_every_event_key_and_value_of_events_toml` fails until Task 10 adds the disclosure rows; add the rows to `docs/telemetry.md` now instead: `pane_open` and `pane_close` kinds, `how` values, `error` kind, `focus`, and the `agent_pane` row "An agent's `arkos open`, `arkos video` or `arkos panes` command ran, which one and whether it succeeded." so the suite is green at the end of this task).
- [ ] **Step 5: Run** `nice -n 15 ~/.cargo/bin/cargo test --workspace`; expect PASS.
- [ ] **Step 6: Commit** `A video tab in the model: persisted, reopened, promoted from old viewers`.

### Task 4: The playback core, headless

**Files:**
- Create: `crates/arkos/src/video.rs`
- Modify: `crates/arkos/src/main.rs` (`mod video;`)

**Interfaces:**
- Produces:
  ```rust
  /// `12`, `12.5`, `m:ss`, `m:ss.fff`, `h:mm:ss[.fff]` to seconds; Err(text) otherwise.
  pub fn parse_time(s: &str) -> Result<f64, String>;
  /// `m:ss.f`, hours when 3600 or more: 72.25 is "1:12.2", 3725.0 is "1:02:05.0".
  pub fn fmt_time(secs: f64) -> String;
  /// `a-b` with either side a time, a < b.
  pub fn parse_range(s: &str) -> Result<(f64, f64), String>;
  pub type Fp = Option<(u128, u64)>;  // config_watch::fingerprint's shape
  pub enum Follow { Nothing, Reload, Waiting, Back }
  /// Spec section 4: `shown` is the fingerprint of the file as loaded, `prev` one poll ago, `now` this poll.
  pub fn follow(shown: Fp, prev: Fp, now: Fp, waiting: bool) -> Follow;
  pub enum PlayerState { Loading, Ready, Reloading, Waiting, Unplayable(String) }
  pub trait Player {
      fn load(&mut self, path: &Path);              // starts loading a new item for the same or another path
      fn play(&mut self); fn pause(&mut self);
      fn seek(&mut self, secs: f64); fn step(&mut self, frames: i32);
      fn set_muted(&mut self, muted: bool); fn set_loop(&mut self, range: Option<(f64, f64)>);
      fn position(&self) -> f64; fn duration(&self) -> Option<f64>;
      fn size(&self) -> Option<(u32, u32)>;
      fn poll(&mut self) -> Vec<PlayerEvent>;     // what happened since the last poll
  }
  pub enum PlayerEvent { Ready { duration: f64, width: u32, height: u32 }, Failed(String), Ended }
  pub struct FakePlayer { pub calls: Vec<String>, pub pending: Vec<PlayerEvent>, pos: f64, dur: Option<f64>, size: Option<(u32, u32)> }  // test only; `calls` records "load /p", "play", "seek 12.0" ...
  pub struct VideoModel { pub path: PathBuf, pub state: PlayerState, pub playing: bool, pub wanted_playing: bool, pub muted: bool, pub loop_range: Option<(f64, f64)>, pub shown_fp: Fp, pub prev_fp: Fp, pub notice: Option<(String, Instant)>, pub inactive: bool }
  impl VideoModel {
      pub fn new(path: PathBuf, at: Option<f64>, loop_range: Option<(f64, f64)>) -> Self;
      pub fn open(&mut self, p: &mut dyn Player, exists: bool);                 // Loading, or Waiting when !exists
      pub fn tick(&mut self, p: &mut dyn Player, fp: Fp, now: Instant) -> bool; // one poll: follow() then apply; true when a reload started
      pub fn reload_now(&mut self, p: &mut dyn Player);
      pub fn apply(&mut self, p: &mut dyn Player, now: Instant);               // drains p.poll(): Ready restores position (clamped), rate and loop (clamped, cleared with notice "loop cleared" when start >= duration); Failed during Reloading keeps the old item, notice "new file cannot be played yet"; Failed during Loading sets Unplayable; Ended pauses or loops
      pub fn play(&mut self, p: &mut dyn Player); pub fn pause(..); pub fn toggle(..); pub fn seek(..., secs: f64); pub fn step(..., n: i32); pub fn set_loop(..., change: LoopChange); pub fn set_muted(...);
      pub fn set_inactive(&mut self, p: &mut dyn Player, inactive: bool);      // pauses, remembering wanted_playing; resumes
      pub fn state_word(&self) -> &'static str;                                 // "ready" | "loading" | "reloading" | "waiting" | "unplayable"
      pub fn status(&self, p: &dyn Player, pane: u32, tab: u64, file_mtime: Option<u64>) -> protocol::VideoState;
  }
  pub const NOTICE_FOR: Duration = Duration::from_secs(3);
  ```
  `LoopChange` is `protocol::LoopChange`.

- [ ] **Step 1: Write the tests** in `video.rs`: `times_parse_in_every_form_and_refuse_nonsense` (`"12"`, `"12.5"`, `"0:12"`, `"1:02:03.250"` to 3723.25, and `"-1"`, `"1:60"`, `"abc"`, `""` are `Err`); `times_format_with_tenths_and_hours`; `ranges_need_a_start_before_the_end`; `a_changing_file_waits_and_a_stable_one_reloads` (shown A, prev A, now A: Nothing; shown A, prev A, now B: Nothing; shown A, prev B, now B: Reload; shown A, prev A, now None: Waiting; waiting and now Some(C) twice stable: Back; shown None (never loaded) and a stable file: Reload); `a_failed_reload_keeps_the_old_item_and_says_so` (FakePlayer: Ready(10 s), then `reload_now`, then `pending Failed`: state back to Ready, position unchanged, notice text); `a_shorter_cut_clamps_the_position_and_clears_a_loop_past_its_end` (position 50 and loop (40, 48), reload reports 30 s: position 30 (last frame), loop cleared, notice "loop cleared"; loop (10, 48) becomes (10, 30)); `switching_away_pauses_and_coming_back_resumes_only_what_was_playing`; `a_missing_file_at_open_waits_then_loads_when_it_appears`; `ended_pauses_without_a_loop_and_seeks_to_the_start_of_one`; `status_reports_what_the_strip_shows`.
- [ ] **Step 2: Run** `nice -n 15 ~/.cargo/bin/cargo test -p arkos video::`; fail to compile.
- [ ] **Step 3: Implement** `video.rs` as in Interfaces. `FakePlayer` under `#[cfg(test)]` except its type is also needed by Task 6's tests, so keep it `pub` under `#[cfg(test)]`.
- [ ] **Step 4: Run** the filter; expect PASS.
- [ ] **Step 5: Commit** `video: the playback model, the follow rule and the time forms, headless`.

### Task 5: The AVFoundation player and the display token

**Files:**
- Create: `crates/arkos/src/av_player.rs`
- Modify: `crates/arkos/Cargo.toml`, `Cargo.lock`, `crates/arkos/src/app_nap.rs`, `crates/arkos/src/main.rs` (`mod av_player;`)

**Interfaces:**
- Consumes: Task 4's `Player`, `PlayerEvent`.
- Produces:
  ```rust
  // av_player.rs, #[cfg(target_os = "macos")]
  pub struct AvPlayer { /* AVPlayer, current AVPlayerItem, AVPlayerItemVideoOutput, last frame, pending events, loop range */ }
  impl AvPlayer { pub fn new() -> Self; /// The newest frame for now, when the output has one; the last one otherwise.
      pub fn frame(&mut self) -> Option<core_video::pixel_buffer::CVPixelBuffer>; pub fn rate(&self) -> f32; }
  impl video::Player for AvPlayer { .. }
  // app_nap.rs
  /// Keeps the display awake while held (NSActivityIdleDisplaySleepDisabled, reason "playing video").
  pub struct DisplayAwake(Retained<ProtocolObject<dyn NSObjectProtocol>>);
  pub fn keep_display_awake() -> DisplayAwake;  // ends the activity when dropped
  ```
  `load`: a new `AVURLAsset` (`fileURLWithPath`), `AVPlayerItem`, the output created once with the three attributes and moved with `removeOutput`/`addOutput`, `replaceCurrentItemWithPlayerItem`, `actionAtItemEnd` = `Pause`; readiness is observed by polling `item.status()` in `poll()` (no KVO): `ReadyToPlay` yields `Ready { duration, width, height }` (size from the item's `presentationSize`), `Failed` yields `Failed(error.localizedDescription)`. `Ended` is detected in `poll()` when the rate is 0 and the position is within a frame of the duration after it was playing. `seek` uses `seekToTime_toleranceBefore_toleranceAfter_completionHandler` with zero tolerances and, when paused, asks the output with `requestNotificationOfMediaDataChangeWithAdvanceInterval(0.0)` then copies a frame on the next `frame()`. `frame()` uses `itemTimeForHostTime(CACurrentMediaTime())`, `hasNewPixelBufferForItemTime`, `copyPixelBufferForItemTime_itemTimeForDisplay`, and rewraps the returned `Retained<objc2_core_video::CVPixelBuffer>` into `core_video::pixel_buffer::CVPixelBuffer` with `TCFType::wrap_under_get_rule(ptr as _)` (the `Retained` keeps its own reference; get rule, then the wrapped value retains once more; document the two owners). `set_loop`: when the position reaches `end` in `poll()`, seek to `start`.

- [ ] **Step 1: Add the dependencies** to `Cargo.toml` (Global Constraints), then `nice -n 15 ~/.cargo/bin/cargo check -p arkos` to fetch and compile them (a cold compile of the AVFoundation binding: let it finish; nothing else runs).
- [ ] **Step 2: Write** `app_nap::keep_display_awake` and `DisplayAwake` (Drop calls `NSProcessInfo::processInfo().endActivity(&token)`), and `av_player.rs` as above, with a module doc that names the pixel buffer attributes as mandatory and cites gpui's `draw_surfaces`.
- [ ] **Step 3: Write a compile-only test** `the_player_module_builds_and_the_fake_is_still_the_only_one_tests_touch` that asserts `std::mem::size_of::<AvPlayer>() > 0` without constructing one (keeps the module in the test build so a signature drift fails the suite).
- [ ] **Step 4: Run** `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets`; warning-free.
- [ ] **Step 5: Commit** `av_player: AVFoundation behind the Player trait, frames as '420f' pixel buffers`.

### Task 6: The video view: picture, strip, scrubber, keys

**Files:**
- Create: `crates/arkos/src/video_view.rs`, `crates/arkos/src/video_element.rs`
- Modify: `crates/arkos/src/keymap.rs` (`Section::Video`, actions), `command_palette.rs` (the test for the new items), `telemetry.rs` and `events.toml` (`palette.command` gains the snake case names), `pane_view.rs` (`render_video` real), `main.rs` (`videos: HashMap<TabId, Entity<VideoView>>`, `video_for`, the action handlers, `mod` lines)

**Interfaces:**
- Consumes: Task 4's `VideoModel`, `Player`; Task 5's `AvPlayer`, `keep_display_awake`.
- Produces:
  ```rust
  // video_view.rs
  pub struct VideoView { pub model: VideoModel, player: Box<dyn Player>, pub frame: Option<CVPixelBuffer>, pub poster: Option<PathBuf>, awake: Option<DisplayAwake>, pub scrub: Option<Scrub>, pub focus: FocusHandle }
  pub struct Scrub { pub dragging: bool, pub was_playing: bool, pub select_from: Option<f64> }
  impl VideoView {
      pub fn new(path: PathBuf, at: Option<f64>, loop_range: Option<(f64, f64)>, player: Box<dyn Player>, cx: &mut Context<Self>) -> Self;
      pub fn tick(&mut self, fp: Fp, cx: &mut Context<Self>);            // the one second poll
      pub fn set_inactive(&mut self, inactive: bool, cx: &mut Context<Self>);
      pub fn saved_state(&self) -> (Option<f64>, Option<(f64, f64)>);    // for Tree::set_video_state
      pub fn status(&self, pane: u32, tab: u64) -> protocol::VideoState;
      // the actions, each calling the model then cx.notify(): play_pause, back(secs), forward(secs), step(n), to_start, toggle_loop, mute, reload_now, seek(secs), set_loop(LoopChange), play, pause, set_muted
  }
  pub enum VideoEvent { StateChanged }  // emitted after pause, seek, loop, so Root saves
  // video_element.rs
  pub const STRIP_H: f32 = 28.0;
  pub fn render(view: &Entity<VideoView>, theme: &Theme, family: SharedString, cx: &mut Context<Root>) -> Div;  // picture (surface or poster or "loading"/reason), strip, overlay button for Unplayable
  /// The scrubber's x for a time and back, pure.
  pub fn scrub_x(secs: f64, duration: f64, width: f32) -> f32; pub fn scrub_time(x: f32, duration: f64, width: f32) -> f64;
  // keymap.rs
  Section::Video ("Video"), after Browser; actions video_play_pause (space), video_back (left), video_forward (right), video_back_fine (shift-left), video_forward_fine (shift-right), video_step_back (","), video_step_forward ("."), video_start (home, and "0"), video_loop (l), video_mute (m), video_reload (r), all `context: Some("Video")`; config names "video play pause", "video back", ...; palette entries through the existing `ACTIONS` table plus `OpenFileInPane` ("Open file in pane…", no key) reusing the URL prompt with a path.
  ```
  `Root::video_for(tab, path, cx) -> Entity<VideoView>` makes the view on first render with `Box::new(AvPlayer::new())`, subscribes to `VideoEvent::StateChanged` to `tree.set_video_state` and `save`, and calls `model.open` with `path.exists()`. The element's root `div` has `.key_context("Video")`, `.track_focus(&view.focus)`, and while `model.playing` it calls `window.request_animation_frame()` and pulls `player.frame()` into `view.frame` (through an `AvPlayer` downcast: give `Player` a `fn frame(&mut self) -> Option<CVPixelBuffer>` with a default `None` instead, so no downcast). `surface(frame).object_fit(ObjectFit::Contain)` centred on `theme.chrome.window_bg`; the strip per spec section 3.3; the status word from `model.notice` within `NOTICE_FOR` else from `state_word` (nothing when ready); "Open in QuickTime Player" runs `md_view`'s `open_media` path (make `open_media` `pub(crate)`). The display token is taken when `model.playing` becomes true and dropped when false or inactive.

- [ ] **Step 1: Write the tests.** `keymap.rs`: extend the sections test (`titles` gains "Video" after "Browser"); `every_video_action_acts_on_the_focused_video_only` (each `video *` name has `context: Some("Video")` and `Section::Video`; none of the keys is bound in the `Terminal` context). `command_palette.rs`: the items test finds "Open file in pane…" and every "Video: ..." label (`keymap::label_for`). `telemetry.rs`: the allowlist test covers the new palette names once listed. `video_element.rs`: `scrub_x_and_scrub_time_are_inverses_and_clamp` (0 and the duration map to 0 and width; a negative x is 0; x past the width is the duration). `video_view.rs` (with `FakePlayer`): `a_view_pauses_when_inactive_and_saves_its_state_on_pause` (construct `VideoView` inside `gpui::TestAppContext`? The repo's views are not tested through gpui; keep this test on `VideoModel` already covered in Task 4 and test here only `saved_state` through a view built with `cx.new` in a `#[gpui::test]` if the crate already uses `gpui::test` anywhere; `grep -rn "gpui::test" crates/arkos/src` and skip the test when it is not used, noting that in the commit).
- [ ] **Step 2: Run** `nice -n 15 ~/.cargo/bin/cargo test -p arkos keymap:: command_palette:: video_element::`; fail.
- [ ] **Step 3: Implement** the keymap rows, palette items, telemetry names (`events.toml` `palette.command` gains `video_play_pause`, `video_back`, `video_forward`, `video_back_fine`, `video_forward_fine`, `video_step_back`, `video_step_forward`, `video_start`, `video_loop`, `video_mute`, `video_reload`, `open_file_in_pane`; `docs/telemetry.md`'s palette row gains them), `video_view.rs`, `video_element.rs`, `render_video` in `pane_view.rs` (replacing Task 3's placeholder), `Root.videos`, `video_for`, the `Command` handlers in `main.rs` dispatching to the focused pane's active video tab's view, and `OpenFileInPane`: the URL prompt (`pane_view::UrlPrompt`) gains a `kind: PromptKind { Url { app_mode }, Path }` so its submit routes to `Root::open_path_in_pane(path, pane, cx)` (Task 7 defines it; here call `open_viewer` for now and let Task 7 switch it).
- [ ] **Step 4: Run** the filters; PASS. `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets`; warning-free.
- [ ] **Step 5: Hand check (the implementer runs the dev binary from the worktree, no bundle):** `nice -n 15 ~/.cargo/bin/cargo run -p arkos -- /tmp/videotest` on a folder with a short `.mp4` (make one: `ffmpeg -f lavfi -i testsrc=duration=10:size=640x360:rate=24 -f lavfi -i sine=frequency=440:duration=10 -pix_fmt yuv420p /tmp/videotest/test.mp4`), double-click it in the sidebar once Task 7 lands, or for now open it through "Open file in pane…": the picture shows, Space plays with sound, the scrubber moves, comma steps a frame, L loops, M mutes. Note what you saw in the commit message. If the window panics in `draw_surfaces`, the pixel buffer attributes are wrong: fix before committing.
- [ ] **Step 6: Commit** `A video tab draws its frames, with a control strip and keys`.

### Task 7: The window's own ways in, the file watcher, close and reopen

**Files:**
- Modify: `crates/arkos/src/sidebar.rs` (`open_viewer` routing, `watched_paths`, `paths_changed`), `main.rs` (`open_path_in_pane`, `close_tab` releases the view, tab switch and window hide call `set_inactive`, telemetry `pane_open` with `Opened::Poster`), `md_view.rs` and `viewer.rs` (`ViewerEvent::OpenVideo(PathBuf)` from the poster's Cmd+click; `open_media` stays the fallback), `pane_view.rs` (the tab's active state change notifies the view)

**Interfaces:**
- Produces:
  ```rust
  impl Root {
      /// Opens `path` as the kind its name says: a video as `Tab::Video`, else the viewer. Pinned.
      pub fn open_path_in_pane(&mut self, path: &Path, pane: PaneId, cx: &mut Context<Self>) -> (usize, bool /* reused */);
      /// A video tab's active state follows its pane: only the active tab of a visible pane plays.
      pub fn update_video_activity(&mut self, cx: &mut Context<Self>);
  }
  ViewerEvent::OpenVideo(PathBuf)
  ```
  `open_viewer` with a video path builds a `Tab::Video` instead of `Tab::Viewer` in all three `OpenPlan` arms (preview and pin semantics unchanged; `find_viewer` becomes `find_path` for the plan). `watched_paths` adds every `videos` view's path; `paths_changed` calls `view.tick(fingerprint(p), cx)` for each view on `p` (the tick's own `follow` handles stability) and returns whether any matched, as `reload_viewers_for` does. `update_video_activity` runs after every tab activation, pane close, maximize change, `window_active` change and `window_hidden` tick (hook into the places `dock.rs`'s snapshot is built, `main.rs` ~2336): a view is inactive unless its tab is its pane's active one, the pane is visible (not hidden by another's maximize) and the window is active and not hidden. The poster's Cmd+click in `md_view.rs` emits `OpenVideo(canonical path)` when `media::checked_open_path` passes; `viewer_for`'s subscription handles it by opening in the pane right of the document's pane (`tree.panes()` order) or splitting right, through `open_path_in_pane`, with `Opened::Poster`.

- [ ] **Step 1: Write the tests.** `sidebar.rs`: `a_video_in_the_tree_opens_as_a_video_tab_and_is_reused` (plan with `Tab::Video` present on the path: `Activate`; `open_viewer` on `/p/cut.mp4` makes `Tab::Video`); `watched_paths_include_every_video_tab` (a `Root` is not constructible in tests; test the pure helper `video_paths(&HashMap<TabId, PathBuf>)` extracted for it, or assert through the existing `watched_paths` test pattern if one exists). `main.rs`: `a_closed_video_reopens_with_its_position` (close a `Tab::Video { at: Some(12.0), .. }` through the pure `Closed` mapping and reopen: the same fields). `viewer.rs`: `a_poster_cmd_click_asks_for_a_video_tab` (the `media_element` press handler is a closure; test the pure decision `poster_press(platform: bool, kind: Media, link: Option<&str>, remote: bool) -> PosterPress { Caret, Link, OpenVideo, QuickTime, Reveal }` extracted from it).
- [ ] **Step 2: Run** `nice -n 15 ~/.cargo/bin/cargo test -p arkos sidebar:: viewer::`; fail.
- [ ] **Step 3: Implement** as in Interfaces. In `close_tab`, `self.videos.remove(&id)` drops the view and its player and token. Telemetry: `pane_open(PaneKind::Video, Opened::Explorer | Poster | Palette)` at each entry, `pane_close` through `kind_of`.
- [ ] **Step 4: Run** the filters; PASS. `nice -n 15 ~/.cargo/bin/cargo test --workspace` once; PASS.
- [ ] **Step 5: Hand check:** double-click the test `.mp4` in the sidebar; switch tabs (it pauses, comes back playing); `ffmpeg -y -f lavfi -i testsrc2=duration=6:size=640x360:rate=24 -pix_fmt yuv420p /tmp/videotest/test.mp4` while it plays: within two seconds the picture changes to the new pattern and the position holds; `rm` then restore the file: "waiting for file" then back. Note the outcome in the commit.
- [ ] **Step 6: Commit** `The sidebar and the poster open a video tab; a video follows its file`.

### Task 8: The window answers `Direct`: placement, panes, video ops

**Files:**
- Create: `crates/arkos/src/direct.rs`
- Modify: `crates/arkos/src/main.rs` (`preview_panes: HashMap<Option<SessionId>, PaneId>`, `route_events` arm for `Event::Direct`, `mod direct;`)

**Interfaces:**
- Consumes: Task 1's types; Task 7's `open_path_in_pane`; Task 6's `VideoView` methods; Task 3's `Tree::tabs_on`, `pane_at`, `pane_index`, `find_session`.
- Produces:
  ```rust
  // direct.rs
  pub enum OpenStep { Reuse { pane: PaneId, idx: usize }, AddTo { pane: PaneId }, SplitFrom { pane: PaneId, side: Side } }
  /// Spec section 5: where an Open lands. `anchor` is the calling session's pane (None without one: the focused pane).
  pub fn plan_open(tree: &Tree, path: &Path, place: &Placement, reuse: bool, anchor: Option<PaneId>, preview: Option<PaneId>) -> Result<OpenStep, String>;
  // errors, exact: "pane {n} does not exist; this window has {count}" ; a Split needs an anchor or the focused pane (always present)
  pub fn side_of(s: SplitSide) -> Side;
  pub fn panes_report(tree: &Tree, project: &Path, you: Option<SessionId>, titles: &dyn Fn(PaneId, usize) -> String) -> PanesReport;  // kind strings: "terminal" | "markdown" | "text" | "browser" | "video"; `path` for viewer and video tabs; `session` for terminals
  pub fn kind_string(tab: &Tab) -> &'static str;
  /// Which video tabs a Video op means: by path, the only one, or the error text of ruling 7.
  pub fn pick_videos(tree: &Tree, path: Option<&Path>, op_is_status: bool) -> Result<Vec<(PaneId, usize)>, String>;
  impl Root {
      pub fn handle_direct(&mut self, id: u64, action: PaneAction, session: Option<SessionId>, cx: &mut Context<Self>);
  }
  ```
  `handle_direct`: for `Open`, `anchor = session.and_then(|s| self.tree.find_session(s)).map(|(p, _)| p)`; `preview = self.preview_panes.get(&session_key)` filtered by `tree.pane(..).is_some()` where `session_key = session` (None for no session); plan; apply: `Reuse` activates the tab in its pane without `tree.focus` (use `pane_mut` to set `active` and `mru` directly, or add `Tree::activate_quiet(pane, idx)` that does not focus), `AddTo` uses `open_path_in_pane` then restores `tree.focused` to what it was unless `focus`, `SplitFrom` splits with the new tab then restores focus likewise; records the new or reused pane in `preview_panes` when `place` was `Preview`; for a video applies `at`, `playing`, `loop_change`, and when reusing a video whose file changed calls `reload_now`; then waits for the view's state to leave `Loading`/`Reloading` (poll the view each frame with a deadline of `CUSTOMIZE_WAIT - 300 ms`, through `cx.spawn` and a timer) and answers `DirectDone { id, outcome: Ok(PaneOutcome::Opened { .. }) }`; errors answer `Err(text)`. For `Video`, `pick_videos` then the op on each view; `Close` goes through `close_tab`; the answer is `Videos(states)` (after `Reload`, wait as for `Open`). For `Panes`, `panes_report` with titles from `pane_view`'s title logic (extract `tab_title(root, pane, idx) -> String` if it is not already a function). Each `Direct` records `telemetry::agent_pane(command, ok)`. The answer goes through `self.sessions.client.send(Request::DirectDone { .. })` as `customize` does.

- [ ] **Step 1: Write the tests** in `direct.rs`: `an_open_leaves_the_focus_where_it_was_unless_asked` (through `plan_open` plus a small pure `apply_focus(tree_focused_before, new_pane, focus: bool) -> PaneId`); `the_first_open_splits_the_anchor_right_and_the_second_reuses_the_preview_pane`; `a_preview_pane_that_closed_makes_a_new_split`; `pane_n_in_and_out_of_range`; `a_split_side_is_honoured`; `an_open_path_is_reused_unless_new` (a Viewer on the path counts); `without_a_session_the_anchor_is_the_focused_pane`; `panes_report_numbers_panes_in_cmd_order_and_marks_you_active_and_focused`; `pick_videos_follows_ruling_7` (none: `no video tab open`; two without a path: the two lines; two with status: both; a path: that one).
- [ ] **Step 2: Run** `nice -n 15 ~/.cargo/bin/cargo test -p arkos direct::`; fail.
- [ ] **Step 3: Implement** `direct.rs` and the `Root` glue; `route_events` gets `Event::Direct { id, action, session } => self.handle_direct(*id, action.clone(), *session, cx)`.
- [ ] **Step 4: Run** the filter; PASS. `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets`; warning-free.
- [ ] **Step 5: Commit** `The window places an agent's open beside it and answers Direct`.

### Task 9: `arkos open`, `arkos video`, `arkos panes`

**Files:**
- Create: `crates/arkos/src/open_cli.rs`
- Modify: `crates/arkos/src/agent_cli.rs` (`USAGE`; make `split`, `subcommand`, `usage`, `fail`, `Env`, `Reach` `pub(crate)` where they are not), `main.rs` (dispatch `open`, `video`, `panes` before the folder argument; `mod open_cli;`)

**Interfaces:**
- Consumes: Task 1's types; Task 4's `parse_time`, `parse_range`, `fmt_time`; `hook_cli::send_one_with`; `media::kind`, `media::is_link`.
- Produces:
  ```rust
  pub const OPEN_USAGE: &str = "usage: arkos open <file> [--pane N | --split right|down|left|up] [--at TIME] [--paused | --playing]\n                  [--loop A-B | --loop off] [--raw] [--new] [--wait] [--focus] [--window PID] [--json]\nsee: arkos guide\n";
  pub const VIDEO_USAGE: &str = ...;  // the eight forms of spec section 6
  pub const PANES_USAGE: &str = "usage: arkos panes [--all] [--window PID] [--json]\nsee: arkos guide\n";
  pub fn open_cli(args: &[String]) -> i32; pub fn video_cli(args: &[String]) -> i32; pub fn panes_cli(args: &[String]) -> i32;   // real env
  pub fn open_cli_to(args: &[String], env: &DirectEnv, out: &mut dyn Write, err: &mut dyn Write) -> i32;  // and video_cli_to, panes_cli_to
  pub struct DirectEnv<'a> { pub socket: &'a Path, pub session: Option<SessionId>, pub cwd: &'a Path, pub wait: Duration }
  /// The file checks before anything is sent: canonical path and kind, or the refusal text.
  pub fn classify_open(path: &Path, cwd: &Path, raw: bool, wait: bool) -> Result<(PathBuf, OpenKind), String>;
  // refusal texts, exact: "no such file: {path}" ; "images are not shown in a pane yet; open it with `open {path}` (Preview)" ; "audio is not shown in a pane yet; play it with `open -a 'QuickTime Player' {path}`" ; "a .webm is not shown in a pane; show it with `open -R {path}` (Finder)" ; "a web address is not a file; open it in the browser pane (Cmd+Shift+B)" ; "{path} is a folder"
  pub fn target_from(env: &DirectEnv, window_flag: Option<u32>) -> WindowTarget;   // Pid > Session > Only
  pub fn direct(socket: &Path, window: WindowTarget, action: PaneAction, wait: Duration) -> Result<(u32, PathBuf, Result<PaneOutcome, String>), Reach>;
  pub fn opened_line(o: &PaneOutcome, file_name: &str, new_pane: bool) -> String;   // "opened video x.mp4 in pane 2 (new pane), 0:12.0 of 1:48.5, paused, 1920x1080" / "shown ..." / "reloaded, shown ..." / "opened notes.md in pane 2, rendered"
  pub fn status_line(v: &VideoState, file_name: &str) -> String;                   // spec section 6
  pub fn panes_lines(r: &PanesReport, pid: u32) -> Vec<String>;
  pub fn outcome_json(pid: u32, o: &PaneOutcome) -> serde_json::Value;             // the shapes of spec sections 5 to 7
  ```
  Flags with values are `--pane N`, `--split SIDE`, `--at T`, `--loop X`, `--window PID` (extend `split` to take a value-flag list, or parse pairs in `open_cli.rs` with a local helper). Exit code mapping: `Reach::NoWindow` and daemon errors `no window running` / `session .. is not shown in a window` are 3; every other daemon error, refusal and `Err` outcome is 1; usage 2. `env.session` comes from `ARKOS_SESSION_ID` parsed as `u64`. `--json` prints `outcome_json` or `{ "error": text, "exit": n }` on stderr-less stdout and nothing else.

- [ ] **Step 1: Write the tests** with the fake daemon pattern of `agent_cli.rs` (`fake_daemon` returning `Response::Directed`): `open_refuses_what_it_cannot_show_before_sending` (missing file; image; audio; webm; https; a folder; `--at 1:60`; `--pane x`; the exact texts and exit 1; `--wait` lets a missing `.mp4` through); `open_sends_the_right_action_and_prints_the_line` (`--at 0:12 --loop 0:12-0:18 --playing --pane 2`: the captured `PaneAction::Open` has `at: Some(12.0)`, `loop_change: Set { 12.0, 18.0 }`, `playing: Some(true)`, `place: Pane(2)`, `reuse: true`; the line and `--json`); `open_targets_the_session_then_the_pid_then_only`; `open_exits_3_without_a_window_and_1_on_a_refused_outcome`; `video_subcommands_map_to_ops` (each of the eight forms to its `VideoOp`, `seek` parses times, `loop` the three forms, `step` the default +1); `panes_prints_one_line_per_pane_with_you_active_and_focused`; `every_usage_text_names_see_arkos_guide`.
- [ ] **Step 2: Run** `nice -n 15 ~/.cargo/bin/cargo test -p arkos open_cli::`; fail.
- [ ] **Step 3: Implement** `open_cli.rs`; add to `agent_cli::USAGE` after the `guide` line: `       arkos open <file> ...          show a video or a document in a pane beside you`, `       arkos video <command> ...      status, play, pause, seek, step, loop, reload, close, mute`, `       arkos panes                    this window's panes and tabs, numbered as Cmd+1..9`; dispatch in `main.rs` beside `guide`.
- [ ] **Step 4: Run** the filter; PASS. `nice -n 15 ~/.cargo/bin/cargo test --workspace`; PASS.
- [ ] **Step 5: Hand check** from a shell inside a dev Arkos window (the dev binary is on no PATH: use `target/debug/arkos`): `target/debug/arkos panes`, then `target/debug/arkos open /tmp/videotest/test.mp4 --at 0:03 --paused` (a pane splits right, paused on the frame, focus stays), the same command again (reused, no second tab), `--loop 0:02-0:04 --playing` (loops with sound), `target/debug/arkos video status --json`, `target/debug/arkos video close`. Note the outcome in the commit.
- [ ] **Step 6: Commit** `arkos open, video and panes: the agent's command line for the video pane`.

### Task 10: Discovery and documentation

**Files:**
- Modify: `crates/arkos/src/hook_cli.rs` (`CONTEXT_LINES`, `context_lines`), `guide.rs` (`SKILL_DESCRIPTION`, the coverage test), `doctor.rs` (the outdated-skill line), `docs/agent-guide.md`, `docs/guide.md`, `README.md`, `docs/development.md`, `docs/telemetry.md` (verify every row from Tasks 3 and 6 is present)

**Interfaces:**
- Produces: `CONTEXT_LINES = "This session runs inside Arkos IDE.\nTo show the user a video or a document in a pane beside you, or to change Arkos's theme, fonts or settings, run `arkos guide` first and follow it.\n"` (and the `{exe} guide` form the same); `SKILL_DESCRIPTION = "Show a video or a markdown file in an Arkos IDE pane, control its playback, or customize Arkos: themes, fonts and settings. Use when the user asks to see, preview, play or watch a video or a document, or to change how Arkos looks or behaves."`; `doctor::themes_section`'s `Outdated` line becomes `older than this build ({old}); agents will not know about video panes until it is reinstalled`.

- [ ] **Step 1: Write the tests.** `hook_cli.rs`: update the `CONTEXT_LINES` assertions (they compare to the constant, so only `context_lines(false, ..)`'s literal needs the new second line); add `the_hook_line_mentions_showing_a_video` (`CONTEXT_LINES.contains("video")`). `guide.rs`: `the_guide_names_every_open_video_and_panes_command_and_flag`: for each of `["arkos open", "arkos video status", "arkos video play", "arkos video pause", "arkos video seek", "arkos video step", "arkos video loop", "arkos video reload", "arkos video close", "arkos video mute", "arkos video unmute", "arkos panes", "--pane", "--split", "--at", "--paused", "--playing", "--loop", "--raw", "--new", "--wait", "--focus", "--window", "--all", "ARKOS_SESSION_ID"]` assert `TEXT.contains(s)`; also assert `SKILL_DESCRIPTION.contains("video")` and the front matter test still passes (the description holds `: ` so stays quoted). `doctor.rs`: the outdated line test.
- [ ] **Step 2: Run** `nice -n 15 ~/.cargo/bin/cargo test -p arkos hook_cli:: guide:: doctor::`; fail.
- [ ] **Step 3: Write the guide section** in `docs/agent-guide.md`: rewrite section 1's first paragraph to the new allow-list (you may show a video or a document in a pane and control a video; you may not run commands in Arkos's terminals or reach a session); in section 2 add that `ARKOS_SESSION_ID` also tells `arkos open` which window is yours; insert a new section 3 "Showing the user a file" (renumber the rest) with: the three commands and every flag with the spec's exact meanings and exit codes, the time forms, the preview pane rule (first open splits your pane to the right, later opens reuse that pane, `--pane N` from `arkos panes` to choose), that focus stays with you unless `--focus`, that a video tab follows its file (reloads within two seconds of a change, holding the position), the one word `arkos video reload`, the `--json` shapes from spec sections 5 to 7, and the worked example from spec section 10 verbatim; and in the Rules section the six rules of spec section 10. Then `hook_cli.rs`, `guide.rs`, `doctor.rs`.
- [ ] **Step 4: Documentation.** `docs/guide.md`: a "Video pane" section after "Browser pane" (spec section 11, first bullet), the "Images and media" Cmd+click sentence ("Cmd+click opens it in a video tab beside the document; a file Arkos cannot play opens in QuickTime Player"), the Keyboard table rows for the video keys, "Themes for agents" retitled "Agents" with a paragraph on `arkos open`. `README.md`: the feature list gains "a video pane"; the agents bullet reads "run `arkos guide` in an Arkos terminal for how to show the user a video or a document and how to change the theme and settings". `docs/development.md` Status: the milestone 9 paragraph and the hand checks of spec section 11 as list rows. `docs/telemetry.md`: confirm the rows (the `telemetry` crate's disclosure tests pass).
- [ ] **Step 5: Run** `nice -n 15 ~/.cargo/bin/cargo test --workspace`; PASS. `nice -n 15 ~/.cargo/bin/cargo check --workspace --all-targets`; warning-free. `grep -rn "—" crates docs README.md CLAUDE.md 2>/dev/null | grep -v target` finds nothing new.
- [ ] **Step 6: Hand check** (the user, at the screen, from a fresh Claude Code session in a dev window with the hooks installed pointing at the dev binary, or after the release): "show me the animatic beside you and stop on the radio scene" ends with the video paused in a pane to the right, and the agent ran `arkos guide` first. Record in Status.
- [ ] **Step 7: Commit** `Agents discover the video pane: the hook line, the skill, the guide and the docs`.

---

## Self-review notes

- Spec coverage: 3.1 Task 3; 3.2 Tasks 5 and 6; 3.3 and 3.4 Task 6; 3.5 Tasks 6 and 7; 4 Tasks 4 and 7; 5 Tasks 8 and 9; 6 Tasks 8 and 9; 7 Tasks 8 and 9; 8 Tasks 1 and 2; 9 Task 7; 10 Task 10; 11 Task 10; 12 Tasks 3, 6 and 8; 13 spread as named; 14 out of scope; 15 calls 1 to 4 and 12 to 14 Task 8, 5 Task 4, 6 Task 9, 7 Task 6, 8 Tasks 7 and 9, 9 Task 8, 10 Tasks 1 and 2, 11 Task 10, 15 Task 5.
- The `Eq` loss on `Request`/`Response`/`Event` (the `f64` fields) is the one ripple into existing code; Task 1 Step 4 checks it. If a test somewhere needs `Eq` (a `HashSet` of messages, say), wrap times as `u32` milliseconds instead and say so in the commit.
- Task 6's `gpui::test` question is left to the implementer with the rule: no gpui test harness is introduced if none exists; the model is covered headless in Task 4.
- Proportion: the plan is about 1.2 times the spec; code blocks are signatures and test names, no bodies except the exact copy the spec fixes (error texts, usage lines, the hook line, the skill description).
