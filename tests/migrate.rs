extern crate centralgithook;
extern crate git2;
extern crate tempdir;

mod helpers;

use centralgithook::migrate::RepoHost;
use centralgithook::migrate;
use self::tempdir::TempDir;
use std::str;

#[test]
fn test_initial_import()
{
    let host = helpers::TestHost::new();
    let workspace = TempDir::new("workspace").expect("folder workspace should be created");
    let central_repo_path = workspace.path().join("central");
    println!("    ");
    println!("    ########### SETUP: create central repository ########### ");
    host.create_project("central").expect("error: create_project");
    let central_repo = helpers::create_repository(&central_repo_path);

    let central_head = helpers::commit_files(&central_repo,
                                    &central_repo_path,
                                    &vec!["modules/moduleA/added_in_central.txt",
                                          "modules/moduleB/added_in_central.txt",
                                          "modules/moduleC/added_in_central.txt"]);

    println!("    ########### SETUP: create module repositories ########### ");

    let module_names = vec!["moduleA", "moduleB", "moduleC"];

    println!("    ########### START: calling initial_import ########### ");
    let td_scratch = TempDir::new("scratch").expect("folder scratch should be created");
    let scratch = migrate::Scratch::new(&td_scratch.path(), &host);

    println!("central_head: {}", central_head);

    migrate::initial_import(&scratch,
                            &helpers::_oid_to_sha1(&central_head.as_bytes()),
                            "central",
                            &central_repo_path /* &Path::new("/tmp/testscratch") */)
        .expect("call central_submit");

    let shell = migrate::Shell { cwd: workspace.path().to_path_buf() };

    for m in module_names {
        shell.command(&format!("git clone {}", &host.remote_url(&format!("modules/{}", m))));
        assert!(workspace.path().join(m).join("added_in_central.txt").exists());
    }
    // std::thread::sleep_ms(1111111);
}

