extern crate centralgithook;
use centralgithook::*;
#[allow(dead_code)]
mod helpers;
extern crate git2;
use std::cell::UnsafeCell;
extern crate tempdir;
use tempdir::TempDir;


struct MockHooks
{
    called: UnsafeCell<String>,
}


impl MockHooks
{
    fn new() -> Self { MockHooks { called: UnsafeCell::new(String::new()) }}

    fn set_called(&self, s: &str)
    {
        unsafe {
            (*self.called.get()).clear();
            (*self.called.get()).push_str(s);
        }
    }

    fn called(&self) -> &str
    {
        unsafe {
            &*self.called.get()
        }
    }
}

impl Hooks for MockHooks
{
    fn review_upload(&self,
                     _scratch: &Scratch,
                     newrev: git2::Object,
                     module: &str)
        -> ReviewUploadResult
    {
        self.set_called(&format!("review_upload(_,{},{})", newrev.id(), module));
        ReviewUploadResult::Central
    }

    fn project_created(&self, _scratch: &Scratch)
    {
        self.set_called(&format!("project_created(_)"));
    }

    fn central_submit(&self, _scratch: &Scratch, newrev: git2::Object)
    {
        self.set_called(&format!("central_submit(_,{})", newrev.id()));
    }
}

#[test]
fn test_hook()
{
    let host = helpers::TestHost::new();
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = Scratch::new(&td.path().join("scratch"), &host);
    let hooks = MockHooks::new();
    host.create_project("central");
    let central = helpers::TestRepo::new(&td.path().join("central"));
    central.add_file("foo_module_a/initial_a");
    let head = central.commit("on_branch_tmp");

    central.shell.command(&format!("git remote add origin {}", &host.remote_url("central")));
    central.shell.command("git push origin master");

    dispatch(vec![
        format!("ref-update"),
        format!("--refname"), format!("refs/for/master"),
        format!("--newrev"), format!("{}",head),
        format!("--project"), format!("central"),
    ], &hooks, &host, &scratch);

    assert_eq!(hooks.called(), format!("review_upload(_,{},central)", head));
}
