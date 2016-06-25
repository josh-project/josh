extern crate centralgithook;
extern crate tempdir;

mod helpers;

use centralgithook::migrate::RepoHost;
use centralgithook::migrate;
use tempdir::TempDir;

#[test]
fn test_initial_import()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let central = helpers::TestRepo::new(&td.path().join("central"));
    let host = helpers::TestHost::new();
    let scratch = migrate::Scratch::new(&td.path().join("scratch"), &host);
    let shell = migrate::Shell { cwd: td.path().to_path_buf() };

    host.create_project("central").expect("create central failed");
    central.shell.command(&format!("git remote add origin {}", &host.remote_url("central")));

    central.add("modules/module_a/initial_a");
    central.add("modules/module_b/initial_b");
    let head = central.commit("initial");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch,
                            scratch.transfer(&head, &host.repo_dir("central")))
        .expect("call central_submit");

    shell.command(&format!("git clone {}",
                           &host.remote_url("modules/module_a")));
    assert!(td.path().join("module_a").join("initial_a").exists());

    shell.command(&format!("git clone {}",
                           &host.remote_url("modules/module_b")));
    assert!(td.path().join("module_b").join("initial_b").exists());


    central.add("modules/module_a/added_a");
    central.add("modules/module_c/added_c");
    let head = central.commit("add");

    central.shell.command("git push origin master");
    migrate::central_submit(&scratch,
                            scratch.transfer(&head, &host.repo_dir("central")))
        .expect("call central_submit");

    let module_a = helpers::TestRepo::new(&td.path().join("module_a"));

    module_a.shell.command("git pull");
    assert!(td.path().join("module_a").join("added_a").exists());

    shell.command(&format!("git clone {}",
                           &host.remote_url("modules/module_c")));
    assert!(td.path().join("module_c").join("added_c").exists());

    module_a.add("added/in_mod_a");
    let head = module_a.commit("module_a_commit");

    migrate::module_review_upload(&scratch,
                                  scratch.transfer(&head, &module_a.path),
                                  "modules/module_a",
                                  "central")
        .expect("module_review_upload failed");

    central.shell.command("git fetch origin for/master:for/master");
    let for_master = central.repo.revparse_single("for/master").expect("no refs/for/master").id();

    migrate::central_submit(&scratch,
                            scratch.transfer(&helpers::_oid_to_sha1(for_master.as_bytes()),
                            &host.repo_dir("central")))
        .expect("call central_submit");

    module_a.shell.command("git pull");
    let new_head = module_a.repo.revparse_single("origin/master").expect("no origin/master").id();
    assert_eq!(helpers::_oid_to_sha1(new_head.as_bytes()), head);

}
