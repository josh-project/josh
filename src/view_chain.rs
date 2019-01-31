use super::replace_subtree;
use super::View;
use git2::*;
use std::path::Path;
use std::path::PathBuf;

pub struct ChainView {
    pub first: Box<dyn View>,
    pub second: Box<dyn View>,
}

impl View for ChainView {
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid> {
        if let Some(r) = self.first.apply(&repo, &tree) {
            if let Ok(t) = repo.find_tree(r) {
                return self.second.apply(&repo, &t);
            }
        }
        return None
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Option<Oid> {
        let p = self.first.apply(&repo, &parent_tree).expect("no tree");
        let p = repo.find_tree(p).expect("no tree");
        let a = self.second.unapply(&repo, &tree, &p).expect("no tree");
        self.first
            .unapply(&repo, &repo.find_tree(a).expect("no tree"), &parent_tree)
    }
}
