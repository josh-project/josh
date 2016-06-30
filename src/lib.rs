extern crate git2;
extern crate tempdir;
#[macro_use]
extern crate log;

mod centralgit;
mod scratch;
mod shell;
mod gerrit;

pub use scratch::Scratch;
pub use gerrit::Gerrit;
pub use gerrit::find_repos;
pub use shell::Shell;
pub use centralgit::CentralGit;

pub enum ReviewUploadResult
{
    Uploaded(git2::Oid),
    RejectNoFF,
    RejectMerge,
    NoChanges,
    Central,
}

pub trait Hooks
{
    fn review_upload(&self, scratch: &Scratch, newrev: git2::Object, module: &str) -> ReviewUploadResult;
    fn project_created(&self, scratch: &Scratch);
    fn central_submit(&self, scratch: &Scratch, newrev: git2::Object);
}

pub trait RepoHost
{
    fn central(&self) -> &str;
    fn projects(&self) -> Vec<String>;

    fn remote_url(&self, &str) -> String;
    fn fetch_url(&self, module: &str) -> String
    {
        self.remote_url(module)
    }
}

