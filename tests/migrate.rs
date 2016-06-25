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
    let ws = TempDir::new("cgh_ws").expect("folder cgh_ws should be created");
    let central = helpers::TestRepo::new(&td.path().join("central"));
    let host = helpers::TestHost::new();
    let scratch = migrate::Scratch::new(&td.path().join("scratch"), &host);
    let shell = migrate::Shell { cwd: ws.path().to_path_buf() };

    central.add("modules/module_a/initial_a");
    central.add("modules/module_b/initial_b");
    let central_head = central.commit("initial");

    migrate::central_submit(&scratch,
                            scratch.transfer(&central_head, &central.path))
        .expect("call central_submit");

    shell.command(&format!("git clone {}",
                           &host.remote_url("modules/module_a")));
    assert!(ws.path().join("module_a").join("initial_a").exists());

    shell.command(&format!("git clone {}",
                           &host.remote_url("modules/module_b")));
    assert!(ws.path().join("module_b").join("initial_b").exists());


    central.add("modules/module_a/added_a");
    central.add("modules/module_c/added_c");
    let central_head = central.commit("add");

    migrate::central_submit(&scratch,
                            scratch.transfer(&central_head, &central.path))
        .expect("call central_submit");

    let module_a = helpers::TestRepo::new(&ws.path().join("module_a"));

    module_a.shell.command("git pull");
    assert!(ws.path().join("module_a").join("added_a").exists());

    shell.command(&format!("git clone {}",
                           &host.remote_url("modules/module_c")));
    assert!(ws.path().join("module_c").join("added_c").exists());
    // std::thread::sleep_ms(1111111);
}
