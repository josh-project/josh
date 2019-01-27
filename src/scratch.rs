extern crate git2;

use super::SubdirView;
use super::UnapplyView;
use super::View;
use super::replace_subtree;
use git2::*;
use shell::Shell;
use std::collections::HashMap;
use std::path::Path;

pub type ViewCache = HashMap<Oid, Oid>;
pub type ViewCaches = HashMap<String, ViewCache>;

pub fn view_ref_root(module: &str) -> String
{
    format!("refs/namespaces/{}/refs", module)
}

pub fn view_ref(module: &str, branch: &str) -> String
{
    format!("{}/heads/{}", view_ref_root(module), branch)
}

enum CommitKind
{
    Normal(Oid),
    Merge(Oid, Oid),
    Orphan,
}

// takes everything from base except it's tree and replaces it with the tree
// given
pub fn rewrite(repo: &Repository, base: &Commit, parents: &[&Commit], tree: &Tree) -> Oid
{
    if parents.len() == 0 {
        ::std::fs::remove_file(repo.path().join("HEAD")).expect("can't remove HEAD");
    } else {
        repo.set_head_detached(parents[0].id())
            .expect("rewrite: can't detach head");
    }
    repo.commit(
        Some("HEAD"),
        &base.author(),
        &base.committer(),
        &base.message().unwrap_or("no message"),
        tree,
        parents,
    ).expect("rewrite: can't commit")
}

pub fn unapply_view(repo: &Repository, current: Oid, view: &View, old: Oid, new: Oid)
    -> UnapplyView
{
    if old == new {
        return UnapplyView::NoChanges;
    }

    match repo.graph_descendant_of(new, old) {
        Err(_) | Ok(false) => {
            debug!("graph_descendant_of({},{})", new, old);
            return UnapplyView::RejectNoFF;
        }
        Ok(true) => (),
    }

    debug!("==== walking commits from {} to {}", old, new);

    let walk = {
        let mut walk = repo.revwalk().expect("walk: can't create revwalk");
        walk.set_sorting(Sort::REVERSE | Sort::TOPOLOGICAL);
        let range = format!("{}..{}", old, new);
        walk.push_range(&range)
            .expect(&format!("walk: invalid range: {}", range));;
        walk.hide(old).expect("walk: can't hide");
        walk
    };

    let mut current = current;
    for rev in walk {
        let rev = rev.expect("walk: invalid rev");

        debug!("==== walking commit {}", rev);

        let module_commit = repo.find_commit(rev)
            .expect("walk: object is not actually a commit");

        if module_commit.parents().count() > 1 {
            // TODO: invectigate the possibility of allowing merge commits
            return UnapplyView::RejectMerge;
        }

        debug!("==== Rewriting commit {}", rev);

        let tree = module_commit.tree().expect("walk: commit has no tree");
        let parent = repo.find_commit(current)
            .expect("walk: current object is no commit");

        let new_tree =
            view.unapply(&repo, &tree, &parent.tree().expect("walk: parent has no tree"));

        current = rewrite(
            &repo,
            &module_commit,
            &vec![&parent],
            &repo.find_tree(new_tree).expect("can't find rewritten tree"),
        );
    }
    return UnapplyView::Done(current);
}


pub fn new(path: &Path) -> Repository
{
    Repository::init_bare(&path).expect("could not init scratch")
}

pub fn apply_view_to_branch(
    repo: &Repository,
    branchname: &str,
    view: &str,
    caches: &mut ViewCaches,
)
{
    if view == "." {
        return;
    }

    let view_cache = caches
        .entry(format!("{}--{}", &branchname, &view))
        .or_insert(ViewCache::new());

    let viewobj = SubdirView::new(&Path::new(&view));

    debug!("apply_view_to_branch {}", branchname);
    if let Ok(branch) = repo.find_branch(branchname, git2::BranchType::Local) {
        let r = branch.into_reference().target().expect("no ref");

        if let Some(view_commit) = apply_view_cached(&repo, &viewobj, r, view_cache) {
            println!("applied view to branch {}", branchname);
            repo.reference(&view_ref(&view, &branchname), view_commit, true, "apply_view")
                .expect("can't create reference");

            if branchname == "master" {

            repo.reference(&format!("refs/namespaces/{}/HEAD", &view), view_commit, true, "apply_view")
                .expect("can't create reference");
            }
        } else {
            println!("can't apply view to branch {}", branchname);
        };
    };
}




pub fn apply_view(repo: &Repository, view: &View, newrev: Oid) -> Option<Oid>
{
    return apply_view_cached(&repo, view, newrev, &mut ViewCache::new());
}

pub fn apply_view_cached(
    repo: &Repository,
    view: &View,
    newrev: Oid,
    view_cache: &mut ViewCache,
) -> Option<Oid>
{
    let walk = {
        let mut walk = repo.revwalk().expect("walk: can't create revwalk");
        walk.set_sorting(Sort::REVERSE | Sort::TOPOLOGICAL);
        walk.push(newrev).expect("walk.push");
        walk
    };


    if let Some(id) = view_cache.get(&newrev) {
        return Some(id.clone());
    }


    'walk: for commit in walk {
        let commit = repo.find_commit(commit.unwrap()).unwrap();
        if view_cache.contains_key(&commit.id()) {
            continue 'walk;
        }

        let tree = commit.tree().expect("commit has no tree");

        let new_tree = if let Some(tree_id) = view.apply(&tree) {
            repo.find_tree(tree_id)
                .expect("central_submit: can't find tree")
        } else {
            continue 'walk;
        };

        match match commit.parents().count() {
            2 => {
                let parent1 = commit.parents().nth(0).unwrap().id();
                let parent2 = commit.parents().nth(1).unwrap().id();
                match (view_cache.get(&parent1), view_cache.get(&parent2)) {
                    (Some(&parent1), Some(&parent2)) => CommitKind::Merge(parent1, parent2),
                    (Some(&parent), None) => CommitKind::Normal(parent),
                    (None, Some(&parent)) => CommitKind::Normal(parent),
                    _ => CommitKind::Orphan,
                }
            }
            1 => {
                let parent = commit.parents().nth(0).unwrap().id();
                match view_cache.get(&parent) {
                    Some(&parent) => CommitKind::Normal(parent),
                    _ => CommitKind::Orphan,
                }
            }
            0 => CommitKind::Orphan,
            _ => panic!("commit with {} parents: {}", commit.parents().count(), commit.id()),
        } {
            CommitKind::Merge(parent1, parent2) => {
                let parent1 = repo.find_commit(parent1).unwrap();
                let parent2 = repo.find_commit(parent2).unwrap();
                view_cache
                    .insert(commit.id(), rewrite(&repo, &commit, &[&parent1, &parent2], &new_tree));
            }
            CommitKind::Normal(parent) => {
                let parent = repo.find_commit(parent).unwrap();
                if new_tree.id() == parent.tree().unwrap().id() {
                    view_cache.insert(commit.id(), parent.id());
                } else {
                    view_cache.insert(commit.id(), rewrite(&repo, &commit, &[&parent], &new_tree));
                }
            }
            CommitKind::Orphan => {
                view_cache.insert(commit.id(), rewrite(&repo, &commit, &[], &new_tree));
            }
        }
    }

    return view_cache.get(&newrev).map(|&id| id);
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
                        rewrite(&repo, &commit, &[&parent1, &parent2], &new_tree),
                    );
                    continue 'walk;
                }
            }
            1 => {
                let parent = commit.parents().nth(0).unwrap().id();
                let parent = *map.get(&parent).unwrap();
                let parent = repo.find_commit(parent).unwrap();
                map.insert(commit.id(), rewrite(&repo, &commit, &[&parent], &new_tree));
                continue 'walk;
            }
            0 => {}
            _ => panic!("commit with {} parents: {}", commit.parents().count(), commit.id()),
        }

        map.insert(commit.id(), rewrite(&repo, &commit, &[], &new_tree));
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
