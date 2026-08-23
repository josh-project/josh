use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;
use josh_core::objects;

#[derive(Debug, Clone)]
pub struct Revision {
    pub commit_oid: String,
    pub author: String,
    pub timestamp: String,
}

pub fn read_revisions(
    transaction: &josh_core::cache::Transaction,
    change: &Change,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<Revision>> {
    let odb = transaction.odb();
    let change_id = match change.id() {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };

    let head = match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => oid,
        None => return Ok(Vec::new()),
    };

    // The change's diffs subtree, when the commit has one.
    let diffs_of = |tree: git2::Oid| -> Option<git2::Oid> {
        crate::store::get_tree(
            transaction,
            odb,
            tree,
            &std::path::Path::new("diffs").join(encode_change_id_path(change_id)),
        )
    };

    let mut revs: Vec<Revision> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut walk = objects::RevWalk::new(odb);
    walk.simplify_first_parent();
    walk.push(head)?;

    for oid in walk.into_topo_vec(|_| false)? {
        let commit = objects::CommitData::read(odb, oid)?;
        let Ok(cur_tree) = commit.tree_id() else {
            continue;
        };
        let cid_tree = match diffs_of(cur_tree) {
            Some(t) => t,
            None => continue,
        };
        let parent_cid_tree = commit
            .first_parent_id()
            .and_then(|p| josh_core::git::read_tree_id(odb, p).ok())
            .and_then(diffs_of);
        if parent_cid_tree == Some(cid_tree) {
            continue;
        }
        let Ok(parsed) = commit.parsed() else {
            continue;
        };
        // The subtree is content-addressed by the (commit, base) pair, so its
        // oid identifies the revision.
        if !seen.insert(cid_tree.to_string()) {
            continue;
        }
        revs.push(Revision {
            commit_oid: cid_tree.to_string(),
            author: parsed
                .author()
                .map(|a| String::from_utf8_lossy(a.email).into_owned())
                .unwrap_or_default(),
            timestamp: parsed
                .committer()
                .map(|t| t.seconds().to_string())
                .unwrap_or_default(),
        });
    }

    revs.reverse();
    Ok(revs)
}
