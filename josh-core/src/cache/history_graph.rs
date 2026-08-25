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
//! commit's parent count and the sequence distance to its farther first-two
//! parent in spare bytes (see `oid_from_hint`), so cache backends can classify
//! commits as merges/orphans and detect sequence-number jumps without reading
//! them.

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
    pub reachable_roots: Vec<gix_hash::ObjectId>,
}

/// Returns just the sequence number for `input`.
///
/// Unlike [`collect_history_graph_info`], this never reads the roots blob: the
/// sequence number is available directly from the cached hint, so callers that
/// only compare sequence numbers avoid a per-commit `find_blob` + parse that
/// would otherwise be discarded.
pub fn compute_sequence_number(
    transaction: &Transaction,
    input: gix_hash::ObjectId,
) -> anyhow::Result<u64> {
    Ok(ensure_hint_cached(transaction, input)?.0.sequence_number)
}

/// Returns the cached hint for `input` without reading the roots blob. All
/// fields are decoded from the same cached hint, so cache backends can make
/// eligibility decisions without any commit read.
pub fn compute_history_hint(
    transaction: &Transaction,
    input: gix_hash::ObjectId,
) -> anyhow::Result<HistoryGraphHint> {
    Ok(ensure_hint_cached(transaction, input)?.0)
}

/// Computes sequence number and reachable roots for `input` in a single walk,
/// memoizing intermediate results so repeated calls are O(new commits).
///
/// Inside the walk we work with `(seq, roots_blob_oid)` tuples and only touch
/// the ODB when parents disagree on the roots blob — in linear history every
/// commit reuses its parent's blob OID, avoiding read/write entirely.
pub fn collect_history_graph_info(
    transaction: &Transaction,
    input: gix_hash::ObjectId,
) -> anyhow::Result<HistoryGraphInfo> {
    let (hint, blob) = ensure_hint_cached(transaction, input)?;

    Ok(HistoryGraphInfo {
        sequence_number: hint.sequence_number,
        reachable_roots: read_roots_blob(transaction.odb(), blob)?,
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
    parent_ids: &[gix_hash::ObjectId],
) -> anyhow::Result<bool> {
    if parent_ids.is_empty()
        || parent_ids
            .iter()
            .any(|x| *x == gix_hash::ObjectId::null(gix_hash::Kind::Sha1))
    {
        return Ok(false);
    }

    // Ensure each parent's graph info is cached, then collect the cached blob
    // OIDs. If all parents reference the same blob, their root sets are
    // identical — they trivially share every root without reading any blob.
    let parent_blobs: Vec<gix_hash::ObjectId> = parent_ids
        .iter()
        .map(|p| Ok(ensure_hint_cached(transaction, *p)?.1))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let first_blob = parent_blobs[0];
    if parent_blobs.iter().all(|b| *b == first_blob) {
        return Ok(true);
    }

    // Parents disagree on the roots blob: read each blob and intersect.
    let mut common: std::collections::BTreeSet<gix_hash::ObjectId> =
        read_roots_blob(transaction.odb(), first_blob)?
            .into_iter()
            .collect();
    for blob_oid in &parent_blobs[1..] {
        if common.is_empty() {
            return Ok(false);
        }
        let p_set: std::collections::BTreeSet<_> = read_roots_blob(transaction.odb(), *blob_oid)?
            .into_iter()
            .collect();
        common = common.intersection(&p_set).copied().collect();
    }
    Ok(!common.is_empty())
}

/// Ensures `(sequence_number, reachable_roots)` are cached for `input` and
/// returns the cached `(hint, roots_blob_oid)`. Performs a
/// topological walk only if neither piece is cached for `input`. Inside the
/// walk, each commit's roots blob is reused from its parent when parents
/// agree, so the common case (linear or shared-root merges) avoids ODB reads
/// and writes.
fn ensure_hint_cached(
    transaction: &Transaction,
    input: gix_hash::ObjectId,
) -> anyhow::Result<(HistoryGraphHint, gix_hash::ObjectId)> {
    if let Some(hint) = try_read_cached_hint(transaction, input)? {
        return Ok(hint);
    }

    let odb = transaction.odb();
    if !odb.contains(input) {
        return Err(anyhow!("ensure_hint_cached: input does not exist"));
    }

    let parent_ids = crate::git::read_parent_ids(odb, input)?;

    // Fast path: every parent already has both pieces cached.
    let parents_hint: Option<Vec<(u64, gix_hash::ObjectId)>> = parent_ids
        .iter()
        .map(|p| {
            Ok(try_read_cached_hint(transaction, *p)?
                .map(|(hint, blob)| (hint.sequence_number, blob)))
        })
        .collect::<anyhow::Result<_>>()?;

    if let Some(parents_hint) = parents_hint {
        let hint = derive_from_parents(odb, input, &parents_hint)?;
        store_hint(transaction, input, hint)?;
        return Ok(hint);
    }

    log::info!("ensure_hint_cached: new_walk for {:?}", input);
    let mut walk = crate::objects::RevWalk::new(odb);
    walk.push(input)?;

    // Prune ancestors that already have *both* pieces cached. Pruning on seq#
    // alone would skip commits with cached seq# but missing roots, leaving
    // their roots unpopulated.
    // The callback cannot propagate errors, so treat a failed lookup as "not
    // cached": the walk then visits the commit and the fallible body reports
    // the same error properly.
    let sorted = walk.into_topo_vec(|id| {
        transaction
            .known(crate::filter::sequence_number(), id)
            .unwrap_or(false)
            && transaction
                .known(crate::filter::reachable_roots(), id)
                .unwrap_or(false)
    })?;

    for &oid in sorted.iter().rev() {
        let parents_hint: Vec<(u64, gix_hash::ObjectId)> = crate::git::read_parent_ids(odb, oid)?
            .into_iter()
            .map(|p| {
                try_read_cached_hint(transaction, p)?
                    .map(|(hint, blob)| (hint.sequence_number, blob))
                    .ok_or_else(|| anyhow!("parent {} hint missing during walk for {}", p, oid))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let hint = derive_from_parents(odb, oid, &parents_hint)?;
        store_hint(transaction, oid, hint)?;
    }

    try_read_cached_hint(transaction, input)?
        .ok_or_else(|| anyhow!("missing graph info after walk for {}", input))
}

/// Given that all parents have cached `(seq, roots_blob_oid)`, derive the
/// `(hint, roots_blob_oid)` for `self_oid`. Performs blob I/O
/// only when parents disagree on the blob; otherwise reuses the parent blob
/// OID (or, for the root case, writes a single-element blob).
fn derive_from_parents(
    odb: &josh_memodb::Odb,
    self_oid: gix_hash::ObjectId,
    parents_hint: &[(u64, gix_hash::ObjectId)],
) -> anyhow::Result<(HistoryGraphHint, gix_hash::ObjectId)> {
    if parents_hint.is_empty() {
        // Parentless: this commit *is* its own only reachable root.
        return Ok((
            HistoryGraphHint {
                sequence_number: 0,
                parent_count: 0,
                jump_delta: 0,
                jump_is_second: false,
            },
            write_roots_blob(odb, &[self_oid])?,
        ));
    }

    let parent_count = parents_hint.len().min(255) as u8;

    let seq = parents_hint
        .iter()
        .map(|(s, _)| *s)
        .max()
        .expect("non-empty")
        + 1;

    // Distance to the farther of the first two parents. The max parent is at
    // distance exactly 1, so for two-parent merges this captures the only
    // non-trivial distance; parents beyond the first two are not covered.
    let d0 = seq - parents_hint[0].0;
    let (jump_delta, jump_is_second) = match parents_hint.get(1) {
        Some((s1, _)) if seq - s1 > d0 => ((seq - s1).min(127) as u8, true),
        _ => (d0.min(127) as u8, false),
    };

    let first_blob = parents_hint[0].1;
    let roots_blob = if parents_hint.iter().all(|(_, b)| *b == first_blob) {
        first_blob
    } else {
        let mut set: std::collections::BTreeSet<gix_hash::ObjectId> = Default::default();
        for (_, blob_oid) in parents_hint {
            set.extend(read_roots_blob(odb, *blob_oid)?);
        }
        let roots: Vec<_> = set.into_iter().collect();
        write_roots_blob(odb, &roots)?
    };

    Ok((
        HistoryGraphHint {
            sequence_number: seq,
            parent_count,
            jump_delta,
            jump_is_second,
        },
        roots_blob,
    ))
}

fn try_read_cached_hint(
    transaction: &Transaction,
    input: gix_hash::ObjectId,
) -> anyhow::Result<Option<(HistoryGraphHint, gix_hash::ObjectId)>> {
    let Some(seq) = transaction.get(crate::filter::sequence_number(), input)? else {
        return Ok(None);
    };
    let Some(roots_blob) = transaction.get(crate::filter::reachable_roots(), input)? else {
        return Ok(None);
    };
    Ok(Some((hint_from_oid(seq), roots_blob)))
}

fn store_hint(
    transaction: &Transaction,
    input: gix_hash::ObjectId,
    hint: (HistoryGraphHint, gix_hash::ObjectId),
) -> anyhow::Result<()> {
    let (hint, roots_blob) = hint;
    transaction.insert(
        crate::filter::sequence_number(),
        input,
        oid_from_hint(hint),
        true,
    )?;
    transaction.insert(crate::filter::reachable_roots(), input, roots_blob, true)?;
    Ok(())
}

fn write_roots_blob(
    odb: &josh_memodb::Odb,
    roots: &[gix_hash::ObjectId],
) -> anyhow::Result<gix_hash::ObjectId> {
    let mut bytes = Vec::with_capacity(roots.len() * 20);
    for r in roots {
        bytes.extend_from_slice(r.as_bytes());
    }
    Ok(odb.write(gix_object::Kind::Blob, &bytes))
}

fn read_roots_blob(
    odb: &josh_memodb::Odb,
    oid: gix_hash::ObjectId,
) -> anyhow::Result<Vec<gix_hash::ObjectId>> {
    let (kind, content) = odb.read(oid)?;
    if kind != gix_object::Kind::Blob {
        return Err(anyhow!("reachable_roots object {} is not a blob", oid));
    }
    let content = &content[..];
    if content.len() % 20 != 0 {
        return Err(anyhow!(
            "malformed reachable_roots blob {}: length {} not a multiple of 20",
            oid,
            content.len()
        ));
    }
    let mut out = Vec::with_capacity(content.len() / 20);
    for chunk in content.chunks_exact(20) {
        out.push(gix_hash::ObjectId::try_from(chunk)?);
    }
    Ok(out)
}

/// Encode a [`HistoryGraphHint`] into a 20-byte git OID (SHA-1 sized). Bytes
/// 0-9 of the OID are zero, byte 10 packs the jump: bit 7 set when the jump
/// parent is the second parent, bits 0-6 the jump delta (saturated at 127).
/// Byte 11 holds the parent count (capped at 255) and bytes 12-19 the
/// big-endian sequence number.
pub(crate) fn oid_from_hint(hint: HistoryGraphHint) -> gix_hash::ObjectId {
    let mut bytes = [0u8; 20];
    bytes[10] = ((hint.jump_is_second as u8) << 7) | hint.jump_delta;
    bytes[11] = hint.parent_count;
    // place the 8 integer bytes at the end (big-endian)
    bytes[20 - 8..].copy_from_slice(&hint.sequence_number.to_be_bytes());
    // Safe: length is exactly 20
    gix_hash::ObjectId::from_bytes_or_panic(&bytes)
}

/// Decode a hint from an OID encoded by `oid_from_hint`.
pub(crate) fn hint_from_oid(oid: gix_hash::ObjectId) -> HistoryGraphHint {
    let b = oid.as_bytes();
    let mut n = [0u8; 8];
    n.copy_from_slice(&b[20 - 8..]); // take the last 8 bytes
    HistoryGraphHint {
        sequence_number: u64::from_be_bytes(n),
        parent_count: b[11],
        jump_delta: b[10] & 0x7f,
        jump_is_second: b[10] & 0x80 != 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{HistoryGraphHint, hint_from_oid, oid_from_hint};

    #[test]
    fn oid_hint_roundtrip_uses_last_10_bytes() {
        let hint = HistoryGraphHint {
            sequence_number: 0x0123_4567_89ab_cdef_u64,
            parent_count: 7,
            jump_delta: 42,
            jump_is_second: true,
        };
        let oid = oid_from_hint(hint);
        let bytes = oid.as_bytes();

        assert!(bytes[..10].iter().all(|byte| *byte == 0));
        assert_eq!(bytes[10], 0x80 | 42);
        assert_eq!(bytes[11], 7);
        assert_eq!(&bytes[12..], &hint.sequence_number.to_be_bytes());
        assert_eq!(hint_from_oid(oid), hint);
    }
}
