extern crate git2;
extern crate tempdir;
#[macro_use]
extern crate log;

mod scratch;
mod shell;

pub use scratch::Scratch;
pub use scratch::SubdirView;
pub use shell::Shell;
pub use scratch::module_ref;
pub use scratch::module_ref_root;

#[derive(Clone)]
pub enum ModuleToSubdir
{
    Done(git2::Oid, bool),
    RejectNoFF,
    RejectMerge,
    NoChanges,
}
