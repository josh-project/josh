use std::collections::HashMap;
use std::sync::{Arc, RwLock};

const FORMAT_VERSION: u64 = 6;

#[derive(Eq, PartialEq, PartialOrd, Hash, Clone, Copy)]
struct JoshOid(git2::Oid);

type OidMap = HashMap<JoshOid, JoshOid>;

lazy_static! {
    static ref FORWARD_MAPS: Arc<RwLock<FilterCache>> =
        Arc::new(RwLock::new(FilterCache::new()));
}

fn forward() -> Arc<RwLock<FilterCache>> {
    FORWARD_MAPS.clone()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FilterCache {
    maps: HashMap<String, OidMap>,

    version: u64,
}

impl serde::ser::Serialize for JoshOid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let JoshOid(oid) = *self;
        serializer.serialize_bytes(oid.as_bytes())
    }
}

struct OidVisitor;

impl<'de> serde::de::Visitor<'de> for OidVisitor {
    type Value = JoshOid;

    fn expecting(
        &self,
        formatter: &mut std::fmt::Formatter,
    ) -> std::fmt::Result {
        formatter.write_str("20 bytes")
    }

    fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if let Ok(oid) = git2::Oid::from_bytes(value) {
            Ok(JoshOid(oid))
        } else {
            Err(E::custom("err: invalid oid"))
        }
    }
}

impl<'de> serde::de::Deserialize<'de> for JoshOid {
    fn deserialize<D>(deserializer: D) -> Result<JoshOid, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        deserializer.deserialize_bytes(OidVisitor)
    }
}

impl FilterCache {
    fn set(&mut self, filter_spec: &str, from: git2::Oid, to: git2::Oid) {
        self.maps
            .entry(filter_spec.to_string())
            .or_insert_with(|| {
                log::debug!("new spec {:?}", filter_spec);
                OidMap::new()
            })
            .insert(JoshOid(from), JoshOid(to));
    }

    fn get(&self, filter_spec: &str, from: git2::Oid) -> git2::Oid {
        if let Some(m) = self.maps.get(filter_spec) {
            if let Some(JoshOid(oid)) = m.get(&JoshOid(from)).cloned() {
                return oid;
            }
        }
        if filter_spec == ":nop" {
            return from;
        }
        if self.version == 0 {
            if let Ok(r) = forward().read() {
                return r.get(filter_spec, from);
            }
        }
        return git2::Oid::zero();
    }

    fn has(
        &self,
        repo: &git2::Repository,
        filter_spec: &str,
        from: git2::Oid,
    ) -> bool {
        if filter_spec == ":nop" {
            return true;
        }
        if let Some(m) = self.maps.get(filter_spec) {
            if m.contains_key(&JoshOid(from)) {
                // Only report an object as cached if it exists in the object database.
                // This forces a rebuild in case the object was garbage collected.
                let oid = self.get(filter_spec, from);
                return oid == git2::Oid::zero()
                    || repo.odb().unwrap().exists(oid);
            }
        }
        if self.version == 0 {
            if let Ok(r) = forward().read() {
                return r.has(repo, filter_spec, from);
            }
        }
        return false;
    }

    fn new() -> FilterCache {
        return FilterCache {
            maps: HashMap::new(),
            version: FORMAT_VERSION,
        };
    }

    fn merge(&mut self, other: &FilterCache) {
        for (filter_spec, om) in other.maps.iter() {
            let m = self
                .maps
                .entry(filter_spec.to_string())
                .or_insert_with(OidMap::new);
            m.extend(om);
        }
    }
}

#[tracing::instrument]
fn try_load(path: &std::path::Path) -> FilterCache {
    log::debug!("load file");
    let file_size = std::fs::metadata(&path)
        .map(|x| x.len() / (1024 * 1024))
        .unwrap_or(0);
    tracing::info!("file size: {}", file_size);
    if let Ok(f) = std::fs::File::open(path) {
        if let Ok(m) = bincode::deserialize_from::<_, FilterCache>(f) {
            tracing::info!("mapfile loaded from: {:?}", &path);
            if m.version == FORMAT_VERSION {
                log::debug!("version ok");
                return m;
            } else {
                log::debug!("version mismatch");
                tracing::info!("mapfile version mismatch: {:?}", &path);
            }
        }
        tracing::error!("deserialize_from: {:?}", &path);
    }
    tracing::info!("no map file loaded from: {:?}", &path);
    FilterCache::new()
}

pub fn load(path: &std::path::Path) {
    *(forward().write().unwrap()) = try_load(&path.join("josh_forward_maps"));
}

pub fn persist(path: &std::path::Path) {
    persist_file(
        &*super::filter_cache::forward().read().unwrap(),
        &path.join("josh_forward_maps"),
    )
    .ok();
}
#[tracing::instrument(skip(m))]
fn persist_file(
    m: &FilterCache,
    path: &std::path::Path,
) -> crate::JoshResult<()> {
    log::debug!("persist_file");
    bincode::serialize_into(std::fs::File::create(path)?, &m)?;
    let file_size = std::fs::metadata(&path)
        .map(|x| x.len() / (1024 * 1024))
        .unwrap_or(0);
    tracing::info!("persisted: {:?}, file size: {} MiB", &path, file_size);
    return Ok(());
}

struct Stats {
    missed: usize,
    hit: usize,
    insert: usize,
}

pub struct Transaction {
    fm: FilterCache,
    stats: std::cell::RefCell<Stats>,
}

impl Transaction {
    pub fn new() -> Transaction {
        log::debug!("new transaction");
        Transaction {
            fm: FilterCache {
                maps: HashMap::new(),
                version: 0,
            },

            stats: std::cell::RefCell::new(Stats {
                missed: 0,
                hit: 0,
                insert: 0,
            }),
        }
    }

    pub fn insert(&mut self, spec: &str, from: git2::Oid, to: git2::Oid) {
        self.fm.set(spec, from, to);
        self.stats.borrow_mut().insert += 1;
    }

    pub fn has(
        &self,
        spec: &str,
        repo: &git2::Repository,
        from: git2::Oid,
    ) -> bool {
        let r = self.fm.has(&repo, spec, from);
        if r {
            self.stats.borrow_mut().hit += 1;
        } else {
            self.stats.borrow_mut().missed += 1;
        }
        return r;
    }

    pub fn get(&self, spec: &str, from: git2::Oid) -> git2::Oid {
        self.fm.get(spec, from)
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        rs_tracing::trace_scoped!("merge");
        let stats = self.stats.borrow();
        log::debug!(
            "/Transaction hit: {}, miss: {}, insert: {}",
            stats.hit,
            stats.missed,
            stats.insert
        );
        let s =
            tracing::span!(tracing::Level::TRACE, "write_lock forward_maps");
        let _e = s.enter();
        forward()
            .try_write()
            .map(|mut fm_locked| {
                fm_locked.merge(&self.fm);
            })
            .ok();
    }
}
