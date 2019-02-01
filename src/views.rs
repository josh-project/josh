use super::replace_subtree;
use git2::*;
use std::path::Path;
use std::path::PathBuf;

pub trait View {
    fn apply(&self, repo: &git2::Repository, tree: &git2::Tree) -> Option<git2::Oid>;
    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> Option<git2::Oid>;
}

struct SubdirView {
    subdir: PathBuf,
}

impl View for SubdirView {
    fn apply(&self, _repo: &Repository, tree: &Tree) -> Option<Oid> {
        tree.get_path(&self.subdir).map(|x| x.id()).ok()
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Option<Oid> {
        Some(replace_subtree(&repo, &self.subdir, &tree, &parent_tree))
    }
}

struct PrefixView {
    prefix: PathBuf,
}

impl View for PrefixView {
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid> {
        let empty = repo
            .find_tree(repo.treebuilder(None).unwrap().write().unwrap())
            .unwrap();
        Some(replace_subtree(&repo, &self.prefix, &tree, &empty))
    }

    fn unapply(&self, _repo: &Repository, tree: &Tree, _parent_tree: &Tree) -> Option<Oid> {
        tree.get_path(&self.prefix).map(|x| x.id()).ok()
    }
}

struct ChainView {
    first: Box<dyn View>,
    second: Box<dyn View>,
}

impl View for ChainView {
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid> {
        if let Some(r) = self.first.apply(&repo, &tree) {
            if let Ok(t) = repo.find_tree(r) {
                return self.second.apply(&repo, &t);
            }
        }
        return None;
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Option<Oid> {
        let p = self.first.apply(&repo, &parent_tree).expect("no tree");
        let p = repo.find_tree(p).expect("no tree");
        let a = self.second.unapply(&repo, &tree, &p).expect("no tree");
        self.first
            .unapply(&repo, &repo.find_tree(a).expect("no tree"), &parent_tree)
    }
}

fn create_view_node(name: &str) -> Box<dyn View> {
    if name.starts_with("+/") {
        return Box::new(PrefixView {
            prefix: Path::new(&name[2..]).to_owned(),
        });
    } else if name.starts_with("/") {
        return Box::new(SubdirView {
            subdir: Path::new(&name[1..]).to_owned(),
        });
    }
    return Box::new(NopView);
}

struct NopView;

impl View for NopView {
    fn apply(&self, _repo: &Repository, tree: &Tree) -> Option<Oid> {
        Some(tree.id())
    }

    fn unapply(&self, _repo: &Repository, tree: &Tree, _parent_tree: &Tree) -> Option<Oid> {
        Some(tree.id())
    }
}

pub fn build_view(viewstr: &str) -> Box<dyn View> {
    let mut chain: Box<dyn View> = Box::new(NopView);
    for v in viewstr.split("!") {
        let new = create_view_node(&v);
        chain = Box::new(ChainView {
            first: chain,
            second: new,
        });
    }
    return chain;
}
