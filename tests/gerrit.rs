extern crate centralgithook;
extern crate tempdir;
#[macro_use]
extern crate log;
extern crate env_logger;

// mod helpers;

use centralgithook::scratch::RepoHost;
use centralgithook::gerrit::Gerrit;
use centralgithook::shell::Shell;
use tempdir::TempDir;

fn sorted(mut v: Vec<String>) -> Vec<String>
{
    v.sort();
    v
}

#[test]
fn test_gerrit()
{
    let td = TempDir::new("gerrit_test").expect("folder gerrit_test should be created");
    let shell = Shell { cwd: td.path().to_path_buf() };
    shell.command("mkdir -p bin/gerrit.sh");

    shell.command("mkdir -p git/bsw/central.git");
    let gerrit = Gerrit::new(&td.path().join("git/bsw/central.git"),
                             "central",
                             "auto",
                             "123");
    assert_eq!("central", gerrit.central());
    assert_eq!(vec!["central"], gerrit.projects());
    assert_eq!(td.path().join("git/bsw/central.git").to_str().unwrap(),
               gerrit.fetch_url("central"));
    assert_eq!("ssh://auto@gerrit-test-git:123/bsw/central.git",
               gerrit.remote_url("central"));
}

#[test]
fn test_gerrit_takes_topmost_central()
{
    let td = TempDir::new("gerrit_test").expect("folder gerrit_test should be created");
    let shell = Shell { cwd: td.path().to_path_buf() };
    shell.command("mkdir -p bin/gerrit.sh");

    shell.command("mkdir -p git/bsw/central.git");
    shell.command("mkdir -p git/bsw/bla/central.git");
    let gerrit = Gerrit::new(&td.path().join("git/bsw/bla/central.git"),
                             "central",
                             "auto",
                             "123");
    assert_eq!(vec!["bla/central", "central"], gerrit.projects());
}

#[test]
fn test_gerrit_sufix_stripping()
{
    let td = TempDir::new("gerrit_test").expect("folder gerrit_test should be created");
    let shell = Shell { cwd: td.path().to_path_buf() };
    shell.command("mkdir -p bin/gerrit.sh");

    shell.command("mkdir -p git/bsw/central.git");
    shell.command("mkdir -p git/bsw/module.git.git");
    let gerrit = Gerrit::new(&td.path().join("git/bsw/module.git.git"),
                             "central",
                             "auto",
                             "123");
    assert_eq!(vec!["central", "module.git"], sorted(gerrit.projects()));

    println!("{}", shell.command("tree"));
}
