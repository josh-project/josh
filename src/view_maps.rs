use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use git2::*;

pub type ViewMap = HashMap<Oid, Oid>;

pub struct ViewMaps {
    maps: HashMap<String, ViewMap>,
    upsteam: Option<Arc<RwLock<ViewMaps>>>,
}

impl ViewMaps {
    pub fn set(&mut self, viewstr: &str, from: Oid, to: Oid) {
        let mut m = self
            .maps
            .entry(viewstr.to_string())
            .or_insert_with(ViewMap::new);
        m.insert(from, to);
    }

    pub fn get(&self, viewstr: &str, from: Oid) -> Oid {
        if let Some(m) = self.maps.get(viewstr) {
            return m.get(&from).cloned().unwrap_or_else(Oid::zero);
        }
        if let Some(upsteam) = self.upsteam.clone() {
            trace_scoped!("read_lock: get", "viewstr": viewstr, "from": from.to_string());
            return upsteam.read().unwrap().get(viewstr, from);
        }
        return Oid::zero();
    }

    pub fn has(&self, viewstr: &str, from: Oid) -> bool {
        if let Some(m) = self.maps.get(viewstr) {
            return m.contains_key(&from);
        }
        if let Some(upsteam) = self.upsteam.clone() {
            trace_scoped!("read_lock: has", "viewstr": viewstr, "from": from.to_string());
            return upsteam.read().unwrap().has(viewstr, from);
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
            let mut m = self
                .maps
                .entry(viewstr.to_string())
                .or_insert_with(ViewMap::new);
            m.extend(om);
        }
    }
}
