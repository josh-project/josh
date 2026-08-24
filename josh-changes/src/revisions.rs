use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;
use crate::store::DiffData;

/// One stored state of a change: the diff metadata plus the author and time
/// of the ref write that introduced it. Identity is the serialized `diff`'s
/// tree id (content-addressed, so identical diffs collapse).
#[derive(Debug, Clone)]
pub struct Revision {
    pub diff: DiffData,
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
    let diffs_of = |tree: gix_hash::ObjectId| -> Option<gix_hash::ObjectId> {
        crate::store::get_tree(
            transaction,
            odb,
            tree,
            &std::path::Path::new(crate::layout::DIFFS_PATH).join(encode_change_id_path(change_id)),
        )
    };

    let mut revs: Vec<Revision> = Vec::new();
    let mut seen: std::collections::HashSet<gix_hash::ObjectId> = std::collections::HashSet::new();

    for entry in crate::store::walk_ref_history(transaction, head)? {
        let Some(cid_tree) = diffs_of(entry.tree) else {
            continue;
        };
        if entry.parent_tree.and_then(diffs_of) == Some(cid_tree) {
            continue;
        }
        // The subtree is content-addressed by the (commit, base) pair, so its
        // oid identifies the revision.
        if !seen.insert(cid_tree) {
            continue;
        }
        let Ok(parsed) = entry.commit.parsed() else {
            continue;
        };
        // Advisory display data: an undecodable historical entry is skipped,
        // not fatal.
        let Ok(diff) = crate::store::deserialize_tree::<DiffData>(transaction, cid_tree) else {
            continue;
        };
        revs.push(Revision {
            diff,
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
