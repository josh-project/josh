//! Union helpers: read across `refs/josh/changes/*` + every
//! `refs/josh/remotes/*/changes/*` and merge results. The `_union` variants
//! merge across every discovered (scope, branch) combination; the
//! `_in_scopes` variants restrict the merge to a caller-supplied scope list
//! (typically the output of `refs_on_branch`). Callers that touch metadata
//! for many changes should resolve scopes once and reuse them.

use crate::change::{Change, list_changes};
use crate::comments::{Comment, read_comments};
use crate::forges::github::read_github_ids;
use crate::refs::{ChangesRef, all_changes_refs};
use crate::revisions::{Revision, read_revisions};
use crate::store::read_pr_data;
use crate::votes::{VoteData, list_votes, read_vote};

/// List changes across the local ref and every per-remote ref. Deduped by
/// change-id; if a change-id appears in multiple refs, the Local entry's
/// `tip`/`base` wins (it reflects the user's working tree).
pub fn list_all_changes(repo: &git2::Repository) -> anyhow::Result<Vec<Change>> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Change> = Vec::new();
    for scope in all_changes_refs(repo)? {
        for c in list_changes(repo, &scope)? {
            let id = match c.id() {
                Some(id) => id.to_string(),
                None => continue,
            };
            if seen.insert(id) {
                out.push(c);
            }
        }
    }
    Ok(out)
}

/// First `Some(_)` PR data found among the per-remote refs (Local is not
/// expected to hold PR data; if present, it is ignored).
pub fn read_pr_data_union(
    repo: &git2::Repository,
    change_id: &str,
) -> anyhow::Result<Option<String>> {
    for scope in all_changes_refs(repo)? {
        if matches!(scope, ChangesRef::Local { .. }) {
            continue;
        }
        if let Some(json) = read_pr_data(repo, change_id, &scope)? {
            return Ok(Some(json));
        }
    }
    Ok(None)
}

/// Concatenate comments from every ref. Dedupe by `Comment.id` (content hash);
/// on collision the Remote entry wins (it carries authoritative `gh_ids`).
pub fn read_comments_union(
    repo: &git2::Repository,
    change_id: &str,
) -> anyhow::Result<Vec<Comment>> {
    use std::collections::HashMap;
    // Insert Local first, then Remotes — Remotes overwrite on key collision.
    let mut by_id: HashMap<String, Comment> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for scope in all_changes_refs(repo)? {
        for c in read_comments(repo, change_id, &scope)? {
            if !by_id.contains_key(&c.id) {
                order.push(c.id.clone());
            }
            by_id.insert(c.id.clone(), c);
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect())
}

/// Prefer the Local vote for `user`; fall back to the first Remote that has one.
pub fn read_vote_union(
    repo: &git2::Repository,
    change_id: &str,
    user: Option<&str>,
) -> anyhow::Result<Option<VoteData>> {
    for scope in all_changes_refs(repo)? {
        if let Some(v) = read_vote(repo, change_id, user, &scope)? {
            return Ok(Some(v));
        }
    }
    Ok(None)
}

/// Concatenate votes across every ref. Dedupe `(user, sha, state)` triples;
/// Local entries are inserted first and therefore win on collision.
pub fn list_votes_union(
    repo: &git2::Repository,
    change_id: &str,
) -> anyhow::Result<Vec<(String, VoteData)>> {
    use std::collections::HashSet;
    let mut seen: HashSet<(String, String, String)> = HashSet::new();
    let mut out: Vec<(String, VoteData)> = Vec::new();
    for scope in all_changes_refs(repo)? {
        for (user, data) in list_votes(repo, change_id, &scope)? {
            let key = (user.clone(), data.sha.clone(), data.state.clone());
            if seen.insert(key) {
                out.push((user, data));
            }
        }
    }
    Ok(out)
}

/// Concatenate revisions from every ref, dedupe by `commit_oid`, sort by
/// timestamp ascending.
pub fn read_revisions_union(
    repo: &git2::Repository,
    change: &Change,
) -> anyhow::Result<Vec<Revision>> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Revision> = Vec::new();
    for scope in all_changes_refs(repo)? {
        for r in read_revisions(repo, change, &scope)? {
            if seen.insert(r.commit_oid.clone()) {
                out.push(r);
            }
        }
    }
    out.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    Ok(out)
}

/// Union of `gh_ids` maps across every per-remote ref. Collisions keep the
/// first entry encountered (in `all_changes_refs` order).
pub fn read_github_ids_union(
    repo: &git2::Repository,
    change_id: &str,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let mut out: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for scope in all_changes_refs(repo)? {
        for (k, v) in read_github_ids(repo, change_id, &scope)? {
            out.entry(k).or_insert(v);
        }
    }
    Ok(out)
}

pub fn list_changes_in_scopes(
    repo: &git2::Repository,
    scopes: &[ChangesRef],
) -> anyhow::Result<Vec<Change>> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Change> = Vec::new();
    for scope in scopes {
        for c in list_changes(repo, scope)? {
            let id = match c.id() {
                Some(id) => id.to_string(),
                None => continue,
            };
            if seen.insert(id) {
                out.push(c);
            }
        }
    }
    Ok(out)
}

pub fn read_pr_data_in_scopes(
    repo: &git2::Repository,
    change_id: &str,
    scopes: &[ChangesRef],
) -> anyhow::Result<Option<String>> {
    for scope in scopes {
        if matches!(scope, ChangesRef::Local { .. }) {
            continue;
        }
        if let Some(json) = read_pr_data(repo, change_id, scope)? {
            return Ok(Some(json));
        }
    }
    Ok(None)
}

pub fn read_comments_in_scopes(
    repo: &git2::Repository,
    change_id: &str,
    scopes: &[ChangesRef],
) -> anyhow::Result<Vec<Comment>> {
    use std::collections::HashMap;
    let mut by_id: HashMap<String, Comment> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for scope in scopes {
        for c in read_comments(repo, change_id, scope)? {
            if !by_id.contains_key(&c.id) {
                order.push(c.id.clone());
            }
            by_id.insert(c.id.clone(), c);
        }
    }
    Ok(order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect())
}

pub fn read_vote_in_scopes(
    repo: &git2::Repository,
    change_id: &str,
    user: Option<&str>,
    scopes: &[ChangesRef],
) -> anyhow::Result<Option<VoteData>> {
    for scope in scopes {
        if let Some(v) = read_vote(repo, change_id, user, scope)? {
            return Ok(Some(v));
        }
    }
    Ok(None)
}
