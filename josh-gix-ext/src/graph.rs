//! Reachability queries over an object source, so they see the same objects as the rest of
//! a transaction -- including the filtered commits it has not written out yet.
//!
//! Both queries are merge-base computations: `gix_revision` walks the commits it reads from
//! `objects`, keeping the answers consistent with what the caller can see.

use crate::{git2_oid, gix_oid};

/// A commit the walk cannot read is indistinguishable from unrelated history, so the inputs
/// are checked up front: asking about an object that is not there is a caller bug, not an
/// answer of "no".
fn ensure_commit(objects: &impl gix_object::Find, oid: git2::Oid) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    let data = objects
        .try_find(&gix_oid(oid), &mut buf)
        .map_err(|e| anyhow::anyhow!("{oid}: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("object {oid} not found"))?;
    if data.kind != gix_object::Kind::Commit {
        return Err(anyhow::anyhow!(
            "object {} is not a commit but a {:?}",
            oid,
            data.kind
        ));
    }
    Ok(())
}

/// Whether `ancestor` is reachable from `commit` by following parents. A commit does not
/// descend from itself.
pub fn is_descendant_of(
    objects: &impl gix_object::Find,
    commit: git2::Oid,
    ancestor: git2::Oid,
) -> anyhow::Result<bool> {
    if commit == ancestor {
        return Ok(false);
    }
    ensure_commit(objects, commit)?;
    ensure_commit(objects, ancestor)?;
    let ancestor = gix_oid(ancestor);
    let mut graph = gix_revision::Graph::new(objects, None);
    // Reachable is exactly "is one of the best common ancestors of the two".
    let bases = gix_revision::merge_base(gix_oid(commit), &[ancestor], &mut graph)
        .map_err(|e| anyhow::anyhow!("is_descendant_of: {e}"))?;
    Ok(bases.is_some_and(|bases| bases.iter().any(|base| *base == ancestor)))
}

/// The best common ancestor of every commit in `commits`, or `None` when they do not all
/// share history.
pub fn merge_base_octopus(
    objects: &impl gix_object::Find,
    commits: &[git2::Oid],
) -> anyhow::Result<Option<git2::Oid>> {
    let (first, rest) = commits
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("merge_base_octopus: no commits"))?;
    for commit in commits {
        ensure_commit(objects, *commit)?;
    }
    let rest: Vec<_> = rest.iter().map(|c| gix_oid(*c)).collect();
    let mut graph = gix_revision::Graph::new(objects, None);
    let base = gix_revision::merge_base::octopus(gix_oid(*first), &rest, &mut graph)
        .map_err(|e| anyhow::anyhow!("merge_base_octopus: {e}"))?;
    Ok(base.map(|id| git2_oid(&id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRepo {
        _dir: tempfile::TempDir,
        repo: git2::Repository,
    }

    impl TestRepo {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = git2::Repository::init_bare(dir.path()).unwrap();
            TestRepo { _dir: dir, repo }
        }

        /// Commits are dated by their index so the merge-base walk sees a plausible history;
        /// equal timestamps would still be correct, just slower to settle.
        fn commit(&self, seconds: i64, parents: &[git2::Oid]) -> git2::Oid {
            let sig =
                git2::Signature::new("Test", "test@example.com", &git2::Time::new(seconds, 0))
                    .unwrap();
            let tree_id = self.repo.treebuilder(None).unwrap().write().unwrap();
            let tree = self.repo.find_tree(tree_id).unwrap();
            let parents: Vec<_> = parents
                .iter()
                .map(|&p| self.repo.find_commit(p).unwrap())
                .collect();
            let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
            self.repo
                .commit(None, &sig, &sig, "c", &tree, &parent_refs)
                .unwrap()
        }
    }

    #[test]
    fn descends_along_parent_edges_only() {
        let t = TestRepo::new();
        let root = t.commit(1000, &[]);
        let mid = t.commit(1001, &[root]);
        let tip = t.commit(1002, &[mid]);
        let odb = t.repo.odb().unwrap();
        let objects = crate::Git2Odb(&odb);

        assert!(is_descendant_of(&objects, tip, root).unwrap());
        assert!(is_descendant_of(&objects, tip, mid).unwrap());
        assert!(!is_descendant_of(&objects, root, tip).unwrap());
        assert!(!is_descendant_of(&objects, tip, tip).unwrap());
    }

    #[test]
    fn merge_descends_from_both_sides_but_the_sides_do_not() {
        let t = TestRepo::new();
        let root = t.commit(1000, &[]);
        let left = t.commit(1001, &[root]);
        let right = t.commit(1002, &[root]);
        let merge = t.commit(1003, &[left, right]);
        let odb = t.repo.odb().unwrap();
        let objects = crate::Git2Odb(&odb);

        assert!(is_descendant_of(&objects, merge, left).unwrap());
        assert!(is_descendant_of(&objects, merge, right).unwrap());
        assert!(!is_descendant_of(&objects, left, right).unwrap());
        assert!(!is_descendant_of(&objects, right, left).unwrap());
    }

    #[test]
    fn unrelated_histories_never_descend() {
        let t = TestRepo::new();
        let a = t.commit(1000, &[]);
        let b = t.commit(1001, &[]);
        let odb = t.repo.odb().unwrap();
        let objects = crate::Git2Odb(&odb);

        assert!(!is_descendant_of(&objects, a, b).unwrap());
        assert!(!is_descendant_of(&objects, b, a).unwrap());
        assert_eq!(merge_base_octopus(&objects, &[a, b]).unwrap(), None);
    }

    #[test]
    fn octopus_base_is_shared_by_every_input() {
        let t = TestRepo::new();
        let root = t.commit(1000, &[]);
        let a = t.commit(1001, &[root]);
        let b = t.commit(1002, &[root]);
        let c = t.commit(1003, &[a]);
        let odb = t.repo.odb().unwrap();
        let objects = crate::Git2Odb(&odb);

        assert_eq!(
            merge_base_octopus(&objects, &[c, b]).unwrap(),
            Some(root),
            "a fork joins at the root"
        );
        assert_eq!(
            merge_base_octopus(&objects, &[c, a]).unwrap(),
            Some(a),
            "an ancestor is its own best base"
        );
        assert_eq!(merge_base_octopus(&objects, &[c]).unwrap(), Some(c));

        let unrelated = t.commit(1004, &[]);
        assert_eq!(
            merge_base_octopus(&objects, &[c, b, unrelated]).unwrap(),
            None
        );
    }

    #[test]
    fn a_missing_commit_is_an_error() {
        let t = TestRepo::new();
        let root = t.commit(1000, &[]);
        let odb = t.repo.odb().unwrap();
        let objects = crate::Git2Odb(&odb);
        let missing = git2::Oid::from_str("1234567890123456789012345678901234567890").unwrap();

        assert!(is_descendant_of(&objects, missing, root).is_err());
        assert!(merge_base_octopus(&objects, &[root, missing]).is_err());
    }
}
