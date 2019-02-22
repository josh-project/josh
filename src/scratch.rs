extern crate crypto;
extern crate git2;

use super::build_view;
use super::UnapplyView;
use super::View;
use super::*;
use git2::*;
use std::collections::HashMap;
use std::path::Path;

pub type ViewCache = HashMap<Oid, Oid>;
pub type ViewCaches = HashMap<String, ViewCache>;

use self::crypto::digest::Digest;
use self::crypto::sha1::Sha1;

fn all_equal(a: Parents, b: &[&Commit]) -> bool {
    let a: Vec<_> = a.collect();
    if a.len() != b.len() {
        return false;
    }

    for (x, y) in b.iter().zip(a.iter()) {
        if x.id() != y.id() {
            return false;
        }
    }
    return true;
}

// takes everything from base except it's tree and replaces it with the tree
// given
pub fn rewrite(repo: &Repository, base: &Commit, parents: &[&Commit], tree: &Tree) -> Oid {
    if base.tree().unwrap().id() == tree.id() && all_equal(base.parents(), parents) {
        // Looks like an optimization, but in fact serves to not change the commit in case
        // it was signed.
        return base.id();
    }

    let result = repo
        .commit(
            None,
            &base.author(),
            &base.committer(),
            &base.message().unwrap_or("no message"),
            tree,
            parents,
        )
        .expect("rewrite: can't commit {:?}");

    result
}

pub fn unapply_view(
    repo: &Repository,
    current: Oid,
    viewobj: &View,
    old: Oid,
    new: Oid,
) -> UnapplyView {
    trace_scoped!(
        "unapply_view",
        "repo": repo.path(),
        "current": format!("{:?}", current),
        "old": format!("{:?}", old),
        "new": format!("{:?}", new));

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
            .unwrap_or_else(|_| panic!("walk: invalid range: {}", range));;
        walk.hide(old).expect("walk: can't hide");
        walk
    };

    let mut current = current;
    for rev in walk {
        let rev = rev.expect("walk: invalid rev");

        debug!("==== walking commit {}", rev);

        let module_commit = repo
            .find_commit(rev)
            .expect("walk: object is not actually a commit");

        if module_commit.parents().count() > 1 {
            // TODO: invectigate the possibility of allowing merge commits
            return UnapplyView::RejectMerge;
        }

        debug!("==== Rewriting commit {}", rev);

        let tree = module_commit.tree().expect("walk: commit has no tree");
        let parent = repo
            .find_commit(current)
            .expect("walk: current object is no commit");

        let new_tree = viewobj.unapply(
            &repo,
            &tree,
            &parent.tree().expect("walk: parent has no tree"),
        );

        let new_tree = repo.find_tree(new_tree).expect("can't find rewritten tree");
        let check = viewobj.apply_to_tree(&repo, &new_tree);

        if check != tree.id() {
            println!("##### reverse transform mismatch");
            return UnapplyView::RejectMerge;
        }
        current = rewrite(&repo, &module_commit, &[&parent], &new_tree);
    }

    return UnapplyView::Done(current);
}

pub fn new(path: &Path) -> Repository {
    Repository::init_bare(&path).expect("could not init scratch")
}

fn transform_commit(
    repo: &Repository,
    viewobj: &View,
    from_refsname: &str,
    to_refname: &str,
    view_cache: &mut ViewCache,
) {
    if let Ok(reference) = repo.find_reference(&from_refsname) {
        let r = reference.target().expect("no ref");

        if let Some(view_commit) = apply_view_cached(&repo, &*viewobj, r, view_cache) {
            repo.reference(&to_refname, view_commit, true, "apply_view")
                .expect("can't create reference");
        }
    };
}

pub fn apply_view_to_branch(
    repo: &Repository,
    branchname: &str,
    viewobj: &dyn View,
    view_cache: &mut ViewCache,
    ns: &str,
) {
    trace_scoped!(
        "apply_view_to_branch",
        "repo": repo.path(),
        "branchname": branchname,
        "viewstr": viewobj.viewstr());

    let to_refname = format!("refs/namespaces/{}/refs/heads/{}", &ns, &branchname);
    let to_head = format!("refs/namespaces/{}/HEAD", &ns);
    let from_refsname = format!("refs/heads/{}", branchname);

    debug!("apply_view_to_branch {}", branchname);
    transform_commit(&repo, &*viewobj, &from_refsname, &to_refname, view_cache);

    if branchname == "master" {
        transform_commit(&repo, &*viewobj, "refs/heads/master", &to_head, view_cache);
    }
}

pub fn apply_view(repo: &Repository, view: &View, newrev: Oid) -> Option<Oid> {
    return apply_view_cached(&repo, view, newrev, &mut ViewCache::new());
}

pub fn apply_view_cached(
    repo: &Repository,
    view: &dyn View,
    newrev: Oid,
    view_cache: &mut ViewCache,
) -> Option<Oid> {
    if let Some(id) = view_cache.get(&newrev) {
        return Some(*id);
    }
    let tname = format!("apply_view_cached {:?}", newrev);
    trace_begin!(&tname, "viewstr": view.viewstr());

    let walk = {
        let mut walk = repo.revwalk().expect("walk: can't create revwalk");
        walk.set_sorting(Sort::REVERSE | Sort::TOPOLOGICAL);
        walk.push(newrev).expect("walk.push");
        walk
    };

    let empty = empty_tree(repo).id();

    let mut in_commit_count = 0;
    let mut out_commit_count = 0;
    let mut empty_tree_count = 0;
    'walk: for commit in walk {
        in_commit_count += 1;
        let commit = repo.find_commit(commit.unwrap()).unwrap();
        if view_cache.contains_key(&commit.id()) {
            continue 'walk;
        }

        let (new_tree, parent_transforms) = view.apply_to_commit(&repo, &commit);

        if new_tree == empty {
            empty_tree_count += 1;
            continue 'walk;
        }

        let mut transformed_parents = vec![];
        for (transform, parent_id) in parent_transforms {
            match transform {
                None => {
                    if let Some(parent) = apply_view_cached(&repo, view, parent_id, view_cache) {
                        let parent = repo.find_commit(parent).unwrap();
                        transformed_parents.push(parent);
                    }
                }
                Some(tview) => {
                    if let Some(parent) = apply_view(&repo, &*tview, parent_id) {
                        let parent = repo.find_commit(parent).unwrap();
                        transformed_parents.push(parent);
                    }
                }
            }
        }

        let transformed_parent_refs: Vec<&_> = transformed_parents.iter().collect();

        if transformed_parent_refs.len() == 0 && commit.parents().count() != 0 {}

        if let [only_parent] = transformed_parent_refs.as_slice() {
            if new_tree == only_parent.tree().unwrap().id() {
                if commit.tree().expect("missing tree").id()
                    != commit.parents().next().unwrap().tree().unwrap().id()
                {
                    view_cache.insert(commit.id(), only_parent.id());
                    continue 'walk;
                }
            }
        }

        let new_tree = repo
            .find_tree(new_tree)
            .expect("apply_view_cached: can't find tree");
        let transformed = rewrite(&repo, &commit, &transformed_parent_refs, &new_tree);
        view_cache.insert(commit.id(), transformed);
        out_commit_count += 1;
    }

    trace_end!(
        &tname,
        "in_commit_count": in_commit_count,
        "out_commit_count": out_commit_count,
        "empty_tree_count": empty_tree_count
    );
    return view_cache.get(&newrev).cloned();
}
