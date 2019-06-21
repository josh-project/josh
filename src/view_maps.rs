use std::collections::HashMap;

use git2::*;

pub type ViewMap = HashMap<Oid, Oid>;

pub struct ViewMaps {
    maps: HashMap<String, ViewMap>,
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
        return Oid::zero();
    }

    pub fn has(&self, viewstr: &str, from: Oid) -> bool {
        if let Some(m) = self.maps.get(viewstr) {
            return m.contains_key(&from);
        }
        return false;
    }

    pub fn new() -> ViewMaps {
        return ViewMaps {
            maps: HashMap::new(),
        };
    }
}

