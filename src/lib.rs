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
    fn apply(&self, tree: &git2::Tree) -> Option<git2::Oid>;
    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> git2::Oid;
}
