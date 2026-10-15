//! Diff gutters for the viewer (spec 6): the file against its worktree's HEAD, as a line diff
//! through `similar`, kept as one mark per current line plus the deleted lines and where they
//! were. The base comes from `git cat-file blob HEAD:<path>` run from the file's own worktree.

use crate::explorer::git_read;
use crate::viewer::{split_lines, MAX_PLAIN_BYTES, SNIFF};
use similar::{capture_diff_slices_deadline, Algorithm, DiffTag};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// The longest one diff runs before `similar` settles for a coarser answer.
const DIFF_DEADLINE: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Added,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffMarks {
    /// One per current line.
    pub marks: Vec<Option<Mark>>,
    /// (index of the current line the deletion sits before, the deleted lines). An index equal
    /// to the line count means lines were deleted at the end.
    pub deleted_before: Vec<(usize, Vec<String>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayRow {
    Line(usize),
    /// (which deletion, which line of it)
    Deleted(usize, usize),
}

impl DiffMarks {
    pub fn has_changes(&self) -> bool {
        self.marks.iter().any(Option::is_some) || !self.deleted_before.is_empty()
    }
}

/// Line diff of `base` against the current `lines`. The base is split as the viewer splits the
/// file (`split_lines`), so line endings, tabs and a final newline alone are not changes. A
/// replaced run marks its new lines modified (the old ones are not kept: the gutter says
/// "changed here"), and old lines beyond the new ones are a deletion after them; a pure
/// insertion marks added; a pure deletion is recorded before the next current line.
pub fn compute(base: &str, lines: &[String]) -> DiffMarks {
    let old = split_lines(base);
    let ops = capture_diff_slices_deadline(Algorithm::Myers, &old, lines, Some(Instant::now() + DIFF_DEADLINE));
    let mut marks = vec![None; lines.len()];
    let mut deleted_before = Vec::new();
    for op in ops {
        let (old_range, new_range) = (op.old_range(), op.new_range());
        match op.tag() {
            DiffTag::Equal => {}
            DiffTag::Insert => marks[new_range].fill(Some(Mark::Added)),
            DiffTag::Delete => deleted_before.push((new_range.start, old[old_range].to_vec())),
            DiffTag::Replace => {
                marks[new_range.clone()].fill(Some(Mark::Modified));
                if old_range.len() > new_range.len() {
                    deleted_before.push((new_range.end, old[old_range.start + new_range.len()..old_range.end].to_vec()));
                }
            }
        }
    }
    DiffMarks { marks, deleted_before }
}

/// The worktree root of `dir`, through git, so a file in another worktree diffs against that
/// worktree's HEAD.
fn worktree_root(dir: &Path) -> Option<PathBuf> {
    let out = git_read(dir)?.args(["rev-parse", "--show-toplevel"]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n')))
}

/// `HEAD:<relative path>` of the file's worktree, as committed (`cat-file blob`: no textconv,
/// no filter). `None` outside a repository, for a file HEAD does not have (untracked, or new),
/// and for a base that is binary or over `MAX_PLAIN_BYTES`. Never reads the working file.
/// Background executor only.
pub fn base_for(path: &Path) -> Option<String> {
    let root = worktree_root(path.parent()?)?;
    let rel = path.canonicalize().ok()?.strip_prefix(root.canonicalize().ok()?).ok()?.to_path_buf();
    let mut spec = std::ffi::OsString::from("HEAD:");
    spec.push(rel.as_os_str());
    let mut child = git_read(&root)?.args(["cat-file", "blob"]).arg(&spec).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut bytes = Vec::new();
    let read = child.stdout.take()?.take(MAX_PLAIN_BYTES + 1).read_to_end(&mut bytes);
    if read.is_err() || bytes.len() as u64 > MAX_PLAIN_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    if !child.wait().ok()?.success() || bytes[..bytes.len().min(SNIFF)].contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// How many rows the element paints: the lines, plus the deleted ones when `inline` is on.
pub fn row_count(line_count: usize, d: Option<&DiffMarks>, inline: bool) -> usize {
    match d {
        Some(d) if inline => line_count + d.deleted_before.iter().map(|(_, l)| l.len()).sum::<usize>(),
        _ => line_count,
    }
}

/// The display row of `line` with the inline diff on: the line plus every deleted line before it.
pub fn row_of_line(d: &DiffMarks, line: usize) -> usize {
    line + d.deleted_before.iter().filter(|(before, _)| *before <= line).map(|(_, l)| l.len()).sum::<usize>()
}

/// The line a display row shows or, for a deleted row, the line it sits before (the last line
/// for a deletion at the end); the last line past the end.
pub fn line_at_row(d: &DiffMarks, rows: &[DisplayRow], row: usize) -> usize {
    let last = d.marks.len().saturating_sub(1);
    match rows.get(row) {
        Some(DisplayRow::Line(i)) => *i,
        Some(DisplayRow::Deleted(k, _)) => d.deleted_before.get(*k).map(|(before, _)| (*before).min(last)).unwrap_or(last),
        None => last,
    }
}

/// The rows the element paints: every current line, with the deleted lines interleaved where
/// they were when `inline` is on.
pub fn display_rows(line_count: usize, d: &DiffMarks, inline: bool) -> Vec<DisplayRow> {
    let mut out = Vec::with_capacity(row_count(line_count, Some(d), inline));
    let mut del = d.deleted_before.iter().enumerate().peekable();
    for i in 0..=line_count {
        while let Some((k, (_, lines))) = del.next_if(|(_, (before, _))| inline && *before == i) {
            out.extend((0..lines.len()).map(|j| DisplayRow::Deleted(k, j)));
        }
        if i < line_count {
            out.push(DisplayRow::Line(i));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explorer::fixture_git;
    use std::path::PathBuf;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn added_modified_and_deleted_lines_are_marked() {
        let base = "a\nb\nc\nd\n";
        let now = lines("a\nB\nc\nd\ne\n");
        let d = compute(base, &now);
        assert_eq!(d.marks, vec![None, Some(Mark::Modified), None, None, Some(Mark::Added)]);
        assert!(d.deleted_before.is_empty());
        let now = lines("a\nd\n");
        let d = compute(base, &now);
        assert_eq!(d.marks, vec![None, None]);
        assert_eq!(d.deleted_before, vec![(1, vec!["b".to_string(), "c".to_string()])], "b and c were deleted before d");
    }

    #[test]
    fn marks_for_a_deletion_at_the_top_and_a_clean_file() {
        let base = "x\na\nb\n";
        let now = lines("a\nb\n");
        let d = compute(base, &now);
        assert_eq!(d.deleted_before, vec![(0, vec!["x".to_string()])], "a deletion before line 1");
        assert_eq!(d.marks, vec![None, None]);
        let clean = compute("a\nb\n", &lines("a\nb\n"));
        assert_eq!(clean, DiffMarks { marks: vec![None, None], deleted_before: vec![] });
        assert!(!clean.has_changes());
        let end = compute("a\nb\nc\n", &lines("a\nb\n"));
        assert_eq!(end.deleted_before, vec![(2, vec!["c".to_string()])], "deleted at the end: before index == line count");
    }

    #[test]
    fn line_endings_tabs_and_a_missing_final_newline_are_not_changes() {
        // The viewer's lines come through `split_lines`: the base is read the same way.
        assert!(!compute("a\nb\n", &lines("a\nb")).has_changes(), "a lost final newline alone marks nothing");
        assert!(!compute("a\nb", &lines("a\nb\n")).has_changes(), "nor a gained one");
        assert!(!compute("a\r\nb\r\n", &crate::viewer::split_lines("a\r\nb\r\n")).has_changes(), "a CRLF file against a CRLF base");
        assert!(!compute("a\nb\n", &crate::viewer::split_lines("a\r\nb\r\n")).has_changes(), "a CRLF checkout of an LF base");
        assert!(!compute("\tx\n", &crate::viewer::split_lines("\tx\n")).has_changes(), "a tab is four spaces on both sides");
    }

    #[test]
    fn an_emptied_file_is_all_deleted_before_line_one_and_a_new_one_all_added() {
        let d = compute("a\nb\n", &[]);
        assert_eq!(d, DiffMarks { marks: vec![], deleted_before: vec![(0, vec!["a".into(), "b".into()])] });
        assert!(d.has_changes());
        assert_eq!(compute("", &lines("a\n")).marks, vec![Some(Mark::Added)]);
    }

    #[test]
    fn a_replacement_longer_than_the_new_lines_leaves_the_rest_deleted_after_them() {
        let d = compute("a\nb\nc\nd\n", &lines("a\nX\nd\n"));
        assert_eq!(d.marks, vec![None, Some(Mark::Modified), None]);
        assert_eq!(d.deleted_before, vec![(2, vec!["c".to_string()])]);
    }

    #[test]
    fn display_rows_interleave_deleted_lines_only_when_inline() {
        let d = DiffMarks { marks: vec![None, None], deleted_before: vec![(0, vec!["x".into()]), (2, vec!["y".into(), "z".into()])] };
        assert_eq!(display_rows(2, &d, false), vec![DisplayRow::Line(0), DisplayRow::Line(1)]);
        assert_eq!(display_rows(2, &d, true), vec![DisplayRow::Deleted(0, 0), DisplayRow::Line(0), DisplayRow::Line(1), DisplayRow::Deleted(1, 0), DisplayRow::Deleted(1, 1)]);
        assert_eq!(display_rows(2, &DiffMarks::default(), true), vec![DisplayRow::Line(0), DisplayRow::Line(1)], "no diff: the plain rows");
    }

    #[test]
    fn rows_and_lines_convert_both_ways_for_the_scroll() {
        let d = DiffMarks { marks: vec![None; 3], deleted_before: vec![(0, vec!["x".into()]), (2, vec!["y".into(), "z".into()]), (3, vec!["w".into()])] };
        let rows = display_rows(3, &d, true);
        assert_eq!(row_count(3, Some(&d), true), rows.len());
        assert_eq!(row_count(3, Some(&d), false), 3);
        assert_eq!(row_count(3, None, true), 3);
        for line in 0..3 {
            assert_eq!(rows[row_of_line(&d, line)], DisplayRow::Line(line), "line {line}");
        }
        assert_eq!(line_at_row(&d, &rows, 0), 0, "a deleted row maps to the line it sits before");
        assert_eq!(line_at_row(&d, &rows, 4), 2, "y, before line 2");
        assert_eq!(line_at_row(&d, &rows, 6), 2, "deleted at the end: the last line");
        assert_eq!(line_at_row(&d, &rows, 99), 2, "past the end: the last line");
    }

    /// A temp dir holding the repository one level down, so a worktree made beside it
    /// (`root.parent()`) lands inside the temp dir.
    fn repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let run = |args: &[&str]| {
            let st = fixture_git(&root).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert!(st.success(), "git {args:?}");
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@example.com"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(root.join("src/a.rs"), "one\ntwo\n").unwrap();
        std::fs::write(root.join("blob.bin"), b"\x89PNG\0\0").unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);
        (dir, root)
    }

    #[test]
    fn the_base_is_head_for_a_tracked_file_and_none_otherwise() {
        let _lock = crate::client::fork_lock();
        let (_dir, root) = repo();
        std::fs::write(root.join("src/a.rs"), "one\nTWO\n").unwrap();
        assert_eq!(base_for(&root.join("src/a.rs")).as_deref(), Some("one\ntwo\n"));
        std::fs::write(root.join("src/new.rs"), "").unwrap();
        assert_eq!(base_for(&root.join("src/new.rs")), None, "untracked");
        std::fs::write(root.join("blob.bin"), "now text\n").unwrap();
        assert_eq!(base_for(&root.join("blob.bin")), None, "a binary base is not diffed");
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("f.txt"), "").unwrap();
        assert_eq!(base_for(&outside.path().join("f.txt")), None, "not a repository");
    }

    #[test]
    fn the_base_comes_from_the_files_own_worktree() {
        let _lock = crate::client::fork_lock();
        let (_dir, root) = repo();
        let root = root.canonicalize().unwrap();
        let wt = root.parent().unwrap().join(format!("{}-wt", root.file_name().unwrap().to_string_lossy()));
        crate::worktrees::create(&root, "wt", &wt).unwrap();
        // Commit a different a.rs in the worktree, so its HEAD differs from main's.
        std::fs::write(wt.join("src/a.rs"), "wt\n").unwrap();
        let run = |args: &[&str]| assert!(fixture_git(&wt).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap().success());
        run(&["commit", "-q", "-am", "wt"]);
        std::fs::write(wt.join("src/a.rs"), "wt\nmore\n").unwrap();
        assert_eq!(base_for(&wt.join("src/a.rs")).as_deref(), Some("wt\n"), "the worktree's HEAD, not main's");
        assert_eq!(base_for(&root.join("src/a.rs")).as_deref(), Some("one\ntwo\n"));
        std::fs::write(wt.join("src/only_here.rs"), "x\n").unwrap();
        assert_eq!(base_for(&wt.join("src/only_here.rs")), None, "a file the worktree's HEAD lacks has no base");
        crate::worktrees::remove(&root, &wt, true).unwrap();
    }
}
