extern crate tracing;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use self::tracing::{error, info};

#[derive(Eq, PartialEq, PartialOrd, Hash, Clone, Copy)]
pub struct ViewMapOid(git2::Oid);

pub type ViewMap = HashMap<ViewMapOid, ViewMapOid>;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ViewMaps {
    maps: HashMap<String, ViewMap>,

    #[serde(skip)]
    upsteam: Option<Arc<RwLock<ViewMaps>>>,
}

impl serde::ser::Serialize for ViewMapOid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let ViewMapOid(oid) = *self;
        serializer.serialize_bytes(oid.as_bytes())
    }
}

struct OidVisitor;

impl<'de> serde::de::Visitor<'de> for OidVisitor {
    type Value = ViewMapOid;

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
            Ok(ViewMapOid(oid))
        } else {
            Err(E::custom("err: invalid oid"))
        }
    }
}

impl<'de> serde::de::Deserialize<'de> for ViewMapOid {
    fn deserialize<D>(deserializer: D) -> Result<ViewMapOid, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        deserializer.deserialize_bytes(OidVisitor)
    }
}

impl ViewMaps {
    pub fn set(&mut self, viewstr: &str, from: git2::Oid, to: git2::Oid) {
        self.maps
            .entry(viewstr.to_string())
            .or_insert_with(ViewMap::new)
            .insert(ViewMapOid(from), ViewMapOid(to));
    }

    pub fn get(&self, viewstr: &str, from: git2::Oid) -> git2::Oid {
        if let Some(m) = self.maps.get(viewstr) {
            if let Some(ViewMapOid(oid)) = m.get(&ViewMapOid(from)).cloned() {
                return oid;
            }
        }
        if let Some(upsteam) = self.upsteam.clone() {
            return upsteam.read().unwrap().get(viewstr, from);
        }
        if viewstr == ":nop=nop" {
            return from;
        }
        return git2::Oid::zero();
    }

    pub fn has(
        &self,
        repo: &git2::Repository,
        viewstr: &str,
        from: git2::Oid,
    ) -> bool {
        if let Some(m) = self.maps.get(viewstr) {
            if m.contains_key(&ViewMapOid(from)) {
                // Only report an object as cached if it exists in the object database.
                // This forces a rebuild in case the object was garbage collected.
                let oid = self.get(viewstr, from);
                return oid == git2::Oid::zero()
                    || repo.odb().unwrap().exists(oid);
            }
        }
        if let Some(upsteam) = self.upsteam.clone() {
            /* let _trace_s = span!(Level::TRACE,"read_lock: has",  ?viewstr, from=?from.to_string()); */
            return upsteam.read().unwrap().has(repo, viewstr, from);
        }
        return false;
    }

    pub fn new() -> ViewMaps {
        return ViewMaps {
            maps: HashMap::new(),
            upsteam: None,
        };
    }

    pub fn new_downstream(u: Arc<RwLock<ViewMaps>>) -> ViewMaps {
        return ViewMaps {
            maps: HashMap::new(),
            upsteam: Some(u),
        };
    }

    pub fn merge(&mut self, other: &ViewMaps) {
        for (viewstr, om) in other.maps.iter() {
            let m = self
                .maps
                .entry(viewstr.to_string())
                .or_insert_with(ViewMap::new);
            m.extend(om);
        }
    }

    pub fn stats(&self) -> HashMap<String, usize> {
        let mut count = 0;
        let mut s = HashMap::new();
        for (viewstr, m) in self.maps.iter() {
            if m.len() > 1 {
                count += m.len();
                s.insert(viewstr.to_string(), m.len());
            }
        }
        s.insert("total".to_string(), count);
        return s;
    }
}

pub fn try_load(path: &std::path::Path) -> ViewMaps {
    info!("trying to load: {:?}", &path);
    if let Ok(f) = std::fs::File::open(path) {
        if let Ok(m) = bincode::deserialize_from(f) {
            info!("mapfile loaded from: {:?}", &path);
            return m;
        }
        error!("deserialize_from: {:?}", &path);
    }
    info!("no map file loaded from: {:?}", &path);
    ViewMaps::new()
}

pub fn persist(m: &ViewMaps, path: &std::path::Path) {
    info!("persisting: {:?}", &path);
    let f = ok_or!(tempfile::NamedTempFile::new_in(path.parent().unwrap()), {
        error!("NamedTempFile::new");
        return;
    });

    ok_or!(bincode::serialize_into(&f, &m), {
        error!("serialize_into: {:?}", &path);
        return;
    });

    ok_or!(f.persist(path), {
        error!("persist: {:?}", &path);
        return;
    });
    info!("persisted: {:?}", &path);
}
