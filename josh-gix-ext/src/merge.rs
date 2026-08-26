//! Three-way merges over an object source, so the merge reads and writes the same objects as
//! the rest of a transaction -- including the trees it has not written out yet.
//!
//! josh merges bare trees: there is no worktree to read from, no attributes to consult and no
//! external merge drivers to run, so the platforms below are the empty configuration of each.

/// Which side wins a conflicting hunk.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Favor {
    Ours,
    Theirs,
}

/// The scratch state a merge needs. Cheap to build and not worth caching: the merges josh
/// runs are per-push, not per-commit.
struct Platforms {
    diff: gix_diff::blob::Platform,
    blob: gix_merge::blob::Platform,
}

fn attributes() -> gix_worktree::Stack {
    // No worktree root and no id mappings, so every attribute lookup misses and nothing
    // touches the filesystem.
    gix_worktree::Stack::new(
        std::path::PathBuf::new(),
        gix_worktree::stack::State::AttributesStack(gix_worktree::stack::state::Attributes::new(
            Default::default(),
            None,
            gix_worktree::stack::state::attributes::Source::IdMapping,
            Default::default(),
        )),
        gix_glob::pattern::Case::Sensitive,
        Vec::new(),
        Vec::new(),
    )
}

fn platforms() -> Platforms {
    let filter = || gix_filter::Pipeline::new(Default::default(), Default::default());
    Platforms {
        diff: gix_diff::blob::Platform::new(
            Default::default(),
            gix_diff::blob::Pipeline::new(
                Default::default(),
                filter(),
                Vec::new(),
                Default::default(),
            ),
            gix_diff::blob::pipeline::Mode::ToGit,
            attributes(),
        ),
        blob: gix_merge::blob::Platform::new(
            gix_merge::blob::Pipeline::new(Default::default(), filter(), Default::default()),
            gix_merge::blob::pipeline::Mode::ToGit,
            attributes(),
            Vec::new(),
            Default::default(),
        ),
    }
}

fn options(favor: Option<Favor>) -> gix_merge::tree::Options {
    use gix_merge::blob::builtin_driver::{binary, text};
    let (conflict, binary_conflict) = match favor {
        None => (
            text::Conflict::Keep {
                style: Default::default(),
                marker_size: text::Conflict::DEFAULT_MARKER_SIZE
                    .try_into()
                    .expect("non-zero"),
            },
            None,
        ),
        Some(Favor::Ours) => (
            text::Conflict::ResolveWithOurs,
            Some(binary::ResolveWith::Ours),
        ),
        Some(Favor::Theirs) => (
            text::Conflict::ResolveWithTheirs,
            Some(binary::ResolveWith::Theirs),
        ),
    };
    gix_merge::tree::Options {
        rewrites: Some(Default::default()),
        blob_merge: gix_merge::blob::platform::merge::Options {
            is_virtual_ancestor: false,
            resolve_binary_with: binary_conflict,
            text: text::Options {
                diff_algorithm: gix_diff::blob::Algorithm::Histogram,
                conflict,
            },
        },
        blob_merge_command_ctx: Default::default(),
        fail_on_conflict: None,
        marker_size_multiplier: 0,
        symlink_conflicts: None,
        tree_conflicts: None,
    }
}

/// Turn a merge result into the tree it produced, erroring on anything git would call a
/// conflict. A side preference resolves conflicting content, so what remains unresolved are
/// the conflicts no side preference can decide (a path deleted on one side and modified on
/// the other, say).
fn write_result(
    objects: &impl gix_object::Write,
    mut outcome: gix_merge::tree::Outcome<'_>,
    labels: (gix_hash::ObjectId, gix_hash::ObjectId),
) -> anyhow::Result<gix_hash::ObjectId> {
    let unresolved = gix_merge::tree::TreatAsUnresolved::default();
    if outcome.has_unresolved_conflicts(unresolved) {
        let paths: Vec<_> = outcome
            .conflicts
            .iter()
            .filter(|c| c.is_unresolved(unresolved))
            .map(|c| c.ours.location().to_string())
            .collect();
        return Err(anyhow::anyhow!(
            "merge of {} and {} conflicts in {}",
            labels.0,
            labels.1,
            paths.join(", ")
        ));
    }
    let id = outcome
        .tree
        .write(|tree| gix_object::Write::write(objects, tree))
        .map_err(|e| anyhow::anyhow!("writing merge result: {e}"))?;
    Ok(id)
}

/// Merge `ours` and `theirs` against their common ancestor `base`, all trees.
pub fn merge_trees(
    objects: &(impl gix_object::FindObjectOrHeader + gix_object::Write),
    base: gix_hash::ObjectId,
    ours: gix_hash::ObjectId,
    theirs: gix_hash::ObjectId,
) -> anyhow::Result<gix_hash::ObjectId> {
    let mut platforms = platforms();
    let outcome = gix_merge::tree(
        &base,
        &ours,
        &theirs,
        gix_merge::blob::builtin_driver::text::Labels::default(),
        objects,
        |buf| gix_object::Write::write_buf(objects, gix_object::Kind::Blob, buf),
        &mut gix_diff::tree::State::default(),
        &mut platforms.diff,
        &mut platforms.blob,
        options(None),
    )?;
    write_result(objects, outcome, (ours, theirs))
}

/// Merge two commits, finding their merge base the way git does -- several equally good bases
/// are merged into a virtual one first. A `favor` decides conflicting hunks, leaving only the
/// conflicts no side preference can resolve.
pub fn merge_commits(
    objects: &(impl gix_object::FindObjectOrHeader + gix_object::Write),
    ours: gix_hash::ObjectId,
    theirs: gix_hash::ObjectId,
    favor: Option<Favor>,
) -> anyhow::Result<gix_hash::ObjectId> {
    let mut platforms = platforms();
    let mut graph = gix_revwalk::Graph::new(objects, None);
    let outcome = gix_merge::commit(
        ours,
        theirs,
        gix_merge::blob::builtin_driver::text::Labels::default(),
        &mut graph,
        &mut platforms.diff,
        &mut platforms.blob,
        objects,
        &mut |id| id.to_string(),
        gix_merge::commit::Options {
            allow_missing_merge_base: true,
            use_first_merge_base: false,
            tree_merge: options(favor),
        },
    )?;
    write_result(objects, outcome.tree_merge, (ours, theirs))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRepo {
        _dir: tempfile::TempDir,
        repo: gix::Repository,
    }

    impl TestRepo {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = gix::init_bare(dir.path()).unwrap();
            TestRepo { _dir: dir, repo }
        }

        fn tree(&self, files: &[(&str, &str)]) -> gix_hash::ObjectId {
            let mut entries = files
                .iter()
                .map(|(name, content)| gix_object::tree::Entry {
                    mode: gix_object::tree::EntryKind::Blob.into(),
                    filename: (*name).into(),
                    oid: crate::write_blob(&self.repo.objects, content.as_bytes()).unwrap(),
                })
                .collect::<Vec<_>>();
            entries.sort();
            crate::write_tree_now(&self.repo.objects, entries).unwrap()
        }

        fn commit(
            &self,
            tree: gix_hash::ObjectId,
            parents: &[gix_hash::ObjectId],
        ) -> gix_hash::ObjectId {
            let signature = gix_actor::Signature {
                name: "Test".into(),
                email: "test@example.com".into(),
                time: gix_actor::date::Time {
                    seconds: 1000,
                    offset: 0,
                },
            };
            crate::write_commit(
                &self.repo.objects,
                tree,
                parents,
                &signature,
                &signature,
                "c",
            )
            .unwrap()
        }

        fn file(&self, tree: gix_hash::ObjectId, name: &str) -> String {
            let entry = crate::path_entry(&self.repo.objects, tree, std::path::Path::new(name))
                .unwrap()
                .unwrap_or_else(|| panic!("{name} not in tree"));
            crate::blob_text(&self.repo.objects, entry.oid)
        }
    }

    #[test]
    fn overlapping_edits_conflict() {
        let t = TestRepo::new();
        let base = t.tree(&[("a", "1\n")]);
        let ours = t.tree(&[("a", "ours\n")]);
        let theirs = t.tree(&[("a", "theirs\n")]);

        let err = merge_trees(&t.repo.objects, base, ours, theirs)
            .unwrap_err()
            .to_string();
        assert!(err.contains("conflicts in a"), "{err}");
    }

    #[test]
    fn a_favor_decides_conflicting_content() {
        let t = TestRepo::new();
        let base = t.commit(t.tree(&[("a", "1\n")]), &[]);
        let ours = t.commit(t.tree(&[("a", "ours\n")]), &[base]);
        let theirs = t.commit(t.tree(&[("a", "theirs\n")]), &[base]);

        let merged = merge_commits(&t.repo.objects, ours, theirs, Some(Favor::Ours)).unwrap();
        assert_eq!(t.file(merged, "a"), "ours\n");

        let merged = merge_commits(&t.repo.objects, ours, theirs, Some(Favor::Theirs)).unwrap();
        assert_eq!(t.file(merged, "a"), "theirs\n");
    }

    #[test]
    fn a_favor_cannot_decide_delete_against_modify() {
        let t = TestRepo::new();
        let base = t.commit(t.tree(&[("a", "1\n"), ("b", "1\n")]), &[]);
        let ours = t.commit(t.tree(&[("b", "1\n")]), &[base]);
        let theirs = t.commit(t.tree(&[("a", "modified\n"), ("b", "1\n")]), &[base]);

        let err = merge_commits(&t.repo.objects, ours, theirs, Some(Favor::Ours))
            .unwrap_err()
            .to_string();
        assert!(err.contains("conflicts in a"), "{err}");
    }
}
