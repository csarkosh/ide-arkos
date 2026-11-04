# Worktrees Everywhere Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The sidebar's Worktrees section lists the side worktrees of every repository under the open folder, live through FSEvents, each with the time since any of its files changed; and `arkos worktrees` gives Claude the same list.

**Architecture:** Three new modules free of gpui do the work: `wt_scan.rs` (the walks: find repositories, newest file, age text), `fsevents.rs` (a thin binding to Apple's FSEvents stream, delivering directory paths over a `futures` channel), and `wt_watch.rs` (the pure event rules: which path means what). `worktrees.rs` keeps the model, now keyed by repository and sorted by age; `sidebar.rs` drives the stream and paints the rows; `worktrees_cli.rs` composes the same scan and list functions for the command. The create prompt and the "+" go.

**Tech Stack:** Rust, gpui 0.2.2 (csarkosh fork), `libc` (`statfs`), `futures::channel::mpsc`, CoreServices FSEvents bound with `#[link(name = "CoreServices", kind = "framework")]` as `av_player.rs` binds QuartzCore, git through `explorer::git_read` (which already passes `--no-optional-locks`).

**Spec:** `docs/superpowers/specs/2026-11-03-worktrees-everywhere-design.md`

## Global Constraints

- Tests are headless (`cargo test --workspace`), named as sentences, written before the code. Doc comments are plain prose; no em-dashes anywhere (code, docs, commits).
- Builds with `nice -n 15`, never `--release`, one cargo at a time; finish each task with `nice -n 15 cargo check --workspace --all-targets` showing no warnings.
- Every colour is read from `root.theme` at paint time, never cached. A re-sort of the rows is not animated; nothing new animates, so `Root::any_motion` is untouched.
- Skipped folders, verbatim from the spec: `node_modules`, `target`, `build`, `dist`, `.build`, `Pods`, `DerivedData`, `vendor`, `__pycache__`, `.venv`, `venv`; in a home folder also `Library` and `Applications`. Hidden cache folders: `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.next`, `.nuxt`, `.gradle`, `.turbo`, `.cache`, `.parcel-cache`, `.svelte-kit`. `.DS_Store` never counts.
- Numbers: stream latency 1.0 s; discovery depth cap 4 below the open folder; discovery budget 2,000 folders read; age-walk budget 50,000 entries per worktree; 16 extra stream paths; the full pass every 10 minutes; a row dims at 7 days.
- Paths are canonical (`std::fs::canonicalize`) wherever they are stored or compared; only display turns them relative.
- Main checkouts are never rows. Removal of a worktree is git's, through the existing menu; nothing new removes anything.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- The spec's telemetry line is dropped: windowless agent commands (`arkos theme`, `arkos settings`) record nothing today, since `agent_pane` is the window's; `arkos worktrees` follows them. Task 9 amends the spec.

## Review Focus

1. The open folder reached through a symlink (`~/Projects` as a link): FSEvents reports the real path, and a row's edit must still reset its age. Pinned in Task 6 (`an_event_under_a_symlinked_open_folder_still_matches_its_worktree`).
2. The open folder is itself a linked worktree (its `.git` is a file): the list must show its main repository's side worktrees, itself included, with the main checkout absent. Pinned in Task 4.
3. A worktree with no files yet beyond its `.git` (a fresh `git worktree add` of an orphan branch) must show a blank age, never `now`. Pinned in Task 2.
4. `cargo build` in a worktree writes thousands of files under `target/`: its age must not move. Pinned in Task 6 (`a_build_writing_target_is_dropped_before_it_costs_anything`).
5. A repository `git_read` refuses must be named by the command, not skipped, with the rest still listed. Pinned in Task 8.

---

### Task 1: Finding repositories (`wt_scan.rs`, part one)

**Files:**
- Create: `crates/arkos/src/wt_scan.rs`
- Modify: `crates/arkos/src/main.rs` (the `mod` list, in alphabetical order with `mod worktrees;`)

**Interfaces:**
- Produces:
  ```rust
  pub const SKIP_DIRS: &[&str];            // the skipped folders (not the home-only two)
  pub const HOME_SKIP_DIRS: &[&str];       // "Library", "Applications"
  pub const CACHE_DIRS: &[&str];           // the hidden cache folders
  pub const DISCOVERY_DEPTH: usize = 4;
  pub const DISCOVERY_BUDGET: usize = 2_000;
  /// Whether a folder of this name is never entered: skipped, or a cache. `home` adds HOME_SKIP_DIRS.
  pub fn skipped_dir(name: &str, home: bool) -> bool;
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Discovery { pub repos: Vec<PathBuf>, pub folders_read: usize, pub capped: bool }
  /// Repositories under `open` (canonical): the one `open` is inside, found upward, first; then
  /// every folder holding a `.git` (directory or file) found by a walk that does not enter
  /// repositories, hidden folders, symlinks or skipped folders, `DISCOVERY_DEPTH` deep, reading at
  /// most `budget` folders. `open` itself is entered even when it is a repository.
  pub fn find_repos(open: &Path, budget: usize) -> Discovery;
  /// The repository `path` is inside: the nearest ancestor (itself included) holding `.git`.
  pub fn enclosing_repo(path: &Path) -> Option<PathBuf>;
  /// `canonicalize`, or the path as given when it does not exist.
  pub fn canonical(path: &Path) -> PathBuf;
  ```

- [ ] **Step 1: Write the failing tests** in `wt_scan.rs`'s `mod tests`, building fixtures with `tempfile::tempdir()` and `std::fs::create_dir_all` (a `.git` directory, or a `.git` file holding `gitdir: ...`, marks a repository; no git needed):

```rust
#[test]
fn skipped_folders_are_never_entered_and_the_home_ones_only_in_a_home_folder() {
    assert!(skipped_dir("node_modules", false) && skipped_dir("target", false) && skipped_dir(".pytest_cache", false));
    assert!(!skipped_dir("Library", false) && skipped_dir("Library", true));
    assert!(!skipped_dir("src", false) && !skipped_dir(".github", false), "hidden folders are not skipped by name");
}

#[test]
fn repositories_are_found_below_the_open_folder_and_the_walk_stops_at_each() {
    // d/a/.git, d/b/sub/.git (depth 2), d/a/inner/.git (inside a repo: not found), d/node_modules/x/.git (skipped),
    // d/.hidden/y/.git (hidden: not found), d/link -> d/a (symlink: not entered)
    let found = find_repos(&d, DISCOVERY_BUDGET);
    assert_eq!(found.repos, vec![d.join("a"), d.join("b/sub")]);
    assert!(!found.capped);
}

#[test]
fn a_git_file_marks_a_repository_too() { /* d/w/.git is a file: d/w is in repos */ }

#[test]
fn the_open_folder_inside_a_repository_names_that_repository_first() {
    // d/.git, open = d/crates/x: repos == [d]; and open = d itself: repos == [d] plus any found below
    assert_eq!(enclosing_repo(&d.join("crates/x")), Some(d.clone()));
    assert_eq!(find_repos(&d.join("crates/x"), DISCOVERY_BUDGET).repos, vec![d.clone()]);
}

#[test]
fn the_depth_cap_and_the_budget_stop_the_walk() {
    // d/1/2/3/4/.git is at depth 4 (found); d/1/2/3/4/5/.git would be depth 5 (not found)
    // a folder with 30 empty subfolders and budget 10: folders_read <= 10, capped
}

#[test]
fn canonical_resolves_a_symlinked_folder_and_keeps_a_missing_path() {
    // tempdir is under /var, really /private/var: canonical(&d) starts with "/private"
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `nice -n 15 cargo test -p arkos wt_scan::`
Expected: FAIL (the module does not exist / functions not found)

- [ ] **Step 3: Implement** the constants and functions above in `crates/arkos/src/wt_scan.rs`. `find_repos` is an explicit stack walk over `read_dir` using `DirEntry::file_type` (a symlink's type is `is_symlink`, never followed); `home` is `open == dirs::home_dir()`, or compare with `std::env::var_os("HOME")` as the rest of the crate does (grep `HOME` in `paths.rs` and use the same helper).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `nice -n 15 cargo test -p arkos wt_scan::`
Expected: PASS, and `nice -n 15 cargo check --workspace --all-targets` shows no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/wt_scan.rs crates/arkos/src/main.rs
git commit -m "Find the repositories under a folder, stopping at each" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Ages (`wt_scan.rs`, part two)

**Files:**
- Modify: `crates/arkos/src/wt_scan.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const AGE_BUDGET: usize = 50_000;
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct Newest { pub at: SystemTime, pub capped: bool }
  /// The newest modification time of any file under `dir`, skipping `.git`, symlinks, any folder
  /// holding a `.git` of its own, skipped and cache folders, and `.DS_Store`; `None` when no file
  /// counts. Reads at most `budget` entries; past it the result is what was found, `capped`.
  pub fn newest_change(dir: &Path, budget: usize) -> Option<Newest>;
  /// The short age: `now` under a minute, then `4 min`, `7 h`, `2 d`, `3 w`, `5 mo`, `2 y`.
  pub fn age_text(age: Duration) -> String;
  /// `7d`, `36h`, `90m`, `2w` to a duration; `None` for anything else.
  pub fn parse_age(text: &str) -> Option<Duration>;
  pub const STALE_AFTER: Duration = Duration::from_secs(7 * 86_400);
  ```

- [ ] **Step 1: Write the failing tests** (set file times with `filetime` if the crate is in the lock file, else with `std::fs::File::set_modified`, which is stable):

```rust
#[test]
fn the_newest_file_wins_and_git_caches_other_checkouts_and_ds_store_do_not_count() {
    // w/src/a.rs at T, w/.git/index at T+100, w/target/x at T+100, w/.pytest_cache/y at T+100,
    // w/nested/.git (dir) and w/nested/z at T+100, w/.DS_Store at T+100, w/.github/ci.yml at T+10
    assert_eq!(newest_change(&w, AGE_BUDGET).unwrap(), Newest { at: T + 10s, capped: false }, "the hidden .github counts, the rest do not");
}

#[test]
fn a_worktree_with_no_files_has_no_age() { /* only w/.git: newest_change(&w, AGE_BUDGET) == None */ }

#[test]
fn the_budget_caps_the_walk_and_says_so() { /* 100 files, budget 10: Some(Newest { capped: true, .. }) */ }

#[test]
fn ages_read_short() {
    assert_eq!(age_text(Duration::from_secs(30)), "now");
    assert_eq!(age_text(Duration::from_secs(4 * 60)), "4 min");
    assert_eq!(age_text(Duration::from_secs(7 * 3600)), "7 h");
    assert_eq!(age_text(Duration::from_secs(2 * 86_400)), "2 d");
    assert_eq!(age_text(Duration::from_secs(3 * 7 * 86_400)), "3 w");
    assert_eq!(age_text(Duration::from_secs(150 * 86_400)), "5 mo");
    assert_eq!(age_text(Duration::from_secs(800 * 86_400)), "2 y");
}

#[test]
fn an_age_flag_parses_days_hours_minutes_and_weeks() {
    assert_eq!(parse_age("7d"), Some(Duration::from_secs(7 * 86_400)));
    assert_eq!(parse_age("36h"), Some(Duration::from_secs(36 * 3600)));
    assert_eq!(parse_age("2w"), Some(Duration::from_secs(14 * 86_400)));
    assert_eq!(parse_age("90m"), Some(Duration::from_secs(90 * 60)));
    assert!(parse_age("7").is_none() && parse_age("d").is_none() && parse_age("7 days").is_none());
}
```

- [ ] **Step 2: Run to verify they fail.** `nice -n 15 cargo test -p arkos wt_scan::` fails on the new names.

- [ ] **Step 3: Implement.** Thresholds for `age_text`: under 60 s `now`; under 60 min minutes; under 24 h hours; under 7 d days; under 30 d weeks; under 365 d months (days / 30); else years (days / 365), all integer division.

- [ ] **Step 4: Run to verify they pass**, and `cargo check` clean.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/wt_scan.rs
git commit -m "A worktree's age from its newest file, and the short age text" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: The create prompt goes

**Files:**
- Modify: `crates/arkos/src/sidebar.rs:240-262` (the `plus`), `:862-1037` (`Stage`, `Drivers`, `Prompt`, `CreateFailure`, `create_failure_target`), `:1039-1085` (`render_prompt`), `:1446-1495` (`read_prompt_drivers`, `create_worktree`), `:1518-1537` (the prompt branch of `wt_key`), tests `:1923-2051`
- Modify: `crates/arkos/src/main.rs:288-301` (`wt_prompt`, `wt_prompt_drivers`, `wt_prompt_focus`), `:695-701`, `:1251`, `:1285`, and every other `wt_prompt` use (`grep -n wt_prompt crates/arkos/src/*.rs`)
- Modify: `crates/arkos/src/explorer.rs` (`filter_drivers` if the prompt was its only caller: `grep -rn filter_drivers crates/arkos/src`; keep `read_filter_args`)
- Modify: `docs/guide.md:67-77` (drop the "+" sentences and the filter-driver sentence), `docs/development.md:716-718` (drop "+ creates `../proj-feat` on a new branch;")

**Interfaces:** none new; `wt_key` keeps its signature.

- [ ] **Step 1: Delete** the code and tests listed, and the two doc sentences. The header's `worktrees-header` keeps its click; `wt_focused` stays (Ctrl+Shift+G).

- [ ] **Step 2: Verify** `nice -n 15 cargo test -p arkos` is green and `grep -rn "wt_prompt\|Drivers\|create_worktree\|default_path" crates/arkos/src` finds nothing; `cargo check` clean; `grep -n "+" docs/guide.md` around the Worktrees paragraph shows no create sentence.

- [ ] **Step 3: Commit**

```bash
git add -A crates/arkos/src docs/guide.md docs/development.md
git commit -m "The Worktrees section no longer creates worktrees: Claude does" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: The model by repository (`worktrees.rs`)

**Files:**
- Modify: `crates/arkos/src/worktrees.rs` (all of it bar `parse_worktree_list`, `remove`, `Changes`, `WtRow::Change`)
- Modify: `crates/arkos/src/main.rs:642-643` (`Worktrees::new` keeps its signature)

**Interfaces:**
- Consumes: `wt_scan::{canonical, Newest, STALE_AFTER}` (Tasks 1 and 2), `explorer::git_read`.
- Produces:
  ```rust
  pub struct Worktree {               // `is_main` removed
      pub path: PathBuf,              // canonical
      pub repo: PathBuf,              // the main checkout, canonical: the first block git printed
      pub head: String,
      pub branch: Option<String>,
      pub missing: bool,
      pub bare: bool,
      pub locked: bool,               // new: a `locked` line
  }
  /// The mtimes of `<repo>/.git/worktrees/<name>/{HEAD,gitdir,locked}` (None for a file that is not there).
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
  pub struct Stamp { pub head: Option<SystemTime>, pub gitdir: Option<SystemTime>, pub locked: Option<SystemTime> }
  pub fn stamp(repo: &Path, name: &str) -> Stamp;
  /// `git worktree list` for the repository `path` is in (a main checkout or a linked worktree):
  /// the side worktrees only, each with `repo` set, paths canonical. Err is the notice text:
  /// `explorer::STATUS_OFF` for a refused filter config, else git's words.
  pub fn list_side(path: &Path) -> Result<Vec<Worktree>, String>;
  /// The worktree's `.git/worktrees/<name>` folder name: the last component of the `gitdir` line git
  /// prints (`parse_worktree_list` keeps it as `admin_name: Option<String>`).
  pub struct Age { pub changed: Option<SystemTime>, pub capped: bool, pub lagging: bool }
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum Watched { Watching, NotLocal, Failed(String), FolderGone }
  pub struct Worktrees {
      pub by_repo: BTreeMap<PathBuf, Vec<Worktree>>,   // replaces `list`
      pub ages: HashMap<PathBuf, Age>,
      pub stamps: HashMap<PathBuf, Stamp>,              // by worktree path
      pub errors: BTreeMap<PathBuf, String>,            // repositories that could not be listed
      pub watched: Watched,
      pub hold_order: bool,                             // the pointer or keyboard is in the section
      pub selected: Option<PathBuf>,                    // was Option<usize>
      // expanded, changes, loading, loading_since, stale, opened_at as today
  }
  impl Worktrees {
      pub fn set_repo(&mut self, repo: &Path, list: Vec<Worktree>);   // replaces that repo's rows; drops state of rows gone
      pub fn remove_repo(&mut self, repo: &Path);
      pub fn set_age(&mut self, path: &Path, age: Age);
      pub fn touched(&mut self, path: &Path, at: SystemTime);         // age = now; keeps capped
      pub fn mark_missing(&mut self, path: &Path);
      pub fn repo_of(&self, path: &Path) -> Option<&Path>;             // which repo lists this worktree
      pub fn worktree_at(&self, dir: &Path) -> Option<&Worktree>;      // the listed worktree `dir` is inside
      pub fn all(&self) -> impl Iterator<Item = &Worktree>;
      pub fn ordered(&self) -> Vec<&Worktree>;   // newest first; no age last; stable by path; frozen while hold_order
      pub fn rows(&self) -> Vec<WtRow>;          // as today, over ordered()
      pub fn find(&self, path: &Path) -> Option<&Worktree>;
      pub fn is_stale(&self, path: &Path, now: SystemTime) -> bool;   // age >= STALE_AFTER
      pub fn not_git(&self) -> bool;             // no repositories and no errors
  }
  pub enum WtRow { Worktree { wt: Worktree, age: Option<Age>, expanded, loading, dirty }, Change { .. } }
  ```
  `ordered()` while `hold_order` returns the order last computed (store `order: Vec<PathBuf>` and recompute only when `hold_order` is false, in `ordered()` itself or a `resort()` the paint calls).

- [ ] **Step 1: Write the failing tests** (git fixtures as `gitstatus.rs`'s tests make them: `explorer::git_test` or the crate's test helper that runs `git init` with the user config blinded; `list_side` tests need real git):

```rust
#[test]
fn the_porcelain_list_keeps_the_admin_name_and_locked() { /* parse: `gitdir`-less blocks parse; a block with `locked` sets locked */ }

#[test]
fn listing_from_a_main_checkout_gives_its_side_worktrees_without_itself() {
    // git init main; git worktree add ../side -b side; list_side(main) == [side], side.repo == canonical(main)
}

#[test]
fn listing_from_inside_a_linked_worktree_gives_the_same_list() {
    // list_side(side) == list_side(main): side itself is in it, main is not (Review Focus 2)
}

#[test]
fn a_stamp_moves_when_the_worktree_switches_branch_and_not_on_a_status() {
    // stamp before; `git status` in side; stamp equal. `git checkout -b other` in side; stamp.head differs
}

#[test]
fn rows_come_newest_first_with_unaged_last_and_hold_while_the_pointer_is_in() {
    // two repos, three worktrees; set_age on two; ordered() == [newer, older, unaged]
    // hold_order = true; touched(older, now); ordered() unchanged; hold_order = false; ordered() == [older, newer, unaged]
}

#[test]
fn a_repository_set_again_drops_the_state_of_rows_that_went() { /* expanded + changes of a removed wt gone; selected cleared if gone */ }

#[test]
fn a_stale_worktree_is_one_unchanged_for_a_week() { /* is_stale at 6 d false, 7 d true, no age false */ }
```
  Update the existing `rows_list_worktrees_and_the_changes_of_the_expanded_ones` and `set_list` tests to the new API; delete `default_path_sits_beside_the_project_named_after_the_branch`.

- [ ] **Step 2: Run to verify they fail.** `nice -n 15 cargo test -p arkos worktrees::`

- [ ] **Step 3: Implement.** Delete `default_path`, `branch_exists`, `create`. `list_side` runs `git_read(path)?.args(["worktree", "list", "--porcelain", "-z"])`; the first block's path (canonicalised) is `repo`; `missing` blocks keep the path git printed, canonicalised when it exists. `main.rs:642-643` still compiles (`Worktrees::new(Vec<PathBuf>)`). `sidebar.rs` is adjusted only as far as it compiles, Task 8 finishes it: `render_worktree_rows` matches `WtRow::Worktree { age, .. }` and compares `selected` by path (`root.worktrees.selected.as_deref() == Some(wt.path.as_path())`), `wt_key`'s Up and Down step a path through `ordered()`, `worktree_menu_items` loses its `is_main` branch, and `refresh_worktrees` calls `list_side(&root.project)` applying the result with `set_repo` (or an error with `errors`). Name each such edit in the report.

- [ ] **Step 4: Run to verify they pass**, `cargo test -p arkos` whole crate green (stubs included), `cargo check` clean.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/worktrees.rs crates/arkos/src/sidebar.rs crates/arkos/src/main.rs
git commit -m "Worktrees by repository: side worktrees only, with ages, stamps and a newest-first order" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: The FSEvents binding (`fsevents.rs`)

**Files:**
- Create: `crates/arkos/src/fsevents.rs`
- Modify: `crates/arkos/src/main.rs` (`mod fsevents;`)

**Interfaces:**
- Produces:
  ```rust
  pub const LATENCY: f64 = 1.0;
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum Event {
      /// Something changed in this directory (per-directory flavour; the path is as the kernel reports it, real).
      Changed(PathBuf),
      /// The kernel dropped events: rescan from the start.
      MustScan,
      /// A watched root was renamed, moved or removed.
      RootChanged,
  }
  pub struct Stream { /* FSEventStreamRef, the boxed sender, the dispatch queue */ }
  impl Stream {
      /// Starts a stream over `paths` with `kFSEventStreamCreateFlagWatchRoot`, delivering since
      /// `since` (`None` for now) on `tx`. Err is the reason, for the section's notice.
      pub fn start(paths: &[PathBuf], since: Option<u64>, tx: UnboundedSender<Event>) -> Result<Stream, String>;
      pub fn latest_event_id(&self) -> u64;
  }
  impl Drop for Stream { /* Stop, Invalidate, Release; then the sender is dropped */ }
  /// Whether the volume holding `path` is local (`statfs`, `MNT_LOCAL`); a network volume takes a
  /// stream and delivers nothing.
  pub fn local_volume(path: &Path) -> bool;
  ```
  `Stream` is `Send`; it is not `Sync` and is owned by `Root`.

- [ ] **Step 1: Write the failing tests**:

```rust
#[test]
fn a_write_in_a_watched_folder_is_reported_within_seconds() {
    // tempdir d (canonical); let (tx, mut rx) = unbounded(); let s = Stream::start(&[d.clone()], None, tx).unwrap();
    // write d/sub/file.txt; block_on with a 10 s deadline (futures::executor::block_on + a timeout loop over try_next)
    // assert some Event::Changed(p) arrived with p == d or p == d.join("sub")
    // drop(s): the test ends without hanging
}

#[test]
fn the_latest_event_id_moves_after_an_event() { /* id after the event > id before */ }

#[test]
fn a_temporary_folder_is_on_a_local_volume_and_a_missing_path_is_not() {
    assert!(local_volume(&d));
    assert!(!local_volume(Path::new("/nonexistent/arkos")));
}
```
  If `Stream::start` fails on a CI runner, the first two tests print the reason and return (not panic), so the suite stays green there; a comment says why.

- [ ] **Step 2: Run to verify they fail.** `nice -n 15 cargo test -p arkos fsevents::`

- [ ] **Step 3: Implement.** The extern block: `FSEventStreamCreate`, `FSEventStreamSetDispatchQueue`, `FSEventStreamStart`, `FSEventStreamStop`, `FSEventStreamInvalidate`, `FSEventStreamRelease`, `FSEventStreamGetLatestEventId`, plus `dispatch_queue_create` and `dispatch_release` from libdispatch (`#[link(name = "System")]` is implicit; declare `dispatch_queue_create(label: *const c_char, attr: *const c_void)`), and CoreFoundation `CFArrayCreate`/`CFStringCreateWithCString`/`CFRelease` (`#[link(name = "CoreFoundation", kind = "framework")]`). Flags: `kFSEventStreamCreateFlagNone | kFSEventStreamCreateFlagWatchRoot` (0x4); event flags checked: `kFSEventStreamEventFlagMustScanSubDirs` (0x1), `kFSEventStreamEventFlagRootChanged` (0x20). `sinceWhen`: `kFSEventStreamEventIdSinceNow` is `0xFFFFFFFFFFFFFFFF`. The callback's `info` is a `*mut UnboundedSender<Event>` from `Box::into_raw`, freed in `Drop` after `FSEventStreamRelease`. The callback converts each `char*` path with `CStr` and sends; it never panics (a send to a dropped receiver is ignored).

- [ ] **Step 4: Run to verify they pass**, `cargo check` clean.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/fsevents.rs crates/arkos/src/main.rs
git commit -m "A thin FSEvents stream: directory changes over a channel" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: The event rules (`wt_watch.rs`)

**Files:**
- Create: `crates/arkos/src/wt_watch.rs`
- Modify: `crates/arkos/src/main.rs` (`mod wt_watch;`)

**Interfaces:**
- Consumes: `wt_scan::{skipped_dir, DISCOVERY_DEPTH}`, `worktrees::{Worktrees, Stamp, stamp}` (Task 4).
- Produces:
  ```rust
  /// Whether an event path is worth looking at: not under a skipped or cache folder, and not under
  /// `.git` except `.git/worktrees` and its immediate children. `home` as in `skipped_dir`.
  pub fn passes(path: &Path, open: &Path, home: bool) -> bool;
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum Rule {
      /// `<repo>/.git/worktrees` itself: list that repository again.
      RelistRepo(PathBuf),
      /// `<repo>/.git/worktrees/<name>`: compare the stamp; relist only if it moved.
      CheckStamp { repo: PathBuf, name: String },
      /// Inside a listed worktree: it changed now (after the `.DS_Store` check).
      Touched(PathBuf),
      /// Read that folder once: a `.git` there registers a repository (one not known yet below the
      /// open folder within the discovery depth, or one nested in a known repository's own files,
      /// which is how nested repositories are found), a known repository whose `.git` is gone is dropped.
      Unknown(PathBuf),
      /// Past the discovery depth and inside no known repository: nothing.
      Ignore,
  }
  /// The innermost known repository or worktree holding `path` decides: a worktree means `Touched`,
  /// a repository's `.git/worktrees` the two stamp rules, a repository's other files `Unknown`.
  pub fn classify(path: &Path, open: &Path, model: &Worktrees) -> Rule;
  /// Whether the only entry in `dir` newer than `since` is `.DS_Store` (one read_dir).
  pub fn only_ds_store_newer(dir: &Path, since: SystemTime) -> bool;
  pub const FULL_PASS_EVERY: Duration = Duration::from_secs(600);
  pub const EXTRA_PATHS: usize = 16;
  /// The stream's path set: the open folder plus up to EXTRA_PATHS worktrees outside it, sorted;
  /// and which worktrees were left out (they lag).
  pub fn stream_paths(open: &Path, model: &Worktrees) -> (Vec<PathBuf>, Vec<PathBuf>);
  ```

- [ ] **Step 1: Write the failing tests** (a model built by hand with `set_repo`; paths canonical):

```rust
#[test]
fn a_build_writing_target_is_dropped_before_it_costs_anything() {
    assert!(!passes(&open.join("ide-arkos/.worktrees/x/target/debug"), &open, false));
    assert!(!passes(&open.join("a/node_modules/b"), &open, false));
    assert!(!passes(&open.join("a/.pytest_cache"), &open, false));
    assert!(!passes(&open.join("a/.git/objects/ab"), &open, false));
    assert!(passes(&open.join("a/.git/worktrees"), &open, false) && passes(&open.join("a/.git/worktrees/x"), &open, false));
    assert!(!passes(&open.join("a/.git/worktrees/x/logs"), &open, false), "deeper than a worktree's own folder");
    assert!(passes(&open.join("a/.github/workflows"), &open, false), "hidden but not a cache");
}

#[test]
fn events_classify_by_where_they_land() {
    // model: repo R = open/a with side worktree W = open/a/.worktrees/w
    assert_eq!(classify(&R.join(".git/worktrees"), &open, &m), Rule::RelistRepo(R.clone()));
    assert_eq!(classify(&R.join(".git/worktrees/w"), &open, &m), Rule::CheckStamp { repo: R.clone(), name: "w".into() });
    assert_eq!(classify(&W.join("src"), &open, &m), Rule::Touched(W.clone()));
    assert_eq!(classify(&W, &open, &m), Rule::Touched(W.clone()));
    assert_eq!(classify(&R.join("src"), &open, &m), Rule::Unknown(R.join("src")), "one read_dir: a nested repository registers when first touched");
    assert_eq!(classify(&open.join("new"), &open, &m), Rule::Unknown(open.join("new")));
    assert_eq!(classify(&open.join("1/2/3/4/5/6"), &open, &m), Rule::Ignore, "past the discovery depth and in no repository");
}

#[test]
fn an_event_under_a_symlinked_open_folder_still_matches_its_worktree() {
    // tempdir real; link -> real; model built from canonical(link); classify(real.join("a/.worktrees/w/src"), &canonical(&link), &m) is Touched
}

#[test]
fn a_folder_whose_only_newer_entry_is_ds_store_does_not_count() { /* dir with a.rs at T and .DS_Store at T+10: only_ds_store_newer(dir, T+5) true; add b.rs at T+10: false */ }

#[test]
fn the_stream_covers_the_open_folder_and_up_to_sixteen_outside_worktrees() {
    // 20 worktrees outside open: paths.len() == 17, left_out.len() == 4, open first
}
```

- [ ] **Step 2: Run to verify they fail.** `nice -n 15 cargo test -p arkos wt_watch::`

- [ ] **Step 3: Implement** as pure functions over `Worktrees::{by_repo, worktree_at}`.

- [ ] **Step 4: Run to verify they pass**, `cargo check` clean.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/wt_watch.rs crates/arkos/src/main.rs
git commit -m "The worktree watcher's rules: what each changed folder means" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: The window drives the stream (`sidebar.rs`, `main.rs`, `explorer_watch.rs`)

**Files:**
- Modify: `crates/arkos/src/sidebar.rs:1314-1334` (`refresh_worktrees` becomes the full pass), new `Root` methods beside it
- Modify: `crates/arkos/src/main.rs:288` area (new fields), `:5097` (`refresh_worktrees` call stays, now the first load), window close (`after_window_closed` at `:595`: the `Root` drop releases the stream; confirm `Root` is dropped on close or add an explicit `stop_worktree_watch` where the window's other watchers stop)
- Modify: `crates/arkos/src/explorer_watch.rs:68-80, 126-140` (`SlowTick` loses `worktrees`; the tick still calls `mark_all_stale` + `load_pending_worktrees` for expanded rows, and now `root.tick_worktree_ages(cx)`)

**Interfaces:**
- Consumes: Tasks 1, 2, 4, 5 and 6.
- Produces (on `Root`, in `sidebar.rs`):
  ```rust
  /// The first load and the safety-net pass: find repositories, list each, age each; rows show as
  /// each stage lands; a result older than a newer pass is dropped (`worktrees_seq`).
  pub fn refresh_worktrees(&mut self, cx: &mut Context<Self>);
  /// Creates the stream over `wt_watch::stream_paths` (recreating with `since` when the set
  /// changes) and spawns the drain task; sets `worktrees.watched`.
  pub fn start_worktree_watch(&mut self, cx: &mut Context<Self>);
  /// One event from the stream, by `wt_watch::classify`; git and walks run on the background executor.
  pub fn worktree_event(&mut self, event: fsevents::Event, cx: &mut Context<Self>);
  /// Every 10 s from the slow tick: notify when the section is open and has rows (the age text
  /// is computed at paint); on the 10 minute mark, `refresh_worktrees`.
  pub fn tick_worktree_ages(&mut self, cx: &mut Context<Self>);
  ```
  New `Root` fields: `wt_stream: Option<fsevents::Stream>`, `wt_stream_paths: Vec<PathBuf>`, `wt_pass_at: Instant`, `wt_events: Option<UnboundedReceiver<fsevents::Event>>` (taken by the drain task).
  Pure helpers in `explorer_watch.rs`, tested: `full_pass_due(last: Instant, now: Instant) -> bool` (`FULL_PASS_EVERY`), and `SlowTick { rediff }` only.

- [ ] **Step 1: Write the failing tests**: in `explorer_watch.rs`, `the_full_pass_runs_every_ten_minutes` (`full_pass_due`); update `slow_tick_plan`'s test (no `worktrees` field). In `sidebar.rs`'s tests, a pure `watched_notice(w: &Watched, status_off: Option<&str>) -> Option<&'static str>` with `the_section_says_when_the_folder_is_not_watched`: `Watching` → None; `NotLocal` → `"not watched: the folder is on a network volume"`; `Failed(_)` → `"not watched: the file watcher could not start"`; `FolderGone` → `"the folder is gone"`.

- [ ] **Step 2: Run to verify they fail.**

- [ ] **Step 3: Implement.** `refresh_worktrees`: spawn on the background executor `wt_scan::find_repos(open, DISCOVERY_BUDGET)`, then for each repo `worktrees::list_side` (spawn up to 4 at once with `futures::stream::FuturesUnordered` or a plain loop of `cx.background_executor().spawn` tasks joined in order; the builder picks), applying each with `set_repo`/`errors` as it lands, then `newest_change` per side worktree applied with `set_age`; the stamps are read after each list (`stamp` per worktree, stored). `worktree_event`: `MustScan` → `refresh_worktrees`; `RootChanged` → re-resolve: open folder exists → `refresh_worktrees`, else `watched = FolderGone` and `by_repo.clear()`; `Changed(p)` → `passes` then `classify`: `RelistRepo` runs `list_side` for that repo; `CheckStamp` compares `stamp()` (read off the UI thread) with the stored one, relisting on a difference; `Touched(w)` stats `w` (missing → `mark_missing`), else `only_ds_store_newer` when the stored age is older than a minute, then `touched(w, now)`; `Unknown(d)` reads `d` once (`read_dir`, microseconds, at most once a second per folder by the stream's latency): a `.git` there and `d` not a known repository → `list_side(d)`, `set_repo`, stamps and `newest_change` for its side worktrees (this is how a nested repository registers); no `.git` and `d` a known repository → `remove_repo`; otherwise nothing. After any list change, `stream_paths` is recomputed; a changed set recreates the stream with `since = latest_event_id()`, and the left-out worktrees get `ages[..].lagging = true`. `start_worktree_watch`: `local_volume(open)` false → `NotLocal`, no stream; `Stream::start` Err(e) → `Failed(e)`; else `Watching`, drain task `while let Some(ev) = rx.next().await { this.update(cx, |root, cx| root.worktree_event(ev, cx)) }`. `main.rs:5097`: call `start_worktree_watch` right after `refresh_worktrees`. `hold_order` is true while the pointer is over the list (the hover is wired in Task 8) or `wt_focused` with a selection, else false; the paint reads `ordered()`.

- [ ] **Step 4: Run** `nice -n 15 cargo test -p arkos` green, `cargo check` clean. Then a hand run: `nice -n 15 cargo run -p arkos -- ~/Projects` in the scratch sandbox (`arkos sandbox ~/Projects` built from this tree), touch a file in a worktree, see its age reset within two seconds; `git worktree add` one and see it appear; `rm -rf` one and see `missing`; `cargo build` in one and see its age stay. Record what was seen in the task report.

- [ ] **Step 5: Commit**

```bash
git add crates/arkos/src/sidebar.rs crates/arkos/src/main.rs crates/arkos/src/explorer_watch.rs
git commit -m "The Worktrees section follows FSEvents: every repository under the folder, live" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: The rows (`sidebar.rs` paint) and the user guide

**Files:**
- Modify: `crates/arkos/src/sidebar.rs:1086-1212` (`render_worktree_rows`), `:1538-1575` (`wt_key` moves `selected` by path), `:815-818` (`clamp_selection` goes or takes the ordered paths), the `WtMenuAction` items (`:840-860`: no `is_main` branch)
- Modify: `docs/guide.md:67-77`, `docs/development.md:716-719` (Status rows)

**Interfaces:**
- Consumes: `Worktrees::{ordered, rows, selected, is_stale, watched, errors}` (Task 4), `wt_scan::age_text` (Task 2), `hold_order` set in Task 7.
- Produces: pure, tested in `sidebar.rs`:
  ```rust
  /// The row's name line: the folder name, and the branch only when it differs (`detached` when none).
  pub fn wt_row_label(wt: &Worktree) -> (String, Option<String>);
  /// The row's place line: relative to `open` when inside it, else `~`-shortened.
  pub fn wt_row_place(path: &Path, open: &Path, home: &Path) -> String;
  /// The age cell: `age_text` of the age, with `+` after a capped walk ("at least this recent"), `~` after a lagging one; empty with no age.
  pub fn wt_row_age(age: Option<&Age>, now: SystemTime) -> String;
  ```

- [ ] **Step 1: Write the failing tests**:

```rust
#[test]
fn a_row_names_the_folder_and_the_branch_only_when_it_differs() {
    assert_eq!(wt_row_label(&wt("/p/.worktrees/csv-grid", Some("csv-grid"))), ("csv-grid".into(), None));
    assert_eq!(wt_row_label(&wt("/p/.worktrees/release", Some("spinner-crossfade"))), ("release".into(), Some("spinner-crossfade".into())));
    assert_eq!(wt_row_label(&wt("/p/x", None)), ("x".into(), Some("detached".into())));
}

#[test]
fn a_row_places_the_worktree_relative_to_the_open_folder_or_under_home() {
    assert_eq!(wt_row_place(Path::new("/Users/me/Projects/a/.worktrees/w"), Path::new("/Users/me/Projects"), Path::new("/Users/me")), "a/.worktrees/w");
    assert_eq!(wt_row_place(Path::new("/Users/me/other/w"), Path::new("/Users/me/Projects"), Path::new("/Users/me")), "~/other/w");
    assert_eq!(wt_row_place(Path::new("/Volumes/x/w"), Path::new("/Users/me/Projects"), Path::new("/Users/me")), "/Volumes/x/w");
}

#[test]
fn the_age_cell_marks_a_capped_walk_and_a_lagging_one_and_is_blank_without_an_age() {
    assert_eq!(wt_row_age(Some(&age(4 * 60, false, false)), now), "4 min");
    assert_eq!(wt_row_age(Some(&age(4 * 60, true, false)), now), "4 min+");
    assert_eq!(wt_row_age(Some(&age(4 * 60, false, true)), now), "4 min~");
    assert_eq!(wt_row_age(None, now), "");
}
```
  Update `the_worktree_menu_offers_a_terminal_and_removal_but_never_for_the_main_checkout` to `..._for_every_row` (no main), and `the_worktrees_selection_stays_on_a_row` to the path-keyed selection (a selected path that went away reads `None`; Down from none selects the first ordered row).

- [ ] **Step 2: Run to verify they fail.**

- [ ] **Step 3: Implement the paint.** A worktree row is two lines tall (`2 * ROW_H` less the padding the builder finds right): line one `name`, dimmed branch, right-aligned age (`text_dim`; a stale row's name in `text_dim` too; `missing` in `theme.tree.deleted` as today), line two the place at `style.ui(11.0)` in `text_dim`. A `Change` row is as today. The list's empty states, each one dim line as `outline.rs:102` does: `watched_notice` text when not `Watching`; else `errors` present and nothing listed → the first error's text (`STATUS_OFF` reads as today); else no rows → `"no worktrees"`. Selection and keys by path: Up/Down step through `ordered()` (begun in Task 4, finished here). `on_hover` of the `worktree-rows` list sets `hold_order` (Task 7 declares it; this task wires the hover). The sidebar's redraw for ages comes from `tick_worktree_ages` (Task 7); nothing here animates.

- [ ] **Step 4: Docs.** `docs/guide.md:67-77`: rewrite the Worktrees paragraph: what is listed (every repository under the open folder, side worktrees only, a folder of repositories works), the row (name, branch when it differs, age since any file changed, place), newest first, a week dims, `missing`, live within about a second, builds and caches do not count, the network-volume line; keep expand/menu/Ctrl+Shift+G sentences. `docs/development.md` Status, replacing the Worktrees row: "Worktrees on `~/Projects`: one row per side worktree of every repo, newest first; touch a file in one: its age reads `now` within two seconds and it moves to the top (not while the pointer is over the list); `cargo build` in one: its age holds; `rm -rf` one: `missing` within two seconds; `git worktree add`: it appears; expand one: a shimmer, then its changed files; Remove on a dirty one is refused with git's words, force removes it; open `~`: the sidebar does not stall; Activity Monitor: Arkos idle while nothing changes".

- [ ] **Step 5: Run** tests green, `cargo check` clean; a throwaway local shoot story (`OUT=<scratch> scripts/shoot.sh <story>`, story and fixture deleted after, never committed) of a folder with two repositories and three side worktrees in Material, to see the rows; keep the still in the scratch folder for the task report.

- [ ] **Step 6: Commit**

```bash
git add crates/arkos/src/sidebar.rs docs/guide.md docs/development.md
git commit -m "Worktree rows: name, age since any file changed, and place; newest first" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: `arkos worktrees` (`worktrees_cli.rs`) and the agent guide

**Files:**
- Create: `crates/arkos/src/worktrees_cli.rs`
- Modify: `crates/arkos/src/main.rs` (`mod worktrees_cli;` and the dispatch at `:4586-4601`: `Some("worktrees") => std::process::exit(worktrees_cli::worktrees_cli(&args[2..]))`, before `theme`)
- Modify: `crates/arkos/src/agent_cli.rs:19-36` (`USAGE` gains `       arkos worktrees [<folder>] ...  the side worktrees under a folder, newest change first, for pruning`)
- Modify: `docs/agent-guide.md` (a `### \`arkos worktrees\`` section after `### \`arkos panes\``, before `### Times`; the exit-codes paragraph and the JSON list gain its lines), `crates/arkos/src/guide.rs:265-275` (the named list gains `"arkos worktrees"`, `"--stale"`; the usage loop gains `worktrees_cli::WORKTREES_USAGE`)
- Modify: `docs/superpowers/specs/2026-11-03-worktrees-everywhere-design.md` (replace the telemetry bullet with: "No telemetry: windowless agent commands record nothing, as `arkos theme` does.")

**Interfaces:**
- Consumes: `wt_scan::{find_repos, newest_change, parse_age, age_text, canonical, DISCOVERY_BUDGET, AGE_BUDGET}` (Tasks 1 and 2), `worktrees::list_side` (Task 4), `agent_cli::usage`.
- Produces:
  ```rust
  pub const WORKTREES_USAGE: &str = "usage: arkos worktrees [<folder>] [--stale <age>] [--json]\nsee: arkos guide\n";
  pub fn worktrees_cli(args: &[String]) -> i32;
  /// Testable: `cwd` for the default folder, `now` for the ages.
  pub fn worktrees_cli_to(args: &[String], cwd: &Path, now: SystemTime, out: &mut dyn Write, err: &mut dyn Write) -> i32;
  /// The pass the window runs, whole: repositories, their side worktrees, each one's age.
  pub struct Survey { pub worktrees: Vec<(Worktree, Option<Newest>)>, pub errors: Vec<(PathBuf, String)> }
  pub fn survey(open: &Path) -> Survey;
  pub fn text_lines(s: &Survey, open: &Path, now: SystemTime, stale: Option<Duration>) -> Vec<String>;
  pub fn json(s: &Survey, now: SystemTime, stale: Option<Duration>) -> serde_json::Value;
  ```
  Text line: `{age:>6}  {name}{branch}  {place}` where branch is ` ({branch})` only when it differs or `(detached)`, place as `wt_row_place` with `~` for home, `missing` appended to a missing one. Errors go to stderr as `cannot list {repo}: {reason}`. JSON: `{ "folder": open, "worktrees": [ { "path", "name", "branch" | null, "repository", "changed": secs | null, "age_seconds": secs | null, "missing", "locked", "capped" } ], "repositories": [ { "path", "error"? } ] }`. `--stale X` keeps worktrees with `age_seconds >= X` (no age: kept, since unknown is not fresh). Exit: 0 (also with an empty list); 1 when `<folder>` does not exist (`no such folder: {path}`); 2 usage (a bad `--stale`, an extra word, an unknown flag), the usage text on stderr ending `see: arkos guide`, and with `--json` an error is `{ "error", "exit" }` on stdout as the other commands do.

- [ ] **Step 1: Write the failing tests** (git fixtures as in Task 3):

```rust
#[test]
fn the_command_lists_side_worktrees_newest_first_with_ages_and_places() {
    // open/a (main) with side open/a/.worktrees/w1 (file touched 2 d ago) and open/w2 beside it (touched 1 h ago)
    // worktrees_cli_to(&[open], open, now, out, err) == 0; lines == ["   1 h  w2  w2", "   2 d  w1  a/.worktrees/w1"]
}

#[test]
fn stale_keeps_only_worktrees_unchanged_for_at_least_that_long() { /* --stale 1d leaves w1 only */ }

#[test]
fn json_carries_every_field_and_a_refused_repository_is_named_not_skipped() {
    // a second repo whose .git/config sets filter.x.process=something-unreadable (as explorer.rs's refusal tests do):
    // its entry in "repositories" has "error", its worktrees are absent, exit 0, the others listed (Review Focus 5)
}

#[test]
fn a_bad_age_or_a_missing_folder_is_refused_with_the_right_exit_code() {
    // --stale soon: 2 and the usage on stderr; /nonexistent: 1 and "no such folder: /nonexistent"; --json variants print { "error", "exit" }
}

#[test]
fn the_default_folder_is_the_current_one_and_a_subfolder_resolves_to_its_repository() { /* cwd = open/a/src: lists a's side worktrees */ }
```
  In `guide.rs`, extend the named list and the usage loop.

- [ ] **Step 2: Run to verify they fail.** `nice -n 15 cargo test -p arkos worktrees_cli:: guide::`

- [ ] **Step 3: Implement**, parsing with a local copy of `open_cli.rs`'s `parse` shape (`valued = ["--stale"]`, no switches; make `open_cli::parse` and `Parsed` `pub(crate)` instead of copying if that is a two-line change).

- [ ] **Step 4: Docs.** The guide section, in the voice of `### \`arkos panes\``: what it lists, the text line, `--stale`, `--json` fields, that it works from any folder without a window, and one line: "It only reports. To prune, confirm with the user, then run `git worktree remove <path>` from that worktree's repository; `--force` for one with changes." Exit codes and the JSON shapes join the existing lists. Spec amendment as above.

- [ ] **Step 5: Run** `nice -n 15 cargo test --workspace` green (the arkosd pty tests need a terminal; skip them as `docs/development.md` Tests says if they fail for that reason only), `cargo check` clean; `nice -n 15 cargo run -p arkos -- worktrees ~/Projects` prints Cyrus's eight side worktrees newest first, and `--stale 7d --json` a subset.

- [ ] **Step 6: Commit**

```bash
git add crates/arkos/src/worktrees_cli.rs crates/arkos/src/main.rs crates/arkos/src/agent_cli.rs crates/arkos/src/guide.rs docs/agent-guide.md docs/superpowers/specs/2026-11-03-worktrees-everywhere-design.md
git commit -m "arkos worktrees: the side worktrees under a folder, with ages, for pruning" -m "Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
