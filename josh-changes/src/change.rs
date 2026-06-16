use crate::refs::ChangesRef;
use crate::store::store_diff_data;
use anyhow::anyhow;
use josh_core::filter::tree;
use josh_core::objects;
use josh_core::trailers::{commit_change_meta, parse_change_meta};

#[derive(Debug, Clone)]
pub struct Change {
    pub(crate) author: String,
    pub(crate) id: Option<String>,
    pub(crate) series: Vec<String>,
    pub(crate) commit: git2::Oid,
    pub(crate) base: git2::Oid,
}

impl Change {
    pub fn new(
        transaction: &josh_core::cache::Transaction,
        commit: git2::Oid,
    ) -> anyhow::Result<Self> {
        Ok(Self::from_commit(&objects::CommitData::read(
            &transaction.odb()?,
            commit,
        )?))
    }

    pub(crate) fn from_commit(commit: &objects::CommitData) -> Self {
        let author = commit
            .parsed()
            .and_then(|c| Ok(c.author()?.email.to_owned()))
            .map(|email| String::from_utf8_lossy(&email).into_owned())
            .unwrap_or_default();
        let mut change = Self {
            author,
            id: None,
            series: Vec::new(),
            commit: commit.id(),
            base: git2::Oid::ZERO_SHA1,
        };
        let (id, series) = commit_change_meta(commit);
        change.id = id;
        change.series = series;

        change
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn series(&self) -> &[String] {
        &self.series
    }

    pub fn commit(&self) -> git2::Oid {
        self.commit
    }

    pub fn base(&self) -> git2::Oid {
        self.base
    }

    pub fn set_base(&mut self, base: git2::Oid) {
        self.base = base;
    }

    pub fn contributing(
        &self,
        transaction: &josh_core::cache::Transaction,
    ) -> anyhow::Result<Vec<git2::Oid>> {
        // First-parent walk down to (but not including) the base.
        let odb = transaction.odb()?;
        let mut walk = objects::RevWalk::new(&odb);
        walk.simplify_first_parent();
        walk.push(self.commit)?;
        let base = self.base;
        let mut oids = walk.into_topo_vec(|oid| base != git2::Oid::ZERO_SHA1 && oid == base)?;
        oids.retain(|oid| *oid != base);
        if oids.first() == Some(&self.commit) {
            oids.remove(0);
        }
        Ok(oids)
    }

    /// The change-ids this change depends on (the changes in its downstack),
    /// restricted to `known` and excluding itself, in downstack order, deduped.
    ///
    /// Dependencies are matched by each contributing commit's `Change-Id`
    /// trailer, not by commit oid: a stored change's tip is a downstack-split
    /// commit whose oid does not appear in another change's contributing
    /// history, so oid matching only ever links the un-split (identity) changes.
    pub fn dependency_ids(
        &self,
        transaction: &josh_core::cache::Transaction,
        known: &std::collections::HashSet<String>,
    ) -> anyhow::Result<Vec<String>> {
        let odb = transaction.odb()?;
        let mut deps = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for oid in self.contributing(transaction)? {
            let commit = objects::CommitData::read(&odb, oid)?;
            let (id, _) = commit_change_meta(&commit);
            let id = match id {
                Some(id) => id,
                None => continue,
            };
            if Some(id.as_str()) == self.id() || !known.contains(&id) {
                continue;
            }
            if seen.insert(id.clone()) {
                deps.push(id);
            }
        }
        Ok(deps)
    }
}

pub fn encode_change_id_path(id: &str) -> String {
    id.replace('/', "%2F")
}

/// Create a real merge commit that has the target branch tip and the change
/// head as its two parents. The tree is the change head's tree (no content
/// merge needed). Author and committer are copied from the head commit.
pub fn create_synthetic_merge_commit(
    transaction: &josh_core::cache::Transaction,
    pr_head: git2::Oid,
    target_branch_tip: git2::Oid,
    message: &str,
) -> anyhow::Result<git2::Oid> {
    let odb = transaction.odb()?;
    let head = objects::CommitData::read(&odb, pr_head)?;
    let tree = head.tree_id()?;

    objects::write_commit_with_signatures_of(
        &odb,
        &head,
        tree,
        &[target_branch_tip, pr_head],
        message,
    )
}

pub(crate) fn decode_change_id_path(enc: &str) -> String {
    enc.replace("%2F", "/")
}

pub(crate) fn split_changes(
    transaction: &josh_core::cache::Transaction,
    changes: std::collections::HashMap<git2::Oid, Change>,
) -> anyhow::Result<Vec<Change>> {
    if changes.values().next().map(|c| c.base) == Some(git2::Oid::ZERO_SHA1) {
        return Ok(changes.into_values().collect());
    }

    changes
        .into_values()
        .map(|c| {
            let filter = josh_core::filter::Filter::new().downstack(c.base);
            let new_oid = josh_core::filter::apply_to_commit(filter, c.commit, transaction)?;
            let mut result = c;
            result.commit = new_oid;
            Ok(result)
        })
        .collect()
}

pub(crate) fn get_changes(
    transaction: &josh_core::cache::Transaction,
    tip: git2::Oid,
    base: git2::Oid,
) -> anyhow::Result<std::collections::HashMap<git2::Oid, Change>> {
    let odb = transaction.odb()?;
    let mut walk = objects::RevWalk::new(&odb);
    walk.simplify_first_parent();
    walk.push(tip)?;
    let mut oids = walk.into_topo_vec(|oid| base != git2::Oid::ZERO_SHA1 && oid == base)?;
    oids.retain(|oid| *oid != base);
    oids.reverse();

    let mut changes = std::collections::HashMap::new();
    for rev in oids {
        let commit = objects::CommitData::read(&odb, rev)?;
        let mut change = Change::from_commit(&commit);
        if change.id.is_none() {
            continue;
        }
        change.base = base;
        changes.insert(change.commit, change);
    }

    Ok(changes)
}

pub fn sync_changes(
    transaction: &josh_core::cache::Transaction,
    tip: git2::Oid,
    base: git2::Oid,
    branch: &str,
) -> anyhow::Result<Vec<Change>> {
    let changes = get_changes(transaction, tip, base)?;
    let changes = split_changes(transaction, changes)?;
    let scope = ChangesRef::Local {
        branch: branch.to_string(),
    };
    for c in &changes {
        let _ = store_diff_data(transaction, c, &scope);
    }
    Ok(changes)
}

/// Run a local-scope sync end-to-end: derive tip from HEAD, derive base from
/// `refs/remotes/origin/<HEAD branch>`, and store discovered changes under the
/// `ChangesRef::Local { branch }` ref.
///
/// The `branch` argument selects the scope ref; the HEAD branch (which may
/// differ when a caller passes an explicit target) is used only to find the
/// base commit.
pub fn sync_local(
    repo: &git2::Repository,
    transaction: &josh_core::cache::Transaction,
    branch: &str,
) -> anyhow::Result<Vec<Change>> {
    let head = repo.head()?.peel_to_commit()?;
    let head_branch = repo.head()?.shorthand().ok().map(|s| s.to_string());
    let base_oid = head_branch
        .as_ref()
        .and_then(|b| {
            repo.find_reference(&format!("refs/remotes/origin/{}", b))
                .ok()
                .and_then(|r| r.peel_to_commit().ok())
                .map(|c| c.id())
        })
        .unwrap_or(git2::Oid::ZERO_SHA1);
    sync_changes(transaction, head.id(), base_oid, branch)
}

pub fn list_changes(
    transaction: &josh_core::cache::Transaction,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<Change>> {
    let odb = transaction.odb()?;
    let tree = match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => objects::CommitData::read(&odb, oid)?.tree_id()?,
        None => return Ok(Vec::new()),
    };

    let diffs_tree =
        match crate::store::get_tree(transaction, &odb, tree, std::path::Path::new("diffs")) {
            Some(t) => t,
            None => return Ok(Vec::new()),
        };

    let mut changes = Vec::new();
    for entry in tree::read_tree(transaction, &odb, diffs_tree)?.entries() {
        let change_id = decode_change_id_path(std::str::from_utf8(entry.filename).unwrap_or(""));
        if change_id.is_empty() || !entry.mode.is_tree() {
            continue;
        }
        let subtree = match tree::read_tree(transaction, &odb, objects::git2_oid(&entry.oid)) {
            Ok(t) => t,
            Err(_) => continue,
        };
        // The subtree has a single blob named by its content hash.
        // Read it to get tip and base OIDs.
        let mut tip_oid = git2::Oid::ZERO_SHA1;
        let mut base_oid = git2::Oid::ZERO_SHA1;
        for se in subtree.entries() {
            let blob = match tree::blob_bytes(&odb, objects::git2_oid(&se.oid)) {
                Some(b) => b,
                None => continue,
            };
            let content = String::from_utf8_lossy(&blob);
            if let Some((tip_str, base_str)) = content.split_once('\n') {
                tip_oid = git2::Oid::from_str(tip_str).unwrap_or(git2::Oid::ZERO_SHA1);
                base_oid = git2::Oid::from_str(base_str).unwrap_or(git2::Oid::ZERO_SHA1);
            }
            break;
        }
        if tip_oid == git2::Oid::ZERO_SHA1 {
            continue;
        }
        let commit = match objects::CommitData::read(&odb, tip_oid) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut change = Change::from_commit(&commit);
        change.base = base_oid;
        if change.id.is_none() {
            change.id = Some(change_id);
        }
        changes.push(change);
    }
    Ok(changes)
}

pub fn resolve_change(
    transaction: &josh_core::cache::Transaction,
    head: git2::Oid,
    spec: &str,
) -> anyhow::Result<Change> {
    let repo = transaction.repo();
    let odb = transaction.odb()?;
    // Try as a full OID first.
    if let Ok(oid) = git2::Oid::from_str(spec) {
        if let Ok(commit) = objects::CommitData::read(&odb, oid) {
            return Ok(Change::from_commit(&commit));
        }
    }

    // Try as a revparse (branch, tag, short SHA).
    // PORT: rev-parse stays on the git2 handle until flag day (gix rev_parse then).
    if let Ok(obj) = repo.revparse_single(spec) {
        if let Ok(commit) = obj.peel_to_commit()
            && let Ok(commit) = objects::CommitData::read(&odb, commit.id())
        {
            return Ok(Change::from_commit(&commit));
        }
    }

    // Walk from head to find a commit with matching Change-Id.
    let mut walk = objects::RevWalk::new(&odb);
    walk.simplify_first_parent();
    walk.push(head)?;
    for oid in walk.into_topo_vec(|_| false)? {
        if let Ok(c) = objects::CommitData::read(&odb, oid) {
            let message = c
                .message()
                .ok()
                .and_then(|m| std::str::from_utf8(m).ok())
                .unwrap_or("");
            let (id, _) = parse_change_meta(message);
            if id.as_deref() == Some(spec) {
                return Ok(Change::from_commit(&c));
            }
        }
    }

    Err(anyhow!("could not resolve '{}' to a commit", spec))
}
