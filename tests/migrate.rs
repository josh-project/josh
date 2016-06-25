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
    let central_repo = helpers::TestRepo::new(&td.path().join("central"));

    let central_head = central_repo.commit_files(&vec!["modules/moduleA/added_in_central.txt",
                                                       "modules/moduleB/added_in_central.txt",
                                                       "modules/moduleC/added_in_central.txt"]);

    let host = helpers::TestHost::new();
    host.create_project("central").expect("error: create_project");

    let scratch = migrate::Scratch::new(&td.path().join("scratch"), &host);

    migrate::initial_import(&scratch,
                            scratch.transfer(&helpers::_oid_to_sha1(&central_head.as_bytes()),
                                             &central_repo.path),
                            "central")
        .expect("call initial_import");


    let ws = TempDir::new("cgh_ws").expect("folder cgh_ws should be created");
    let shell = migrate::Shell { cwd: ws.path().to_path_buf() };

    shell.command(&format!("git clone {}",
                           &host.remote_url("modules/moduleA")));
    assert!(ws.path().join("moduleA").join("added_in_central.txt").exists());

    shell.command(&format!("git clone {}",
                           &host.remote_url("modules/moduleB")));
    assert!(ws.path().join("moduleB").join("added_in_central.txt").exists());

    shell.command(&format!("git clone {}",
                           &host.remote_url("modules/moduleC")));
    assert!(ws.path().join("moduleC").join("added_in_central.txt").exists());
    // std::thread::sleep_ms(1111111);
}
