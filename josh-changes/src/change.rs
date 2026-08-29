use crate::refs::ChangesRef;
use crate::store::store_diff_data;
use anyhow::{Context, anyhow};
use josh_core::objects;
use josh_core::trailers::{commit_change_meta, parse_change_meta};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Change {
    pub(crate) author: String,
    pub(crate) id: Option<String>,
    pub(crate) series: Vec<String>,
    pub(crate) commit: gix_hash::ObjectId,
    pub(crate) base: gix_hash::ObjectId,
}

impl Change {
    pub fn new(
        transaction: &josh_core::cache::Transaction,
        commit: gix_hash::ObjectId,
    ) -> anyhow::Result<Self> {
        Ok(Self::from_commit(&objects::CommitData::read(
            transaction.odb(),
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
            base: gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
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

    pub fn commit(&self) -> gix_hash::ObjectId {
        self.commit
    }

    pub fn base(&self) -> gix_hash::ObjectId {
        self.base
    }

    pub fn set_base(&mut self, base: gix_hash::ObjectId) {
        self.base = base;
    }

    pub fn contributing(
        &self,
        transaction: &josh_core::cache::Transaction,
    ) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
        // First-parent walk down to (but not including) the base.
        let odb = transaction.odb();
        let mut walk = objects::RevWalk::new(odb);
        walk.simplify_first_parent();
        walk.push(self.commit)?;
        let base = self.base;
        let mut oids = walk.into_topo_vec(|oid| {
            base != gix_hash::ObjectId::null(gix_hash::Kind::Sha1) && oid == base
        })?;
        oids.retain(|oid| *oid != base);
        if oids.first() == Some(&self.commit) {
            oids.remove(0);
        }
        Ok(oids)
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
    pr_head: gix_hash::ObjectId,
    target_branch_tip: gix_hash::ObjectId,
    message: &str,
) -> anyhow::Result<gix_hash::ObjectId> {
    let odb = transaction.odb();
    let head = objects::CommitData::read(odb, pr_head)?;
    let tree = head.tree_id()?;

    objects::write_commit_with_signatures_of(
        odb,
        &head,
        tree,
        &[target_branch_tip, pr_head],
        message,
    )
}

pub fn split_changes(
    transaction: &josh_core::cache::Transaction,
    changes: std::collections::HashMap<gix_hash::ObjectId, Change>,
) -> anyhow::Result<Vec<Change>> {
    if changes.values().next().map(|c| c.base)
        == Some(gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
    {
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

pub fn get_changes(
    transaction: &josh_core::cache::Transaction,
    tip: gix_hash::ObjectId,
    base: gix_hash::ObjectId,
) -> anyhow::Result<std::collections::HashMap<gix_hash::ObjectId, Change>> {
    let odb = transaction.odb();
    let mut walk = objects::RevWalk::new(odb);
    walk.simplify_first_parent();
    walk.push(tip)?;
    let mut oids = walk.into_topo_vec(|oid| {
        base != gix_hash::ObjectId::null(gix_hash::Kind::Sha1) && oid == base
    })?;
    oids.retain(|oid| *oid != base);
    oids.reverse();

    let mut changes = std::collections::HashMap::new();
    for rev in oids {
        let commit = objects::CommitData::read(odb, rev)?;
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
    tip: gix_hash::ObjectId,
    base: gix_hash::ObjectId,
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
    let odb = transaction.odb();
    // Pre-tree-format entries must fail loudly rather than drop changes;
    // `josh changes sync --clean` rebuilds the ref.
    let Some(data) = crate::store::read_filtered::<crate::layout::ChangesRefData>(
        transaction,
        scope,
        crate::store::namespace_filter(crate::layout::DIFFS_PATH),
    )
    .with_context(|| "undecodable diff data; run `josh changes sync --clean` to rebuild the ref")?
    else {
        return Ok(Vec::new());
    };

    // Sort by change id: map iteration order is unspecified, tree entry order
    // was sorted.
    let mut entries: Vec<_> = data.diffs.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut changes = Vec::new();
    for (change_id, data) in entries {
        let tip_oid = gix_hash::ObjectId::from_str(&data.commit)
            .unwrap_or(gix_hash::ObjectId::null(gix_hash::Kind::Sha1));
        let base_oid = gix_hash::ObjectId::from_str(&data.base)
            .unwrap_or(gix_hash::ObjectId::null(gix_hash::Kind::Sha1));
        if tip_oid == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
            continue;
        }
        let commit = match objects::CommitData::read(odb, tip_oid) {
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
    head: gix_hash::ObjectId,
    spec: &str,
) -> anyhow::Result<Change> {
    let odb = transaction.odb();
    // Try as a full OID first.
    if let Ok(oid) = gix_hash::ObjectId::from_str(spec) {
        if let Ok(commit) = objects::CommitData::read(odb, oid) {
            return Ok(Change::from_commit(&commit));
        }
    }

    // Try as a revparse (branch, tag, short SHA). An error -- a spec that looks
    // like a too-short oid prefix, say -- only means this is not a revision; the
    // spec may still name a change-id, so fall through to the walk below.
    if let Some(oid) = transaction.rev_parse(spec).ok().flatten()
        && let Ok(oid) = objects::peel_to_commit(odb, oid)
        && let Ok(commit) = objects::CommitData::read(odb, oid)
    {
        return Ok(Change::from_commit(&commit));
    }

    // Walk from head to find a commit with matching Change-Id.
    let mut walk = objects::RevWalk::new(odb);
    walk.simplify_first_parent();
    walk.push(head)?;
    for oid in walk.into_topo_vec(|_| false)? {
        if let Ok(c) = objects::CommitData::read(odb, oid) {
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
