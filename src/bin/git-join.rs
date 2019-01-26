extern crate grib;
extern crate clap;
extern crate git2;
use grib::Shell;
use grib::scratch;
use git2::*;
use std::path::Path;
use grib::replace_subtree;
use std::collections::HashMap;

const TMP_NAME: &'static str = "refs/centralgit/tmp_fd2db5f8_bac2_4a1e_9487_4ac3414788aa";

// force push of the new revision-object to temp repo
fn transfer<'a>(repo: &'a Repository, rev: &str, source: &Path) -> Object<'a>
{
    // TODO: implement using libgit
    let target = &repo.path();
    let shell = Shell {
        cwd: source.to_path_buf(),
    };
    shell.command(&format!("git update-ref {} {}", TMP_NAME, rev));
    shell.command(&format!("git push --force {} {}", &target.to_string_lossy(), TMP_NAME));

    let obj = repo.revparse_single(rev)
        .expect("can't find transfered ref");
    return obj;
}

pub fn join_to_subdir(
    repo: &Repository,
    dst: Oid,
    path: &Path,
    src: Oid,
    signature: &Signature,
) -> Oid
{
    let dst = repo.find_commit(dst).unwrap();
    let src = repo.find_commit(src).unwrap();

    let walk = {
        let mut walk = repo.revwalk().expect("walk: can't create revwalk");
        walk.set_sorting(Sort::REVERSE | Sort::TOPOLOGICAL);
        walk.push(src.id()).expect("walk.push");
        walk
    };

    let empty = repo.find_tree(repo.treebuilder(None).unwrap().write().unwrap())
        .unwrap();
    let mut map = HashMap::<Oid, Oid>::new();

    'walk: for commit in walk {
        let commit = repo.find_commit(commit.unwrap()).unwrap();
        let tree = commit.tree().expect("commit has no tree");
        let new_tree = repo.find_tree(replace_subtree(&repo, path, &tree, &empty))
            .expect("can't find tree");

        match commit.parents().count() {
            2 => {
                let parent1 = commit.parents().nth(0).unwrap().id();
                let parent2 = commit.parents().nth(1).unwrap().id();
                if let (Some(&parent1), Some(&parent2)) = (map.get(&parent1), map.get(&parent2)) {
                    let parent1 = repo.find_commit(parent1).unwrap();
                    let parent2 = repo.find_commit(parent2).unwrap();

                    map.insert(
                        commit.id(),
                        scratch::rewrite(&repo, &commit, &[&parent1, &parent2], &new_tree),
                    );
                    continue 'walk;
                }
            }
            1 => {
                let parent = commit.parents().nth(0).unwrap().id();
                let parent = *map.get(&parent).unwrap();
                let parent = repo.find_commit(parent).unwrap();
                map.insert(commit.id(), scratch::rewrite(&repo, &commit, &[&parent], &new_tree));
                continue 'walk;
            }
            0 => {}
            _ => panic!("commit with {} parents: {}", commit.parents().count(), commit.id()),
        }

        map.insert(commit.id(), scratch::rewrite(&repo, &commit, &[], &new_tree));
    }

    let final_tree = repo.find_tree(
        replace_subtree(&repo, path, &src.tree().unwrap(), &dst.tree().unwrap()),
    ).expect("can't find tree");

    let parents = [&dst, &repo.find_commit(map[&src.id()]).unwrap()];
    repo.set_head_detached(parents[0].id())
        .expect("join: can't detach head");

    let join_commit = repo.commit(
        Some("HEAD"),
        signature,
        signature,
        &format!("join repo into {:?}", path),
        &final_tree,
        &parents,
    ).unwrap();
    return join_commit;
}

fn main()
{
    let args = clap::App::new("git-join")
        .arg(clap::Arg::with_name("source").long("source").takes_value(true))
        .arg(clap::Arg::with_name("branch").long("branch").takes_value(true))
        .arg(clap::Arg::with_name("subdir").long("subdir").takes_value(true))
        .get_matches();

    let branch = args.value_of("branch").expect("missing branch");
    let source = args.value_of("source").expect("missing source");
    let subdir = args.value_of("subdir").expect("missing subdir");

    let td = Path::new("/tmp/git-join2/");
    let scratch = scratch::new(&td.join("scratch"));
    let repo = git2::Repository::open(".").expect("can't open repo");
    let central_head = repo.revparse_single(branch).expect("can't find branch");
    let shell = Shell {
        cwd: scratch.path().to_path_buf(),
    };
    scratch
        .find_reference("refs/heads/join_source")
        .map(|mut r| {
            r.delete().ok();
        })
        .ok();
    shell.command(&format!("git fetch {} {}:join_source", source, branch));
    transfer(&scratch, &format!("{}", central_head.id()), &Path::new("."));
    let module_head = scratch
        .revparse_single("join_source")
        .expect("can'f find join_source");

    let signature = scratch.signature().unwrap();
    let result = join_to_subdir(
        &scratch,
        central_head.id(),
        &Path::new(subdir),
        module_head.id(),
        &signature,
    );

    scratch
        .find_reference("refs/heads/result")
        .map(|mut r| {
            r.delete().ok();
        })
        .ok();
    scratch
        .reference("refs/heads/join_result", result, true, "join")
        .ok();
    let shell = Shell {
        cwd: Path::new(".").to_path_buf(),
    };
    repo.find_reference("refs/heads/join")
        .map(|mut r| {
            r.delete().ok();
        })
        .ok();
    shell.command(&format!("git fetch {:?} join_result:join", scratch.path()));
}
