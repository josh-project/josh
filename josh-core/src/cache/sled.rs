use anyhow::anyhow;
use std::sync::LazyLock;

use super::CACHE_VERSION;
use super::backend::{CacheBackend, HistoryGraphHint};
use crate::filter;
use crate::filter::Filter;

/// Process-wide state of the on-disk sled cache.
///
/// sled takes a process-exclusive file lock on the cache directory and permits only one open handle
/// per path per process, so the db is a single shared instance rather than per-backend -- every
/// [`SledCacheBackend`] forwards here. Its lifetime is driven by `active`, the number of live
/// transactions (plus any [`SledCacheBackend::pin`]): the db is opened lazily on the first
/// transaction and dropped once the last one ends, releasing the lock.
struct State {
    /// Cache directory, set by [`SledCacheBackend::new`]. `None` until the first backend is built.
    path: Option<std::path::PathBuf>,
    db: Option<sled::Db>,
    trees: std::collections::HashMap<gix_hash::ObjectId, sled::Tree>,
    active: usize,
}

static STATE: LazyLock<std::sync::Mutex<State>> = LazyLock::new(|| {
    std::sync::Mutex::new(State {
        path: None,
        db: None,
        trees: Default::default(),
        active: 0,
    })
});

impl State {
    /// Open the db if it isn't already. Errors if no [`SledCacheBackend::new`] ever configured a
    /// cache path.
    fn ensure_open(&mut self) -> anyhow::Result<&sled::Db> {
        if self.db.is_none() {
            let path = self
                .path
                .as_ref()
                .ok_or_else(|| anyhow!("sled cache path not configured"))?;
            self.db = Some(
                sled::Config::default()
                    .path(path.join(format!("josh/cache/{}/sled/", CACHE_VERSION)))
                    .flush_every_ms(Some(200))
                    .open()?,
            );
        }
        Ok(self.db.as_ref().unwrap())
    }

    fn tree(&mut self, filter: Filter) -> anyhow::Result<sled::Tree> {
        if let Some(tree) = self.trees.get(&filter.id()) {
            return Ok(tree.clone());
        }
        let tree = self.ensure_open()?.open_tree(filter::spec(filter))?;
        self.trees.insert(filter.id(), tree.clone());
        Ok(tree)
    }

    /// Flush pending writes, then drop the cached tree handles and the db, releasing sled's file
    /// lock on the cache directory.
    fn close(&mut self) {
        if let Some(Err(e)) = self.db.as_ref().map(|db| db.flush()) {
            log::error!("failed to flush sled cache: {e}");
        }
        self.trees.clear();
        self.db = None;
    }
}

/// Per-filter on-disk cache backed by sled.
///
/// The backend is a thin handle onto the process-wide sled db (see [`State`]); the db itself is
/// opened lazily and dropped automatically once no transaction is active. Constructing a backend
/// with [`SledCacheBackend::new`] configures the cache directory; a long-running process (the proxy)
/// that wants the cache hot for its whole lifetime additionally calls [`SledCacheBackend::pin`].
#[derive(Clone, Default)]
pub struct SledCacheBackend;

impl SledCacheBackend {
    /// Configure the on-disk cache directory and return a backend handle. Must be called before any
    /// transaction opens; later calls repoint the shared cache path.
    pub fn new(path: impl AsRef<std::path::Path>) -> Self {
        STATE.lock().unwrap().path = Some(path.as_ref().to_path_buf());
        Self
    }

    /// Keep the db open for the lifetime of the process regardless of transaction activity. This is
    /// an unbalanced [`CacheBackend::begin`]: it opens the db and bumps the active count once
    /// without a matching `end`, so the cache stays hot and the lock is held until the process
    /// exits.
    pub fn pin(&self) -> anyhow::Result<()> {
        let mut state = STATE.lock().unwrap();
        state.ensure_open()?;
        state.active += 1;
        Ok(())
    }
}

impl CacheBackend for SledCacheBackend {
    fn read(
        &self,
        filter: Filter,
        from: gix_hash::ObjectId,
        _hint: HistoryGraphHint,
        // Sled stores records by filter alone, so the key kind needs no special handling.
        _tree_keyed: bool,
    ) -> anyhow::Result<Option<gix_hash::ObjectId>> {
        let tree = STATE.lock().unwrap().tree(filter)?;
        if let Some(oid) = tree.get(from.as_bytes())? {
            Ok(Some(gix_hash::ObjectId::try_from(&oid[..])?))
        } else {
            Ok(None)
        }
    }

    fn write(
        &self,
        filter: Filter,
        from: gix_hash::ObjectId,
        to: gix_hash::ObjectId,
        _hint: HistoryGraphHint,
        _tree_keyed: bool,
    ) -> anyhow::Result<()> {
        let tree = STATE.lock().unwrap().tree(filter)?;
        tree.insert(from.as_bytes(), to.as_bytes())?;
        Ok(())
    }

    /// Open the db (if needed) and count this transaction as active.
    fn begin(&self) {
        let mut state = STATE.lock().unwrap();
        if let Err(e) = state.ensure_open() {
            log::error!("failed to open sled cache: {e}");
            return;
        }
        state.active += 1;
    }

    /// Drop this transaction from the active count; when the last one ends, flush and close the db,
    /// releasing sled's exclusive lock. A later [`CacheBackend::begin`] reopens it transparently.
    fn end(&self) {
        let mut state = STATE.lock().unwrap();
        state.active = state.active.saturating_sub(1);
        if state.active == 0 {
            state.close();
        }
    }
}

/// Run a maintenance closure against the shared db, opening it temporarily if no transaction is
/// holding it open and closing it again afterwards. Returns `None` if no cache path is configured.
fn with_maintenance_db<T>(
    f: impl FnOnce(&sled::Db) -> anyhow::Result<T>,
) -> anyhow::Result<Option<T>> {
    let mut state = STATE.lock().unwrap();
    if state.path.is_none() {
        return Ok(None);
    }
    let opened_here = state.db.is_none();
    let result = f(state.ensure_open()?);
    // If we opened the db just for this maintenance op, close it again to keep the "open only while
    // a transaction is active" invariant.
    if opened_here && state.active == 0 {
        state.close();
    }
    result.map(Some)
}

/// Remove all entries from every tree in the on-disk cache.
///
/// This nukes the persistent sled cache. Doing so is safe: the sled cache is
/// ephemeral and, when configured, backed by a remote cache. Primarily intended
/// for benchmarks and tests that need a cold cache between runs. Does nothing if
/// no cache directory has been configured.
pub fn sled_clear() -> anyhow::Result<()> {
    with_maintenance_db(|db| {
        for name in db.tree_names() {
            db.open_tree(&name)?.clear()?;
        }
        Ok(())
    })?;
    Ok(())
}

pub fn sled_print_stats() -> anyhow::Result<()> {
    let printed = with_maintenance_db(|db| {
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
    })?;

    if printed.is_none() {
        return Err(anyhow!("cache not initialized"));
    }

    Ok(())
}
