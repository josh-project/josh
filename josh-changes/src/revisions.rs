use crate::change::{Change, encode_change_id_path};
use crate::refs::ChangesRef;

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
    let repo = transaction.repo();
    let change_id = match change.id() {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };

    let head = match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => repo.find_commit(oid)?,
        None => return Ok(Vec::new()),
    };

    let mut revs: Vec<Revision> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut walk = repo.revwalk()?;
    walk.simplify_first_parent()?;
    walk.push(head.id())?;

    for oid in walk {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let tree = match commit.parent(0) {
            Ok(p) => (p.tree().ok(), commit.tree().ok()),
            Err(_) => (None, commit.tree().ok()),
        };
        let (parent_tree, cur_tree) = tree;
        let cur_tree = match cur_tree {
            Some(t) => t,
            None => continue,
        };

        let diffs_tree = match cur_tree
            .get_name("diffs")
            .and_then(|e| e.to_object(repo).ok())
            .and_then(|o| o.peel_to_tree().ok())
        {
            Some(t) => t,
            None => continue,
        };
        let cid_tree = match diffs_tree
            .get_name(&encode_change_id_path(change_id))
            .and_then(|e| e.to_object(repo).ok())
            .and_then(|o| o.peel_to_tree().ok())
        {
            Some(t) => t,
            None => continue,
        };

        let parent_cid_tree = parent_tree.as_ref().and_then(|pt| {
            let diffs = pt
                .get_name("diffs")?
                .to_object(repo)
                .ok()?
                .peel_to_tree()
                .ok()?;
            let cid = diffs
                .get_name(&encode_change_id_path(change_id))?
                .to_object(repo)
                .ok()?
                .peel_to_tree()
                .ok()?;
            Some(cid)
        });

        for entry in cid_tree.iter() {
            let commit_oid = entry.name().unwrap_or("").to_string();
            if commit_oid.is_empty() || seen.contains(&commit_oid) {
                continue;
            }
            let is_new = parent_cid_tree
                .as_ref()
                .and_then(|pt| pt.get_name(&commit_oid))
                .map_or(true, |e| e.id() != entry.id());
            if !is_new {
                continue;
            }
            let time = commit.time();
            seen.insert(commit_oid.clone());
            revs.push(Revision {
                commit_oid,
                author: commit.author().email().unwrap_or("").to_string(),
                timestamp: time.seconds().to_string(),
            });
        }
    }

    revs.reverse();
    Ok(revs)
}
