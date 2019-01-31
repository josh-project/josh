extern crate git2;
extern crate tempdir;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;

pub mod base_repo;
pub mod cgi;
mod filelock;
pub mod run_proxy;
pub mod scratch;
mod shell;
mod treeops;
mod view_chain;
mod view_prefix;
mod view_subdir;
pub mod virtual_repo;

pub use filelock::FileLock;
pub use run_proxy::*;
pub use scratch::*;
pub use shell::thread_local_temp_dir;
pub use shell::Shell;
pub use treeops::*;
pub use view_subdir::SubdirView;

use git2::*;
use std::path::Path;
use view_chain::ChainView;
use view_prefix::PrefixView;

#[derive(Clone)]
pub enum UnapplyView {
    Done(git2::Oid),
    RejectNoFF,
    RejectMerge,
    NoChanges,
}

pub trait View {
    fn apply(&self, repo: &git2::Repository, tree: &git2::Tree) -> Option<git2::Oid>;
    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> Option<git2::Oid>;
}

fn create_view_node(name: &str) -> Box<dyn View> {
    if name.starts_with("+/") {
        return Box::new(PrefixView::new(&Path::new(&name[2..])));
    } else if name.starts_with("/") {
        return Box::new(SubdirView::new(&Path::new(&name[1..])));
    }
    return Box::new(NopView);
}

struct NopView;

impl View for NopView {
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid> {
        Some(tree.id())
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Option<Oid> {
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
