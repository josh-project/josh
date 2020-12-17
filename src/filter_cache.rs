use std::collections::HashMap;
use std::sync::{Arc, RwLock};

const FORMAT_VERSION: u64 = 5;

#[derive(Eq, PartialEq, PartialOrd, Hash, Clone, Copy)]
pub struct JoshOid(git2::Oid);

pub type OidMap = HashMap<JoshOid, JoshOid>;

lazy_static! {
    static ref FORWARD_MAPS: Arc<RwLock<FilterCache>> =
        Arc::new(RwLock::new(FilterCache::new("forward".to_owned())));
    static ref BACKWARD_MAPS: Arc<RwLock<FilterCache>> =
        Arc::new(RwLock::new(FilterCache::new("backward".to_owned())));
}

fn forward() -> Arc<RwLock<FilterCache>> {
    FORWARD_MAPS.clone()
}

pub fn backward() -> Arc<RwLock<FilterCache>> {
    BACKWARD_MAPS.clone()
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct FilterCache {
    maps: HashMap<String, OidMap>,

    version: u64,

    #[serde(skip)]
    name: String,

    #[serde(skip)]
    upsteam: Option<Arc<RwLock<FilterCache>>>,
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

pub fn lookup_forward(
    repo: &git2::Repository,
    spec: &str,
    oid: git2::Oid,
) -> Option<git2::Oid> {
    if let Ok(f) = forward().read() {
        if f.has(&repo, &spec, oid) {
            return Some(f.get(&spec, oid));
        }
    }
    return None;
}

impl FilterCache {
    pub fn set(&mut self, filter_spec: &str, from: git2::Oid, to: git2::Oid) {
        let prev = self.get(&filter_spec, from);
        if prev != git2::Oid::zero() {
            return;
        }
        self.maps
            .entry(filter_spec.to_string())
            .or_insert_with(OidMap::new)
            .insert(JoshOid(from), JoshOid(to));
    }

    pub fn get(&self, filter_spec: &str, from: git2::Oid) -> git2::Oid {
        if let Some(m) = self.maps.get(filter_spec) {
            if let Some(JoshOid(oid)) = m.get(&JoshOid(from)).cloned() {
                return oid;
            }
        }
        if filter_spec == ":nop" {
            return from;
        }
        if let Some(upsteam) = self.upsteam.clone() {
            if let Ok(upsteam) = upsteam.read() {
                return upsteam.get(filter_spec, from);
            }
        }
        return git2::Oid::zero();
    }

    pub fn has(
        &self,
        repo: &git2::Repository,
        filter_spec: &str,
        from: git2::Oid,
    ) -> bool {
        if let Some(m) = self.maps.get(filter_spec) {
            if m.contains_key(&JoshOid(from)) {
                // Only report an object as cached if it exists in the object database.
                // This forces a rebuild in case the object was garbage collected.
                let oid = self.get(filter_spec, from);
                return oid == git2::Oid::zero()
                    || repo.odb().unwrap().exists(oid);
            }
        }
        if let Some(upsteam) = self.upsteam.clone() {
            /* let _trace_s = span!(Level::TRACE,"read_lock: has",  ?filter_spec, from=?from.to_string()); */
            return upsteam.read().unwrap().has(repo, filter_spec, from);
        }
        return false;
    }

    fn new(name: String) -> FilterCache {
        return FilterCache {
            name: name,
            maps: HashMap::new(),
            upsteam: None,
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

    pub fn stats(&self) -> HashMap<String, usize> {
        let mut count = 0;
        let mut s = HashMap::new();
        for (filter_spec, m) in self.maps.iter() {
            if m.len() > 1 {
                count += m.len();
                s.insert(filter_spec.to_string(), m.len());
            }
        }
        s.insert("total".to_string(), count);
        return s;
    }
}

#[tracing::instrument]
fn try_load(path: &std::path::Path) -> FilterCache {
    let file_size = std::fs::metadata(&path)
        .map(|x| x.len() / (1024 * 1024))
        .unwrap_or(0);
    tracing::info!("file size: {}", file_size);
    if let Ok(f) = std::fs::File::open(path) {
        if let Ok(m) = bincode::deserialize_from::<_, FilterCache>(f) {
            tracing::info!("mapfile loaded from: {:?}", &path);
            if m.version == FORMAT_VERSION {
                return m;
            } else {
                tracing::info!("mapfile version mismatch: {:?}", &path);
            }
        }
        tracing::error!("deserialize_from: {:?}", &path);
    }
    tracing::info!("no map file loaded from: {:?}", &path);
    FilterCache::new(path.file_name().unwrap().to_string_lossy().to_string())
}

pub fn load(path: &std::path::Path) {
    *(forward().write().unwrap()) = try_load(&path.join("josh_forward_maps"));
    *(backward().write().unwrap()) = try_load(&path.join("josh_backward_maps"));
}

pub fn persist(path: &std::path::Path) {
    persist_file(
        &*super::filter_cache::backward().read().unwrap(),
        &path.join("josh_backward_maps"),
    )
    .ok();
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
    bincode::serialize_into(std::fs::File::create(path)?, &m)?;
    let file_size = std::fs::metadata(&path)
        .map(|x| x.len() / (1024 * 1024))
        .unwrap_or(0);
    tracing::info!("persisted: {:?}, file size: {} MiB", &path, file_size);
    return Ok(());
}

fn try_merge_both(fm: &FilterCache, bm: &FilterCache) {
    rs_tracing::trace_scoped!("merge");
    tracing::span!(tracing::Level::TRACE, "write_lock backward_maps").in_scope(
        || {
            backward()
                .try_write()
                .map(|mut bm_locked| {
                    tracing::span!(
                        tracing::Level::TRACE,
                        "write_lock forward_maps"
                    )
                    .in_scope(|| {
                        forward()
                            .try_write()
                            .map(|mut fm_locked| {
                                bm_locked.merge(&bm);
                                fm_locked.merge(&fm);
                            })
                            .ok();
                    });
                })
                .ok();
        },
    );
}

pub fn new_downstream(u: &Arc<RwLock<FilterCache>>) -> FilterCache {
    return FilterCache {
        maps: HashMap::new(),
        name: u.read().unwrap().name.clone(),
        upsteam: Some(u.clone()),
        version: FORMAT_VERSION,
    };
}

pub struct Transaction {
    fm: FilterCache,
    bm: FilterCache,
    spec: String,
}

impl Transaction {
    pub fn new(spec: String) -> Transaction {
        Transaction {
            fm: new_downstream(&super::filter_cache::forward()),
            bm: new_downstream(&super::filter_cache::backward()),
            spec: spec,
        }
    }

    pub fn insert(&mut self, from: git2::Oid, to: git2::Oid) {
        self.fm.set(&self.spec, from, to);
        if to != git2::Oid::zero() {
            self.bm.set(&self.spec, to, from);
        }
    }

    pub fn has(&self, repo: &git2::Repository, from: git2::Oid) -> bool {
        self.fm.has(&repo, &self.spec, from)
    }

    pub fn get(&self, from: git2::Oid) -> git2::Oid {
        self.fm.get(&self.spec, from)
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        try_merge_both(&self.fm, &self.bm);
    }
}
