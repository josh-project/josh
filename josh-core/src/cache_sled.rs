use std::sync::LazyLock;

use crate::cache::{CACHE_VERSION, CacheBackend};
use crate::filter::Filter;
use crate::{JoshResult, filter, josh_error};

static DB: LazyLock<std::sync::Mutex<Option<sled::Db>>> = LazyLock::new(Default::default);

pub fn sled_print_stats() -> JoshResult<()> {
    let db = DB.lock()?;
    let db = match db.as_ref() {
        Some(db) => db,
        None => return Err(josh_error("cache not initialized")),
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

pub fn sled_open_josh_trees() -> JoshResult<(sled::Tree, sled::Tree, sled::Tree)> {
    let db = DB.lock()?;
    let db = match db.as_ref() {
        Some(db) => db,
        None => return Err(josh_error("cache not initialized")),
    };

    let path_tree = db.open_tree("_paths")?;
    let invert_tree = db.open_tree("_invert")?;
    let trigram_index_tree = db.open_tree("_trigram_index")?;

    Ok((path_tree, invert_tree, trigram_index_tree))
}

pub fn sled_load(path: &std::path::Path) -> JoshResult<()> {
    let db = sled::Config::default()
        .path(path.join(format!("josh/{}/sled/", CACHE_VERSION)))
        .flush_every_ms(Some(200))
        .open()?;

    *DB.lock()? = Some(db);

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
        _sequence_number: u128,
    ) -> JoshResult<Option<git2::Oid>> {
        let mut trees = self.trees.lock()?;
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
        _sequence_number: u128,
    ) -> JoshResult<()> {
        let mut trees = self.trees.lock()?;
        let tree = trees
            .entry(filter.id())
            .or_insert_with(|| insert_sled_tree(filter));

        tree.insert(from.as_bytes(), to.as_bytes())?;
        Ok(())
    }
}
