extern crate tracing;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/* use self::tracing::{span, Level}; */

pub type ViewMap = HashMap<git2::Oid, git2::Oid>;

pub struct ViewMaps {
    maps: HashMap<String, ViewMap>,
    upsteam: Option<Arc<RwLock<ViewMaps>>>,
}

impl ViewMaps {
    pub fn set(&mut self, viewstr: &str, from: git2::Oid, to: git2::Oid) {
        self.maps
            .entry(viewstr.to_string())
            .or_insert_with(ViewMap::new)
            .insert(from, to);
    }

    pub fn get(&self, viewstr: &str, from: git2::Oid) -> git2::Oid {
        if let Some(m) = self.maps.get(viewstr) {
            if let Some(oid) = m.get(&from).cloned() {
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
            if m.contains_key(&from) {
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
