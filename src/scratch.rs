extern crate crypto;
extern crate git2;

use super::build_view;
use super::UnapplyView;
use super::View;
use super::*;
use git2::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub type ViewMap = HashMap<Oid, Oid>;
pub type ViewMaps = HashMap<String, ViewMap>;

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
    backward_maps: Arc<Mutex<ViewMaps>>,
    viewobj: &View,
    old: Oid,
    new: Oid,
) -> UnapplyView {
    trace_scoped!(
        "unapply_view",
        "repo": repo.path(),
        "old": format!("{:?}", old),
        "new": format!("{:?}", new));
    debug!("unapply_view");

    if old == new {
        return UnapplyView::NoChanges;
    }

    let current = {
        let mut backward_maps = backward_maps.lock().unwrap();

        let mut bm = backward_maps
            .entry(viewobj.viewstr())
            .or_insert_with(ViewMap::new);

        *some_or!(bm.get(&old), {
            debug!("not in backward_map({},{})", viewobj.viewstr(), old);
            return UnapplyView::RejectNoFF;
        })
    };

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
        walk.push(new).expect("walk.push");
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
    forward_maps: &mut ViewMaps,
    backward_map: &mut ViewMap,
) {
    if let Ok(reference) = repo.find_reference(&from_refsname) {
        let r = reference.target().expect("no ref");

        let view_commit = apply_view_cached(&repo, &*viewobj, r, forward_maps, backward_map);
        if view_commit != git2::Oid::zero() {
            repo.reference(&to_refname, view_commit, true, "apply_view")
                .expect("can't create reference");
        }
    };
}

pub fn apply_view_to_branch(
    repo: &Repository,
    branchname: &str,
    viewobj: &dyn View,
    forward_maps: &mut ViewMaps,
    backward_map: &mut ViewMap,
    ns: &str,
) {
    trace_scoped!(
        "apply_view_to_branch",
        "repo": repo.path(),
        "branchname": branchname,
        "viewstr": viewobj.viewstr());

    let to_branch = format!("refs/namespaces/{}/refs/heads/{}", &ns, &branchname);
    let to_refs_for = format!("refs/namespaces/{}/refs/for/{}", &ns, &branchname);
    let to_refs_drafts = format!("refs/namespaces/{}/refs/drafts/{}", &ns, &branchname);
    let to_head = format!("refs/namespaces/{}/HEAD", &ns);
    let from_refsname = format!("refs/heads/{}", branchname);

    debug!("apply_view_to_branch {}", branchname);
    transform_commit(
        &repo,
        &*viewobj,
        &from_refsname,
        &to_branch,
        forward_maps,
        backward_map,
    );
    transform_commit(
        &repo,
        &*viewobj,
        &from_refsname,
        &to_refs_for,
        forward_maps,
        backward_map,
    );
    transform_commit(
        &repo,
        &*viewobj,
        &from_refsname,
        &to_refs_drafts,
        forward_maps,
        backward_map,
    );

    if branchname == "master" {
        transform_commit(
            &repo,
            &*viewobj,
            "refs/heads/master",
            &to_head,
            forward_maps,
            backward_map,
        );
    }
}

pub fn apply_view_cached(
    repo: &Repository,
    view: &dyn View,
    newrev: Oid,
    forward_maps: &mut ViewMaps,
    backward_map: &mut ViewMap,
) -> Oid {
    {
        let mut forward_map = forward_maps
            .entry(view.viewstr())
            .or_insert_with(ViewMap::new);
        if let Some(id) = forward_map.get(&newrev) {
            return *id;
        }
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

        {
            let mut forward_map = forward_maps
                .entry(view.viewstr())
                .or_insert_with(ViewMap::new);
            if forward_map.contains_key(&commit.id()) {
                continue 'walk;
            }
        }

        let (new_tree, transformed_parents_ids) =
            view.apply_to_commit(&repo, &commit, forward_maps);

        if new_tree == empty {
            empty_tree_count += 1;
            continue 'walk;
        }

        let mut transformed_parents = vec![];
        for parent_id in transformed_parents_ids {
            if let Ok(parent) = repo.find_commit(parent_id) {
                transformed_parents.push(parent);
            }
        }

        let transformed_parent_refs: Vec<&_> = transformed_parents.iter().collect();
        let mut filtered_transformed_parent_refs: Vec<&_> = vec![];

        for transformed_parent in transformed_parent_refs {
            if new_tree != transformed_parent.tree().unwrap().id() {
                filtered_transformed_parent_refs.push(transformed_parent);
                continue;
            }
            if commit.tree().expect("missing tree").id()
                == commit.parents().next().unwrap().tree().unwrap().id()
            {
                filtered_transformed_parent_refs.push(transformed_parent);
                continue;
            }
        }

        if filtered_transformed_parent_refs.len() == 0 && transformed_parents.len() != 0 {
            let mut forward_map = forward_maps
                .entry(view.viewstr())
                .or_insert_with(ViewMap::new);
            forward_map.insert(commit.id(), transformed_parents[0].id());
            continue 'walk;
        }

        let new_tree = repo
            .find_tree(new_tree)
            .expect("apply_view_cached: can't find tree");
        let transformed = rewrite(&repo, &commit, &filtered_transformed_parent_refs, &new_tree);
        {
            let mut forward_map = forward_maps
                .entry(view.viewstr())
                .or_insert_with(ViewMap::new);
            forward_map.insert(commit.id(), transformed);
        }
        backward_map.insert(transformed, commit.id());
        out_commit_count += 1;
    }

    trace_end!(
        &tname,
        "in_commit_count": in_commit_count,
        "out_commit_count": out_commit_count,
        "empty_tree_count": empty_tree_count
    );
    let mut forward_map = forward_maps
        .entry(view.viewstr())
        .or_insert_with(ViewMap::new);

    if let Some(id) = forward_map.get(&newrev).cloned() {
        return id;
    } else {
        forward_map.insert(newrev, Oid::zero());
        return Oid::zero();
    }
}
