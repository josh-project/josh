extern crate centralgithook;
extern crate tempdir;
#[macro_use]
extern crate log;
extern crate env_logger;

mod helpers;

use centralgithook::scratch::RepoHost;
use centralgithook::scratch::Scratch;
use centralgithook::shell::Shell;
use centralgithook::migrate;
use centralgithook::migrate::ReviewUploadResult;
use tempdir::TempDir;


struct TestSetup<'a>
{
    td: TempDir,
    central: helpers::TestRepo,
    scratch: Scratch<'a>,
    shell: Shell,
}

impl<'a> TestSetup<'a>
{
    fn new(host: &'a helpers::TestHost) -> Self
    {
        host.create_project("modules/module_a");
        host.create_project("modules/module_b");
        host.create_project("modules/module_c");

        env_logger::init().ok();
        let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
        let central = helpers::TestRepo::new(&td.path().join("central"));
        let scratch = Scratch::new(&td.path().join("scratch"), host);
        let shell = Shell { cwd: td.path().to_path_buf() };

        host.create_project("central");
        central.shell.command(&format!("git remote add origin {}", &host.remote_url("central")));

        return TestSetup {
            td: td,
            central: central,
            scratch: scratch,
            shell: shell,
        };
    }
}

#[test]
fn test_initial_import()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    central.add_file("modules/module_b/initial_b");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    shell.command(&format!("git clone {}", &host.remote_url("modules/module_b")));

    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));
    let module_b = helpers::TestRepo::new(&td.path().join("module_b"));

    assert!(module_a.has_file("initial_a"));
    assert!(module_b.has_file("initial_b"));
}

#[test]
fn test_create_project()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));
    assert!(module_a.has_file("initial_a"));

    host.create_project("modules");
    migrate::project_created(&scratch);

    shell.command(&format!("git clone {}", &host.remote_url("modules")));
    let modules = helpers::TestRepo::new(&td.path().join("modules"));
    assert!(modules.has_file("module_a/initial_a"));
}

#[test]
fn test_change_and_add_modules()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    central.add_file("modules/module_a/added_a");
    central.add_file("modules/module_c/added_c");
    let head = central.commit("add_a_and_c");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    shell.command(&format!("git clone {}", &host.remote_url("modules/module_c")));

    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));
    let module_c = helpers::TestRepo::new(&td.path().join("module_c"));

    assert!(module_a.has_file("added_a"));
    assert!(module_c.has_file("added_c"));
}

#[test]
fn test_add_module_not_on_host()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_new/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    central.add_file("modules/module_new/added_new");
    let head = central.commit("add_new");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_new")));

    let module_new = helpers::TestRepo::new(&td.path().join("module_new"));

    assert!(!module_new.has_file("added_new"));
}

#[test]
fn test_remove_module_dir()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));

    assert!(module_a.has_file("initial_a"));

    central.shell.command("rm -Rf modules/module_a");
    central.shell.command("git add .");
    println!("{}", central.shell.command("git status"));
    let head = central.commit("remove a");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));
}

#[test]
fn test_add_module_empty_on_host()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_new/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    central.add_file("modules/module_new/added_new");
    let head = central.commit("add_new");

    central.shell.command("git push origin master");
    host.create_project("modules/module_new");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_new")));

    let module_new = helpers::TestRepo::new(&td.path().join("module_new"));

    assert!(module_new.has_file("added_new"));
}

#[test]
fn test_central_review_upload()
{
    let host = helpers::TestHost::new();
    let TestSetup { td: _td , central, scratch, shell: _ } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master:master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    central.add_file("modules/module_a/added");
    let head = central.commit("add_addmit");
    central.shell.command("git push origin master:refs/for/master");

    if let ReviewUploadResult::Central =
           migrate::review_upload(&scratch,
                                  scratch.transfer(&head, &central.path),
                                  "central") {
    }
    else { assert!(false); }

    central.rev("for/master");
}

#[test]
fn test_module_review_upload_rejects_merges()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master:master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));

    module_a.shell.command("git checkout -b tmp");
    module_a.add_file("added_tmp");
    module_a.commit("on_branch_tmp");

    module_a.shell.command("git checkout master");
    module_a.add_file("added_master");

    module_a.commit("on_branch_master");
    module_a.shell.command("git merge --no-ff tmp");

    module_a.shell.command("git push origin master:refs/for/master");

    let head = module_a.rev("master");

    if let ReviewUploadResult::RejectMerge =
           migrate::review_upload(&scratch,
                                  scratch.transfer(&head, &host.repo_dir("modules/module_a")),
                                  "modules/module_a") {
    }
    else { assert!(false); }

    module_a.rev("for/master");
}

#[test]
fn test_module_review_upload_rejects_non_fast_forward()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master:master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));

    module_a.shell.command("git checkout -b tmp");
    module_a.add_file("added_tmp");
    module_a.commit("on_branch_tmp");

    module_a.shell.command("git checkout master");
    module_a.add_file("added_master");

    module_a.commit("on_branch_master");

    module_a.shell.command("git push origin master:master");
    module_a.shell.command("git push origin tmp:refs/for/tmp");

    let head = module_a.rev("tmp");

    if let ReviewUploadResult::RejectNoFF =
           migrate::review_upload(&scratch,
                                  scratch.transfer(&head, &host.repo_dir("modules/module_a")),
                                  "modules/module_a") {
    }
    else { assert!(false); }

    module_a.rev("for/master");
}

#[test]
fn test_central_review_upload_rejects_merges()
{
    let host = helpers::TestHost::new();
    let TestSetup { td:_td , central, scratch, shell: _ } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master:master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    central.shell.command("git checkout -b tmp");
    central.add_file("modules/module_a/added_tmp");
    central.commit("on_branch_tmp");

    central.shell.command("git checkout master");
    central.add_file("modules/module_a/added_master");

    central.commit("on_branch_master");
    central.shell.command("git merge --no-ff tmp");

    central.shell.command("git push origin master:refs/for/master");

    let head = central.rev("master");

    if let ReviewUploadResult::RejectMerge =
           migrate::review_upload(&scratch,
                                  scratch.transfer(&head, &host.repo_dir("central")),
                                  "central") {
    }
    else { assert!(false); }

    central.rev("for/master");
}

#[test]
fn test_module_review_upload()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));
    module_a.shell.command("git pull");

    module_a.add_file("added/in_mod_a");
    let head = module_a.commit("module_a_commit");

    if let ReviewUploadResult::Uploaded(oid) =
           migrate::review_upload(&scratch,
                                  scratch.transfer(&head, &module_a.path),
                                  "modules/module_a") {
        scratch.push(oid, host.central(), "refs/for/master");
    }
    else { assert!(false); }

    central.shell.command("git fetch origin for/master:for/master");
    let for_master = central.rev("for/master");

    migrate::central_submit(&scratch,
                            scratch.transfer(&for_master, &host.repo_dir("central")));

    central.shell.command("git rebase for/master");
    assert_eq!(central.rev("master"), central.rev("for/master"));
    assert_eq!(central.rev("master"), central.rev("HEAD"));
    assert!(central.has_file("modules/module_a/added/in_mod_a"));

    module_a.shell.command("git pull");
    assert_eq!(module_a.rev("origin/master"), head);
}

#[test]
fn test_module_review_upload_1_level()
{
    let host = helpers::TestHost::new();
    host.create_project("foo_module_a");
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    central.add_file("modules/module_a/initial_b");
    central.add_file("foo_module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("foo_module_a")));
    let foo_module_a = helpers::TestRepo::new(&td.path().join("foo_module_a"));
    foo_module_a.shell.command("git pull");

    foo_module_a.add_file("added/in_mod_a");
    let head = foo_module_a.commit("module_a_commit");

    if let ReviewUploadResult::Uploaded(oid) =
           migrate::review_upload(&scratch,
                                  scratch.transfer(&head, &foo_module_a.path),
                                  "foo_module_a") {
        scratch.push(oid, host.central(), "refs/for/master");
    }
    else { assert!(false); }

    central.shell.command("git fetch origin for/master:for/master");
    let for_master = central.rev("for/master");

    migrate::central_submit(&scratch,
                            scratch.transfer(&for_master, &host.repo_dir("central")));

    central.shell.command("git rebase for/master");
    assert_eq!(central.rev("master"), central.rev("for/master"));
    assert_eq!(central.rev("master"), central.rev("HEAD"));
    assert!(central.has_file("foo_module_a/added/in_mod_a"));

    foo_module_a.shell.command("git pull");
    assert_eq!(foo_module_a.rev("origin/master"), head);
}

#[test]
fn test_module_review_upload_4_levels()
{
    let host = helpers::TestHost::new();
    host.create_project("foo/modules/bla/module_a");
    let TestSetup { td, central, scratch, shell } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    central.add_file("modules/module_a/initial_b");
    central.add_file("foo/modules/bla/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("foo/modules/bla/module_a")));
    let foo_module_a = helpers::TestRepo::new(&td.path().join("module_a"));
    foo_module_a.shell.command("git pull");

    foo_module_a.add_file("added/in_mod_a");
    let head = foo_module_a.commit("module_a_commit");

    if let ReviewUploadResult::Uploaded(oid) =
           migrate::review_upload(&scratch,
                                  scratch.transfer(&head, &foo_module_a.path),
                                  "foo/modules/bla/module_a") {
        scratch.push(oid, host.central(), "refs/for/master");
    }
    else { assert!(false); }

    central.shell.command("git fetch origin for/master:for/master");
    let for_master = central.rev("for/master");

    migrate::central_submit(&scratch,
                            scratch.transfer(&for_master, &host.repo_dir("central")));

    central.shell.command("git rebase for/master");
    assert_eq!(central.rev("master"), central.rev("for/master"));
    assert_eq!(central.rev("master"), central.rev("HEAD"));
    assert!(central.has_file("foo/modules/bla/module_a/added/in_mod_a"));

    foo_module_a.shell.command("git pull");
    assert_eq!(foo_module_a.rev("origin/master"), head);
}
