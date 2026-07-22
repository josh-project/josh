//! Per-commit history-graph hints: sequence number and reachable-roots set.
//!
//! Both are bottom-up monoid folds over the same DAG (`seq(C) = max(seq(p))+1`,
//! `roots(C) = ⋃ roots(p)`), so they're computed together in a single
//! topological walk and cached in two parallel filter slots
//! (`filter::sequence_number()`, `filter::reachable_roots()`).
//!
//! Storage of the roots set: a git blob whose content is the concatenation of
//! 20-byte root OIDs (sorted, deduplicated). The blob's OID is what the
//! `reachable_roots` cache slot stores. In linear history every commit reuses
//! its parent's blob OID — no read or write — so the hot path is cheap.
//!
//! The `sequence_number` slot stores a synthetic OID that also carries the
//! commit's parent count in a spare byte (see `oid_from_hint`), so cache
//! backends can classify commits as merges/orphans without reading them.

use anyhow::anyhow;

use super::backend::HistoryGraphHint;
use super::transaction::Transaction;

/// Per-commit graph info derived from a single topological walk:
/// - `sequence_number` strictly greater than every parent's sequence number
///   (so sorting by it yields topological order).
/// - `reachable_roots`: sorted, deduplicated set of root commits (parentless
///   commits) reachable from the commit.
#[derive(Debug, Clone)]
pub struct HistoryGraphInfo {
    pub sequence_number: u64,
    pub reachable_roots: Vec<git2::Oid>,
}

/// Returns just the sequence number for `input`.
///
/// Unlike [`collect_history_graph_info`], this never reads the roots blob: the
/// sequence number is available directly from the cached hint, so callers that
/// only compare sequence numbers avoid a per-commit `find_blob` + parse that
/// would otherwise be discarded.
pub fn compute_sequence_number(transaction: &Transaction, input: git2::Oid) -> anyhow::Result<u64> {
    Ok(ensure_hint_cached(transaction, input)?.0)
}

/// Returns the cached `(sequence number, parent count)` for `input` without
/// reading the roots blob. Both values are decoded from the same cached hint,
/// so cache backends can make eligibility decisions without any commit read.
pub fn compute_history_hint(
    transaction: &Transaction,
    input: git2::Oid,
) -> anyhow::Result<HistoryGraphHint> {
    let (sequence_number, parent_count, _) = ensure_hint_cached(transaction, input)?;
    Ok(HistoryGraphHint {
        sequence_number,
        parent_count,
    })
}

/// Computes sequence number and reachable roots for `input` in a single walk,
/// memoizing intermediate results so repeated calls are O(new commits).
///
/// Inside the walk we work with `(seq, roots_blob_oid)` tuples and only touch
/// the ODB when parents disagree on the roots blob — in linear history every
/// commit reuses its parent's blob OID, avoiding read/write entirely.
pub fn collect_history_graph_info(
    transaction: &Transaction,
    input: git2::Oid,
) -> anyhow::Result<HistoryGraphInfo> {
    let (seq, _, blob) = ensure_hint_cached(transaction, input)?;

    Ok(HistoryGraphInfo {
        sequence_number: seq,
        reachable_roots: read_roots_blob(transaction.repo(), blob)?,
    })
}

/// Returns true iff the set of root commits reachable from all `parent_ids`
/// has non-empty intersection — i.e. they share at least one common ancestor.
/// This is the cheap analogue of `repo.merge_base_many(parent_ids).is_ok()` for
/// the case where the caller only needs the existence answer, not the merge
/// base OID itself. Zero OIDs in `parent_ids` cause the function to return
/// `Ok(false)` (matching `merge_base_many`'s error behavior on invalid input).
pub fn parents_share_root(
    transaction: &Transaction,
    parent_ids: &[git2::Oid],
) -> anyhow::Result<bool> {
    if parent_ids.is_empty() || parent_ids.iter().any(|x| *x == git2::Oid::zero()) {
        return Ok(false);
    }

    // Ensure each parent's graph info is cached, then collect the cached blob
    // OIDs. If all parents reference the same blob, their root sets are
    // identical — they trivially share every root without reading any blob.
    let parent_blobs: Vec<git2::Oid> = parent_ids
        .iter()
        .map(|p| Ok(ensure_hint_cached(transaction, *p)?.2))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let first_blob = parent_blobs[0];
    if parent_blobs.iter().all(|b| *b == first_blob) {
        return Ok(true);
    }

    // Parents disagree on the roots blob: read each blob and intersect.
    let mut common: std::collections::BTreeSet<git2::Oid> =
        read_roots_blob(transaction.repo(), first_blob)?
            .into_iter()
            .collect();
    for blob_oid in &parent_blobs[1..] {
        if common.is_empty() {
            return Ok(false);
        }
        let p_set: std::collections::BTreeSet<_> = read_roots_blob(transaction.repo(), *blob_oid)?
            .into_iter()
            .collect();
        common = common.intersection(&p_set).copied().collect();
    }
    Ok(!common.is_empty())
}

/// Ensures `(sequence_number, reachable_roots)` are cached for `input` and
/// returns the cached `(seq, parent_count, roots_blob_oid)`. Performs a
/// topological walk only if neither piece is cached for `input`. Inside the
/// walk, each commit's roots blob is reused from its parent when parents
/// agree, so the common case (linear or shared-root merges) avoids ODB reads
/// and writes.
fn ensure_hint_cached(
    transaction: &Transaction,
    input: git2::Oid,
) -> anyhow::Result<(u64, u8, git2::Oid)> {
    if let Some(hint) = try_read_cached_hint(transaction, input)? {
        return Ok(hint);
    }

    if !transaction.repo().odb()?.exists(input) {
        return Err(anyhow!("ensure_hint_cached: input does not exist"));
    }

    let parent_ids = crate::git::read_parent_ids(transaction.repo(), input)?;

    // Fast path: every parent already has both pieces cached.
    let parents_hint: Option<Vec<(u64, git2::Oid)>> = parent_ids
        .iter()
        .map(|p| Ok(try_read_cached_hint(transaction, *p)?.map(|(seq, _, blob)| (seq, blob))))
        .collect::<anyhow::Result<_>>()?;

    if let Some(parents_hint) = parents_hint {
        let hint = derive_from_parents(transaction.repo(), input, &parents_hint)?;
        store_hint(transaction, input, hint)?;
        return Ok(hint);
    }

    log::info!("ensure_hint_cached: new_walk for {:?}", input);
    let mut walk = transaction.repo().revwalk()?;
    walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
    walk.push(input)?;

    // Hide ancestors that already have *both* pieces cached. Hiding on seq#
    // alone would skip commits with cached seq# but missing roots, leaving
    // their roots unpopulated.
    // The callback cannot propagate errors, so treat a failed lookup as "not
    // cached": the walk then visits the commit and the fallible body reports
    // the same error properly.
    let mut hide = |id| {
        transaction
            .known(crate::filter::sequence_number(), id)
            .unwrap_or(false)
            && transaction
                .known(crate::filter::reachable_roots(), id)
                .unwrap_or(false)
    };
    let walk = walk.with_hide_callback(&mut hide)?;

    for c in walk {
        let oid = c?;
        let parents_hint: Vec<(u64, git2::Oid)> =
            crate::git::read_parent_ids(transaction.repo(), oid)?
                .into_iter()
                .map(|p| {
                    try_read_cached_hint(transaction, p)?
                        .map(|(seq, _, blob)| (seq, blob))
                        .ok_or_else(|| anyhow!("parent {} hint missing during walk for {}", p, oid))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
        let hint = derive_from_parents(transaction.repo(), oid, &parents_hint)?;
        store_hint(transaction, oid, hint)?;
    }

    try_read_cached_hint(transaction, input)?
        .ok_or_else(|| anyhow!("missing graph info after walk for {}", input))
}

/// Given that all parents have cached `(seq, roots_blob_oid)`, derive the
/// `(seq, parent_count, roots_blob_oid)` for `self_oid`. Performs blob I/O
/// only when parents disagree on the blob; otherwise reuses the parent blob
/// OID (or, for the root case, writes a single-element blob).
fn derive_from_parents(
    repo: &git2::Repository,
    self_oid: git2::Oid,
    parents_hint: &[(u64, git2::Oid)],
) -> anyhow::Result<(u64, u8, git2::Oid)> {
    if parents_hint.is_empty() {
        // Parentless: this commit *is* its own only reachable root.
        return Ok((0, 0, write_roots_blob(repo, &[self_oid])?));
    }

    let parent_count = parents_hint.len().min(255) as u8;

    let seq = parents_hint
        .iter()
        .map(|(s, _)| *s)
        .max()
        .expect("non-empty")
        + 1;

    let first_blob = parents_hint[0].1;
    let roots_blob = if parents_hint.iter().all(|(_, b)| *b == first_blob) {
        first_blob
    } else {
        let mut set: std::collections::BTreeSet<git2::Oid> = Default::default();
        for (_, blob_oid) in parents_hint {
            set.extend(read_roots_blob(repo, *blob_oid)?);
        }
        let roots: Vec<_> = set.into_iter().collect();
        write_roots_blob(repo, &roots)?
    };

    Ok((seq, parent_count, roots_blob))
}

fn try_read_cached_hint(
    transaction: &Transaction,
    input: git2::Oid,
) -> anyhow::Result<Option<(u64, u8, git2::Oid)>> {
    let Some(seq) = transaction.get(crate::filter::sequence_number(), input)? else {
        return Ok(None);
    };
    let Some(roots_blob) = transaction.get(crate::filter::reachable_roots(), input)? else {
        return Ok(None);
    };
    Ok(Some((
        u64_from_oid(seq),
        parent_count_from_oid(seq),
        roots_blob,
    )))
}

fn store_hint(
    transaction: &Transaction,
    input: git2::Oid,
    hint: (u64, u8, git2::Oid),
) -> anyhow::Result<()> {
    let (seq, parent_count, roots_blob) = hint;
    transaction.insert(
        crate::filter::sequence_number(),
        input,
        oid_from_hint(seq, parent_count),
        true,
    )?;
    transaction.insert(crate::filter::reachable_roots(), input, roots_blob, true)?;
    Ok(())
}

fn write_roots_blob(repo: &git2::Repository, roots: &[git2::Oid]) -> anyhow::Result<git2::Oid> {
    let mut bytes = Vec::with_capacity(roots.len() * 20);
    for r in roots {
        bytes.extend_from_slice(r.as_bytes());
    }
    Ok(repo.blob(&bytes)?)
}

fn read_roots_blob(repo: &git2::Repository, oid: git2::Oid) -> anyhow::Result<Vec<git2::Oid>> {
    let blob = repo.find_blob(oid)?;
    let content = blob.content();
    if content.len() % 20 != 0 {
        return Err(anyhow!(
            "malformed reachable_roots blob {}: length {} not a multiple of 20",
            oid,
            content.len()
        ));
    }
    let mut out = Vec::with_capacity(content.len() / 20);
    for chunk in content.chunks_exact(20) {
        out.push(git2::Oid::from_bytes(chunk)?);
    }
    Ok(out)
}

/// Encode a `(sequence number, parent count)` hint into a 20-byte git OID
/// (SHA-1 sized). Bytes 0-10 of the OID are zero, byte 11 holds the parent
/// count (capped at 255), and bytes 12-19 contain the big-endian sequence
/// number.
pub(crate) fn oid_from_hint(seq: u64, parent_count: u8) -> git2::Oid {
    let mut bytes = [0u8; 20];
    bytes[11] = parent_count;
    // place the 8 integer bytes at the end (big-endian)
    bytes[20 - 8..].copy_from_slice(&seq.to_be_bytes());
    // Safe: length is exactly 20
    git2::Oid::from_bytes(&bytes).expect("20-byte OID construction cannot fail")
}

/// Decode the sequence number from an OID encoded by `oid_from_hint`.
pub(crate) fn u64_from_oid(oid: git2::Oid) -> u64 {
    let b = oid.as_bytes();
    let mut n = [0u8; 8];
    n.copy_from_slice(&b[20 - 8..]); // take the last 8 bytes
    u64::from_be_bytes(n)
}

/// Decode the parent count from an OID encoded by `oid_from_hint`.
pub(crate) fn parent_count_from_oid(oid: git2::Oid) -> u8 {
    oid.as_bytes()[11]
}

#[cfg(test)]
mod tests {
    use super::{oid_from_hint, parent_count_from_oid, u64_from_oid};

    #[test]
    fn oid_hint_roundtrip_uses_last_9_bytes() {
        let value = 0x0123_4567_89ab_cdef_u64;
        let oid = oid_from_hint(value, 7);
        let bytes = oid.as_bytes();

        assert!(bytes[..11].iter().all(|byte| *byte == 0));
        assert_eq!(bytes[11], 7);
        assert_eq!(&bytes[12..], &value.to_be_bytes());
        assert_eq!(u64_from_oid(oid), value);
        assert_eq!(parent_count_from_oid(oid), 7);
    }
}
