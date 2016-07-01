
extern crate centralgithook;
use centralgithook::*;
#[allow(dead_code)]
mod helpers;
extern crate git2;
extern crate tempdir;
use tempdir::TempDir;

fn sorted(mut v: Vec<String>) -> Vec<String>
{
    v.sort();
    v
}

#[test]
fn test_find_all_subtrees()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let host = helpers::TestHost::new();
    let scratch = Scratch::new(&td.path().join("scratch"), &host);
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("foo");
    let head = scratch.transfer(&repo.commit("1"), &repo.path);
    let subdirs = scratch.find_all_subdirs(&head.as_commit().unwrap().tree().unwrap());
    assert_eq!(0, subdirs.len());

    repo.add_file("bla/foo");
    let head = scratch.transfer(&repo.commit("2"), &repo.path);
    let subdirs = scratch.find_all_subdirs(&head.as_commit().unwrap().tree().unwrap());
    assert_eq!(vec![
        format!("bla"),
    ], sorted(subdirs));

    repo.add_file("a/b/c/d/foo");
    let head = scratch.transfer(&repo.commit("2"), &repo.path);
    let subdirs = scratch.find_all_subdirs(&head.as_commit().unwrap().tree().unwrap());
    assert_eq!(vec![
        format!("a"),
        format!("a/b"),
        format!("a/b/c"),
        format!("a/b/c/d"),
        format!("bla"),
    ], sorted(subdirs));
}
