//! Links and images in a markdown document (images and links spec): heading anchors, the lookup
//! behind a wikilink, and where a pasted or dropped image goes and how its path is written. Pure
//! but for the file system reads the lookup and the free name need.

use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};

/// How many entries the workspace lookup looks at before it gives up.
const LOOKUP_LIMIT: usize = 20_000;

/// A heading's anchor, as GitHub makes it: lowercased, every character that is not a letter, a
/// digit, a space, `-` or `_` dropped, and the spaces turned into `-`.
pub fn anchor(text: &str) -> String {
    text.trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// Every heading's anchor in document order, a repeated one numbered `-1`, `-2` after the first.
pub fn anchors<'a>(headings: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    headings
        .into_iter()
        .map(|h| {
            let base = anchor(h);
            let n = seen.entry(base.clone()).or_insert(0);
            let out = if *n == 0 { base } else { format!("{base}-{n}") };
            *n += 1;
            out
        })
        .collect()
}

/// The heading an anchor names, by index into `headings`: an exact match, else one ignoring case
/// and percent escapes (`#My%20Notes` as typed by hand).
pub fn heading_for(headings: &[String], target: &str) -> Option<usize> {
    let all = anchors(headings.iter().map(String::as_str));
    all.iter().position(|a| a == target).or_else(|| {
        let loose = anchor(&target.replace("%20", " "));
        all.iter().position(|a| *a == loose)
    })
}

/// The folder a wikilink is looked up under: the nearest folder holding `.git` at or above
/// `base_dir`, else `base_dir`.
pub fn workspace_root(base_dir: &Path) -> PathBuf {
    base_dir.ancestors().find(|d| d.join(".git").exists()).unwrap_or(base_dir).to_path_buf()
}

/// The file a link's path names: the path beside the document, or with `.md` added, or else the
/// first file under the workspace (breadth first, so the shallowest) whose name is `name` or
/// `name.md`, ignoring case; a name with a `/` must match the end of the file's path. Hidden
/// folders, `target` and `node_modules` are skipped, and the walk stops after `LOOKUP_LIMIT`
/// entries.
pub fn resolve(base_dir: &Path, name: &str) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let beside = base_dir.join(name);
    if beside.exists() {
        return Some(beside);
    }
    let with_md = base_dir.join(format!("{name}.md"));
    if with_md.is_file() {
        return Some(with_md);
    }
    if Path::new(name).is_absolute() {
        return None;
    }
    find_under(&workspace_root(base_dir), name)
}

fn find_under(root: &Path, name: &str) -> Option<PathBuf> {
    let wanted = name.to_lowercase();
    let wanted_md = format!("{wanted}.md");
    let matches = |p: &Path| {
        let s = p.to_string_lossy().to_lowercase();
        [&wanted, &wanted_md].iter().any(|w| s.ends_with(&format!("/{w}")))
    };
    let mut queue = VecDeque::from([root.to_path_buf()]);
    let mut looked = 0;
    while let Some(dir) = queue.pop_front() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = read.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            looked += 1;
            if looked > LOOKUP_LIMIT {
                return None;
            }
            let path = e.path();
            let file_name = e.file_name().to_string_lossy().into_owned();
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                if !file_name.starts_with('.') && file_name != "target" && file_name != "node_modules" {
                    queue.push_back(path);
                }
            } else if matches(&path) {
                return Some(path);
            }
        }
    }
    None
}

/// The folder pasted and dropped images go in for document `doc`: `pattern` (the config's
/// `viewer.image_destination`) relative to the document's folder, `{name}` the document's file name
/// without its extension; empty is the document's folder.
pub fn image_dir(doc: &Path, pattern: &str) -> PathBuf {
    let dir = doc.parent().unwrap_or(Path::new("/"));
    let stem = doc.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let pattern = pattern.trim().replace("{name}", &stem);
    if pattern.is_empty() {
        dir.to_path_buf()
    } else {
        normalize(&dir.join(pattern))
    }
}

/// The first of `image.<ext>`, `image-1.<ext>`, `image-2.<ext>` in `dir` that `taken` says is free.
pub fn image_name(dir: &Path, ext: &str, taken: impl Fn(&Path) -> bool) -> PathBuf {
    (0..)
        .map(|n| dir.join(if n == 0 { format!("image.{ext}") } else { format!("image-{n}.{ext}") }))
        .find(|p| !taken(p))
        .expect("an unbounded range")
}

/// `path` as a link from a document in `base_dir`: relative (with `..` as needed) when the two
/// share more than the root, else absolute; written in angle brackets when it holds a space or a
/// bracket, as CommonMark wants.
pub fn link_path(path: &Path, base_dir: &Path) -> String {
    let (p, b) = (normalize(path), normalize(base_dir));
    let pc: Vec<Component> = p.components().collect();
    let bc: Vec<Component> = b.components().collect();
    let common = pc.iter().zip(&bc).take_while(|(x, y)| x == y).count();
    let text = if common > 1 {
        let mut parts: Vec<String> = vec!["..".to_string(); bc.len() - common];
        parts.extend(pc[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
        parts.join("/")
    } else {
        p.to_string_lossy().into_owned()
    };
    if text.contains([' ', '(', ')', '<', '>']) {
        format!("<{text}>")
    } else {
        text
    }
}

/// Whether a file is an image Arkos draws, by its extension.
pub fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "tif", "tiff"].contains(&e.to_lowercase().as_str()))
}

/// A path with `.` and `..` resolved without touching the disk.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_are_github_s_with_repeats_numbered() {
        assert_eq!(anchor("Hello, World!"), "hello-world");
        assert_eq!(anchor("  A.B c_d-e  "), "ab-c_d-e");
        assert_eq!(anchor("Ünïcode Straße"), "ünïcode-straße");
        assert_eq!(anchors(["Intro", "Setup", "Intro", "Intro"]), vec!["intro", "setup", "intro-1", "intro-2"]);
        let heads = vec!["My Notes".to_string(), "Next".to_string()];
        assert_eq!(heading_for(&heads, "next"), Some(1));
        assert_eq!(heading_for(&heads, "My%20Notes"), Some(0), "an anchor typed by hand");
        assert_eq!(heading_for(&heads, "missing"), None);
    }

    #[test]
    fn a_link_resolves_beside_then_with_md_then_anywhere_under_the_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("docs/deep")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::write(root.join("docs/here.md"), "").unwrap();
        std::fs::write(root.join("docs/deep/Far Note.md"), "").unwrap();
        std::fs::write(root.join("node_modules/x/gone.md"), "").unwrap();
        std::fs::write(root.join(".hidden/secret.md"), "").unwrap();
        std::fs::write(root.join("pic.png"), "").unwrap();
        let base = root.join("docs");
        assert_eq!(workspace_root(&base.join("deep")), root);
        assert_eq!(resolve(&base, "here.md"), Some(base.join("here.md")));
        assert_eq!(resolve(&base, "here"), Some(base.join("here.md")), ".md added");
        assert_eq!(resolve(&base, "far note"), Some(root.join("docs/deep/Far Note.md")), "found under the workspace, ignoring case");
        assert_eq!(resolve(&base, "deep/Far Note"), Some(root.join("docs/deep/Far Note.md")));
        assert_eq!(resolve(&base, "pic.png"), Some(root.join("pic.png")));
        assert_eq!(resolve(&base, "gone"), None, "node_modules is skipped");
        assert_eq!(resolve(&base, "secret"), None, "hidden folders are skipped");
        assert_eq!(resolve(&base, ""), None);
    }

    #[test]
    fn images_go_in_the_pattern_s_folder_under_the_first_free_name() {
        let doc = Path::new("/w/notes/plan.md");
        assert_eq!(image_dir(doc, ""), Path::new("/w/notes"));
        assert_eq!(image_dir(doc, "images"), Path::new("/w/notes/images"));
        assert_eq!(image_dir(doc, "{name}.assets"), Path::new("/w/notes/plan.assets"));
        assert_eq!(image_dir(doc, "../assets"), Path::new("/w/assets"));
        let taken = |p: &Path| p.ends_with("image.png") || p.ends_with("image-1.png");
        assert_eq!(image_name(Path::new("/w"), "png", taken), Path::new("/w/image-2.png"));
        assert_eq!(image_name(Path::new("/w"), "jpg", taken), Path::new("/w/image.jpg"));
    }

    #[test]
    fn a_path_is_written_relative_to_the_document_and_bracketed_with_a_space() {
        let base = Path::new("/w/notes");
        assert_eq!(link_path(Path::new("/w/notes/image.png"), base), "image.png");
        assert_eq!(link_path(Path::new("/w/notes/images/a.png"), base), "images/a.png");
        assert_eq!(link_path(Path::new("/w/assets/a.png"), base), "../assets/a.png");
        assert_eq!(link_path(Path::new("/other/a.pdf"), base), "/other/a.pdf", "nothing shared but the root: absolute");
        assert_eq!(link_path(Path::new("/w/notes/my pic.png"), base), "<my pic.png>");
        assert!(is_image(Path::new("a.PNG")) && !is_image(Path::new("a.pdf")));
    }
}
