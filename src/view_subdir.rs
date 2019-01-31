use super::replace_subtree;
use super::View;
use git2::*;
use std::path::Path;
use std::path::PathBuf;

pub struct SubdirView {
    subdir: PathBuf,
}

impl SubdirView {
    pub fn new(subdir: &Path) -> SubdirView {
        SubdirView {
            subdir: subdir.to_path_buf(),
        }
    }
}

impl View for SubdirView {
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid> {
        tree.get_path(&self.subdir).map(|x| x.id()).ok()
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Option<Oid> {
        Some(replace_subtree(&repo, &self.subdir, &tree, &parent_tree))
    }
}
