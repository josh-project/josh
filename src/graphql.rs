#![allow(unused_variables)]

use super::*;
use juniper::{graphql_object, EmptyMutation, EmptySubscription, FieldResult};

pub struct Revision {
    filter: filter::Filter,
    commit_id: git2::Oid,
}

fn find_paths(
    transaction: &cache::Transaction,
    tree: git2::Tree,
    at: Option<String>,
    depth: Option<i32>,
    kind: git2::ObjectType,
) -> JoshResult<Vec<std::path::PathBuf>> {
    let tree = if let Some(at) = at.as_ref() {
        if at.is_empty() {
            tree
        } else {
            let path = std::path::Path::new(&at).to_owned();
            transaction.repo().find_tree(tree.get_path(&path)?.id())?
        }
    } else {
        tree
    };

    let base = std::path::Path::new(&at.as_ref().unwrap_or(&"".to_string())).to_owned();

    let mut ws = vec![];
    tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
        if Some(kind) == entry.kind() {
            if let Some(name) = entry.name() {
                let path = std::path::Path::new(root).join(name);
                if let Some(limit) = depth {
                    if path.components().count() as i32 > limit {
                        return 1;
                    }
                }
                ws.push(base.join(path));
            }
        }
        0
    })?;
    Ok(ws)
}

impl Revision {
    fn files_or_dirs(
        &self,
        at: Option<String>,
        depth: Option<i32>,
        context: &Context,
        kind: git2::ObjectType,
    ) -> FieldResult<Option<Vec<Path>>> {
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.commit_id)?;
        let tree = filter::apply(&transaction, self.filter, commit.tree()?)?;
        let tree_id = tree.id();
        let paths = find_paths(&transaction, tree, at, depth, kind)?;
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
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.commit_id)?;
        let filter_commit = filter::apply_to_commit(self.filter, &commit, &transaction)?;
        Ok(format!("{}", filter_commit))
    }

    fn summary(&self, context: &Context) -> FieldResult<String> {
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.commit_id)?;
        let filter_commit = transaction.repo().find_commit(filter::apply_to_commit(
            self.filter,
            &commit,
            &transaction,
        )?)?;
        Ok(filter_commit.summary().unwrap_or("").to_owned())
    }

    fn date(&self, format: String, context: &Context) -> FieldResult<String> {
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.commit_id)?;
        let filter_commit = transaction.repo().find_commit(filter::apply_to_commit(
            self.filter,
            &commit,
            &transaction,
        )?)?;

        let ts = filter_commit.time().seconds();

        let ndt = chrono::NaiveDateTime::from_timestamp(ts, 0);
        Ok(ndt.format(&format).to_string())
    }

    fn rev(
        &self,
        filter: Option<String>,
        original: Option<bool>,
        context: &Context,
    ) -> FieldResult<Option<Revision>> {
        let commit_id = if let Some(true) = original {
            let transaction = context.transaction.lock()?;
            let commit = transaction.repo().find_commit(self.commit_id)?;
            let filter_commit = transaction.repo().find_commit(filter::apply_to_commit(
                self.filter,
                &commit,
                &transaction,
            )?)?;

            history::find_original(
                &transaction,
                self.filter,
                self.commit_id,
                filter_commit.id(),
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
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.commit_id)?;
        let filter_commit = transaction.repo().find_commit(filter::apply_to_commit(
            self.filter,
            &commit,
            &transaction,
        )?)?;

        let parents = filter_commit
            .parent_ids()
            .map(|id| Revision {
                filter: self.filter,
                commit_id: history::find_original(&transaction, self.filter, self.commit_id, id)
                    .unwrap_or_else(|_| git2::Oid::zero()),
            })
            .collect();

        Ok(parents)
    }

    fn files(
        &self,
        at: Option<String>,
        depth: Option<i32>,
        context: &Context,
    ) -> FieldResult<Option<Vec<Path>>> {
        self.files_or_dirs(at, depth, context, git2::ObjectType::Blob)
    }

    fn dirs(
        &self,
        at: Option<String>,
        depth: Option<i32>,
        context: &Context,
    ) -> FieldResult<Option<Vec<Path>>> {
        self.files_or_dirs(at, depth, context, git2::ObjectType::Tree)
    }

    fn file(&self, path: String, context: &Context) -> FieldResult<Option<Path>> {
        let transaction = context.transaction.lock()?;
        let path = std::path::Path::new(&path).to_owned();
        let tree = transaction.repo().find_commit(self.commit_id)?.tree()?;

        let tree = filter::apply(&transaction, self.filter, tree)?;

        if let Some(git2::ObjectType::Blob) = tree.get_path(&path)?.kind() {
            Ok(Some(Path {
                path,
                commit_id: self.commit_id,
                filter: self.filter,
                tree: tree.id(),
            }))
        } else {
            Err(josh_error("not a blob").into())
        }
    }

    fn dir(&self, path: Option<String>, context: &Context) -> FieldResult<Option<Path>> {
        let path = path.unwrap_or_default();
        let transaction = context.transaction.lock()?;
        let tree = transaction.repo().find_commit(self.commit_id)?.tree()?;

        let tree = filter::apply(&transaction, self.filter, tree)?;

        let path = std::path::Path::new(&path).to_owned();

        if path == std::path::Path::new("") {
            return Ok(Some(Path {
                path,
                commit_id: self.commit_id,
                filter: self.filter,
                tree: tree.id(),
            }));
        }

        if let Some(git2::ObjectType::Tree) = tree.get_path(&path)?.kind() {
            Ok(Some(Path {
                path,
                commit_id: self.commit_id,
                filter: self.filter,
                tree: tree.id(),
            }))
        } else {
            Err(josh_error("not a tree").into())
        }
    }

    fn warnings(&self, context: &Context) -> FieldResult<Option<Vec<Warning>>> {
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.commit_id)?;

        let warnings = filter::compute_warnings(&transaction, self.filter, commit.tree()?)
            .into_iter()
            .map(|text| Warning { text })
            .collect();

        Ok(Some(warnings))
    }
}

#[cfg(feature = "search")]
#[graphql_object(context = Context)]
impl Revision {
    fn search(
        &self,
        string: String,
        max_complexity: Option<i32>,
        context: &Context,
    ) -> FieldResult<Option<Vec<SearchResult>>> {
        let max_complexity = max_complexity.unwrap_or(6) as usize;
        let transaction = context.transaction.lock()?;
        let ifilterobj = filter::chain(self.filter, filter::parse(":SQUASH:INDEX")?);
        let tree = transaction.repo().find_commit(self.commit_id)?.tree()?;

        let tree = filter::apply(&transaction, self.filter, tree)?;
        let index_tree = filter::apply(&transaction, ifilterobj, tree.clone())?;

        /* let start = std::time::Instant::now(); */
        let candidates =
            filter::tree::search_candidates(&transaction, &index_tree, &string, max_complexity)?;
        let results = filter::tree::search_matches(&transaction, &tree, &string, &candidates)?;
        /* let duration = start.elapsed(); */

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
                tree: tree.id(),
            };
            r.push(SearchResult { path, matches });
        }
        return Ok(Some(r));
        /* println!("\n Search took {:?}", duration); */
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
    commit_id: git2::Oid,
    filter: filter::Filter,
    tree: git2::Oid,
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

pub fn linecount(repo: &git2::Repository, id: git2::Oid) -> usize {
    if let Ok(blob) = repo.find_blob(id) {
        return blob.content().iter().filter(|x| **x == b'\n').count()
            + if blob.content().is_empty() { 0 } else { 1 };
    }

    if let Ok(tree) = repo.find_tree(id) {
        let mut c = 0;
        for i in tree.iter() {
            c += linecount(repo, i.id());
        }
        return c;
    }
    0
}

struct Markers {
    path: std::path::PathBuf,
    commit_id: git2::Oid,
    filter: filter::Filter,
    topic: String,
}

#[graphql_object(context = Context)]
impl Markers {
    fn data(&self, context: &Context) -> FieldResult<Vec<Document>> {
        let transaction = context.transaction.lock()?;

        let refname = transaction.refname("refs/josh/meta");

        let r = transaction.repo().revparse_single(&refname);
        let tree = if let Ok(r) = r {
            let commit = transaction.repo().find_commit(r.id())?;
            commit.tree()?
        } else {
            filter::tree::empty(transaction.repo())
        };

        let commit = self.commit_id.to_string();

        let path = if self.filter == filter::nop() {
            marker_path(&commit, &self.topic).join(&self.path)
        } else {
            let t = transaction.repo().find_commit(self.commit_id)?.tree()?;
            let o = filter::tree::original_path(&transaction, self.filter, t, &self.path)?;
            marker_path(&commit, &self.topic).join(&o)
        };

        let prev = if let Ok(e) = tree.get_path(&path) {
            let blob = transaction.repo().find_blob(e.id())?;
            std::str::from_utf8(blob.content())?.to_owned()
        } else {
            "".to_owned()
        };

        let lines = prev
            .split('\n')
            .filter(|x| !(*x).is_empty())
            .map(|x| {
                let mut s = x.splitn(2, ':');
                Document {
                    id: s
                        .next()
                        .and_then(|x| git2::Oid::from_str(x).ok())
                        .unwrap_or_else(git2::Oid::zero),
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
        let transaction = context.transaction.lock()?;

        let refname = transaction.refname("refs/josh/meta");

        let r = transaction.repo().revparse_single(&refname);
        let mtree = if let Ok(r) = r {
            let commit = transaction.repo().find_commit(r.id())?;
            commit.tree()?
        } else {
            filter::tree::empty(transaction.repo())
        };

        let commit = self.commit_id.to_string();
        let mtree = mtree
            .get_path(&marker_path(&commit, &self.topic))
            .map(|p| transaction.repo().find_tree(p.id()).ok())
            .ok()
            .flatten()
            .unwrap_or_else(|| filter::tree::empty(transaction.repo()));

        let mtree = if self.filter == filter::nop() {
            mtree
        } else {
            transaction
                .repo()
                .find_tree(filter::tree::repopulated_tree(
                    &transaction,
                    self.filter,
                    transaction.repo().find_commit(self.commit_id)?.tree()?,
                    mtree,
                )?)?
        };
        if let Ok(p) = mtree.get_path(&self.path) {
            return Ok(linecount(transaction.repo(), p.id()) as i32);
        } else if self.path == std::path::Path::new("") {
            return Ok(linecount(transaction.repo(), mtree.id()) as i32);
        }
        Ok(0)
    }
}

impl Path {
    fn internal_serialize<R>(
        &self,
        context: &Context,
        to_result: impl FnOnce(&cache::Transaction, git2::Oid) -> FieldResult<R>,
    ) -> FieldResult<R> {
        let transaction = context.transaction.lock()?;

        let id = if self.path == std::path::Path::new("") {
            self.tree
        } else {
            transaction
                .repo()
                .find_tree(self.tree)?
                .get_path(&self.path)?
                .id()
        };
        to_result(&transaction, id)
    }

    fn serialize_to_serde_value<E>(
        &self,
        context: &Context,
        str_to_value: impl FnOnce(&str) -> Result<serde_json::Value, E>,
    ) -> FieldResult<Document> {
        self.internal_serialize(context, |transaction, id| {
            let blob = transaction.repo().find_blob(id)?;
            let value =
                str_to_value(std::str::from_utf8(blob.content())?).unwrap_or_else(|_| json!({}));
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
            path: normalize_path(&self.path.join(&relative)),
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
            let blob = transaction.repo().find_blob(id)?;
            Ok(Some(std::str::from_utf8(blob.content())?.to_string()))
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
    id: git2::Oid,
    value: serde_json::Value,
}

impl Document {
    fn pointer(&self, pointer: Option<String>) -> serde_json::Value {
        if let Some(pointer) = pointer {
            return self
                .value
                .pointer(&pointer)
                .unwrap_or(&json!({}))
                .to_owned();
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
                    id: git2::Oid::zero(),
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
            id: git2::Oid::zero(),
            value: x.to_owned(),
        })
    }

    fn id() -> String {
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
        let transaction = context.transaction.lock()?;
        let commit_id = transaction
            .repo()
            .find_reference(&self.refname)?
            .target()
            .unwrap_or_else(git2::Oid::zero);

        Ok(Revision {
            filter: filter::parse(&filter.unwrap_or_else(|| ":/".to_string()))?,
            commit_id,
        })
    }
}

pub struct Context {
    pub transaction: std::sync::Arc<std::sync::Mutex<cache::Transaction>>,
    pub to_push: std::sync::Arc<std::sync::Mutex<Vec<(String, git2::Oid)>>>,
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
        .join(&commit)
}

#[derive(juniper::GraphQLInputObject)]
struct MarkersInput {
    path: String,
    data: Vec<String>,
}

#[derive(juniper::GraphQLInputObject)]
struct MarkerInput {
    position: String,
    text: String,
}

fn format_marker(input: &str) -> JoshResult<String> {
    let value = serde_json::from_str::<serde_json::Value>(input)?;
    let line = serde_json::to_string(&value)?;
    let hash = git2::Oid::hash_object(git2::ObjectType::Blob, line.as_bytes())?;
    Ok(format!("{}:{}", &hash, &line))
}

#[graphql_object(context = Context)]
impl RepositoryMut {
    fn meta(
        &self,
        commit: String,
        topic: String,
        add: Vec<MarkersInput>,
        context: &Context,
    ) -> FieldResult<bool> {
        let transaction = context.transaction.lock()?;
        let rev = transaction.refname("refs/josh/meta");

        transaction
            .repo()
            .find_commit(git2::Oid::from_str(&commit)?)?;

        let r = transaction.repo().revparse_single(&rev);
        let (tree, parent) = if let Ok(r) = r {
            let commit = transaction.repo().find_commit(r.id())?;
            let tree = commit.tree()?;
            (tree, Some(commit))
        } else {
            (filter::tree::empty(transaction.repo()), None)
        };

        let mut tree = tree;

        for mm in add {
            let path = mm.path;
            let path = &marker_path(&commit, &topic).join(&path);
            let prev = if let Ok(e) = tree.get_path(path) {
                let blob = transaction.repo().find_blob(e.id())?;
                std::str::from_utf8(blob.content())?.to_owned()
            } else {
                "".to_owned()
            };

            let mm = mm
                .data
                .iter()
                .map(String::as_str)
                .map(format_marker)
                .collect::<JoshResult<Vec<_>>>()?;

            let mut lines = prev
                .split('\n')
                .filter(|x| !(*x).is_empty())
                .collect::<Vec<_>>();
            for marker in mm.iter() {
                lines.push(marker);
            }
            lines.sort_unstable();
            lines.dedup();

            let blob = transaction.repo().blob(lines.join("\n").as_bytes())?;

            tree = filter::tree::insert(transaction.repo(), &tree, path, blob, 0o0100644)?;
        }

        let oid = transaction.repo().commit(
            None,
            &transaction.repo().signature()?,
            &transaction.repo().signature()?,
            "marker",
            &tree,
            &parent.as_ref().into_iter().collect::<Vec<_>>(),
        )?;

        context
            .to_push
            .lock()?
            .push(("refs/josh/meta".to_string(), oid));

        Ok(true)
    }
}

#[graphql_object(context = Context)]
impl Repository {
    fn name(&self) -> &str {
        &self.name
    }

    fn refs(&self, context: &Context, pattern: Option<String>) -> FieldResult<Vec<Reference>> {
        let transaction = context.transaction.lock()?;
        let refname = format!(
            "{}{}",
            self.ns,
            pattern.unwrap_or_else(|| "refs/heads/*".to_string())
        );

        log::debug!("refname: {:?}", refname);

        let mut refs = vec![];

        for reference in transaction.repo().references_glob(&refname)? {
            let r = reference?;
            let name = r
                .name()
                .ok_or_else(|| josh_error("reference without name"))?;

            refs.push(Reference {
                refname: name.to_string(),
            });
        }

        Ok(refs)
    }

    fn rev(&self, context: &Context, at: String, filter: Option<String>) -> FieldResult<Revision> {
        let rev = format!("{}{}", self.ns, at);

        let transaction = context.transaction.lock()?;
        let commit_id = if let Ok(id) = git2::Oid::from_str(&at) {
            id
        } else {
            transaction.repo().revparse_single(&rev)?.id()
        };

        Ok(Revision {
            filter: filter::parse(&filter.unwrap_or_else(|| ":/".to_string()))?,
            commit_id,
        })
    }
}

regex_parsed!(
    UpstreamRef,
    r"refs/josh/upstream/.*[.]git/(?P<reference>refs/heads/.*)",
    [reference]
);

pub fn context(transaction: cache::Transaction) -> Context {
    Context {
        transaction: std::sync::Arc::new(std::sync::Mutex::new(transaction)),
        to_push: std::sync::Arc::new(std::sync::Mutex::new(vec![])),
    }
}

pub type CommitSchema =
    juniper::RootNode<'static, Revision, EmptyMutation<Context>, EmptySubscription<Context>>;

pub fn commit_schema(commit_id: git2::Oid) -> CommitSchema {
    CommitSchema::new(
        Revision {
            commit_id,
            filter: filter::nop(),
        },
        EmptyMutation::new(),
        EmptySubscription::new(),
    )
}

pub type RepoSchema =
    juniper::RootNode<'static, Repository, RepositoryMut, EmptySubscription<Context>>;

pub fn repo_schema(name: String, local: bool) -> RepoSchema {
    let ns = if local {
        "".to_string()
    } else {
        format!("refs/josh/upstream/{}.git/", to_ns(&name))
    };
    RepoSchema::new(
        Repository { name, ns },
        RepositoryMut {},
        EmptySubscription::new(),
    )
}
