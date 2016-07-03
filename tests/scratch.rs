
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
    ],
               sorted(subdirs));

    repo.add_file("a/b/c/d/foo");
    let head = scratch.transfer(&repo.commit("2"), &repo.path);
    let subdirs = scratch.find_all_subdirs(&head.as_commit().unwrap().tree().unwrap());
    assert_eq!(vec![
        format!("a"),
        format!("a/b"),
        format!("a/b/c"),
        format!("a/b/c/d"),
        format!("bla"),
    ],
               sorted(subdirs));

    repo.add_file("a/b/c/.bla/foo");
    let head = scratch.transfer(&repo.commit("2"), &repo.path);
    let subdirs = scratch.find_all_subdirs(&head.as_commit().unwrap().tree().unwrap());
    assert_eq!(vec![
        format!("a"),
        format!("a/b"),
        format!("a/b/c"),
        format!("a/b/c/d"),
        format!("bla"),
    ],
               sorted(subdirs));
}

fn split_subdir_ref(repo: &helpers::TestRepo, module: &str, newrev: git2::Oid) -> Option<git2::Oid>
{
    repo.shell.command("rm -Rf refs/original");
    repo.shell.command("rm -Rf .git-rewrite");

    repo.repo.set_head_detached(newrev).expect("can't detatch head");;

    repo.shell.command(&format!("git filter-branch --subdirectory-filter {}/ -- HEAD",
                                module));

    return Some(repo.repo
        .revparse_single("HEAD")
        .expect("can't find rewritten branch")
        .id());
}


#[test]
fn test_split_subdir_one_commit()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let host = helpers::TestHost::new();
    let scratch = Scratch::new(&td.path().join("scratch"), &host);
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("foo/bla");
    let head = scratch.transfer(&repo.commit("1"), &repo.path);

    assert_eq!(split_subdir_ref(&repo, "foo", head.id()),
               scratch.split_subdir("foo", head.id()));
}

#[test]
fn test_split_subdir_two_commits()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let host = helpers::TestHost::new();
    let scratch = Scratch::new(&td.path().join("scratch"), &host);
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("foo/bla");
    let _ = scratch.transfer(&repo.commit("1"), &repo.path);
    repo.add_file("foo/bla_bla");
    let head = scratch.transfer(&repo.commit("1"), &repo.path);

    assert_eq!(split_subdir_ref(&repo, "foo", head.id()),
               scratch.split_subdir("foo", head.id()));
}

#[test]
fn test_split_subdir_three_commits_middle_unrelated()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let host = helpers::TestHost::new();
    let scratch = Scratch::new(&td.path().join("scratch"), &host);
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("foo/bla");
    let _ = scratch.transfer(&repo.commit("1"), &repo.path);
    repo.add_file("x");
    let _ = scratch.transfer(&repo.commit("1"), &repo.path);
    repo.add_file("foo/bla_bla");
    let head = scratch.transfer(&repo.commit("1"), &repo.path);

    assert_eq!(split_subdir_ref(&repo, "foo", head.id()),
               scratch.split_subdir("foo", head.id()));
}

#[test]
fn test_split_subdir_three_commits_first_unrelated()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let host = helpers::TestHost::new();
    let scratch = Scratch::new(&td.path().join("scratch"), &host);
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("x");
    let _ = scratch.transfer(&repo.commit("1"), &repo.path);
    repo.add_file("foo/bla");
    let _ = scratch.transfer(&repo.commit("1"), &repo.path);
    repo.add_file("foo/bla_bla");
    let head = scratch.transfer(&repo.commit("1"), &repo.path);

    assert_eq!(split_subdir_ref(&repo, "foo", head.id()),
               scratch.split_subdir("foo", head.id()));
}

#[test]
fn test_split_subdir_branch()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let host = helpers::TestHost::new();
    let scratch = Scratch::new(&td.path().join("scratch"), &host);
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("foo/bla");
    let _ = scratch.transfer(&repo.commit("foo_on_master"), &repo.path);
    repo.shell.command("git checkout -b tmp");
    repo.add_file("foo/bla_bla");
    let _ = scratch.transfer(&repo.commit("foo_on_tmp"), &repo.path);
    repo.shell.command("git checkout master");
    repo.shell.command("git merge tmp --no-ff -m foo_merge");

    let head = scratch.transfer(&repo.rev("HEAD"), &repo.path);

    let actual = scratch.split_subdir("foo", head.id());

    scratch.repo.reference("refs/heads/actual", actual.unwrap(), true, "x").expect("err 3");

    let shell = Shell { cwd: scratch.repo.path().to_path_buf() };

    // shell.command("gitk --all");
    assert_eq!(fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
               fparents(&repo.shell
                   .command("git log --pretty=format:%s-@%p --topo-order --grep=foo_ ")));
}

#[test]
fn test_split_subdir_branch_unrelated()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let host = helpers::TestHost::new();
    let scratch = Scratch::new(&td.path().join("scratch"), &host);
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("foo/bla");
    let _ = scratch.transfer(&repo.commit("foo_on_master"), &repo.path);
    repo.shell.command("git checkout -b tmp");
    repo.add_file("foo/bla_bla");
    let _ = scratch.transfer(&repo.commit("foo_on_tmp"), &repo.path);
    repo.add_file("x/bla_bla");
    let _ = scratch.transfer(&repo.commit("x_on_tmp"), &repo.path);
    repo.shell.command("git checkout master");
    repo.shell.command("git merge tmp --no-ff -m foo_merge");

    let head = scratch.transfer(&repo.rev("HEAD"), &repo.path);

    let actual = scratch.split_subdir("foo", head.id());

    scratch.repo.reference("refs/heads/actual", actual.unwrap(), true, "x").expect("err 3");

    let shell = Shell { cwd: scratch.repo.path().to_path_buf() };

    // shell.command("gitk --all");
    assert_eq!(fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
               fparents(&repo.shell
                   .command("git log --pretty=format:%s-@%p --topo-order --grep=foo_ ")));
}

#[test]
fn test_split_merge_identical_to_first()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let host = helpers::TestHost::new();
    let scratch = Scratch::new(&td.path().join("scratch"), &host);
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("foo/initial");
    let _ = scratch.transfer(&repo.commit("foo_initial_on_master"), &repo.path);

    repo.shell.command("git branch tmp");

    repo.shell.command("git checkout master");
    repo.add_file("foo/bla");
    let _ = scratch.transfer(&repo.commit("foo_second_on_master"), &repo.path);

    repo.shell.command("git checkout tmp");
    repo.add_file("foo/bla");
    repo.add_file("foo/bla_bla");
    let _ = scratch.transfer(&repo.commit("foo_second_on_tmp"), &repo.path);

    repo.shell.command("git checkout master");
    repo.shell.command("git merge tmp --no-ff -m foo_merge");

    println!("{}", repo.shell.command("git log"));
    let head = scratch.transfer(&repo.rev("HEAD"), &repo.path);

    let actual = scratch.split_subdir("foo", head.id());

    let shell = Shell { cwd: scratch.repo.path().to_path_buf() };
    scratch.repo.reference("refs/heads/actual", actual.unwrap(), true, "x").expect("err 2");

    // shell.command("gitk --all");
    assert_eq!(fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
               fparents(&repo.shell
                   .command("git log --pretty=format:%s-@%p --topo-order --grep=foo_ ")));
}

fn fparents(s: &str) -> String
{
    let mut o = String::new();
    for l in s.lines() {
        let spl = l.split("@");
        let m = spl.clone().nth(0).unwrap();
        let p = spl.clone().nth(1).unwrap();
        o.push_str(&format!("{}@{}\n",
                            m,
                            match p.len()/6 {
                                2 => "merge",
                                1 => "normal",
                                _ => "orphan",
                            }));
    }

    // println!("fparents:\n{}",o); assert!(false);

    return o;
}

#[test]
fn test_split_merge_identical_to_second()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let host = helpers::TestHost::new();
    let scratch = Scratch::new(&td.path().join("scratch"), &host);
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("foo/bla");
    let _ = scratch.transfer(&repo.commit("foo_on_master"), &repo.path);

    repo.shell.command("git checkout -b tmp");
    repo.add_file("foo/bla_bla");
    let _ = scratch.transfer(&repo.commit("foo_on_tmp"), &repo.path);

    repo.shell.command("git checkout master");
    repo.add_file("foo/bla_bla");
    let _ = scratch.transfer(&repo.commit("foo_on_master_2"), &repo.path);
    repo.shell.command("git merge tmp --no-ff -m foo_merge");
    println!("{}", repo.shell.command("git log"));
    let head = scratch.transfer(&repo.rev("HEAD"), &repo.path);

    let actual = scratch.split_subdir("foo", head.id());

    let shell = Shell { cwd: scratch.repo.path().to_path_buf() };
    scratch.repo.reference("refs/heads/actual", actual.unwrap(), true, "x").expect("err 2");

    // shell.command("gitk --all");
    assert_eq!(fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
               fparents(&repo.shell
                   .command("git log --pretty=format:%s-@%p --topo-order --grep=foo_ ")));
}
