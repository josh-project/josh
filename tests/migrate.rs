extern crate centralgithook;
extern crate tempdir;
#[macro_use]
extern crate log;
extern crate env_logger;

mod helpers;

use centralgithook::migrate::RepoHost;
use centralgithook::migrate;
use tempdir::TempDir;

struct TestSetup<'a>
{
    td: TempDir,
    central: helpers::TestRepo,
    scratch: migrate::Scratch<'a>,
    shell: migrate::Shell,
    head: String,
}

impl<'a> TestSetup<'a>
{
    fn new(host: &'a RepoHost) -> Self
    {
        env_logger::init().unwrap();
        let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
        let central = helpers::TestRepo::new(&td.path().join("central"));
        let scratch = migrate::Scratch::new(&td.path().join("scratch"), host);
        let shell = migrate::Shell { cwd: td.path().to_path_buf() };

        host.create_project("central");
        central.shell.command(&format!("git remote add origin {}", &host.remote_url("central")));

        central.add_file("modules/module_a/initial_a");
        central.add_file("modules/module_b/initial_b");
        let head = central.commit("initial");

        return TestSetup {
            td: td,
            central: central,
            scratch: scratch,
            shell: shell,
            head: head,
        };
    }
}

#[test]
fn test_initial_import()
{
    let host = helpers::TestHost::new();
    let TestSetup { td, central, scratch, shell, head } = TestSetup::new(&host);

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")))
        .expect("call central_submit");

    shell.command(&format!("git clone {}", &host.remote_url("modules/module_a")));
    shell.command(&format!("git clone {}", &host.remote_url("modules/module_b")));
    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));
    let module_b = helpers::TestRepo::new(&td.path().join("module_b"));

    assert!(module_a.has_file("initial_a"));
    assert!(module_b.has_file("initial_b"));

    central.add_file("modules/module_a/added_a");
    central.add_file("modules/module_c/added_c");
    let head = central.commit("add");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch, scratch.transfer(&head, &host.repo_dir("central")))
        .expect("call central_submit");

    module_a.shell.command("git pull");
    shell.command(&format!("git clone {}", &host.remote_url("modules/module_c")));
    let module_c = helpers::TestRepo::new(&td.path().join("module_c"));

    assert!(module_a.has_file("added_a"));
    assert!(module_c.has_file("added_c"));

    module_a.add_file("added/in_mod_a");
    let head = module_a.commit("module_a_commit");

    migrate::module_review_upload(&scratch,
                                  scratch.transfer(&head, &module_a.path),
                                  "modules/module_a",
                                  "central")
        .expect("module_review_upload failed");

    central.shell.command("git fetch origin for/master:for/master");
    let for_master = central.rev("for/master");

    migrate::central_submit(&scratch,
                            scratch.transfer(&for_master, &host.repo_dir("central")))
        .expect("call central_submit");

    central.shell.command("git rebase for/master");
    assert_eq!(central.rev("master"), central.rev("for/master"));
    assert_eq!(central.rev("master"), central.rev("HEAD"));
    assert!(central.has_file("modules/module_a/added/in_mod_a"));

    module_a.shell.command("git pull");
    assert_eq!(module_a.rev("origin/master"), head);
}
