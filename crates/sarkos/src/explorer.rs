//! The Explorer section of the sidebar (spec 6): the project's file tree.

use std::path::PathBuf;

/// One visible line of the tree. The tree model that fills this module in builds them.
#[allow(dead_code)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    pub ignored: bool,
    pub loading: bool,
}
