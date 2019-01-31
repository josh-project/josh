extern crate git2;
extern crate tempdir;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;

mod filelock;
pub mod scratch;
mod shell;
pub mod base_repo;
mod treeops;
mod view_subdir;
mod view_prefix;
mod view_chain;
pub mod virtual_repo;
pub mod cgi;
pub mod run_proxy;

pub use filelock::FileLock;
pub use scratch::*;
pub use shell::Shell;
pub use shell::thread_local_temp_dir;
pub use treeops::*;
pub use view_subdir::SubdirView;
pub use run_proxy::*;

use view_prefix::PrefixView;
use view_chain::ChainView;
use std::path::Path;
use git2::*;

#[derive(Clone)]
pub enum UnapplyView
{
    Done(git2::Oid),
    RejectNoFF,
    RejectMerge,
    NoChanges,
}

pub trait View
{
    fn apply(&self, repo: &git2::Repository, tree: &git2::Tree) -> Option<git2::Oid>;
    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> Option<git2::Oid>;
}

fn create_view_node(name: &str) -> Box<dyn View>
{
    if name.starts_with("+") {
        Box::new(PrefixView::new(&Path::new(&name[1..].trim_left_matches("/"))))
    } else {
        Box::new(SubdirView::new(&Path::new(&name)))
    }
}

struct NopView;


impl View for NopView
{
    fn apply(&self, repo: &Repository, tree: &Tree) -> Option<Oid>
    {
        Some(tree.id())
    }

    fn unapply(&self, repo: &Repository, tree: &Tree, parent_tree: &Tree) -> Option<Oid>
    {
        Some(tree.id())
    }
}

pub fn build_view(viewstr: &str) -> Box<dyn View> {
    let mut chain: Box<dyn View> = Box::new(NopView);
    for v in viewstr.split("/") {
        let new = create_view_node(&v);
        chain = Box::new(ChainView{first: chain, second: new});
    }
    return chain;
}
