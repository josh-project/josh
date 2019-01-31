use super::View;
use super::replace_subtree;
use git2::*;
use std::path::Path;
use std::path::PathBuf;

pub struct ChainView
{
    pub first: Box<dyn View>,
    pub second: Box<dyn View>,
}


impl View for ChainView
{
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid>
    {
        let r = self.first.apply(&repo, &tree).expect("no tree");
        return self.second.apply(&repo, &repo.find_tree(r).expect("no tree"));
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Option<Oid>
    {
        let p = self.first.apply(&repo, &parent_tree).expect("no tree");
        let p = repo.find_tree(p).expect("no tree");
        let a = self.second.unapply(&repo, &tree, &p).expect("no tree");
        self.first.unapply(&repo, &repo.find_tree(a).expect("no tree"), &parent_tree)
    }
}
