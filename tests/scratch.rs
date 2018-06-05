#[allow(dead_code)]
extern crate git2;
extern crate grib;
extern crate tempdir;
mod helpers;
use grib::*;
use std::path::Path;
use std::thread;
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
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo");
    let head = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    let subdirs = find_all_subdirs(&scratch, &head.as_commit().unwrap().tree().unwrap());
    assert_eq!(0, subdirs.len());

    repo.add_file("bla/foo");
    let head = scratch::transfer(&scratch, &repo.commit("2"), &repo.repo.path());
    let subdirs = find_all_subdirs(&scratch, &head.as_commit().unwrap().tree().unwrap());
    assert_eq!(vec![format!("bla")], sorted(subdirs));

    repo.add_file("a/b/c/d/foo");
    let head = scratch::transfer(&scratch, &repo.commit("2"), &repo.repo.path());
    let subdirs = find_all_subdirs(&scratch, &head.as_commit().unwrap().tree().unwrap());
    assert_eq!(
        vec![
            format!("a"),
            format!("a/b"),
            format!("a/b/c"),
            format!("a/b/c/d"),
            format!("bla"),
        ],
        sorted(subdirs)
    );

    repo.add_file("a/b/c/.bla/foo");
    let head = scratch::transfer(&scratch, &repo.commit("2"), &repo.repo.path());
    let subdirs = find_all_subdirs(&scratch, &head.as_commit().unwrap().tree().unwrap());
    assert_eq!(
        vec![
            format!("a"),
            format!("a/b"),
            format!("a/b/c"),
            format!("a/b/c/d"),
            format!("bla"),
        ],
        sorted(subdirs)
    );
}

fn split_subdir_ref(repo: &helpers::TestRepo, module: &str, newrev: git2::Oid)
    -> Option<git2::Oid>
{
    repo.shell.command("rm -Rf refs/original");
    repo.shell.command("rm -Rf .git-rewrite");

    repo.repo
        .set_head_detached(newrev)
        .expect("can't detatch head");;

    repo.shell
        .command(&format!("git filter-branch --subdirectory-filter {}/ -- HEAD", module));

    return Some(
        repo.repo
            .revparse_single("HEAD")
            .expect("can't find rewritten branch")
            .id(),
    );
}


#[test]
fn test_split_subdir_one_commit()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let head = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());

    assert_eq!(
        split_subdir_ref(&repo, "foo", head.id()),
        scratch::apply_view(&scratch, &SubdirView::new(&Path::new("foo")), head.id())
    );
}

#[test]
fn test_split_subdir_two_commits()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let _ = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("foo/bla_bla");
    let head = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());

    assert_eq!(
        split_subdir_ref(&repo, "foo", head.id()),
        scratch::apply_view(&scratch, &SubdirView::new(Path::new("foo")), head.id())
    );
}

#[test]
fn test_split_subdir_does_not_exist()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let _ = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("foo/bla_bla");
    // let head = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());

    // assert_eq!(split_subdir_ref(&repo, "bar", head.id()),
    //            scratch::apply_view(&scratch, "bar", head.id()));
}

#[test]
fn test_split_subdir_two_commits_first_empty()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.shell.command("git commit --allow-empty -m empty");
    repo.add_file("foo/bla_bla");
    let head = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.shell.command("git log");

    assert_eq!(
        split_subdir_ref(&repo, "foo", head.id()),
        scratch::apply_view(&scratch, &SubdirView::new(Path::new("foo")), head.id())
    );
    // assert!(false);
}

#[test]
fn test_split_subdir_three_commits_middle_unrelated()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let _ = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("x");
    let _ = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("foo/bla_bla");
    let head = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());

    assert_eq!(
        split_subdir_ref(&repo, "foo", head.id()),
        scratch::apply_view(&scratch, &SubdirView::new(Path::new("foo")), head.id())
    );
}

#[test]
fn test_split_subdir_three_commits_first_unrelated()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("x");
    let _ = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("foo/bla");
    let _ = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("foo/bla_bla");
    let head = scratch::transfer(&scratch, &repo.commit("1"), &repo.repo.path());

    assert_eq!(
        split_subdir_ref(&repo, "foo", head.id()),
        scratch::apply_view(&scratch, &SubdirView::new(Path::new("foo")), head.id())
    );
}

#[test]
fn test_split_subdir_branch()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let _ = scratch::transfer(&scratch, &repo.commit("foo_on_master"), &repo.repo.path());
    repo.shell.command("git checkout -b tmp");
    repo.add_file("foo/bla_bla");
    let _ = scratch::transfer(&scratch, &repo.commit("foo_on_tmp"), &repo.repo.path());
    repo.shell.command("git checkout master");
    repo.shell.command("git merge tmp --no-ff -m foo_merge");

    let head = scratch::transfer(&scratch, &repo.rev("HEAD"), &repo.repo.path());

    let actual = scratch::apply_view(&scratch, &SubdirView::new(Path::new("foo")), head.id());

    scratch
        .reference("refs/heads/actual", actual.unwrap(), true, "x")
        .expect("err 3");

    let shell = Shell {
        cwd: scratch.path().to_path_buf(),
    };

    // shell.command("gitk --all");
    assert_eq!(
        fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
        fparents(&repo.shell
            .command("git log --pretty=format:%s-@%p --topo-order --grep=foo_ "))
    );
}

#[test]
fn test_split_subdir_branch_unrelated()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let _ = scratch::transfer(&scratch, &repo.commit("foo_on_master"), &repo.repo.path());
    repo.shell.command("git checkout -b tmp");
    repo.add_file("foo/bla_bla");
    let _ = scratch::transfer(&scratch, &repo.commit("foo_on_tmp"), &repo.repo.path());
    repo.add_file("x/bla_bla");
    let _ = scratch::transfer(&scratch, &repo.commit("x_on_tmp"), &repo.repo.path());
    repo.shell.command("git checkout master");
    repo.shell.command("git merge tmp --no-ff -m foo_merge");

    let head = scratch::transfer(&scratch, &repo.rev("HEAD"), &repo.repo.path());

    let actual = scratch::apply_view(&scratch, &SubdirView::new(Path::new("foo")), head.id());

    scratch
        .reference("refs/heads/actual", actual.unwrap(), true, "x")
        .expect("err 3");

    let shell = Shell {
        cwd: scratch.path().to_path_buf(),
    };

    // shell.command("gitk --all");
    assert_eq!(
        fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
        fparents(&repo.shell
            .command("git log --pretty=format:%s-@%p --topo-order --grep=foo_ "))
    );
}

#[test]
fn test_split_merge_identical_to_first()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/initial");
    let _ = scratch::transfer(&scratch, &repo.commit("foo_initial_on_master"), &repo.repo.path());

    repo.shell.command("git branch tmp");

    repo.shell.command("git checkout master");
    repo.add_file("foo/bla");
    let _ = scratch::transfer(&scratch, &repo.commit("foo_second_on_master"), &repo.repo.path());

    repo.shell.command("git checkout tmp");
    repo.add_file("foo/bla");
    repo.add_file("foo/bla_bla");
    let _ = scratch::transfer(&scratch, &repo.commit("foo_second_on_tmp"), &repo.repo.path());

    repo.shell.command("git checkout master");
    repo.shell.command("git merge tmp --no-ff -m foo_merge");

    println!("{:?}", repo.shell.command("git log"));
    let head = scratch::transfer(&scratch, &repo.rev("HEAD"), &repo.repo.path());

    let actual = scratch::apply_view(&scratch, &SubdirView::new(Path::new("foo")), head.id());

    let shell = Shell {
        cwd: scratch.path().to_path_buf(),
    };
    scratch
        .reference("refs/heads/actual", actual.unwrap(), true, "x")
        .expect("err 2");

    // shell.command("gitk --all");
    assert_eq!(
        fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
        fparents(&repo.shell
            .command("git log --pretty=format:%s-@%p --topo-order --grep=foo_ "))
    );
}

fn fparents(ss: &(String, String)) -> String
{
    let (s, _) = ss.clone();
    let mut o = String::new();
    for l in s.lines() {
        let spl = l.split("@");
        let m = spl.clone().nth(0).unwrap();
        let p = spl.clone().nth(1).unwrap();
        o.push_str(&format!(
            "{}@{}\n",
            m,
            match p.len() / 6 {
                2 => "merge",
                1 => "normal",
                _ => "orphan",
            }
        ));
    }

    // println!("fparents:\n{}",o); assert!(false);

    return o;
}

#[test]
fn test_split_merge_identical_to_second()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let _ = scratch::transfer(&scratch, &repo.commit("foo_on_master"), &repo.repo.path());

    repo.shell.command("git checkout -b tmp");
    repo.add_file("foo/bla_bla");
    let _ = scratch::transfer(&scratch, &repo.commit("foo_on_tmp"), &repo.repo.path());

    repo.shell.command("git checkout master");
    repo.add_file("foo/bla_bla");
    let _ = scratch::transfer(&scratch, &repo.commit("foo_on_master_2"), &repo.repo.path());
    repo.shell.command("git merge tmp --no-ff -m foo_merge");
    println!("{:?}", repo.shell.command("git log"));
    let head = scratch::transfer(&scratch, &repo.rev("HEAD"), &repo.repo.path());

    let actual = scratch::apply_view(&scratch, &SubdirView::new(Path::new("foo")), head.id());

    let shell = Shell {
        cwd: scratch.path().to_path_buf(),
    };
    scratch
        .reference("refs/heads/actual", actual.unwrap(), true, "x")
        .expect("err 2");

    // shell.command("gitk --all");
    assert_eq!(
        fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
        fparents(&repo.shell
            .command("git log --pretty=format:%s-@%p --topo-order --grep=foo_ "))
    );
}

#[test]
fn test_join()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let central = helpers::TestRepo::new();
    let module = helpers::TestRepo::new();

    central.add_file("initial_in_central");
    let central_head =
        scratch::transfer(&scratch, &central.commit("central_initial"), &central.repo.path());

    module.add_file("initial_in_module");
    let module_head = scratch::transfer(&scratch, &module.commit("module_initial"), &module.repo.path());

    let shell = Shell {
        cwd: scratch.path().to_path_buf(),
    };
    let signature = git2::Signature::now("test", "test@test.com").unwrap();
    let result = scratch::join_to_subdir(
        &scratch,
        central_head.id(),
        &Path::new("foo"),
        module_head.id(),
        &signature,
    );
    scratch
        .reference("refs/heads/module", module_head.id(), true, "x")
        .expect("err 2");
    scratch
        .reference("refs/heads/central", central_head.id(), true, "x")
        .expect("err 2");
    scratch
        .reference("refs/heads/result", result, true, "x")
        .expect("err 2");
    scratch.reference("HEAD", result, true, "x").expect("err 2");


    assert_eq!(
        fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
        "\
join repo into \"foo\"-@merge
module_initial-@orphan
central_initial-@orphan\n"
    );
    // shell.command("xterm");
}

#[test]
fn test_join_more()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let central = helpers::TestRepo::new();
    let module = helpers::TestRepo::new();

    central.add_file("initial_in_central");
    let central_head =
        scratch::transfer(&scratch, &central.commit("central_initial"), &central.repo.path());

    module.add_file("initial_in_module");
    let _ = scratch::transfer(&scratch, &module.commit("module_initial"), &module.repo.path());
    module.add_file("some/more/in/module");
    let module_head = scratch::transfer(&scratch, &module.commit("module_more"), &module.repo.path());

    let shell = Shell {
        cwd: scratch.path().to_path_buf(),
    };
    let signature = git2::Signature::now("test", "test@test.com").unwrap();
    let result = scratch::join_to_subdir(
        &scratch,
        central_head.id(),
        &Path::new("foo/bar"),
        module_head.id(),
        &signature,
    );
    scratch
        .reference("refs/heads/module", module_head.id(), true, "x")
        .expect("err 2");
    scratch
        .reference("refs/heads/central", central_head.id(), true, "x")
        .expect("err 2");
    scratch
        .reference("refs/heads/result", result, true, "x")
        .expect("err 2");
    scratch.reference("HEAD", result, true, "x").expect("err 2");

    // shell.command("gitk --all");
    assert_eq!(
        fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order")),
        "\
join repo into \"foo/bar\"-@merge
module_more-@normal
module_initial-@orphan
central_initial-@orphan\n"
    );

    central
        .shell
        .command(&format!("git fetch {:?} result:result", scratch.path()));
    central.shell.command("git checkout result");

    assert!(central.has_file("foo/bar/initial_in_module"));
    let splitted =
        scratch::apply_view(&scratch, &SubdirView::new(Path::new("foo/bar")), result).unwrap();
    scratch
        .reference("refs/heads/splitted", splitted, true, "x")
        .expect("err 2");
    // shell.command("gitk --all");
    assert_eq!(module_head.id(), splitted);
}

#[test]
fn test_join_with_merge()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let central = helpers::TestRepo::new();
    let module = helpers::TestRepo::new();

    central.add_file("initial_in_central");
    let central_head =
        scratch::transfer(&scratch, &central.commit("central_initial"), &central.repo.path());

    module.add_file("initial_in_module");
    let _ = scratch::transfer(&scratch, &module.commit("module_initial"), &module.repo.path());

    module.shell.command("git branch tmp");

    module.shell.command("git checkout master");
    module.add_file("some/more/in/module_master");
    let _ = scratch::transfer(&scratch, &module.commit("module_more_on_master"), &module.repo.path());

    module.shell.command("git checkout tmp");
    module.add_file("some/stuff/in/module_tmp");
    let _ = scratch::transfer(&scratch, &module.commit("module_more_on_tmp"), &module.repo.path());

    module.shell.command("git checkout master");
    module.shell.command("git merge tmp --no-ff -m foo_merge");

    module.add_file("extra_file");
    let module_head =
        scratch::transfer(&scratch, &module.commit("module_after_merge"), &module.repo.path());

    let signature = git2::Signature::now("test", "test@test.com").unwrap();
    let result = scratch::join_to_subdir(
        &scratch,
        central_head.id(),
        &Path::new("foo"),
        module_head.id(),
        &signature,
    );
    scratch
        .reference("refs/heads/module", module_head.id(), true, "x")
        .expect("err 2");
    scratch
        .reference("refs/heads/central", central_head.id(), true, "x")
        .expect("err 2");
    scratch
        .reference("refs/heads/result", result, true, "x")
        .expect("err 2");
    scratch.reference("HEAD", result, true, "x").expect("err 2");

    let splitted =
        scratch::apply_view(&scratch, &SubdirView::new(Path::new("foo")), result).unwrap();
    scratch
        .reference("refs/heads/splitted", splitted, true, "x")
        .expect("err 2");
    assert_eq!(module_head.id(), splitted);
}

#[test]
fn test_replace_subtree()
{
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("a");
    let _ = scratch::transfer(&scratch, &repo.commit("initial"), &repo.repo.path());
    repo.shell.command("git branch tmp");
    repo.shell.command("git checkout master");

    repo.add_file("x/x");
    let master = scratch::transfer(&scratch, &repo.commit("initial"), &repo.repo.path());
    let mt = scratch.find_commit(master.id()).unwrap().tree().unwrap();

    repo.shell.command("git checkout tmp");
    repo.add_file("in_subtree");
    let tmp = scratch::transfer(&scratch, &repo.commit("tmp"), &repo.repo.path());
    let st = scratch.find_commit(tmp.id()).unwrap().tree().unwrap();

    let result = scratch
        .find_tree(replace_subtree(&scratch, Path::new("foo"), &st, &mt))
        .unwrap();

    let subdirs = find_all_subdirs(&scratch, &result);
    assert_eq!(vec![format!("foo"), format!("x")], sorted(subdirs));

    let result = scratch
        .find_tree(replace_subtree(&scratch, Path::new("foo/bla"), &st, &mt))
        .unwrap();

    let subdirs = find_all_subdirs(&scratch, &result);
    assert_eq!(vec![format!("foo"), format!("foo/bla"), format!("x")], sorted(subdirs));
}

#[test]
fn test_integration()
{
    let repo = helpers::TestRepo::new();
    let serve_path = repo.repo.path().to_owned();
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    thread::spawn(move || helpers::run_test_server(&serve_path, 8123));
    grib::run_proxy::run_proxy(vec![
        "grib".to_owned(),
        "--local".to_owned(), format!("{:?}", td.path()).trim_matches('"').to_owned(),
        "--remote".to_owned(), "http://localhost:8123".to_owned(),
    ]);

}
