//! `arkos worktrees [<folder>] [--stale <age>] [--json]`: the side worktrees of every repository
//! under a folder, newest change first, so an agent can find stale ones to prune after asking the
//! user. It runs the sidebar's first-load pass itself (find the repositories, list each one's side
//! worktrees, walk each worktree for its newest file change) and prints; it needs no window and no
//! daemon, and it only reports. No gpui here. Exit codes: 0 listed (also an empty list, and a list
//! that left out a repository git could not read), 1 no such folder, 2 usage.

use crate::agent_cli;
use crate::open_cli::parse;
use crate::worktrees::{list_side, wt_row_label, Worktree};
use crate::wt_scan::{age_text, canonical, find_repos, newest_change, parse_age, wt_row_place, Newest, AGE_BUDGET, DISCOVERY_BUDGET};
use serde_json::{json, Value as Json};
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const WORKTREES_USAGE: &str = "usage: arkos worktrees [<folder>] [--stale <age>] [--json]\nsee: arkos guide\n";

/// The pass the window runs, whole: repositories, their side worktrees, each one's age.
pub struct Survey {
    /// The folder surveyed, canonical.
    pub folder: PathBuf,
    /// Every repository found, by its main checkout's path, the ones that failed too.
    pub repos: Vec<PathBuf>,
    /// Each side worktree once, with the newest change the walk found (`None`: no file counts,
    /// or no files to walk).
    pub worktrees: Vec<(Worktree, Option<Newest>)>,
    /// The repositories whose worktrees could not be listed, with the reason.
    pub errors: Vec<(PathBuf, String)>,
}

/// `arkos worktrees ...` from the current folder, now.
pub fn worktrees_cli(args: &[String]) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    worktrees_cli_to(args, &cwd, SystemTime::now(), &mut std::io::stdout().lock(), &mut std::io::stderr().lock())
}

/// Testable: `cwd` for the default folder, `now` for the ages.
pub fn worktrees_cli_to(args: &[String], cwd: &Path, now: SystemTime, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let json_out = args.iter().any(|a| a == "--json");
    let Some(p) = parse(args, &["--stale"], &[]) else { return usage(json_out, None, out, err) };
    if p.words.len() > 1 {
        return usage(json_out, None, out, err);
    }
    let stale = match p.value("--stale") {
        None => None,
        Some(text) => match parse_age(text) {
            Some(age) => Some(age),
            None => return usage(json_out, Some(format!("not an age: \"{text}\"")), out, err),
        },
    };
    let folder = match p.words.first() {
        Some(word) if Path::new(word).is_absolute() => PathBuf::from(word),
        Some(word) => cwd.join(word),
        None => cwd.to_path_buf(),
    };
    if !folder.is_dir() {
        return refuse(json_out, &format!("no such folder: {}", folder.display()), 1, out, err);
    }
    let open = canonical(&folder);
    let s = survey(&open);
    if json_out {
        let _ = writeln!(out, "{}", json(&s, now, stale));
    } else {
        for line in text_lines(&s, &open, now, stale) {
            let _ = writeln!(out, "{line}");
        }
        for (repo, reason) in &s.errors {
            let _ = writeln!(err, "cannot list {}: {reason}", repo.display());
        }
    }
    0
}

/// A usage error: exit 2 with the usage text on stderr (after `why`, when there is one), and with
/// `--json` the error object on stdout as well.
fn usage(json_out: bool, why: Option<String>, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if json_out {
        let text = why.clone().unwrap_or_else(|| WORKTREES_USAGE.lines().next().unwrap_or_default().to_string());
        let _ = writeln!(out, "{}", json!({ "error": text, "exit": 2 }));
    }
    if let Some(why) = why {
        let _ = writeln!(err, "{why}");
    }
    agent_cli::usage(err, WORKTREES_USAGE)
}

/// A refusal with exit `code`: the text on stderr, or with `--json` `{ "error", "exit" }` on stdout.
fn refuse(json_out: bool, text: &str, code: i32, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if json_out {
        let _ = writeln!(out, "{}", json!({ "error": text, "exit": code }));
    } else {
        let _ = writeln!(err, "{text}");
    }
    code
}

/// Finds the repositories under `open` (the one it is inside first), lists each one's side
/// worktrees and walks each worktree for its newest change. A side worktree is a repository to
/// the walk too (it holds a `.git` file); it is listed once, under its main checkout.
pub fn survey(open: &Path) -> Survey {
    let mut s = Survey { folder: open.to_path_buf(), repos: Vec::new(), worktrees: Vec::new(), errors: Vec::new() };
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for found in find_repos(open, DISCOVERY_BUDGET).repos {
        let found = canonical(&found);
        if seen.contains(&found) {
            continue;
        }
        match list_side(&found) {
            Ok(list) => {
                let main = list.first().map_or_else(|| found.clone(), |w| w.repo.clone());
                if !s.repos.contains(&main) {
                    s.repos.push(main);
                }
                for wt in list {
                    if seen.insert(wt.path.clone()) {
                        let newest = if wt.has_files() { newest_change(&wt.path, AGE_BUDGET) } else { None };
                        s.worktrees.push((wt, newest));
                    }
                }
            }
            Err(reason) => {
                if !s.repos.contains(&found) {
                    s.repos.push(found.clone());
                    s.errors.push((found, reason));
                }
            }
        }
    }
    s
}

/// The time since `newest`, zero for a change stamped after `now`.
fn age_of(newest: &Newest, now: SystemTime) -> Duration {
    now.duration_since(newest.at).unwrap_or(Duration::ZERO)
}

/// The worktrees `stale` keeps (unchanged for at least that long, or with no age, since unknown
/// is not fresh), newest first, the ones with no age last, ties by path.
fn ordered(s: &Survey, now: SystemTime, stale: Option<Duration>) -> Vec<&(Worktree, Option<Newest>)> {
    let mut list: Vec<_> = s.worktrees.iter().filter(|(_, n)| match (stale, n) {
        (Some(min), Some(n)) => age_of(n, now) >= min,
        _ => true,
    }).collect();
    list.sort_by(|(a, an), (b, bn)| bn.map(|n| n.at).cmp(&an.map(|n| n.at)).then_with(|| a.path.cmp(&b.path)));
    list
}

/// One line per worktree: `{age:>6}  {name}{branch}  {place}`, the age blank when unknown and
/// followed by `+` after a capped walk, the branch in brackets only when it differs from the
/// folder name, the place as the sidebar shows it, and `  missing` after a missing worktree.
pub fn text_lines(s: &Survey, open: &Path, now: SystemTime, stale: Option<Duration>) -> Vec<String> {
    let home = std::env::var_os("HOME").map(|h| canonical(Path::new(&h))).unwrap_or_default();
    ordered(s, now, stale)
        .into_iter()
        .map(|(wt, newest)| {
            let age = newest.map_or_else(String::new, |n| format!("{}{}", age_text(age_of(&n, now)), if n.capped { "+" } else { "" }));
            let (name, branch) = wt_row_label(wt);
            let branch = branch.map_or_else(String::new, |b| format!(" ({b})"));
            let missing = if wt.missing { "  missing" } else { "" };
            format!("{age:>6}  {name}{branch}  {}{missing}", wt_row_place(&wt.path, open, &home))
        })
        .collect()
}

/// `{ "folder", "worktrees": [...], "repositories": [{ "path", "error"? }] }`, times in whole
/// seconds: `changed` since the Unix epoch, `age_seconds` before `now`.
pub fn json(s: &Survey, now: SystemTime, stale: Option<Duration>) -> Json {
    let text = |p: &Path| p.to_string_lossy().into_owned();
    let worktrees: Vec<Json> = ordered(s, now, stale)
        .into_iter()
        .map(|(wt, newest)| {
            let (name, _) = wt_row_label(wt);
            json!({
                "path": text(&wt.path),
                "name": name,
                "branch": wt.branch,
                "repository": text(&wt.repo),
                "changed": newest.map(|n| n.at.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO).as_secs()),
                "age_seconds": newest.map(|n| age_of(&n, now).as_secs()),
                "missing": wt.missing,
                "locked": wt.locked,
                "capped": newest.is_some_and(|n| n.capped),
            })
        })
        .collect();
    let repositories: Vec<Json> = s
        .repos
        .iter()
        .map(|r| match s.errors.iter().find(|(e, _)| e == r) {
            Some((_, reason)) => json!({ "path": text(r), "error": reason }),
            None => json!({ "path": text(r) }),
        })
        .collect();
    json!({ "folder": text(&s.folder), "worktrees": worktrees, "repositories": repositories })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktrees::create;
    use std::process::Stdio;

    const HOUR: u64 = 3_600;
    const DAY: u64 = 86_400;

    fn git_in(dir: &Path, args: &[&str]) {
        let st = crate::explorer::fixture_git(dir).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap();
        assert!(st.success(), "git {args:?} in {}", dir.display());
    }

    /// A repository at `root` with one committed file, `a.txt`.
    fn repo_at(root: &Path) {
        std::fs::create_dir_all(root).unwrap();
        git_in(root, &["init", "-q", "-b", "main"]);
        git_in(root, &["config", "user.email", "t@example.com"]);
        git_in(root, &["config", "user.name", "t"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git_in(root, &["add", "a.txt"]);
        git_in(root, &["commit", "-q", "-m", "init"]);
    }

    /// Every file under `dir`, `.git` left alone, last changed `ago` before `now`.
    fn touch_all(dir: &Path, now: SystemTime, ago: Duration) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if entry.file_name() == ".git" {
                continue;
            }
            if path.is_dir() {
                touch_all(&path, now, ago);
            } else {
                std::fs::File::options().write(true).open(&path).unwrap().set_modified(now - ago).unwrap();
            }
        }
    }

    /// `open/a` (main) with `open/a/.worktrees/w1` touched 2 days ago and `open/w2` beside it
    /// touched an hour ago. Returns the temp dir, `open` and `a`, canonical.
    fn two_worktrees(now: SystemTime) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let open = dir.path().canonicalize().unwrap().join("open");
        let a = open.join("a");
        repo_at(&a);
        create(&a, "w1", &a.join(".worktrees/w1")).unwrap();
        create(&a, "w2", &open.join("w2")).unwrap();
        touch_all(&a.join(".worktrees/w1"), now, Duration::from_secs(2 * DAY));
        touch_all(&open.join("w2"), now, Duration::from_secs(HOUR));
        (dir, open, a)
    }

    fn run(args: &[&str], cwd: &Path, now: SystemTime) -> (i32, String, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = worktrees_cli_to(&args, cwd, now, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    fn lines(text: &str) -> Vec<&str> {
        text.lines().collect()
    }

    #[test]
    fn the_command_lists_side_worktrees_newest_first_with_ages_and_places() {
        let _lock = crate::client::fork_lock();
        let now = SystemTime::now();
        let (_dir, open, _a) = two_worktrees(now);
        let (code, out, err) = run(&[open.to_str().unwrap()], &open, now);
        assert_eq!(code, 0, "{err}");
        assert_eq!(lines(&out), ["   1 h  w2  w2", "   2 d  w1  a/.worktrees/w1"], "w2 is a repository to the walk too, yet listed once");
        assert_eq!(err, "");
    }

    #[test]
    fn stale_keeps_only_worktrees_unchanged_for_at_least_that_long() {
        let _lock = crate::client::fork_lock();
        let now = SystemTime::now();
        let (_dir, open, _a) = two_worktrees(now);
        let o = open.to_str().unwrap();
        let (code, out, _) = run(&[o, "--stale", "1d"], &open, now);
        assert_eq!(code, 0);
        assert_eq!(lines(&out), ["   2 d  w1  a/.worktrees/w1"]);
        let (_, out, _) = run(&[o, "--stale", "3d"], &open, now);
        assert_eq!(out, "", "none that old: an empty list");
        let (_, out, _) = run(&[o, "--stale", "0m"], &open, now);
        assert_eq!(lines(&out).len(), 2, "zero keeps everything");
        // No age is not fresh: a worktree with no files left, and a missing one, are kept, blank
        // in the age column and last.
        let a = open.join("a");
        create(&a, "w3", &open.join("w3")).unwrap();
        std::fs::remove_file(open.join("w3/a.txt")).unwrap();
        create(&a, "w4", &open.join("w4")).unwrap();
        std::fs::remove_dir_all(open.join("w4")).unwrap();
        let (_, out, _) = run(&[o, "--stale", "1d"], &open, now);
        assert_eq!(lines(&out), ["   2 d  w1  a/.worktrees/w1", "        w3  w3", "        w4  w4  missing"]);
        let (_, out, _) = run(&[o, "--stale", "1d", "--json"], &open, now);
        let v: Json = serde_json::from_str(&out).unwrap();
        let w3 = v["worktrees"].as_array().unwrap().iter().find(|w| w["name"] == "w3").unwrap().clone();
        assert_eq!((w3["changed"].clone(), w3["age_seconds"].clone()), (Json::Null, Json::Null));
    }

    #[test]
    fn a_capped_walk_reads_as_at_least_that_old_and_a_branch_or_detached_head_shows_when_it_differs() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let open = Path::new("/p");
        let wt = |path: &str, branch: Option<&str>| Worktree { path: path.into(), repo: "/p/a".into(), head: "abcd1234".into(), branch: branch.map(Into::into), missing: false, bare: false, locked: false, admin: None };
        let at = |ago: u64, capped: bool| Some(Newest { at: now - Duration::from_secs(ago), capped });
        let s = Survey {
            folder: open.into(),
            repos: vec!["/p/a".into()],
            worktrees: vec![(wt("/p/release", Some("spinner")), at(3 * DAY, true)), (wt("/p/x", None), at(90, false)), (wt("/p/b", Some("b")), at(90, false))],
            errors: vec![],
        };
        assert_eq!(text_lines(&s, open, now, None), [" 1 min  b  b", " 1 min  x (detached)  x", "  3 d+  release (spinner)  release"], "ties by path");
    }

    #[test]
    fn json_carries_every_field_and_a_refused_repository_is_named_not_skipped() {
        let _lock = crate::client::fork_lock();
        let now = SystemTime::now();
        let (_dir, open, a) = two_worktrees(now);
        let w1 = a.join(".worktrees/w1");
        git_in(&a, &["worktree", "lock", w1.to_str().unwrap()]);
        git_in(&open.join("w2"), &["checkout", "-q", "--detach"]);
        // A filter driver name `git_read` will not override: nothing runs there.
        let b = open.join("b");
        repo_at(&b);
        create(&b, "hidden", &b.join(".worktrees/hidden")).unwrap();
        let mut config = std::fs::OpenOptions::new().append(true).open(b.join(".git/config")).unwrap();
        writeln!(config, "[filter \"a b\"]\n\tclean = cat").unwrap();
        let (code, out, err) = run(&[open.to_str().unwrap(), "--json"], &open, now);
        assert_eq!(code, 0, "a partial list is still an answer");
        assert_eq!(err, "", "with --json everything is on stdout");
        let v: Json = serde_json::from_str(&out).unwrap();
        let secs = |d: u64| now.duration_since(UNIX_EPOCH).unwrap().as_secs() - d;
        let p = |p: &Path| p.to_string_lossy().into_owned();
        assert_eq!(
            v,
            json!({
                "folder": p(&open),
                "worktrees": [
                    { "path": p(&open.join("w2")), "name": "w2", "branch": null, "repository": p(&a), "changed": secs(HOUR), "age_seconds": HOUR, "missing": false, "locked": false, "capped": false },
                    { "path": p(&w1), "name": "w1", "branch": "w1", "repository": p(&a), "changed": secs(2 * DAY), "age_seconds": 2 * DAY, "missing": false, "locked": true, "capped": false },
                ],
                "repositories": [
                    { "path": p(&a) },
                    { "path": p(&b), "error": crate::explorer::STATUS_OFF },
                ],
            })
        );
        let (code, out, err) = run(&[open.to_str().unwrap()], &open, now);
        assert_eq!(code, 0);
        assert_eq!(lines(&out).len(), 2);
        assert_eq!(err, format!("cannot list {}: {}\n", b.display(), crate::explorer::STATUS_OFF));
    }

    #[test]
    fn a_bad_age_or_a_missing_folder_is_refused_with_the_right_exit_code() {
        let cwd = Path::new("/");
        let now = SystemTime::now();
        for bad in [&["--stale", "soon"][..], &["--stale"], &["--stale", "7"], &["a", "b"], &["--all"]] {
            let (code, out, err) = run(bad, cwd, now);
            assert_eq!(code, 2, "{bad:?}");
            assert_eq!(out, "", "{bad:?}");
            assert!(err.contains("usage: arkos worktrees") && err.ends_with("see: arkos guide\n"), "{bad:?}: {err}");
            let with_json: Vec<&str> = bad.iter().copied().chain(["--json"]).collect();
            let (code, out, _) = run(&with_json, cwd, now);
            assert_eq!(code, 2, "{with_json:?}");
            let v: Json = serde_json::from_str(&out).unwrap();
            assert_eq!(v["exit"], 2, "{with_json:?}");
            assert!(v["error"].is_string(), "{with_json:?}");
        }
        let (_, out, _) = run(&["--stale", "soon", "--json"], cwd, now);
        assert_eq!(serde_json::from_str::<Json>(&out).unwrap()["error"], "not an age: \"soon\"");
        let (code, out, err) = run(&["/nonexistent"], cwd, now);
        assert_eq!((code, out.as_str(), err.as_str()), (1, "", "no such folder: /nonexistent\n"));
        let (code, out, err) = run(&["/nonexistent", "--json"], cwd, now);
        assert_eq!((code, err.as_str()), (1, ""));
        assert_eq!(serde_json::from_str::<Json>(&out).unwrap(), json!({ "error": "no such folder: /nonexistent", "exit": 1 }));
    }

    #[test]
    fn the_default_folder_is_the_current_one_and_a_subfolder_resolves_to_its_repository() {
        let _lock = crate::client::fork_lock();
        let now = SystemTime::now();
        let (_dir, open, a) = two_worktrees(now);
        let src = a.join("src");
        std::fs::create_dir(&src).unwrap();
        let (code, out, err) = run(&["--json"], &src, now);
        assert_eq!(code, 0, "{err}");
        let v: Json = serde_json::from_str(&out).unwrap();
        assert_eq!(v["folder"], src.to_string_lossy().as_ref());
        let names: Vec<&str> = v["worktrees"].as_array().unwrap().iter().map(|w| w["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["w2", "w1"], "a's side worktrees, found upward from a/src");
        assert_eq!(v["repositories"], json!([{ "path": a.to_string_lossy() }]));
        // A relative folder is read against the current one.
        let (code, out, _) = run(&["a/src"], &open, now);
        assert_eq!(code, 0);
        assert_eq!(lines(&out).len(), 2);
    }
}
