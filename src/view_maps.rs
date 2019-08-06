use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub type ViewMap = HashMap<git2::Oid, git2::Oid>;

pub struct ViewMaps {
    maps: HashMap<String, ViewMap>,
    upsteam: Option<Arc<RwLock<ViewMaps>>>,
}

impl ViewMaps {
    pub fn set(&mut self, viewstr: &str, from: git2::Oid, to: git2::Oid) {
        if self.has(&viewstr, from) {

            /* return; */
            /* if self.get(&viewstr, from) != to */
            /* { */
            /*     println!("{:?} {:?} {:?}", from, self.get(&viewstr, from), to); */
            /* } */
        }
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
            trace_scoped!("read_lock: get", "viewstr": viewstr, "from": from.to_string());
            return upsteam.read().unwrap().get(viewstr, from);
        }
        return git2::Oid::zero();
    }

    pub fn has(&self, viewstr: &str, from: git2::Oid) -> bool {
        if let Some(m) = self.maps.get(viewstr) {
            if m.contains_key(&from) {
                return true;
            }
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

    pub fn stats(&self) -> HashMap<String, usize> {
        let mut s = HashMap::new();
        for (viewstr, m) in self.maps.iter() {
            s.insert(viewstr.to_string(), m.len());
        }
        return s;
    }
}
