extern crate crypto;
extern crate git2;
extern crate tracing;

use self::tracing::{event, span, Level};
use super::view_maps;
use super::views;
use super::UnapplyView;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, RwLock};

fn all_equal(a: git2::Parents, b: &[&git2::Commit]) -> bool {
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
pub fn rewrite(
    repo: &git2::Repository,
    base: &git2::Commit,
    parents: &[&git2::Commit],
    tree: &git2::Tree,
) -> git2::Oid {
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
            &base.message_raw().unwrap_or("no message"),
            tree,
            parents,
        )
        .expect("rewrite: can't commit {:?}");

    result
}

pub fn find_all_views(repo: &git2::Repository, refname: &str) -> HashSet<String> {
    let mut hs = HashSet::new();
    if let Ok(reference) = repo.revparse_single(&refname) {
        let tree = ok_or!(reference.peel_to_tree(), {
            debug!("find_all_views, not a tree: {}", &refname);
            return hs;
        });
        ok_or!(
            tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
                if entry.name() == Some(&"workspace.josh") {
                    hs.insert(format!(":workspace={}", root.trim_matches('/')));
                }
                if root == "" {
                    return 0;
                }
                let v = format!(":/{}", root.trim_matches('/'));
                if v.split("/").count() < 5 {
                    hs.insert(v);
                }

                0
            }),
            {
                return hs;
            }
        );
    }
    return hs;
}

pub fn unapply_view(
    repo: &git2::Repository,
    backward_maps: Arc<RwLock<view_maps::ViewMaps>>,
    viewobj: &dyn views::View,
    old: git2::Oid,
    new: git2::Oid,
) -> UnapplyView {
    let _trace_s = span!( Level::TRACE, "unapply_view", repo = ?repo.path(), ?old, ?new);
    debug!("unapply_view");

    debug!("==== walking commits from {} to {}", old, new);

    let walk = {
        let mut walk = repo.revwalk().expect("walk: can't create revwalk");
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL);
        walk.push(new).expect("walk.push");
        walk.hide(old).expect("walk: can't hide");
        walk
    };

    let mut bm = view_maps::ViewMaps::new_downstream(backward_maps.clone());
    let mut ret = git2::Oid::zero();
    for rev in walk {
        let rev = rev.expect("walk: invalid rev");

        debug!("==== walking commit {}", rev);

        let module_commit = repo
            .find_commit(rev)
            .expect("walk: object is not actually a commit");

        let original_parent_ids: Vec<_> = module_commit.parent_ids().collect();
        let original_parents: Vec<_> = original_parent_ids
            .iter()
            .map(|x| bm.get(&viewobj.viewstr(), *x))
            .map(|x| repo.find_commit(x).expect("id is no commit"))
            .collect();

        let original_parents_refs: Vec<&_> = original_parents.iter().collect();

        debug!("==== Rewriting commit {}", rev);

        let tree = module_commit.tree().expect("walk: commit has no tree");

        let new_trees: HashSet<_> = original_parents_refs
            .iter()
            .map(|x| viewobj.unapply(&repo, &tree, &x.tree().expect("walk: parent has no tree")))
            .collect();

        if new_trees.len() != 1 {
            // Arriving here means one of two things:
            // len == 0 -> somebody is trying to push unrelated history
            //             this could potentially be supported in the futrure
            //             as an "import" feature
            //
            // len > 1 -> This is a merge commit where the parents in the upstream repo
            //            have differences outside of the current view.
            //            It is unclear what base tree to pick in this case.
            debug!("rejecting merge");
            return UnapplyView::RejectMerge;
        }

        let new_tree = repo
            .find_tree(*new_trees.iter().next().unwrap())
            .expect("can't find rewritten tree");

        ret = rewrite(&repo, &module_commit, &original_parents_refs, &new_tree);
        bm.set(&viewobj.viewstr(), module_commit.id(), ret);
    }

    return UnapplyView::Done(ret);
}

pub fn new(path: &Path) -> git2::Repository {
    git2::Repository::init_bare(&path).expect("could not init scratch")
}

fn transform_commit(
    repo: &git2::Repository,
    viewobj: &dyn views::View,
    from_refsname: &str,
    to_refname: &str,
    forward_maps: &mut view_maps::ViewMaps,
    backward_maps: &mut view_maps::ViewMaps,
) {
    if let Ok(reference) = repo.revparse_single(&from_refsname) {
        let original_commit = ok_or!(reference.peel_to_commit(), {
            debug!("transform_commit, not a commit: {}", from_refsname);
            return;
        });
        let view_commit = viewobj.apply_view_to_commit(
            &repo,
            &original_commit,
            forward_maps,
            backward_maps,
            &mut HashMap::new(),
        );
        forward_maps.set(&viewobj.viewstr(), original_commit.id(), view_commit);
        backward_maps.set(&viewobj.viewstr(), view_commit, original_commit.id());
        if view_commit != git2::Oid::zero() {
            repo.reference(&to_refname, view_commit, true, "apply_view")
                .expect("can't create reference");
        }
    };
}

pub fn apply_view_to_refs(
    repo: &git2::Repository,
    viewobj: &dyn views::View,
    refs: &[(String, String)],
    forward_maps: &mut view_maps::ViewMaps,
    backward_maps: &mut view_maps::ViewMaps,
) {
    span!(
        Level::TRACE,
        "apply_view_to_refs",
        repo = ?repo.path(),
        ?refs,
        viewstr=?viewobj.viewstr());

    for (k, v) in refs {
        transform_commit(&repo, &*viewobj, &k, &v, forward_maps, backward_maps);
    }
}

pub fn apply_view_cached(
    repo: &git2::Repository,
    view: &dyn views::View,
    newrev: git2::Oid,
    forward_maps: &mut view_maps::ViewMaps,
    backward_maps: &mut view_maps::ViewMaps,
) -> git2::Oid {
    if forward_maps.has(repo, &view.viewstr(), newrev) {
        return forward_maps.get(&view.viewstr(), newrev);
    }

    let trace_s = span!(Level::TRACE, "apply_view_cached", viewstr = ?view.viewstr());

    let walk = {
        let mut walk = repo.revwalk().expect("walk: can't create revwalk");
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL);
        walk.push(newrev).expect("walk.push");
        walk
    };

    let mut in_commit_count = 0;
    let mut out_commit_count = 0;
    let mut empty_tree_count = 0;
    'walk: for commit in walk {
        in_commit_count += 1;

        let commit = repo.find_commit(commit.unwrap()).unwrap();

        let transformed = view.apply_view_to_commit(
            &repo,
            &commit,
            forward_maps,
            backward_maps,
            &mut HashMap::new(),
        );

        if transformed == git2::Oid::zero() {
            empty_tree_count += 1;
        }
        forward_maps.set(&view.viewstr(), commit.id(), transformed);
        backward_maps.set(&view.viewstr(), transformed, commit.id());
        out_commit_count += 1;
    }

    if !forward_maps.has(&repo, &view.viewstr(), newrev) {
        forward_maps.set(&view.viewstr(), newrev, git2::Oid::zero());
    }
    let rewritten = forward_maps.get(&view.viewstr(), newrev);
    event!(
        parent: &trace_s,
        Level::TRACE,
        ?in_commit_count,
        ?out_commit_count,
        ?empty_tree_count,
        original = ?newrev.to_string(),
        rewritten = ?rewritten.to_string(),
    );
    return rewritten;
}
