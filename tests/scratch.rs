#[allow(dead_code)]
extern crate git2;
extern crate josh;
extern crate tempdir;
mod helpers;
use git2::*;
use josh::*;
use std::path::Path;
use tempdir::TempDir;

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

pub fn apply_view(repo: &Repository, view: &dyn View, newrev: Oid) -> Option<Oid> {
    return Some(apply_view_cached(
        &repo,
        view,
        newrev,
        &mut ViewMaps::new(),
        &mut ViewMap::new(),
    ));
}

const TMP_NAME: &'static str = "refs/centralgit/tmp_fd2db5f8_bac2_4a1e_9487_4ac3414788aa";

// force push of the new revision-object to temp repo
fn transfer<'a>(repo: &'a Repository, rev: &str, source: &Path) -> Object<'a> {
    // TODO: implement using libgit
    let target = &repo.path();
    let shell = Shell {
        cwd: source.to_path_buf(),
    };
    shell.command(&format!("git update-ref {} {}", TMP_NAME, rev));
    shell.command(&format!(
        "git push --force {} {}",
        &target.to_string_lossy(),
        TMP_NAME
    ));

    let obj = repo
        .revparse_single(rev)
        .expect("can't find transfered ref");
    return obj;
}

fn find_all_subdirs(repo: &Repository, tree: &Tree) -> Vec<String> {
    let mut sd = vec![];
    for item in tree {
        if let Ok(st) = repo.find_tree(item.id()) {
            let name = item.name().unwrap();
            if !name.starts_with(".") {
                sd.push(name.to_string());
                for r in find_all_subdirs(&repo, &st) {
                    sd.push(format!("{}/{}", name, r));
                }
            }
        }
    }
    return sd;
}

#[test]
fn test_find_all_subtrees() {
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo");
    let head = transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    let subdirs = find_all_subdirs(&scratch, &head.as_commit().unwrap().tree().unwrap());
    assert_eq!(0, subdirs.len());

    repo.add_file("bla/foo");
    let head = transfer(&scratch, &repo.commit("2"), &repo.repo.path());
    let subdirs = find_all_subdirs(&scratch, &head.as_commit().unwrap().tree().unwrap());
    assert_eq!(vec![format!("bla")], sorted(subdirs));

    repo.add_file("a/b/c/d/foo");
    let head = transfer(&scratch, &repo.commit("2"), &repo.repo.path());
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
    let head = transfer(&scratch, &repo.commit("2"), &repo.repo.path());
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

fn split_subdir_ref(
    repo: &helpers::TestRepo,
    module: &str,
    newrev: git2::Oid,
) -> Option<git2::Oid> {
    repo.shell.command("rm -Rf refs/original");
    repo.shell.command("rm -Rf .git-rewrite");

    repo.repo
        .set_head_detached(newrev)
        .expect("can't detatch head");;

    repo.shell.command(&format!(
        "git filter-branch --subdirectory-filter {}/ -- HEAD",
        module
    ));

    return Some(
        repo.repo
            .revparse_single("HEAD")
            .expect("can't find rewritten branch")
            .id(),
    );
}

#[test]
fn test_split_subdir_one_commit() {
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let head = transfer(&scratch, &repo.commit("1"), &repo.repo.path());

    assert_eq!(
        split_subdir_ref(&repo, "foo", head.id()),
        apply_view(&scratch, &*views::build_view("!/foo"), head.id())
    );
}

#[test]
fn test_split_subdir_two_commits() {
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let _ = transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("foo/bla_bla");
    let head = transfer(&scratch, &repo.commit("1"), &repo.repo.path());

    assert_eq!(
        split_subdir_ref(&repo, "foo", head.id()),
        apply_view(&scratch, &*views::build_view("!/foo"), head.id())
    );
}

#[test]
fn test_split_subdir_does_not_exist() {
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let _ = transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("foo/bla_bla");
    // let head = transfer(&scratch, &repo.commit("1"), &repo.repo.path());

    // assert_eq!(split_subdir_ref(&repo, "bar", head.id()),
    //            apply_view(&scratch, "bar", head.id()));
}

#[test]
fn test_split_subdir_two_commits_first_empty() {
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.shell.command("git commit --allow-empty -m empty");
    repo.add_file("foo/bla_bla");
    let head = transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.shell.command("git log");

    assert_eq!(
        split_subdir_ref(&repo, "foo", head.id()),
        apply_view(&scratch, &*views::build_view("!/foo"), head.id())
    );
    // assert!(false);
}

#[test]
fn test_split_subdir_three_commits_middle_unrelated() {
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let _ = transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("x");
    let _ = transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("foo/bla_bla");
    let head = transfer(&scratch, &repo.commit("1"), &repo.repo.path());

    assert_eq!(
        split_subdir_ref(&repo, "foo", head.id()),
        apply_view(&scratch, &*views::build_view("!/foo"), head.id())
    );
}

#[test]
fn test_split_subdir_three_commits_first_unrelated() {
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("x");
    let _ = transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("foo/bla");
    let _ = transfer(&scratch, &repo.commit("1"), &repo.repo.path());
    repo.add_file("foo/bla_bla");
    let head = transfer(&scratch, &repo.commit("1"), &repo.repo.path());

    assert_eq!(
        split_subdir_ref(&repo, "foo", head.id()),
        apply_view(&scratch, &*views::build_view("!/foo"), head.id())
    );
}

fn fparents(ss: &(String, String)) -> String {
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
fn test_split_merge_identical_to_second() {
    let td = TempDir::new("cgh_test").expect("folder cgh_test should be created");
    let scratch = scratch::new(&td.path().join("scratch"));
    let repo = helpers::TestRepo::new();

    repo.add_file("foo/bla");
    let _ = transfer(&scratch, &repo.commit("foo_on_master"), &repo.repo.path());

    repo.shell.command("git checkout -b tmp");
    repo.add_file("foo/bla_bla");
    let _ = transfer(&scratch, &repo.commit("foo_on_tmp"), &repo.repo.path());

    repo.shell.command("git checkout master");
    repo.add_file("foo/bla_bla");
    let _ = transfer(&scratch, &repo.commit("foo_on_master_2"), &repo.repo.path());
    repo.shell.command("git merge tmp --no-ff -m foo_merge");
    println!("{:?}", repo.shell.command("git log"));
    let head = transfer(&scratch, &repo.rev("HEAD"), &repo.repo.path());

    let actual = apply_view(&scratch, &*views::build_view("!/foo"), head.id());

    let shell = Shell {
        cwd: scratch.path().to_path_buf(),
    };
    scratch
        .reference("refs/heads/actual", actual.unwrap(), true, "x")
        .expect("err 2");

    // shell.command("gitk --all");
    assert_eq!(
        fparents(&shell.command("git log --pretty=format:%s-@%p --topo-order actual")),
        fparents(
            &repo
                .shell
                .command("git log --pretty=format:%s-@%p --topo-order --grep=foo_ ")
        )
    );
}
