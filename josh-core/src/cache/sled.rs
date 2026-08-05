use anyhow::anyhow;
use std::sync::LazyLock;

use super::CACHE_VERSION;
use super::backend::{CacheBackend, HistoryGraphHint};
use crate::filter;
use crate::filter::Filter;

static DB: LazyLock<std::sync::Mutex<Option<sled::Db>>> = LazyLock::new(Default::default);

/// Remove all entries from every tree in the global on-disk cache.
///
/// This nukes the persistent sled cache. Doing so is safe: the sled cache is
/// ephemeral and, when configured, backed by a remote cache. Primarily intended
/// for benchmarks and tests that need a cold cache between runs. Does nothing if
/// the cache has not been loaded.
pub fn sled_clear() -> anyhow::Result<()> {
    let db = DB.lock().unwrap();
    let Some(db) = db.as_ref() else {
        return Ok(());
    };

    for name in db.tree_names() {
        db.open_tree(&name)?.clear()?;
    }

    Ok(())
}

pub fn sled_print_stats() -> anyhow::Result<()> {
    let db = DB.lock().unwrap();
    let db = match db.as_ref() {
        Some(db) => db,
        None => return Err(anyhow!("cache not initialized")),
    };

    db.flush()?;
    log::debug!("Trees:");

    let mut v = vec![];
    for name in db.tree_names() {
        let name = String::from_utf8(name.to_vec())?;
        let t = db.open_tree(&name)?;

        if !t.is_empty() {
            let name = if let Ok(filter) = filter::parse(&name) {
                filter::pretty(filter, 4)
            } else {
                name.clone()
            };
            v.push((t.len(), name));
        }
    }

    v.sort();

    for (len, name) in v.iter() {
        println!("[{}] {}", len, name);
    }

    Ok(())
}

pub fn sled_open_josh_trees() -> anyhow::Result<(sled::Tree, sled::Tree, sled::Tree)> {
    let db = DB.lock().unwrap();
    let db = match db.as_ref() {
        Some(db) => db,
        None => return Err(anyhow!("cache not initialized")),
    };

    let path_tree = db.open_tree("_paths")?;
    let invert_tree = db.open_tree("_invert")?;
    let trigram_index_tree = db.open_tree("_trigram_index")?;

    Ok((path_tree, invert_tree, trigram_index_tree))
}

/// Flush any pending writes of the global cache db to disk. No-op if the cache is not loaded.
pub fn sled_flush() -> anyhow::Result<()> {
    if let Some(db) = DB.lock().unwrap().as_ref() {
        db.flush()?;
    }
    Ok(())
}

/// Drop the global cache db, releasing sled's exclusive file lock on the cache directory. Any
/// remaining `sled::Tree` handles keep the underlying store (and its lock) alive, so callers must
/// drop those first; see [`crate::cache::Transaction::release_cache`].
pub fn sled_unload() {
    *DB.lock().unwrap() = None;
}

pub fn sled_load(path: &std::path::Path) -> anyhow::Result<()> {
    let db = sled::Config::default()
        .path(path.join(format!("josh/cache/{}/sled/", CACHE_VERSION)))
        .flush_every_ms(Some(200))
        .open()?;

    *DB.lock().unwrap() = Some(db);

    Ok(())
}

#[derive(Default)]
pub struct SledCacheBackend {
    trees: std::sync::Mutex<std::collections::HashMap<git2::Oid, sled::Tree>>,
}

fn insert_sled_tree(filter: Filter) -> sled::Tree {
    DB.lock()
        .unwrap()
        .as_ref()
        .expect("Sled DB not initialized")
        .open_tree(filter::spec(filter))
        .expect("Failed to insert Sled tree")
}

impl CacheBackend for SledCacheBackend {
    fn read(
        &self,
        filter: Filter,
        from: git2::Oid,
        _hint: HistoryGraphHint,
    ) -> anyhow::Result<Option<git2::Oid>> {
        let mut trees = self.trees.lock().unwrap();
        let tree = trees
            .entry(filter.id())
            .or_insert_with(|| insert_sled_tree(filter));

        if let Some(oid) = tree.get(from.as_bytes())? {
            let oid = git2::Oid::from_bytes(&oid)?;
            Ok(Some(oid))
        } else {
            Ok(None)
        }
    }

    fn write(
        &self,
        filter: Filter,
        from: git2::Oid,
        to: git2::Oid,
        _hint: HistoryGraphHint,
    ) -> anyhow::Result<()> {
        let mut trees = self.trees.lock().unwrap();
        let tree = trees
            .entry(filter.id())
            .or_insert_with(|| insert_sled_tree(filter));

        tree.insert(from.as_bytes(), to.as_bytes())?;
        Ok(())
    }

    /// Drop the cached per-filter tree handles so that, once the global db is unloaded, sled's file
    /// lock is released. A later read/write transparently reopens the trees.
    fn release(&self) {
        self.trees.lock().unwrap().clear();
    }
}
