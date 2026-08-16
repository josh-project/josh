use crate::refs::ChangesRef;
use crate::store::store_diff_data;
use anyhow::anyhow;
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
        Ok(Self::from_commit(&transaction.repo().find_commit(commit)?))
    }

    pub(crate) fn from_commit(commit: &git2::Commit) -> Self {
        let mut change = Self {
            author: commit.author().email().unwrap_or("").to_string(),
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
        let mut walk = transaction.repo().revwalk()?;
        walk.simplify_first_parent()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
        walk.push(self.commit)?;
        if self.base != git2::Oid::ZERO_SHA1 {
            walk.hide(self.base)?;
        }
        let mut oids: Vec<git2::Oid> = walk.collect::<Result<Vec<_>, _>>()?;
        if oids.first() == Some(&self.commit) {
            oids.remove(0);
        }
        Ok(oids)
    }
}

pub fn encode_change_id_path(id: &str) -> String {
    id.replace('/', "%2F")
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
    let repo = transaction.repo();
    let mut walk = repo.revwalk()?;
    walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
    walk.simplify_first_parent()?;
    walk.push(tip)?;
    if base != git2::Oid::ZERO_SHA1 {
        walk.hide(base)?;
    }

    let mut changes = std::collections::HashMap::new();
    for rev in walk {
        let commit = repo.find_commit(rev?)?;
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

pub fn list_changes(
    transaction: &josh_core::cache::Transaction,
    scope: &ChangesRef,
) -> anyhow::Result<Vec<Change>> {
    let repo = transaction.repo();
    let tree = match transaction.resolve_ref(&scope.ref_name())? {
        Some(oid) => repo.find_commit(oid)?.tree()?,
        None => return Ok(Vec::new()),
    };

    let diffs_tree = match tree
        .get_name("diffs")
        .and_then(|e| e.to_object(repo).ok())
        .and_then(|o| o.peel_to_tree().ok())
    {
        Some(t) => t,
        None => return Ok(Vec::new()),
    };

    let mut changes = Vec::new();
    for entry in diffs_tree.iter() {
        let change_id = decode_change_id_path(entry.name().unwrap_or(""));
        if change_id.is_empty() {
            continue;
        }
        let subtree = match entry
            .to_object(repo)
            .ok()
            .and_then(|o| o.peel_to_tree().ok())
        {
            Some(t) => t,
            None => continue,
        };
        // The subtree has a single blob named by its content hash.
        // Read it to get tip and base OIDs.
        let mut tip_oid = git2::Oid::ZERO_SHA1;
        let mut base_oid = git2::Oid::ZERO_SHA1;
        for se in subtree.iter() {
            let blob = match se.to_object(repo).ok().and_then(|o| o.peel_to_blob().ok()) {
                Some(b) => b,
                None => continue,
            };
            let content = String::from_utf8_lossy(blob.content());
            if let Some((tip_str, base_str)) = content.split_once('\n') {
                tip_oid = git2::Oid::from_str(tip_str).unwrap_or(git2::Oid::ZERO_SHA1);
                base_oid = git2::Oid::from_str(base_str).unwrap_or(git2::Oid::ZERO_SHA1);
            }
            break;
        }
        if tip_oid == git2::Oid::ZERO_SHA1 {
            continue;
        }
        let commit = match repo.find_commit(tip_oid) {
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
    // Try as a full OID first.
    if let Ok(oid) = git2::Oid::from_str(spec) {
        if let Ok(commit) = repo.find_commit(oid) {
            return Ok(Change::from_commit(&commit));
        }
    }

    // Try as a revparse (branch, tag, short SHA).
    // PORT: rev-parse stays on the git2 handle until flag day (gix rev_parse then).
    if let Ok(obj) = repo.revparse_single(spec) {
        if let Ok(commit) = obj.peel_to_commit() {
            return Ok(Change::from_commit(&commit));
        }
    }

    // Walk from head to find a commit with matching Change-Id.
    let mut walk = repo.revwalk()?;
    walk.simplify_first_parent()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(head)?;
    for oid in walk {
        let oid = oid?;
        if let Ok(c) = repo.find_commit(oid) {
            let (id, _) = parse_change_meta(c.message().unwrap_or(""));
            if id.as_deref() == Some(spec) {
                return Ok(Change::from_commit(&c));
            }
        }
    }

    Err(anyhow!("could not resolve '{}' to a commit", spec))
}
