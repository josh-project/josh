
extern crate centralgithook;
use centralgithook::*;
#[allow(dead_code)]
mod helpers;
extern crate git2;
extern crate tempdir;
use tempdir::TempDir;
use std::path::Path;

fn sorted(mut v: Vec<String>) -> Vec<String>
{
    v.sort();
    v
}

#[test]
fn test_find_all_subtrees()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = Scratch::new(&td.path().join("scratch"));
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
    let scratch = Scratch::new(&td.path().join("scratch"));
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
    let scratch = Scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("foo/bla");
    let _ = scratch.transfer(&repo.commit("1"), &repo.path);
    repo.add_file("foo/bla_bla");
    let head = scratch.transfer(&repo.commit("1"), &repo.path);

    assert_eq!(split_subdir_ref(&repo, "foo", head.id()),
               scratch.split_subdir("foo", head.id()));
}

#[test]
fn test_split_subdir_two_commits_first_empty()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = Scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.shell.command("git commit --allow-empty -m empty");
    repo.add_file("foo/bla_bla");
    let head = scratch.transfer(&repo.commit("1"), &repo.path);
    repo.shell.command("git log");

    assert_eq!(split_subdir_ref(&repo, "foo", head.id()),
               scratch.split_subdir("foo", head.id()));
    // assert!(false);
}

#[test]
fn test_split_subdir_three_commits_middle_unrelated()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = Scratch::new(&td.path().join("scratch"));
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
    let scratch = Scratch::new(&td.path().join("scratch"));
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
    let scratch = Scratch::new(&td.path().join("scratch"));
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
    let scratch = Scratch::new(&td.path().join("scratch"));
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
    let scratch = Scratch::new(&td.path().join("scratch"));
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
                            match p.len() / 6 {
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
    let scratch = Scratch::new(&td.path().join("scratch"));
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

#[test]
fn test_join()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = Scratch::new(&td.path().join("scratch"));
    let central = helpers::TestRepo::new(&td.path().join("central"));
    let module = helpers::TestRepo::new(&td.path().join("module"));

    central.add_file("initial_in_central");
    let central_head = scratch.transfer(&central.commit("central_initial"), &central.path);

    module.add_file("initial_in_module");
    let module_head = scratch.transfer(&module.commit("module_initial"), &module.path);

    let shell = Shell { cwd: scratch.repo.path().to_path_buf() };
    shell.command("git config user.name=test");
    shell.command("git config user.email=test@test.com");
    let result = scratch.join_to_subdir(central_head.id(), &Path::new("foo"), module_head.id());
    scratch.repo.reference("refs/heads/module", module_head.id(), true, "x").expect("err 2");
    scratch.repo.reference("refs/heads/central", central_head.id(), true, "x").expect("err 2");
    scratch.repo.reference("refs/heads/result", result, true, "x").expect("err 2");
    scratch.repo.reference("HEAD", result, true, "x").expect("err 2");


    assert_eq!(fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
               "\
join repo into \"foo\"-@merge
module_initial-@orphan
central_initial-@orphan\n");
    // shell.command("xterm");
}

#[test]
fn test_join_more()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = Scratch::new(&td.path().join("scratch"));
    let central = helpers::TestRepo::new(&td.path().join("central"));
    let module = helpers::TestRepo::new(&td.path().join("module"));

    central.add_file("initial_in_central");
    let central_head = scratch.transfer(&central.commit("central_initial"), &central.path);

    module.add_file("initial_in_module");
    let _ = scratch.transfer(&module.commit("module_initial"), &module.path);
    module.add_file("some/more/in/module");
    let module_head = scratch.transfer(&module.commit("module_more"), &module.path);

    let shell = Shell { cwd: scratch.repo.path().to_path_buf() };
    shell.command("git config user.name=test");
    shell.command("git config user.email=test@test.com");
    let result = scratch.join_to_subdir(central_head.id(), &Path::new("foo/bar"), module_head.id());
    scratch.repo.reference("refs/heads/module", module_head.id(), true, "x").expect("err 2");
    scratch.repo.reference("refs/heads/central", central_head.id(), true, "x").expect("err 2");
    scratch.repo.reference("refs/heads/result", result, true, "x").expect("err 2");
    scratch.repo.reference("HEAD", result, true, "x").expect("err 2");

    // shell.command("gitk --all");
    assert_eq!(fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
               "\
join repo into \"foo/bar\"-@merge
module_more-@normal
module_initial-@orphan
central_initial-@orphan\n");

    central.shell.command(&format!("git fetch {:?} result:result", scratch.repo.path()));
    central.shell.command("git checkout result");

    assert!(central.has_file("foo/bar/initial_in_module"));
    let splitted = scratch.split_subdir("foo/bar", result).unwrap();
    scratch.repo.reference("refs/heads/splitted", splitted, true, "x").expect("err 2");
    // shell.command("gitk --all");
    assert_eq!(module_head.id(), splitted);
}

#[test]
fn test_join_with_merge()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = Scratch::new(&td.path().join("scratch"));
    let central = helpers::TestRepo::new(&td.path().join("central"));
    let module = helpers::TestRepo::new(&td.path().join("module"));

    central.add_file("initial_in_central");
    let central_head = scratch.transfer(&central.commit("central_initial"), &central.path);

    module.add_file("initial_in_module");
    let _ = scratch.transfer(&module.commit("module_initial"), &module.path);

    module.shell.command("git branch tmp");

    module.shell.command("git checkout master");
    module.add_file("some/more/in/module_master");
    let _ = scratch.transfer(&module.commit("module_more_on_master"), &module.path);

    module.shell.command("git checkout tmp");
    module.add_file("some/stuff/in/module_tmp");
    let _ = scratch.transfer(&module.commit("module_more_on_tmp"), &module.path);

    module.shell.command("git checkout master");
    module.shell.command("git merge tmp --no-ff -m foo_merge");

    module.add_file("extra_file");
    let module_head = scratch.transfer(&module.commit("module_after_merge"), &module.path);

    let shell = Shell { cwd: scratch.repo.path().to_path_buf() };
    shell.command("git config user.name=test");
    shell.command("git config user.email=test@test.com");
    let result = scratch.join_to_subdir(central_head.id(), &Path::new("foo"), module_head.id());
    scratch.repo.reference("refs/heads/module", module_head.id(), true, "x").expect("err 2");
    scratch.repo.reference("refs/heads/central", central_head.id(), true, "x").expect("err 2");
    scratch.repo.reference("refs/heads/result", result, true, "x").expect("err 2");
    scratch.repo.reference("HEAD", result, true, "x").expect("err 2");

    // let shell = Shell { cwd: scratch.repo.path().to_path_buf() };
    let splitted = scratch.split_subdir("foo", result).unwrap();
    scratch.repo.reference("refs/heads/splitted", splitted, true, "x").expect("err 2");
    // shell.command("gitk --all");
    assert_eq!(module_head.id(), splitted);
}

#[test]
fn test_replace_subtree()
{

    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = Scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new(&td.path().join("repo"));

    repo.add_file("a");
    let _ = scratch.transfer(&repo.commit("initial"), &repo.path);
    repo.shell.command("git branch tmp");
    repo.shell.command("git checkout master");

    repo.add_file("x/x");
    let master = scratch.transfer(&repo.commit("initial"), &repo.path);
    let mt = scratch.repo.find_commit(master.id()).unwrap().tree().unwrap();

    repo.shell.command("git checkout tmp");
    repo.add_file("in_subtree");
    let tmp = scratch.transfer(&repo.commit("tmp"), &repo.path);
    let st = scratch.repo.find_commit(tmp.id()).unwrap().tree().unwrap();

    let result = scratch.replace_subtree(Path::new("foo"), st.id(), &mt);

    let subdirs = scratch.find_all_subdirs(&result);
    assert_eq!(vec![
        format!("foo"),
        format!("x"),
    ],
               sorted(subdirs));

    let result = scratch.replace_subtree(Path::new("foo/bla"), st.id(), &mt);

    let subdirs = scratch.find_all_subdirs(&result);
    assert_eq!(vec![
        format!("foo"),
        format!("foo/bla"),
        format!("x"),
    ],
               sorted(subdirs));

}
