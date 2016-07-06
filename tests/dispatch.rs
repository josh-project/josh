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
    review_upload_return: ReviewUploadResult,
}


impl MockHooks
{
    fn new() -> Self
    {
        MockHooks {
            called: UnsafeCell::new(String::new()),
            review_upload_return: ReviewUploadResult::Central,
        }
    }

    fn set_called(&self, s: &str)
    {
        unsafe {
            (*self.called.get()).clear();
            (*self.called.get()).push_str(s);
        }
    }

    fn called(&self) -> String
    {
        unsafe {
            let s = format!("{}", &*self.called.get());
            (*self.called.get()).clear();
            s
        }
    }
}

impl Hooks for MockHooks
{
    fn branch(&self) -> &str
    {
        "master"
    }

    fn review_upload(&self,
                     _scratch: &Scratch,
                     _host: &RepoHost,
                     newrev: git2::Object,
                     module: &str)
        -> ReviewUploadResult
    {
        self.set_called(&format!("review_upload(_,{},{})", newrev.id(), module));
        self.review_upload_return.clone()
    }

    fn pre_create_project(&self, _scratch: &Scratch, _rev: git2::Oid, project: &str)
    {
        self.set_called(&format!("pre_create_project(_,_,{})", project));
    }

    fn project_created(&self, _scratch: &Scratch, _host: &RepoHost, project: &str)
    {
        self.set_called(&format!("project_created(_,{})", project));
    }

    fn central_submit(&self, _scratch: &Scratch, _host: &RepoHost, newrev: git2::Object)
    {
        self.set_called(&format!("central_submit(_,{})", newrev.id()));
    }
}

#[test]
fn test_dispatch()
{
    let host = helpers::TestHost::new();
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = Scratch::new(&td.path().join("scratch"));
    let mut hooks = MockHooks::new();

    host.create_project("central");
    let central = helpers::TestRepo::new(&td.path().join("central"));
    central.add_file("foo_module_a/initial_a");
    let head = central.commit("on_branch_tmp");
    central.shell.command(&format!("git remote add origin {}", &host.remote_url("central")));
    central.shell.command("git push origin master");

    hooks.review_upload_return = ReviewUploadResult::Central;

    // allow review upload to master
    assert_eq!(0,
               dispatch(vec![
        format!("ref-update"),
        format!("--project"), format!("central"),
        format!("--refname"), format!("refs/for/master"),
        format!("--uploader"), format!("foo"),
        format!("--oldrev"), format!("{}","3424"),
        format!("--newrev"), format!("{}",head),
    ],
                        &hooks,
                        &host,
                        &scratch));

    assert_eq!(hooks.called(), format!("review_upload(_,{},central)", head));

    host.create_project("module");
    let module = helpers::TestRepo::new(&td.path().join("module"));
    module.add_file("foo_module_a/initial_a");
    let head = module.commit("on_branch_tmp");
    module.shell.command(&format!("git remote add origin {}", &host.remote_url("module")));
    module.shell.command("git push origin master");

    hooks.review_upload_return = ReviewUploadResult::Uploaded(git2::Oid::from_str(&head).unwrap(),
                                                              false);
    // reject review upload to module.
    // note that this means in fact that the review will be created on central
    assert_eq!(1,
               dispatch(vec![
        format!("ref-update"),
        format!("--project"), format!("module"),
        format!("--refname"), format!("refs/for/master"),
        format!("--uploader"), format!("foo"),
        format!("--oldrev"), format!("{}","3424"),
        format!("--newrev"), format!("{}",head),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!("review_upload(_,{},module)", head));

    // reject direct push to master on module
    assert_eq!(1,
               dispatch(vec![
        format!("ref-update"),
        format!("--project"), format!("module"),
        format!("--refname"), format!("master"),
        format!("--uploader"), format!("someone"),
        format!("--oldrev"), format!("{}","3424"),
        format!("--newrev"), format!("{}",head),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!(""));

    // allow push to module by automation user
    assert_eq!(0,
               dispatch(vec![
        format!("ref-update"),
        format!("--project"), format!("module"),
        format!("--refname"), format!("master"),
        format!("--uploader"), format!("Automation"),
        format!("--oldrev"), format!("{}","3424"),
        format!("--newrev"), format!("{}",head),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!(""));

    // reject push to master if not automation user even if initial upload
    assert_eq!(1,
               dispatch(vec![
        format!("ref-update"),
        format!("--project"), format!("central"),
        format!("--refname"), format!("master"),
        format!("--uploader"), format!("someone"),
        format!("--oldrev"), format!("{}","0000000000000000000000000000000000000000"),
        format!("--newrev"), format!("{}",head),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!(""));

    // reject push to master if not initial upload
    assert_eq!(1,
               dispatch(vec![
        format!("ref-update"),
        format!("--project"), format!("central"),
        format!("--refname"), format!("master"),
        format!("--uploader"), format!("Automation"),
        format!("--oldrev"), format!("{}","3424"),
        format!("--newrev"), format!("{}",head),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!(""));

    // allow direct push no branch that is not master on central, and do nothing
    assert_eq!(0,
               dispatch(vec![
        format!("ref-update"),
        format!("--project"), format!("central"),
        format!("--refname"), format!("notmaster"),
        format!("--uploader"), format!("don_t_care"),
        format!("--oldrev"), format!("{}","3424"),
        format!("--newrev"), format!("{}",head),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!(""));

    // allow direct push no branch that is not master on module, and do nothing
    assert_eq!(0,
               dispatch(vec![
        format!("ref-update"),
        format!("--project"), format!("module"),
        format!("--refname"), format!("notmaster"),
        format!("--uploader"), format!("don_t_care"),
        format!("--oldrev"), format!("{}","3424"),
        format!("--newrev"), format!("{}",head),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!(""));

    // allow direct push to central master by automation user for initial upload
    assert_eq!(0,
               dispatch(vec![
        format!("ref-update"),
        format!("--project"), format!("central"),
        format!("--refname"), format!("master"),
        format!("--uploader"), format!("Automation"),
        format!("--oldrev"), format!("0000000000000000000000000000000000000000"),
        format!("--newrev"), format!("{}",head),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!("central_submit(_,{})", head));

    // submit only happens on central
    assert_eq!(0,
               dispatch(vec![
        format!("change-merged"),
        format!("--change"), format!("1234"),
        format!("--change-url"), format!("does://not/matter"),
        format!("--change-owner"), format!("ignored"),
        format!("--project"), format!("central"),
        format!("--branch"), format!("{}","master"),
        format!("--topic"), format!("don_not_care"),
        format!("--submitter"), format!("don_not_care"),
        format!("--commit"), format!("{}",head),
        format!("--newrev"), format!("{}",head),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!("central_submit(_,{})", head));

    // project created calls a hook
    assert_eq!(0,
               dispatch(vec![
        format!("project-created"),
        format!("--project"), format!("module"),
        format!("--head"), format!("{}","master"),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!("project_created(_,module)"));

    // project created calls a hook
    assert_eq!(0,
               dispatch(vec![
        format!("validate-project"),
        format!("--project"), format!("module"),
    ],
                        &hooks,
                        &host,
                        &scratch));
    assert_eq!(hooks.called(), format!("pre_create_project(_,_,module)"));
}
