//! Writing a snapshot of in-memory objects to an on-disk packfile with gix-pack.
//!
//! This is the flusher-side half of [`MemOdb`](crate::mem_odb::MemOdb): it takes a `Send` snapshot
//! of `(oid, kind, bytes)` tuples and produces a `pack-<checksum>.pack`/`.idx` pair in the
//! repository's pack directory, without ever opening a repository handle. Objects already present
//! on disk are filtered out first, so a pack contains only genuinely-new objects and the on-disk
//! layout stays deterministic.

use std::io::{Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use gix_object::Exists;
use gix_pack::data::output;

use crate::mem_odb::Snapshot;

/// The directory where `repo`'s objects live (`<commondir>/objects`), captured at
/// [`MemOdb::new`](crate::mem_odb::MemOdb::new) time while a repository handle exists.
///
/// Resolved from the repository's *common* directory rather than its gitdir: a linked worktree has
/// no `objects/` of its own, so its objects live under the common dir. For a non-worktree repo the
/// two are the same.
pub fn objects_dir(repo: &git2::Repository) -> PathBuf {
    repo.commondir().join("objects")
}

fn pack_error(e: impl std::fmt::Display) -> git2::Error {
    git2::Error::from_str(&format!("mem-odb pack write failed: {e}"))
}

/// Compress and write the objects of `snapshot` that are not already present in `objects_dir`
/// (loose, packed, or via alternates) as a single packfile-plus-index pair in
/// `objects_dir/pack`. A no-op if every object is already on disk.
///
/// The pack is deterministic for a given snapshot: entries are compressed at a fixed level and
/// serialized single-threaded in snapshot order, and the file pair is named after the pack
/// trailer checksum (the same rule libgit2 and modern git use), so identical snapshots produce
/// identical packs. Files are written via tempfile-and-rename, index last, so a concurrent
/// reader scanning for `.idx` files never sees a torn pair.
pub(crate) fn write_snapshot(objects_dir: &Path, snapshot: &Snapshot) -> Result<(), git2::Error> {
    // A fresh store handle per flush observes every pack written by previous flushes. Misses are
    // the expected case below, and the default refresh mode re-lists the pack directory on every
    // miss — disable it; the first lookup still loads all indices present now, and loose-object
    // probes stat the filesystem directly either way.
    let mut odb = gix_odb::at(objects_dir.to_owned()).map_err(pack_error)?;
    odb.refresh = gix_odb::store::RefreshMode::Never;

    let to_pack: Vec<_> = snapshot
        .iter()
        .filter(|(oid, _, _)| !odb.exists(oid))
        .map(|(oid, kind, data)| (*oid, *kind, data))
        .collect();
    if to_pack.is_empty() {
        return Ok(());
    }
    let num_entries = u32::try_from(to_pack.len())
        .map_err(|_| pack_error("more objects in one flush than a pack header can count"))?;

    let pack_dir = objects_dir.join("pack");
    std::fs::create_dir_all(&pack_dir).map_err(pack_error)?;

    // Serialize the pack byte stream (header, compressed entries, checksum trailer) through an
    // anonymous spool file in the pack directory: objects are compressed one at a time as the
    // serializer pulls them, so no more than one compressed object is ever held in memory —
    // a snapshot's size is only *typically* bounded by the store's chunk limit (unbounded stores
    // exist, and the limit is an overflow trigger, not a cap).
    let mut spool = std::io::BufWriter::new(tempfile::tempfile_in(&pack_dir).map_err(pack_error)?);
    let mut iter = output::bytes::FromEntriesIter::new(
        to_pack.iter().map(|(oid, kind, data)| {
            output::Entry::from_data(
                &output::Count::from_data(*oid, None),
                // Fixed level 6, the zlib/libgit2/git `pack.compression` default.
                &gix_object::Data::new(data, *kind, gix_hash::Kind::Sha1),
                gix_zlib::Compression::DEFAULT,
            )
            .map(|entry| vec![entry])
        }),
        &mut spool,
        num_entries,
        gix_pack::data::Version::V2,
        gix_hash::Kind::Sha1,
    );
    for written in &mut iter {
        written.map_err(pack_error)?;
    }
    drop(iter);
    spool.flush().map_err(pack_error)?;
    let mut spool = spool.into_inner().map_err(pack_error)?;
    spool.rewind().map_err(pack_error)?;

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
            compression: gix_zlib::Compression::DEFAULT,
        },
    )
    .map_err(pack_error)?;
    // gix marks the freshly-landed pack with a `.keep` file for the caller to remove once its
    // referencing refs exist. josh's flushes carry no such handshake (libgit2's packbuilder wrote
    // no `.keep` either), and a leftover one would exempt the pack from `git repack -d` forever.
    // Derived from `data_path` rather than taking `outcome.keep_path`: when an earlier attempt
    // persisted the pack but failed before completing (missing idx, crash), the retry finds the
    // pack already present and gets `keep_path: None` — deriving the name still heals the marker
    // the earlier attempt left behind (missing-ok, since that branch usually has none to remove).
    if let Some(data_path) = &outcome.data_path {
        if let Err(e) = std::fs::remove_file(data_path.with_extension("keep")) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(pack_error(e));
            }
        }
    }
    Ok(())
}
