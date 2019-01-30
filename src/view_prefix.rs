use super::View;
use super::replace_subtree;
use git2::*;
use std::path::Path;
use std::path::PathBuf;

pub struct PrefixView
{
    prefix: PathBuf,
}

impl PrefixView
{
    pub fn new(prefix: &Path) -> PrefixView
    {
        PrefixView {
            prefix: prefix.to_path_buf(),
        }
    }
}

impl View for PrefixView
{
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid>
    {
        let empty = repo.find_tree(repo.treebuilder(None).unwrap().write().unwrap())
            .unwrap();
        Some(replace_subtree(&repo, &self.prefix, &tree, &empty))
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Option<Oid>
    {
        tree.get_path(&self.prefix).map(|x| x.id()).ok()
    }
}
