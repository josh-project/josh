#![allow(unused_variables)]

use anyhow::anyhow;
use josh_core::filter::Rewrite;
use josh_core::filter::tree;
use josh_core::objects;
use josh_core::objects::CommitData;
use josh_core::{cache, filter, history};
use juniper::{EmptyMutation, EmptySubscription, FieldResult, graphql_object};
use std::str::FromStr;

pub struct Revision {
    filter: filter::Filter,
    commit_id: gix_hash::ObjectId,
}

fn find_paths(
    transaction: &cache::Transaction,
    odb: &josh_core::memodb::Odb,
    tree: gix_hash::ObjectId,
    at: Option<String>,
    depth: Option<i32>,
    kind: gix_object::Kind,
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let tree = match at.as_deref() {
        Some(at) if !at.is_empty() => {
            let entry = tree::get_path_entry(transaction, odb, tree, std::path::Path::new(at))?
                .ok_or_else(|| anyhow!("no such path: {}", at))?;
            if !entry.mode.is_tree() {
                return Err(anyhow!("not a directory: {}", at));
            }
            entry.oid.to_owned()
        }
        _ => tree,
    };

    let base = std::path::Path::new(&at.as_ref().unwrap_or(&"".to_string())).to_owned();

    let mut ws = vec![];
    collect_paths(transaction, odb, tree, &base, 1, depth, kind, &mut ws)?;
    Ok(ws)
}

/// Collect paths of entries of the requested kind. A depth-limited listing never reads the
/// subtrees it could not report.
fn collect_paths(
    transaction: &cache::Transaction,
    odb: &josh_core::memodb::Odb,
    tree: gix_hash::ObjectId,
    prefix: &std::path::Path,
    level: i32,
    depth: Option<i32>,
    kind: gix_object::Kind,
    out: &mut Vec<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let reader = tree::read_tree(transaction, odb, tree)?;
    for entry in reader.entries() {
        let Ok(name) = std::str::from_utf8(entry.filename) else {
            continue;
        };
        let path = prefix.join(name);
        let is_tree = entry.mode.is_tree();
        // Gitlinks are neither blobs nor trees, so they are never listed.
        let matches = if is_tree {
            kind == gix_object::Kind::Tree
        } else {
            kind == gix_object::Kind::Blob && !entry.mode.is_commit()
        };
        if matches && depth.is_none_or(|limit| level <= limit) {
            out.push(path.clone());
        }
        if is_tree && depth.is_none_or(|limit| level < limit) {
            collect_paths(
                transaction,
                odb,
                entry.oid.to_owned(),
                &path,
                level + 1,
                depth,
                kind,
                out,
            )?;
        }
    }
    Ok(())
}

/// Apply the filter to `commit_id` and read back the commit it produced.
fn filtered_commit(
    transaction: &cache::Transaction,
    filter: filter::Filter,
    commit_id: gix_hash::ObjectId,
) -> anyhow::Result<CommitData> {
    let filtered = filter::apply_to_commit(filter, commit_id, transaction)?;
    CommitData::read(transaction.odb(), filtered)
}

pub struct DiffPath {
    a: Option<Path>,
    b: Option<Path>,
}

#[graphql_object(context = Context)]
impl DiffPath {
    fn from(&self) -> FieldResult<Option<Path>> {
        Ok(self.a.clone())
    }

    fn to(&self) -> FieldResult<Option<Path>> {
        Ok(self.b.clone())
    }
}

impl Revision {
    fn files_or_dirs(
        &self,
        at: Option<String>,
        depth: Option<i32>,
        context: &Context,
        kind: gix_object::Kind,
    ) -> FieldResult<Option<Vec<Path>>> {
        let transaction = context.transaction.lock().unwrap();
        let odb = transaction.odb();
        let commit = CommitData::read(odb, self.commit_id)?;
        let x = filter::apply(
            &transaction,
            self.filter,
            Rewrite::from_tree(commit.tree_id()?),
        )?;
        let tree_id = x.tree_id();
        let paths = find_paths(&transaction, odb, tree_id, at, depth, kind)?;
        let mut ws = vec![];
        for path in paths {
            ws.push(Path {
                path,
                commit_id: self.commit_id,
                filter: self.filter,
                tree: tree_id,
            });
        }
        Ok(Some(ws))
    }
}

#[graphql_object(context = Context)]
impl Revision {
    fn filter(&self) -> String {
        filter::spec(self.filter)
    }

    fn hash(&self, context: &Context) -> FieldResult<String> {
        let transaction = context.transaction.lock().unwrap();
        // Existence/type probe: a bogus id (e.g. the zero oid `parents` substitutes when
        // find_original fails) must error here, not filter to a bogus hash under a nop filter.
        CommitData::read(transaction.odb(), self.commit_id)?;
        let filter_commit = filter::apply_to_commit(self.filter, self.commit_id, &transaction)?;
        Ok(format!("{}", filter_commit))
    }

    fn author_email(&self, context: &Context) -> FieldResult<String> {
        let transaction = context.transaction.lock().unwrap();
        let filter_commit = filtered_commit(&transaction, self.filter, self.commit_id)?;
        let email = filter_commit.parsed()?.author()?.email;
        Ok(String::from_utf8_lossy(email).into_owned())
    }

    fn summary(&self, context: &Context) -> FieldResult<String> {
        let transaction = context.transaction.lock().unwrap();
        let filter_commit = filtered_commit(&transaction, self.filter, self.commit_id)?;
        Ok(filter_commit.summary().unwrap_or_default())
    }

    fn message(&self, context: &Context) -> FieldResult<String> {
        let transaction = context.transaction.lock().unwrap();
        let filter_commit = filtered_commit(&transaction, self.filter, self.commit_id)?;
        let message = filter_commit.message()?;
        Ok(String::from_utf8_lossy(message).into_owned())
    }

    fn date(&self, format: String, context: &Context) -> FieldResult<String> {
        let transaction = context.transaction.lock().unwrap();
        let filter_commit = filtered_commit(&transaction, self.filter, self.commit_id)?;

        let ts = filter_commit.parsed()?.committer()?.seconds();

        let ndt = chrono::DateTime::from_timestamp(ts, 0).ok_or(anyhow!("from_timestamp_opt"))?;

        Ok(ndt.format(&format).to_string())
    }

    fn rev(
        &self,
        filter: Option<String>,
        original: Option<bool>,
        context: &Context,
    ) -> FieldResult<Option<Revision>> {
        let commit_id = if let Some(true) = original {
            let transaction = context.transaction.lock().unwrap();
            let filter_commit = filtered_commit(&transaction, self.filter, self.commit_id)?;

            history::find_original(
                &transaction,
                self.filter,
                self.commit_id,
                filter_commit.id(),
                false,
            )?
        } else {
            self.commit_id
        };

        Ok(Some(Revision {
            filter: filter::parse(&filter.unwrap_or_else(|| ":/".to_string()))?,
            commit_id,
        }))
    }

    fn parents(&self, context: &Context) -> FieldResult<Vec<Revision>> {
        let transaction = context.transaction.lock().unwrap();
        let filter_commit_id = filter::apply_to_commit(self.filter, self.commit_id, &transaction)?;

        let parents = josh_core::git::read_parent_ids(transaction.odb(), filter_commit_id)?
            .into_iter()
            .map(|id| Revision {
                filter: self.filter,
                commit_id: history::find_original(
                    &transaction,
                    self.filter,
                    self.commit_id,
                    id,
                    false,
                )
                .unwrap_or_else(|_| gix_hash::ObjectId::null(gix_hash::Kind::Sha1)),
            })
            .collect();

        Ok(parents)
    }

    fn history(
        &self,
        limit: Option<i32>,
        offset: Option<i32>,
        context: &Context,
    ) -> FieldResult<Vec<Revision>> {
        let limit = limit.unwrap_or(1) as usize;
        let offset = offset.unwrap_or(0) as usize;
        let transaction = context.transaction.lock().unwrap();
        let filter_commit = filtered_commit(&transaction, self.filter, self.commit_id)?;

        // First parents only, followed just far enough to fill the requested window.
        let odb = transaction.odb();
        let mut ids = vec![];
        let mut next = Some(filter_commit.id());
        for i in 0..offset + limit {
            let Some(id) = next else { break };
            if i >= offset {
                ids.push(id);
            }
            next = josh_core::git::read_parent_ids(odb, id)?.into_iter().next();
        }

        let mut contained_in = self.commit_id;

        {
            for i in 0..ids.len() {
                let orig =
                    history::find_original(&transaction, self.filter, contained_in, ids[i], true)?;

                if orig != gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
                    ids[i] = orig;
                    contained_in = josh_core::git::read_parent_ids(transaction.odb(), ids[i])?
                        .into_iter()
                        .next()
                        .unwrap_or(ids[i]);
                } else {
                    ids.truncate(i);
                    break;
                }
            }
        }

        Ok(ids
            .into_iter()
            .map(|id| Revision {
                filter: self.filter,
                commit_id: id,
            })
            .collect())
    }

    fn files(
        &self,
        at: Option<String>,
        depth: Option<i32>,
        context: &Context,
    ) -> FieldResult<Option<Vec<Path>>> {
        self.files_or_dirs(at, depth, context, gix_object::Kind::Blob)
    }

    fn dirs(
        &self,
        at: Option<String>,
        depth: Option<i32>,
        context: &Context,
    ) -> FieldResult<Option<Vec<Path>>> {
        self.files_or_dirs(at, depth, context, gix_object::Kind::Tree)
    }

    fn changed_files(
        &self,
        at: Option<String>,
        depth: Option<i32>,
        context: &Context,
    ) -> FieldResult<Option<Vec<DiffPath>>> {
        let transaction = context.transaction.lock().unwrap();
        let filter_commit = filtered_commit(&transaction, self.filter, self.commit_id)?;

        let odb = transaction.odb();
        let (parent_id, parent_tree_id) = match filter_commit.first_parent_id() {
            Some(parent) => (parent, josh_core::git::read_tree_id(odb, parent)?),
            None => (
                gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
            ),
        };

        let filter_tree_id = filter_commit.tree_id()?;
        let d = filter::tree::diff_paths(transaction.odb(), parent_tree_id, filter_tree_id, "")?;

        let df = d
            .into_iter()
            .map(|(path, n)| match n {
                1 => DiffPath {
                    a: None,
                    b: Some(Path {
                        path: std::path::Path::new(&path).to_owned(),
                        commit_id: self.commit_id,
                        filter: self.filter,
                        tree: filter_tree_id,
                    }),
                },
                -1 => DiffPath {
                    a: Some(Path {
                        path: std::path::Path::new(&path).to_owned(),
                        commit_id: parent_id,
                        filter: self.filter,
                        tree: parent_tree_id,
                    }),
                    b: None,
                },
                _ => DiffPath {
                    a: Some(Path {
                        path: std::path::Path::new(&path).to_owned(),
                        commit_id: parent_id,
                        filter: self.filter,
                        tree: parent_tree_id,
                    }),
                    b: Some(Path {
                        path: std::path::Path::new(&path).to_owned(),
                        commit_id: self.commit_id,
                        filter: self.filter,
                        tree: filter_tree_id,
                    }),
                },
            })
            .collect();

        Ok(Some(df))
    }

    fn file(&self, path: String, context: &Context) -> FieldResult<Option<Path>> {
        let transaction = context.transaction.lock().unwrap();
        let path = std::path::Path::new(&path).to_owned();
        let odb = transaction.odb();
        let tree = CommitData::read(odb, self.commit_id)?.tree_id()?;

        let x = filter::apply(&transaction, self.filter, Rewrite::from_tree(tree))?;

        if let Ok(Some(entry)) = tree::get_path_entry(&transaction, odb, x.tree_id(), &path) {
            if !entry.mode.is_tree() && !entry.mode.is_commit() {
                Ok(Some(Path {
                    path,
                    commit_id: self.commit_id,
                    filter: self.filter,
                    tree: x.tree_id(),
                }))
            } else {
                Err(anyhow!("not a blob").into())
            }
        } else {
            Ok(None)
        }
    }

    fn dir(&self, path: Option<String>, context: &Context) -> FieldResult<Option<Path>> {
        let path = path.unwrap_or_default();
        let transaction = context.transaction.lock().unwrap();
        let odb = transaction.odb();
        let tree = CommitData::read(odb, self.commit_id)?.tree_id()?;

        let x = filter::apply(&transaction, self.filter, Rewrite::from_tree(tree))?;

        let path = std::path::Path::new(&path).to_owned();

        if path == std::path::Path::new("") {
            return Ok(Some(Path {
                path,
                commit_id: self.commit_id,
                filter: self.filter,
                tree: x.tree_id(),
            }));
        }

        if let Ok(Some(entry)) = tree::get_path_entry(&transaction, odb, x.tree_id(), &path) {
            if entry.mode.is_tree() {
                Ok(Some(Path {
                    path,
                    commit_id: self.commit_id,
                    filter: self.filter,
                    tree: x.tree_id(),
                }))
            } else {
                Err(anyhow!("not a tree").into())
            }
        } else {
            Ok(None)
        }
    }

    fn warnings(&self, context: &Context) -> FieldResult<Option<Vec<Warning>>> {
        let transaction = context.transaction.lock().unwrap();
        let commit = CommitData::read(transaction.odb(), self.commit_id)?;

        let warnings = filter::compute_warnings(&transaction, self.filter, commit.tree_id()?)
            .into_iter()
            .map(|text| Warning { text })
            .collect();

        Ok(Some(warnings))
    }

    fn search(&self, string: String, context: &Context) -> FieldResult<Option<Vec<SearchResult>>> {
        let transaction = context.transaction.lock().unwrap();
        let odb = transaction.odb();
        let tree = CommitData::read(odb, self.commit_id)?.tree_id()?;

        let x = filter::apply(&transaction, self.filter, Rewrite::from_tree(tree))?;

        // The trigram index is experimental; without it every file is a candidate and
        // search_matches does all the filtering, so results are identical, just slower.
        let candidates = if filter::experimental_features_enabled() {
            let ifilterobj = filter::parse(":SQUASH:INDEX")?;
            let index_tree = filter::apply(&transaction, ifilterobj, x.clone())?;
            josh_search::search_candidates(odb, index_tree.tree_id(), x.tree_id(), &string)?
        } else {
            let mut scan = vec![];
            objects::walk_tree_preorder(odb, x.tree_id(), &mut |parent, entry| {
                if !entry.mode.is_tree()
                    && !entry.mode.is_commit()
                    && let Ok(name) = std::str::from_utf8(entry.filename)
                {
                    let separator = if parent.is_empty() { "" } else { "/" };
                    scan.push(format!("{}{}{}", parent, separator, name));
                }
                Ok(())
            })?;
            scan
        };
        let results = josh_search::search_matches(odb, x.tree_id(), &string, &candidates)?;

        let mut r = vec![];
        for m in results {
            let mut matches = vec![];
            for l in m.1 {
                matches.push(SearchMatch {
                    line: l.0 as i32,
                    text: l.1,
                });
            }
            let path = Path {
                path: std::path::PathBuf::from(m.0),
                commit_id: self.commit_id,
                filter: self.filter,
                tree: x.tree_id(),
            };
            r.push(SearchResult { path, matches });
        }
        Ok(Some(r))
    }
}

pub struct Warning {
    text: String,
}

#[graphql_object(context = Context)]
impl Warning {
    fn message(&self) -> &str {
        &self.text
    }
}

#[derive(Clone)]
pub struct Path {
    path: std::path::PathBuf,
    commit_id: gix_hash::ObjectId,
    filter: filter::Filter,
    tree: gix_hash::ObjectId,
}

#[derive(Clone)]
pub struct SearchMatch {
    line: i32,
    text: String,
}

#[graphql_object(context = Context)]
impl SearchMatch {
    pub fn line(&self) -> i32 {
        self.line
    }
    pub fn text(&self) -> String {
        self.text.clone()
    }
}

pub struct SearchResult {
    path: Path,
    matches: Vec<SearchMatch>,
}

#[graphql_object(context = Context)]
impl SearchResult {
    pub fn path(&self) -> Path {
        self.path.clone()
    }
    pub fn matches(&self) -> Vec<SearchMatch> {
        self.matches.clone()
    }
}

pub fn linecount(
    transaction: &cache::Transaction,
    odb: &josh_core::memodb::Odb,
    id: gix_hash::ObjectId,
) -> usize {
    if let Some(blob) = tree::blob_bytes(odb, id) {
        return blob.iter().filter(|x| **x == b'\n').count() + if blob.is_empty() { 0 } else { 1 };
    }

    if let Ok(reader) = tree::read_tree(transaction, odb, id) {
        return reader
            .entries()
            .map(|e| linecount(transaction, odb, e.oid.to_owned()))
            .sum();
    }
    0
}

struct Markers {
    path: std::path::PathBuf,
    commit_id: gix_hash::ObjectId,
    filter: filter::Filter,
    topic: String,
}

#[graphql_object(context = Context)]
impl Markers {
    fn data(&self, context: &Context) -> FieldResult<Vec<Document>> {
        let transaction_mirror = context.transaction_mirror.lock().unwrap();
        let transaction = context.transaction.lock().unwrap();

        let refname = transaction_mirror.refname("refs/josh/meta");

        // Resolve on the mirror, read objects on the overlay: the overlay repo sees the
        // mirror's objects through a disk alternate.
        let odb = transaction.odb();
        let tree = if let Some(id) = transaction_mirror.resolve_ref(&refname)? {
            CommitData::read(odb, id)?.tree_id()?
        } else {
            filter::tree::empty_id()
        };

        let commit = self.commit_id.to_string();

        let path = if self.filter.is_nop() {
            marker_path(&commit, &self.topic).join(&self.path)
        } else {
            let t = CommitData::read(odb, self.commit_id)?.tree_id()?;
            let o = filter::tree::original_path(&transaction, self.filter, t, &self.path)?;
            marker_path(&commit, &self.topic).join(o)
        };

        let prev = match tree::get_path_entry(&transaction, odb, tree, &path)? {
            Some(entry) => {
                let blob = tree::blob_bytes(odb, entry.oid.to_owned())
                    .ok_or_else(|| anyhow!("not a blob: {}", entry.oid))?;
                std::str::from_utf8(&blob)?.to_owned()
            }
            None => "".to_owned(),
        };

        let lines = prev
            .split('\n')
            .filter(|x| !(*x).is_empty())
            .map(|x| {
                let mut s = x.splitn(2, ':');
                Document {
                    id: s
                        .next()
                        .and_then(|x| gix_hash::ObjectId::from_str(x).ok())
                        .unwrap_or(gix_hash::ObjectId::null(gix_hash::Kind::Sha1)),
                    value: s
                        .next()
                        .and_then(|x| serde_json::from_str::<serde_json::Value>(x).ok())
                        .unwrap_or_default(),
                }
            })
            .collect::<Vec<_>>();

        Ok(lines)
    }

    fn count(&self, context: &Context) -> FieldResult<i32> {
        let transaction_mirror = context.transaction_mirror.lock().unwrap();
        let transaction = context.transaction.lock().unwrap();

        let refname = transaction_mirror.refname("refs/josh/meta");

        let odb = transaction.odb();
        let mtree = if let Some(id) = transaction_mirror.resolve_ref(&refname)? {
            CommitData::read(odb, id)?.tree_id()?
        } else {
            filter::tree::empty_id()
        };

        let commit = self.commit_id.to_string();
        let mtree =
            tree::get_path_entry(&transaction, odb, mtree, &marker_path(&commit, &self.topic))
                .ok()
                .flatten()
                .filter(|entry| entry.mode.is_tree())
                .map(|entry| entry.oid.to_owned())
                .unwrap_or_else(filter::tree::empty_id);

        let mtree = if self.filter.is_nop() {
            mtree
        } else {
            filter::tree::repopulated_tree(
                &transaction,
                self.filter,
                CommitData::read(odb, self.commit_id)?.tree_id()?,
                mtree,
            )?
        };
        if let Ok(Some(p)) = tree::get_path_entry(&transaction, odb, mtree, &self.path) {
            return Ok(linecount(&transaction, odb, p.oid.to_owned()) as i32);
        } else if self.path == std::path::Path::new("") {
            return Ok(linecount(&transaction, odb, mtree) as i32);
        }
        Ok(0)
    }
}

impl Path {
    fn internal_serialize<R>(
        &self,
        context: &Context,
        to_result: impl FnOnce(&cache::Transaction, gix_hash::ObjectId) -> FieldResult<R>,
    ) -> FieldResult<R> {
        let transaction = context.transaction.lock().unwrap();

        let id = if self.path == std::path::Path::new("") {
            self.tree
        } else {
            let odb = transaction.odb();
            let entry = tree::get_path_entry(&transaction, odb, self.tree, &self.path)?
                .ok_or_else(|| anyhow!("no such path: {}", self.path.display()))?;
            entry.oid.to_owned()
        };
        to_result(&transaction, id)
    }

    fn serialize_to_serde_value<E>(
        &self,
        context: &Context,
        str_to_value: impl FnOnce(&str) -> Result<serde_json::Value, E>,
    ) -> FieldResult<Document> {
        self.internal_serialize(context, |transaction, id| {
            let odb = transaction.odb();
            let blob = tree::blob_bytes(odb, id).ok_or_else(|| anyhow!("not a blob: {}", id))?;
            let value =
                str_to_value(std::str::from_utf8(&blob)?).unwrap_or_else(|_| serde_json::json!({}));
            Ok(Document { id, value })
        })
    }
}

#[graphql_object(context = Context)]
impl Path {
    fn path(&self) -> String {
        self.path.to_string_lossy().to_string()
    }

    fn dir(&self, relative: String) -> FieldResult<Path> {
        Ok(Path {
            path: josh_core::normalize_path(&self.path.join(relative)),
            commit_id: self.commit_id,
            filter: self.filter,
            tree: self.tree,
        })
    }

    fn meta(&self, topic: String) -> Markers {
        Markers {
            path: self.path.clone(),
            commit_id: self.commit_id,
            filter: self.filter,
            topic,
        }
    }

    fn rev(&self, filter: String) -> FieldResult<Revision> {
        let hm: std::collections::HashMap<String, String> =
            [("path".to_string(), self.path.to_string_lossy().to_string())]
                .iter()
                .cloned()
                .collect();
        Ok(Revision {
            filter: filter::parse(&strfmt::strfmt(&filter, &hm)?)?,
            commit_id: self.commit_id,
        })
    }

    fn hash(&self, context: &Context) -> FieldResult<String> {
        self.internal_serialize(context, |_transaction, id| Ok(format!("{}", id)))
    }
    fn text(&self, context: &Context) -> FieldResult<Option<String>> {
        self.internal_serialize(context, |transaction, id| {
            let odb = transaction.odb();
            let blob = tree::blob_bytes(odb, id).ok_or_else(|| anyhow!("not a blob: {}", id))?;
            Ok(Some(std::str::from_utf8(&blob)?.to_string()))
        })
    }

    fn toml(&self, context: &Context) -> FieldResult<Document> {
        self.serialize_to_serde_value(context, |blob| {
            toml::de::from_str::<serde_json::Value>(blob)
        })
    }

    fn json(&self, context: &Context) -> FieldResult<Document> {
        self.serialize_to_serde_value(context, |blob| {
            serde_json::from_str::<serde_json::Value>(blob)
        })
    }

    fn yaml(&self, context: &Context) -> FieldResult<Document> {
        self.serialize_to_serde_value(context, |blob| {
            serde_yaml::from_str::<serde_json::Value>(blob)
        })
    }
}

pub struct Document {
    id: gix_hash::ObjectId,
    value: serde_json::Value,
}

impl Document {
    fn pointer(&self, pointer: Option<String>) -> serde_json::Value {
        if let Some(pointer) = pointer {
            self.value
                .pointer(&pointer)
                .unwrap_or(&serde_json::json!({}))
                .to_owned()
        } else {
            self.value.clone()
        }
    }
}

#[graphql_object(context = Context)]
impl Document {
    fn string(&self, at: Option<String>, default: Option<String>) -> Option<String> {
        if let serde_json::Value::String(s) = &self.pointer(at) {
            Some(s.clone())
        } else {
            default
        }
    }

    fn bool(&self, at: Option<String>, default: Option<bool>) -> Option<bool> {
        if let serde_json::Value::Bool(s) = &self.pointer(at) {
            Some(*s)
        } else {
            default
        }
    }

    fn int(&self, at: Option<String>, default: Option<i32>) -> Option<i32> {
        if let serde_json::Value::Number(s) = &self.pointer(at) {
            s.as_i64().map(|x| x as i32)
        } else {
            default
        }
    }

    fn list(&self, at: Option<String>) -> Option<Vec<Document>> {
        let mut v = vec![];
        if let serde_json::Value::Array(a) = &self.pointer(at) {
            for x in a.iter() {
                v.push(Document {
                    id: gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
                    value: x.clone(),
                });
            }
        } else {
            return None;
        }
        Some(v)
    }

    fn value(&self, at: String) -> Option<Document> {
        self.value.pointer(&at).map(|x| Document {
            id: gix_hash::ObjectId::null(gix_hash::Kind::Sha1),
            value: x.to_owned(),
        })
    }

    fn id(&self) -> String {
        self.id.to_string()
    }
}

pub struct Reference {
    refname: String,
}

#[graphql_object(context = Context)]
impl Reference {
    fn name(&self) -> FieldResult<String> {
        Ok(if let Some(r) = UpstreamRef::from_str(&self.refname) {
            r.reference
        } else {
            self.refname.clone()
        })
    }

    fn rev(&self, context: &Context, filter: Option<String>) -> FieldResult<Revision> {
        let transaction_mirror = context.transaction_mirror.lock().unwrap();
        let commit_id = transaction_mirror
            .resolve_ref(&self.refname)?
            .ok_or_else(|| anyhow!("missing ref: {}", self.refname))?;

        Ok(Revision {
            filter: filter::parse(&filter.unwrap_or_else(|| ":/".to_string()))?,
            commit_id,
        })
    }
}

type ToPushSet = std::sync::Arc<
    std::sync::Mutex<std::collections::HashSet<(gix_hash::ObjectId, String, Option<String>)>>,
>;

#[derive(PartialEq, Eq, Clone, Copy)]
enum FetchRequestResult {
    Requested,
    AlreadyCompleted,
}

#[derive(Default)]
pub struct FetchState {
    inner: std::sync::Mutex<(bool, bool)>,
}

impl FetchState {
    fn request(&self) -> FetchRequestResult {
        let (requested, completed) = &mut *self.inner.lock().unwrap();
        if *completed {
            return FetchRequestResult::AlreadyCompleted;
        }

        *requested = true;
        FetchRequestResult::Requested
    }

    pub fn complete(&self) -> bool {
        let (requested, completed) = &mut *self.inner.lock().unwrap();
        *completed = true;
        *requested
    }
}

pub struct Context {
    pub transaction: std::sync::Arc<std::sync::Mutex<cache::Transaction>>,
    pub transaction_mirror: std::sync::Arc<std::sync::Mutex<cache::Transaction>>,
    pub meta_add: std::sync::Arc<
        std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Vec<String>>>,
    >,
    pub to_push: ToPushSet,
    pub fetch_state: FetchState,
}

impl juniper::Context for Context {}

pub struct Repository {
    name: String,
    ns: String,
}

pub struct RepositoryMut {}

fn marker_path(commit: &str, topic: &str) -> std::path::PathBuf {
    std::path::Path::new(topic)
        .join("~")
        .join(&commit[..2])
        .join(&commit[2..5])
        .join(&commit[5..9])
        .join(commit)
}

#[derive(juniper::GraphQLInputObject)]
struct MarkersInput {
    path: String,
    data: Vec<String>,
}

fn format_marker(input: &str) -> anyhow::Result<String> {
    let value = serde_json::from_str::<serde_json::Value>(input)?;
    let line = serde_json::to_string(&value)?;
    let hash = josh_core::objects::hash_blob(line.as_bytes());
    Ok(format!("{}:{}", &hash, &line))
}

struct RevMut {
    at: String,
    filter: filter::Filter,
}

#[graphql_object(context = Context)]
impl RevMut {
    fn push(&self, target: String, repo: Option<String>, context: &Context) -> FieldResult<bool> {
        let transaction = context.transaction.lock().unwrap();

        let filter_commit = filtered_commit(
            &transaction,
            self.filter,
            gix_hash::ObjectId::from_str(&self.at)?,
        )?;

        if let Ok(mut to_push) = context.to_push.lock() {
            to_push.insert((filter_commit.id(), target, repo));
        }

        Ok(true)
    }

    fn meta(&self, topic: String, add: Vec<MarkersInput>, context: &Context) -> FieldResult<bool> {
        if !self.filter.is_nop() {
            return Err(anyhow!("meta mutation for filtered revs is not implemented").into());
        }
        if let Ok(mut meta_add) = context.meta_add.lock() {
            for mm in add {
                let path = mm.path;
                let path = &marker_path(&self.at, &topic).join(path);
                let mut lines = meta_add.get(path).unwrap_or(&vec![]).clone();

                let mm = mm
                    .data
                    .iter()
                    .map(String::as_str)
                    .map(format_marker)
                    .collect::<anyhow::Result<Vec<_>>>()?;

                for marker in mm.into_iter() {
                    lines.push(marker);
                }

                meta_add.insert(path.clone(), lines);
            }
        }

        Ok(true)
    }
}

#[graphql_object(context = Context)]
impl RepositoryMut {
    fn rev(at: String, filter: Option<String>, context: &Context) -> FieldResult<RevMut> {
        if context.fetch_state.request() != FetchRequestResult::AlreadyCompleted {
            return Err(anyhow!("rev(): fetch needed").into());
        }

        let transaction_mirror = context.transaction_mirror.lock().unwrap();

        // Just check that the commit exists
        CommitData::read(transaction_mirror.odb(), gix_hash::ObjectId::from_str(&at)?)?;

        let filter = if let Some(spec) = filter {
            filter::parse(&spec)?
        } else {
            filter::Filter::new()
        };

        Ok(RevMut { at, filter })
    }
}

#[graphql_object(context = Context)]
impl Repository {
    fn name(&self) -> &str {
        &self.name
    }

    fn refs(&self, context: &Context, pattern: Option<String>) -> FieldResult<Vec<Reference>> {
        if context.fetch_state.request() != FetchRequestResult::AlreadyCompleted {
            return Err(anyhow!("refs(): fetch needed").into());
        }

        let transaction_mirror = context.transaction_mirror.lock().unwrap();
        let pattern = pattern.unwrap_or_else(|| "refs/heads/*".to_string());

        tracing::debug!(pattern = pattern, "refs");

        // The namespace stays out of glob matching: it extends the iteration prefix --
        // together with the pattern's literal part before its first metacharacter -- and
        // is stripped off the names matched against the pattern. `glob`'s default
        // `MatchOptions` let `*` and `?` match across `/`.
        let matcher = glob::Pattern::new(&pattern)?;
        let literal_len = pattern.find(['*', '?', '[', '\\']).unwrap_or(pattern.len());
        let prefix = format!("{}{}", self.ns, &pattern[..literal_len]);

        let mut refs = vec![];

        transaction_mirror.for_each_ref_prefixed(&prefix, |name, _| {
            if matcher.matches(&name[self.ns.len()..]) {
                refs.push(Reference {
                    refname: name.to_string(),
                });
            }
            Ok(())
        })?;

        Ok(refs)
    }

    fn rev(&self, context: &Context, at: String, filter: Option<String>) -> FieldResult<Revision> {
        let rev = format!("{}{}", self.ns, at);

        let transaction_mirror = context.transaction_mirror.lock().unwrap();
        let commit_id = {
            let oid = if let Ok(id) = gix_hash::ObjectId::from_str(&at) {
                Some((id, transaction_mirror.odb().contains(id)))
            } else {
                None
            };

            if oid.is_none() && context.fetch_state.request() == FetchRequestResult::Requested {
                return Err(anyhow!("rev(): fetch needed").into());
            }

            // If we already fetched but the requested OID is not present, that's an error
            if let Some((oid, exists)) = oid
                && !exists
            {
                return Err(anyhow!("rev(): oid {oid} not found after fetch").into());
            }

            if let Some((oid, _)) = oid {
                oid
            } else {
                transaction_mirror
                    .rev_parse(&rev)?
                    .ok_or_else(|| anyhow::anyhow!("no such revision: {}", rev))?
            }
        };

        Ok(Revision {
            filter: filter::parse(&filter.unwrap_or_else(|| ":/".to_string()))?,
            commit_id,
        })
    }
}

josh_core::regex_parsed!(
    UpstreamRef,
    r"refs/josh/upstream/.*[.]git/(?P<reference>refs/heads/.*)",
    [reference]
);

pub fn context(transaction: cache::Transaction, transaction_mirror: cache::Transaction) -> Context {
    Context {
        transaction_mirror: std::sync::Arc::new(std::sync::Mutex::new(transaction_mirror)),
        transaction: std::sync::Arc::new(std::sync::Mutex::new(transaction)),
        meta_add: std::sync::Arc::new(std::sync::Mutex::new(Default::default())),
        to_push: std::sync::Arc::new(std::sync::Mutex::new(Default::default())),
        fetch_state: FetchState::default(),
    }
}

pub type CommitSchema =
    juniper::RootNode<Revision, EmptyMutation<Context>, EmptySubscription<Context>>;

pub fn commit_schema(commit_id: gix_hash::ObjectId) -> CommitSchema {
    CommitSchema::new(
        Revision {
            commit_id,
            filter: filter::Filter::new(),
        },
        EmptyMutation::new(),
        EmptySubscription::new(),
    )
}

pub type RepoSchema = juniper::RootNode<Repository, RepositoryMut, EmptySubscription<Context>>;

pub fn repo_schema(name: String, local: bool) -> RepoSchema {
    let ns = if local {
        "".to_string()
    } else {
        format!("refs/josh/upstream/{}.git/", josh_core::to_ns(&name))
    };
    RepoSchema::new(
        Repository { name, ns },
        RepositoryMut {},
        EmptySubscription::new(),
    )
}
