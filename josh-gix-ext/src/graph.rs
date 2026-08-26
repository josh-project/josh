//! Reachability queries over an object source, so they see the same objects as the rest of
//! a transaction -- including the filtered commits it has not written out yet.
//!
//! Both queries are merge-base computations: `gix_revision` walks the commits it reads from
//! `objects`, keeping the answers consistent with what the caller can see.

/// A commit the walk cannot read is indistinguishable from unrelated history, so the inputs
/// are checked up front: asking about an object that is not there is a caller bug, not an
/// answer of "no".
fn ensure_commit(objects: &impl gix_object::Find, oid: gix_hash::ObjectId) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    let data = objects
        .try_find(&oid, &mut buf)
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
    commit: gix_hash::ObjectId,
    ancestor: gix_hash::ObjectId,
) -> anyhow::Result<bool> {
    if commit == ancestor {
        return Ok(false);
    }
    ensure_commit(objects, commit)?;
    ensure_commit(objects, ancestor)?;
    let ancestor = ancestor;
    let mut graph = gix_revision::Graph::new(objects, None);
    // Reachable is exactly "is one of the best common ancestors of the two".
    let bases = gix_revision::merge_base(commit, &[ancestor], &mut graph)
        .map_err(|e| anyhow::anyhow!("is_descendant_of: {e}"))?;
    Ok(bases.is_some_and(|bases| bases.iter().any(|base| *base == ancestor)))
}

/// The best common ancestor of `a` and `b`, erroring when they share no history. With
/// several equally good candidates the choice among them is arbitrary.
pub fn merge_base(
    objects: &impl gix_object::Find,
    a: gix_hash::ObjectId,
    b: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    ensure_commit(objects, a)?;
    ensure_commit(objects, b)?;
    let mut graph = gix_revision::Graph::new(objects, None);
    gix_revision::merge_base(a, &[b], &mut graph)
        .map_err(|e| anyhow::anyhow!("merge_base: {e}"))?
        .map(|bases| bases.first().to_owned())
        .ok_or_else(|| anyhow::anyhow!("{a} and {b} share no history"))
}

/// The best common ancestor of every commit in `commits`, or `None` when they do not all
/// share history.
pub fn merge_base_octopus(
    objects: &impl gix_object::Find,
    commits: &[gix_hash::ObjectId],
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    let (first, rest) = commits
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("merge_base_octopus: no commits"))?;
    for commit in commits {
        ensure_commit(objects, *commit)?;
    }
    let rest = rest.to_vec();
    let mut graph = gix_revision::Graph::new(objects, None);
    let base = gix_revision::merge_base::octopus(*first, &rest, &mut graph)
        .map_err(|e| anyhow::anyhow!("merge_base_octopus: {e}"))?;
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    struct TestRepo {
        _dir: tempfile::TempDir,
        repo: gix::Repository,
        tree: gix_hash::ObjectId,
    }

    impl TestRepo {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = gix::init_bare(dir.path()).unwrap();
            let tree = gix_object::Write::write(
                &repo.objects,
                &gix_object::Tree {
                    entries: Vec::new(),
                },
            )
            .unwrap();
            TestRepo {
                _dir: dir,
                repo,
                tree,
            }
        }

        /// Commits are dated by their index so the merge-base walk sees a plausible history;
        /// equal timestamps would still be correct, just slower to settle.
        fn commit(&self, seconds: i64, parents: &[gix_hash::ObjectId]) -> gix_hash::ObjectId {
            let signature = gix_actor::Signature {
                name: "Test".into(),
                email: "test@example.com".into(),
                time: gix_actor::date::Time { seconds, offset: 0 },
            };
            crate::write_commit(
                &self.repo.objects,
                self.tree,
                parents,
                &signature,
                &signature,
                "c",
            )
            .unwrap()
        }
    }

    #[test]
    fn descends_along_parent_edges_only() {
        let t = TestRepo::new();
        let root = t.commit(1000, &[]);
        let mid = t.commit(1001, &[root]);
        let tip = t.commit(1002, &[mid]);

        assert!(is_descendant_of(&t.repo.objects, tip, root).unwrap());
        assert!(is_descendant_of(&t.repo.objects, tip, mid).unwrap());
        assert!(!is_descendant_of(&t.repo.objects, root, tip).unwrap());
        assert!(!is_descendant_of(&t.repo.objects, tip, tip).unwrap());
    }

    #[test]
    fn merge_base_is_the_join_of_two_lineages() {
        let t = TestRepo::new();
        let root = t.commit(1000, &[]);
        let a = t.commit(1001, &[root]);
        let b = t.commit(1002, &[root]);

        assert_eq!(merge_base(&t.repo.objects, a, b).unwrap(), root);
        assert_eq!(merge_base(&t.repo.objects, a, root).unwrap(), root);
        assert_eq!(merge_base(&t.repo.objects, a, a).unwrap(), a);

        let unrelated = t.commit(1003, &[]);
        assert!(merge_base(&t.repo.objects, a, unrelated).is_err());
    }

    #[test]
    fn a_missing_commit_is_an_error() {
        let t = TestRepo::new();
        let root = t.commit(1000, &[]);
        let missing =
            gix_hash::ObjectId::from_str("1234567890123456789012345678901234567890").unwrap();

        assert!(is_descendant_of(&t.repo.objects, missing, root).is_err());
        assert!(merge_base_octopus(&t.repo.objects, &[root, missing]).is_err());

        assert!(merge_base_octopus(&t.repo.objects, &[]).is_err());
    }
}
