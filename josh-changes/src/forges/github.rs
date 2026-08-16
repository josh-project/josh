use crate::change::{Change, encode_change_id_path, get_changes, split_changes};
use crate::refs::{ChangesRef, StackedChangeRef, StackedRef};
use crate::store::{get_tree, write_changes_tree};
use crate::votes::VoteData;
use anyhow::anyhow;
use josh_core::cache::Transaction;

/// Create a real merge commit that has the target branch tip and PR head as its two parents.
/// The tree is the PR head's tree (no content merge needed).
/// Author and committer are copied from the PR head commit.
pub fn create_synthetic_merge_commit(
    transaction: &Transaction,
    pr_head: git2::Oid,
    target_branch_tip: git2::Oid,
    message: &str,
) -> anyhow::Result<git2::Oid> {
    let repo = transaction.repo();
    let pr_head = repo.find_commit(pr_head)?;
    let target_branch_tip = repo.find_commit(target_branch_tip)?;
    let tree = pr_head.tree()?;
    let author = pr_head.author();
    let committer = pr_head.committer();

    let oid = repo.commit(
        None,
        &author,
        &committer,
        message,
        &tree,
        &[&target_branch_tip, &pr_head],
    )?;

    Ok(oid)
}

#[derive(PartialEq, Clone, Debug)]
pub enum PushMode {
    Normal,
    Publish(String),
}

#[derive(Debug, Clone)]
pub struct PushRef {
    pub ref_name: String,
    pub oid: git2::Oid,
    pub change_id: String,
}

pub fn baseref_and_options(
    refname: &str,
    author: &str,
) -> anyhow::Result<(String, String, Vec<String>, PushMode)> {
    let mut split = refname.splitn(2, '%');
    let push_to = split.next().ok_or(anyhow!("no next"))?.to_owned();

    let options = if let Some(options) = split.next() {
        options.split(',').map(|x| x.to_string()).collect()
    } else {
        vec![]
    };

    let mut baseref = push_to.to_owned();
    let mut push_mode = PushMode::Normal;

    if baseref.starts_with("refs/for") {
        baseref = baseref.replacen("refs/for", "refs/heads", 1)
    }
    if baseref.starts_with("refs/drafts") {
        baseref = baseref.replacen("refs/drafts", "refs/heads", 1)
    }
    if baseref.starts_with("refs/publish/for") {
        push_mode = PushMode::Publish(author.to_string());
        baseref = baseref.replacen("refs/publish/for", "refs/heads", 1)
    }
    Ok((baseref, push_to, options, push_mode))
}

pub(crate) fn changes_to_refs(
    transaction: &Transaction,
    baseref: &str,
    change_author: &str,
    changes: Vec<Change>,
) -> anyhow::Result<Vec<PushRef>> {
    if !change_author.contains('@') {
        return Err(anyhow!(
            "Push option 'author' needs to be set to a valid email address",
        ));
    };

    let changes: Vec<Change> = changes
        .into_iter()
        .filter(|change| change.author == change_author)
        .collect();

    let mut seen = std::collections::HashSet::new();
    for change in changes.iter() {
        if let Some(id) = &change.id {
            if id.contains('@') {
                return Err(anyhow!("Change id must not contain '@'"));
            }
            if !seen.insert(id) {
                return Err(anyhow!(
                    "rejecting to push {:?} with duplicate label",
                    change.commit
                ));
            }
            seen.insert(id);
        }
    }

    let mut refs = vec![];
    for change in changes {
        if let Some(change_id) = change.id {
            let change_ref = StackedChangeRef::Change {
                target: baseref.replacen("refs/heads/", "", 1),
                author: change.author,
                change_id: change_id.clone(),
            };
            refs.push(PushRef {
                ref_name: StackedRef::ChangeRef(change_ref.clone()).ref_name(),
                oid: change.commit,
                change_id: change_id.clone(),
            });
            if let Some(parent_sha) = transaction
                .repo()
                .find_commit(change.commit)?
                .parent_ids()
                .next()
            {
                refs.push(PushRef {
                    ref_name: StackedRef::ChangeRef(change_ref.as_base()).ref_name(),
                    oid: parent_sha,
                    change_id,
                });
            }
        }
    }
    Ok(refs)
}

pub fn build_to_push(
    transaction: &Transaction,
    push_mode: &PushMode,
    baseref: &str,
    ref_with_options: &str,
    oid_to_push: git2::Oid,
    base_oid: git2::Oid,
) -> anyhow::Result<Vec<PushRef>> {
    match push_mode {
        PushMode::Publish(author) => {
            let changes = get_changes(transaction, oid_to_push, base_oid)?;
            let changes = split_changes(transaction, changes)?;

            let mut push_refs = changes_to_refs(transaction, baseref, author, changes)?;

            let target = baseref.replacen("refs/heads/", "", 1);
            push_refs.push(PushRef {
                ref_name: StackedRef::StackHead {
                    target: target.clone(),
                    author: author.clone(),
                }
                .ref_name(),
                oid: oid_to_push,
                change_id: target,
            });

            push_refs.sort_by(|a, b| a.ref_name.cmp(&b.ref_name));
            Ok(push_refs)
        }
        PushMode::Normal => Ok(vec![PushRef {
            ref_name: if ref_with_options.starts_with("refs/") {
                ref_with_options.to_string()
            } else {
                format!("refs/heads/{}", ref_with_options)
            },
            oid: oid_to_push,
            change_id: "JOSH_PUSH".to_string(),
        }]),
    }
}

/// Store a GitHub node ID for a local comment, marking it as posted.
pub fn store_github_id(
    transaction: &Transaction,
    change_id: &str,
    local_hash: &str,
    github_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let blob_oid = transaction.repo().blob(github_id.as_bytes())?;
    let path = std::path::Path::new("gh_ids")
        .join(encode_change_id_path(change_id))
        .join(local_hash);
    write_changes_tree(transaction, &path, blob_oid, None, None, scope)?;
    Ok(())
}

/// Read all GitHub node IDs for a change's comments.
/// Returns a map from local comment hash → GitHub node ID.
pub fn read_github_ids(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let repo = transaction.repo();
    let tree = match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => repo.find_commit(oid)?.tree()?,
        None => return Ok(Default::default()),
    };
    let gh_ids_path = std::path::Path::new("gh_ids").join(encode_change_id_path(change_id));
    let subtree = match get_tree(repo, &tree, &gh_ids_path) {
        Some(t) => t,
        None => return Ok(Default::default()),
    };
    let mut map = std::collections::HashMap::new();
    for entry in subtree.iter() {
        if let Ok(name) = entry.name() {
            if let Ok(blob) = entry.to_object(repo).and_then(|o| o.peel_to_blob()) {
                let github_id = String::from_utf8_lossy(blob.content()).trim().to_string();
                map.insert(name.to_string(), github_id);
            }
        }
    }
    Ok(map)
}

pub fn store_github_vote_id(
    transaction: &Transaction,
    change_id: &str,
    user: &str,
    vote_data: &VoteData,
    scope: &ChangesRef,
) -> anyhow::Result<()> {
    let json = serde_json::to_string(vote_data)?;
    let blob_oid = transaction.repo().blob(json.as_bytes())?;
    let path = std::path::Path::new("gh_vote_ids")
        .join(encode_change_id_path(change_id))
        .join(user);
    write_changes_tree(transaction, &path, blob_oid, None, None, scope)?;
    Ok(())
}

pub fn read_github_vote_ids(
    transaction: &Transaction,
    change_id: &str,
    scope: &ChangesRef,
) -> anyhow::Result<std::collections::HashMap<String, VoteData>> {
    let repo = transaction.repo();
    let tree = match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => repo.find_commit(oid)?.tree()?,
        None => return Ok(Default::default()),
    };
    let path = std::path::Path::new("gh_vote_ids").join(encode_change_id_path(change_id));
    let subtree = match get_tree(repo, &tree, &path) {
        Some(t) => t,
        None => return Ok(Default::default()),
    };
    let mut map = std::collections::HashMap::new();
    for entry in subtree.iter() {
        if let Ok(user) = entry.name() {
            if let Ok(blob) = entry.to_object(repo).and_then(|o| o.peel_to_blob()) {
                if let Ok(data) = serde_json::from_slice::<VoteData>(blob.content()) {
                    map.insert(user.to_string(), data);
                }
            }
        }
    }
    Ok(map)
}

pub fn vote_state_to_github_review(state: &str) -> &'static str {
    match state {
        "approve" => "APPROVE",
        "discuss" => "COMMENT",
        "revise" => "REQUEST_CHANGES",
        _ => "COMMENT",
    }
}
