extern crate git2;
extern crate tempdir;
#[macro_use]
extern crate log;

mod centralgit;
mod scratch;
mod shell;
mod gerrit;
mod dispatch;

pub use scratch::Scratch;
pub use gerrit::Gerrit;
pub use gerrit::find_repos;
pub use shell::Shell;
pub use centralgit::CentralGit;
pub use dispatch::dispatch;

#[derive(Clone)]
pub enum ModuleToSubdir
{
    Done(git2::Oid, bool),
    RejectNoFF,
    RejectMerge,
    NoChanges,
}

pub trait Hooks
{
    fn review_upload(&self,
                     scratch: &Scratch,
                     host: &RepoHost,
                     project_list: &ProjectList,
                     newrev: git2::Object,
                     module: &str)
        -> ModuleToSubdir;
    fn project_created(&self,
                       scratch: &Scratch,
                       host: &RepoHost,
                       project_list: &ProjectList,
                       project: &str);
    fn pre_create_project(&self, scratch: &Scratch, rev: git2::Oid, project: &str);
    fn central_submit(&self,
                      scratch: &Scratch,
                      host: &RepoHost,
                      project_list: &ProjectList,
                      newrev: git2::Object);

    fn branch(&self) -> &str;
}

pub trait RepoHost
{
    fn remote_url(&self, &str) -> String;
    fn local_path(&self, module: &str) -> String
    {
        self.remote_url(module)
    }

    fn prefix(&self) -> &str
    {
        ""
    }

    fn automation_user(&self) -> &str;
}

pub trait ProjectList
{
    fn central(&self) -> &str;
    fn projects(&self) -> Vec<String>;
}
