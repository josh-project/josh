//! Writing a snapshot of in-memory objects to an on-disk packfile with gix-pack.
//!
//! This is the flusher-side half of [`MemOdb`](crate::mem_odb::MemOdb): it takes a `Send` snapshot
//! of `(oid, kind, bytes)` tuples and produces a `pack-<checksum>.pack`/`.idx` pair in the
//! repository's pack directory, without ever opening a repository handle. Objects already present
//! on disk are filtered out first, so a pack contains only genuinely-new objects and the on-disk
//! layout stays deterministic.

use anyhow::Context;
use std::io::{Seek, Write};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use gix_object::Exists;
use gix_pack::data::output;

use crate::mem_odb::Snapshot;

#[cfg(test)]
pub(crate) fn objects_dir(repo: &gix::Repository) -> std::path::PathBuf {
    repo.common_dir().join("objects")
}

/// Compress and write the objects of `snapshot` that are not already present in `objects_dir`
/// (loose, packed, or via alternates) as a single packfile-plus-index pair in
/// `objects_dir/pack`. A no-op if every object is already on disk.
///
/// The pack is deterministic for a given snapshot: entries use the fastest zlib level and are
/// serialized single-threaded in snapshot order, and the file pair is named after the pack
/// trailer checksum, so identical snapshots produce identical packs. Files are written via
/// tempfile-and-rename, index last, so a concurrent reader never sees a torn pair.
pub(crate) fn write_snapshot(objects_dir: &Path, snapshot: &Snapshot) -> anyhow::Result<()> {
    // A fresh store handle per flush observes every pack written by previous flushes. Misses are
    // the expected case below, and the default refresh mode re-lists the pack directory on every
    // miss — disable it; the first lookup still loads all indices present now, and loose-object
    // probes stat the filesystem directly either way.
    let mut odb = gix_odb::at(objects_dir.to_owned()).context("mem-odb pack write failed")?;
    odb.refresh = gix_odb::store::RefreshMode::Never;

    let to_pack: Vec<_> = snapshot
        .iter()
        .filter(|(oid, _, _)| !odb.exists(oid))
        .map(|(oid, kind, data)| (*oid, *kind, data))
        .collect();
    if to_pack.is_empty() {
        return Ok(());
    }
    let num_entries = u32::try_from(to_pack.len()).context("mem-odb pack write failed")?;

    let pack_dir = objects_dir.join("pack");
    std::fs::create_dir_all(&pack_dir).context("mem-odb pack write failed")?;

    // Serialize the pack byte stream (header, compressed entries, checksum trailer) through an
    // anonymous spool file in the pack directory: objects are compressed one at a time as the
    // serializer pulls them, so no more than one compressed object is ever held in memory —
    // a snapshot's size is only *typically* bounded by the store's chunk limit (unbounded stores
    // exist, and the limit is an overflow trigger, not a cap).
    let mut spool = std::io::BufWriter::new(
        tempfile::tempfile_in(&pack_dir).context("mem-odb pack write failed")?,
    );
    let mut iter = output::bytes::FromEntriesIter::new(
        to_pack.iter().map(|(oid, kind, data)| {
            output::Entry::from_data(
                &output::Count::from_data(*oid, None),
                // Favor request latency; maintenance repacks can optimize the durable ratio.
                &gix_object::Data::new(data, *kind, gix_hash::Kind::Sha1),
                gix_zlib::Compression::BEST_SPEED,
            )
            .map(|entry| vec![entry])
        }),
        &mut spool,
        num_entries,
        gix_pack::data::Version::V2,
        gix_hash::Kind::Sha1,
    );
    for written in &mut iter {
        written.context("mem-odb pack write failed")?;
    }
    drop(iter);
    spool.flush().context("mem-odb pack write failed")?;
    let mut spool = spool.into_inner().context("mem-odb pack write failed")?;
    spool.rewind().context("mem-odb pack write failed")?;

    let outcome = gix_pack::Bundle::write_to_directory(
        &mut std::io::BufReader::new(spool),
        Some(&pack_dir),
        &mut gix_features::progress::Discard,
        &AtomicBool::new(false),
        None::<gix_object::find::Never>,
        gix_pack::bundle::write::Options {
            thread_limit: Some(1),
            iteration_mode: gix_pack::data::input::Mode::Verify,
            index_version: gix_pack::index::Version::V2,
            object_hash: gix_hash::Kind::Sha1,
            alloc_limit_bytes: None,
            // Only used to complete thin packs, which this never writes.
            compression: gix_zlib::Compression::BEST_SPEED,
        },
    )
    .context("mem-odb pack write failed")?;
    // gix marks the freshly-landed pack with a `.keep` file. Josh currently assumes that its
    // object directories are maintained only by the commands in `josh-proxy::housekeeping`:
    // mirror repacks use `--keep-unreachable`, and overlay repacks use `--cruft`, so both preserve
    // objects from a shared-store flush whose refs are not published yet. If arbitrary concurrent
    // pruning becomes supported, retain this marker until every transaction sharing the MemOdb
    // has published its refs, or give each transaction its own buffer.
    //
    // Derive the marker from `data_path` instead of taking `outcome.keep_path`. If an earlier
    // attempt persisted the pack but failed before completing, a retry can find the pack already
    // present and return no keep path; deriving the name also heals that leftover marker.
    if let Some(data_path) = &outcome.data_path
        && let Err(error) = std::fs::remove_file(data_path.with_extension("keep"))
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error).context("mem-odb pack write failed");
    }
    Ok(())
}
