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

// takes everything from base except it's tree and replaces it with the tree
// given
pub fn rewrite(repo: &Repository, base: &Commit, parents: &[&Commit], tree: &Tree) -> Oid
{
    let result = repo.commit(
        None,
        &base.author(),
        &base.committer(),
        &base.message().unwrap_or("no message"),
        tree,
        parents,
    ).expect("rewrite: can't commit {:?}");

    result
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

            let ns = view.replace("/","/refs/namespaces/");
            let refname = format!("refs/namespaces/{}/refs/heads/{}", &ns, &branchname);
            repo.reference(&refname, view_commit, true, "apply_view")
                .expect("can't create reference");

            if branchname == "master" {

            repo.reference(&format!("refs/namespaces/{}/HEAD", &ns), view_commit, true, "apply_view")
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

        let mut parents = vec![];
        for parent in commit.parents() {
            if let Some(parent) = view_cache.get(&parent.id()){
                let parent = repo.find_commit(*parent).unwrap();
                parents.push(parent);
            };
        }

        let parent_refs: Vec<&_> = parents.iter().collect();

        if let [only_parent] = parent_refs.as_slice() {
            if new_tree.id() == only_parent.tree().unwrap().id() {
                view_cache.insert(commit.id(), only_parent.id());
                continue 'walk;
            }
        }
        view_cache.insert(commit.id(), rewrite(&repo, &commit, &parent_refs, &new_tree));
    }

    return view_cache.get(&newrev).map(|&id| id);
}

pub fn join_to_subdir(
    repo: &Repository,
    path: &Path,
    src: Oid,
) -> (Oid, Oid)
{
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

        let mut parents = vec![];
        for parent in commit.parents() {
            let parent = map.get(&parent.id()).unwrap();
            let parent = repo.find_commit(*parent).unwrap();
            parents.push(parent);
        }

        let parent_refs: Vec<&_> = parents.iter().collect();
        map.insert(commit.id(), rewrite(&repo, &commit, &parent_refs, &new_tree));
    }

    let in_subdir = repo.find_commit(map[&src.id()]).unwrap();
    return (in_subdir.id(), in_subdir.id());
}
