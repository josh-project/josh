extern crate centralgithook;
extern crate tempdir;
#[macro_use]
extern crate log;
extern crate env_logger;

mod helpers;

use centralgithook::RepoHost;
use centralgithook::ProjectList;
use centralgithook::Scratch;
use centralgithook::Shell;
use centralgithook::CentralGit;
use centralgithook::Hooks;
use centralgithook::ModuleToSubdir;
use tempdir::TempDir;


struct TestSetup
{
    td: TempDir,
    central: helpers::TestRepo,
    scratch: Scratch,
    shell: Shell,
    hooks: CentralGit,
}

impl TestSetup
{
    fn new(host: &helpers::TestHost) -> Self
    {
        let hooks = CentralGit::new("master");

        host.create_project("modules/module_a");
        host.create_project("modules/module_b");
        host.create_project("modules/module_c");

        env_logger::init().ok();
        let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
        let central = helpers::TestRepo::new(&td.path().join("central"));
        let scratch = Scratch::new(&td.path().join("scratch"));


        let shell = Shell { cwd: td.path().to_path_buf() };

        host.create_project("central");
        central.shell.command(&format!("git remote add origin {}", &host.remote_url("central")));

        return TestSetup {
            td: td,
            central: central,
            scratch: scratch,
            shell: shell,
            hooks: hooks,
        };
    }
}

#[test]
fn test_initial_import()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    central.add_file("modules/module_b/initial_b");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

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
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));
    assert!(module_a.has_file("initial_a"));

    host.create_project("modules");
    hooks.project_created(&scratch, &host, &host, "modules");

    shell.command(&format!("git clone {}", &host.remote_url("modules")));
    let modules = helpers::TestRepo::new(&td.path().join("modules"));
    assert!(modules.has_file("module_a/initial_a"));
}

#[test]
fn test_change_and_add_modules()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    central.add_file("modules/module_a/added_a");
    central.add_file("modules/module_c/added_c");
    let head = central.commit("add_a_and_c");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

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
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_new/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    central.add_file("modules/module_new/added_new");
    let head = central.commit("add_new");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_new")));

    let module_new = helpers::TestRepo::new(&td.path().join("module_new"));

    assert!(!module_new.has_file("added_new"));
}

#[test]
fn test_remove_module_dir()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));

    assert!(module_a.has_file("initial_a"));

    central.shell.command("rm -Rf modules/module_a");
    central.shell.command("git add .");
    let head = central.commit("remove a");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));
}

#[test]
fn test_add_module_empty_on_host()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_new/initial_a");
    let _ = central.commit("initial");

    central.add_file("modules/module_new/added_new");
    let head = central.commit("add_new");

    central.shell.command("git push origin master");
    host.create_project("modules/module_new");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_new")));

    let module_new = helpers::TestRepo::new(&td.path().join("module_new"));

    assert!(module_new.has_file("initial_a"));
    assert!(module_new.has_file("added_new"));

    module_new.shell.command("git checkout master~1");
    assert!(!module_new.has_file("added_new"));
    assert!(module_new.has_file("initial_a"));
}

#[test]
fn test_central_review_upload()
{
    ::std::env::set_var("GIT_DIR", "/"); // ensure that we don't care about GIT_DIR

    let host = helpers::TestHost::new();
    let TestSetup { td: _td, central, scratch, shell: _, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master:master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    central.add_file("modules/module_a/added");
    let head = central.commit("add_addmit");
    central.shell.command("git push origin master:refs/for/master");

    if let ModuleToSubdir::Done(_, _) = hooks.review_upload(&scratch,
                       &host,
                       &host,
                       scratch.transfer(&head, &central.path),
                       "central") {
    }
    else {
        assert!(false);
    }

    central.rev("for/master");
}

#[test]
fn test_module_review_upload_rejects_merges()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master:master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

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

    if let ModuleToSubdir::RejectMerge = hooks.review_upload(&scratch,
                       &host,
                       &host,
                       scratch.transfer(&head, &host.repo_dir("modules/module_a")),
                       "modules/module_a") {
    }
    else {
        assert!(false);
    }

    module_a.rev("for/master");
}

#[test]
fn test_module_review_upload_rejects_non_fast_forward()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master:master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

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

    if let ModuleToSubdir::RejectNoFF = hooks.review_upload(&scratch,
                       &host,
                       &host,
                       scratch.transfer(&head, &host.repo_dir("modules/module_a")),
                       "modules/module_a") {
    }
    else {
        assert!(false);
    }

    module_a.rev("for/master");
}

#[test]
fn test_central_review_upload_rejects_merges()
{
    let host = helpers::TestHost::new();
    let TestSetup { td: _td, central, scratch, shell: _, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master:master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    central.shell.command("git checkout -b tmp");
    central.add_file("modules/module_a/added_tmp");
    central.commit("on_branch_tmp");

    central.shell.command("git checkout master");
    central.add_file("modules/module_a/added_master");

    central.commit("on_branch_master");
    central.shell.command("git merge --no-ff tmp");

    central.shell.command("git push origin master:refs/for/master");

    let head = central.rev("master");

    if let ModuleToSubdir::RejectMerge = hooks.review_upload(&scratch,
                       &host,
                       &host,
                       scratch.transfer(&head, &host.repo_dir("central")),
                       "central") {
    }
    else {
        assert!(false);
    }

    central.rev("for/master");
}

#[test]
fn test_module_review_upload()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));
    module_a.shell.command("git pull");

    module_a.add_file("added/in_mod_a");
    let head = module_a.commit("module_a_commit");

    if let ModuleToSubdir::Done(oid, initial) = hooks.review_upload(&scratch,
                                                                    &host,
                                                                    &host,
                                                                    scratch.transfer(&head,
                                                                                  &module_a.path),
                                                                    "modules/module_a") {
        assert!(!initial);
        scratch.push(&host, oid, host.central(), "refs/for/master");
    }
    else {
        assert!(false);
    }

    central.shell.command("git fetch origin for/master:for/master");
    let for_master = central.rev("for/master");

    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&for_master, &host.repo_dir("central")));

    central.shell.command("git rebase for/master");
    assert_eq!(central.rev("master"), central.rev("for/master"));
    assert_eq!(central.rev("master"), central.rev("HEAD"));
    assert!(central.has_file("modules/module_a/added/in_mod_a"));

    module_a.shell.command("git pull");
    assert_eq!(module_a.rev("origin/master"), head);
}

#[test]
fn test_module_review_upload_new_module()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    host.create_project("modules/module_new");
    shell.command(&format!("git clone {}", &host.remote_url("modules/module_new")));
    let module_new = helpers::TestRepo::new(&td.path().join("module_new"));

    module_new.add_file("added/in_mod_new");
    let head = module_new.commit("module_new_commit");

    if let ModuleToSubdir::Done(oid, initial) = hooks.review_upload(&scratch,
                       &host,
                       &host,
                       scratch.transfer(&head, &module_new.path),
                       "modules/module_new") {
        assert!(initial);
        scratch.push(&host, oid, host.central(), "refs/for/master");
    }
    else {
        assert!(false);
    }

    central.shell.command("git fetch origin for/master:for/master");
    let for_master = central.rev("for/master");

    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&for_master, &host.repo_dir("central")));

    central.shell.command("git rebase for/master");
    assert_eq!(central.rev("master"), central.rev("for/master"));
    assert_eq!(central.rev("master"), central.rev("HEAD"));
    assert!(central.has_file("modules/module_new/added/in_mod_new"));

    module_new.shell.command("git pull");
    assert_eq!(module_new.rev("origin/master"), head);
}

#[test]
fn test_module_review_upload_1_level()
{
    let host = helpers::TestHost::new();
    host.create_project("foo_module_a");
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    central.add_file("modules/module_a/initial_b");
    central.add_file("foo_module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("foo_module_a")));
    let foo_module_a = helpers::TestRepo::new(&td.path().join("foo_module_a"));
    foo_module_a.shell.command("git pull");

    foo_module_a.add_file("added/in_mod_a");
    let head = foo_module_a.commit("module_a_commit");

    if let ModuleToSubdir::Done(oid, initial) = hooks.review_upload(&scratch,
                       &host,
                       &host,
                       scratch.transfer(&head, &foo_module_a.path),
                       "foo_module_a") {
        assert!(!initial);
        scratch.push(&host, oid, host.central(), "refs/for/master");
    }
    else {
        assert!(false);
    }

    central.shell.command("git fetch origin for/master:for/master");
    let for_master = central.rev("for/master");

    hooks.central_submit(&scratch,
                         &host,
                         &host,
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
    let TestSetup { td, central, scratch, shell, hooks } = TestSetup::new(&host);

    central.add_file("modules/module_a/initial_a");
    central.add_file("modules/module_a/initial_b");
    central.add_file("foo/modules/bla/module_a/initial_a");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&head, &host.repo_dir("central")));

    shell.command(&format!("git clone {}", &host.remote_url("foo/modules/bla/module_a")));
    let foo_module_a = helpers::TestRepo::new(&td.path().join("module_a"));
    foo_module_a.shell.command("git pull");

    foo_module_a.add_file("added/in_mod_a");
    let head = foo_module_a.commit("module_a_commit");

    if let ModuleToSubdir::Done(oid, initial) = hooks.review_upload(&scratch,
                       &host,
                       &host,
                       scratch.transfer(&head, &foo_module_a.path),
                       "foo/modules/bla/module_a") {
        assert!(!initial);
        scratch.push(&host, oid, host.central(), "refs/for/master");
    }
    else {
        assert!(false);
    }

    central.shell.command("git fetch origin for/master:for/master");
    let for_master = central.rev("for/master");

    hooks.central_submit(&scratch,
                         &host,
                         &host,
                         scratch.transfer(&for_master, &host.repo_dir("central")));

    central.shell.command("git rebase for/master");
    assert_eq!(central.rev("master"), central.rev("for/master"));
    assert_eq!(central.rev("master"), central.rev("HEAD"));
    assert!(central.has_file("foo/modules/bla/module_a/added/in_mod_a"));

    foo_module_a.shell.command("git pull");
    assert_eq!(foo_module_a.rev("origin/master"), head);
}
