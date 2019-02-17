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

        if let Some(new_tree) = viewobj.unapply(
            &repo,
            &tree,
            &parent.tree().expect("walk: parent has no tree"),
        ) {
            current = rewrite(
                &repo,
                &module_commit,
                &[&parent],
                &repo.find_tree(new_tree).expect("can't find rewritten tree"),
            );
        };
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
    viewstr: &str,
    caches: &mut ViewCaches,
) {
    trace_scoped!("apply_view_to_branch", "repo": repo.path(), "branchname": branchname, "viewstr": viewstr);
    let mut view_cache = caches
        .entry(format!("{}--{}", &branchname, &viewstr))
        .or_insert_with(ViewCache::new);

    let ns = {
        let mut hasher = Sha1::new();
        hasher.input_str(&viewstr);
        hasher.result_str()
    };
    let to_refname = format!("refs/namespaces/{}/refs/heads/{}", &ns, &branchname);
    let to_head = format!("refs/namespaces/{}/HEAD", &ns);
    let from_refsname = format!("refs/heads/{}", branchname);

    let viewobj = build_view(&viewstr);

    debug!("apply_view_to_branch {}", branchname);
    transform_commit(
        &repo,
        &*viewobj,
        &from_refsname,
        &to_refname,
        &mut view_cache,
    );

    if branchname == "master" {
        transform_commit(
            &repo,
            &*viewobj,
            "refs/heads/master",
            &to_head,
            &mut view_cache,
        );
    }
}

pub fn apply_view(repo: &Repository, view: &View, newrev: Oid) -> Option<Oid> {
    return apply_view_cached(&repo, view, newrev, &mut ViewCache::new());
}

pub fn apply_view_cached(
    repo: &Repository,
    view: &View,
    newrev: Oid,
    view_cache: &mut ViewCache,
) -> Option<Oid> {
    let walk = {
        let mut walk = repo.revwalk().expect("walk: can't create revwalk");
        walk.set_sorting(Sort::REVERSE | Sort::TOPOLOGICAL);
        walk.push(newrev).expect("walk.push");
        walk
    };

    if let Some(id) = view_cache.get(&newrev) {
        return Some(*id);
    }

    let empty = empty_tree(repo).id();

    'walk: for commit in walk {
        let commit = repo.find_commit(commit.unwrap()).unwrap();
        if view_cache.contains_key(&commit.id()) {
            continue 'walk;
        }

        let full_tree = commit.tree().expect("commit has no tree");

        let new_tree = view.apply(&repo, &full_tree);
        if new_tree == empty {
            continue 'walk;
        }

        let mut transformed_parents = vec![];
        for parent in commit.parents() {
            if let Some(parent) = view_cache.get(&parent.id()) {
                let parent = repo.find_commit(*parent).unwrap();
                transformed_parents.push(parent);
            };
        }

        let transformed_parent_refs: Vec<&_> = transformed_parents.iter().collect();

        if transformed_parent_refs.len() == 0 && commit.parents().count() != 0 {}

        if let [only_parent] = transformed_parent_refs.as_slice() {
            if new_tree == only_parent.tree().unwrap().id() {
                if full_tree.id() != commit.parents().next().unwrap().tree().unwrap().id() {
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
    }

    return view_cache.get(&newrev).cloned();
}
