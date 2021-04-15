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
        if at == "" {
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
    return Ok(ws);
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
        let id = if let Some(true) = original {
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
            filter: filter::parse(&filter.unwrap_or(":/".to_string()))?,
            commit_id: id,
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
                    .unwrap_or(git2::Oid::zero()),
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
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.commit_id)?;
        let tree = filter::apply(&transaction, self.filter, commit.tree()?)?;
        let tree_id = tree.id();

        let paths = find_paths(&transaction, tree, at, depth, git2::ObjectType::Blob)?;

        let mut ws = vec![];
        for p in paths {
            ws.push(Path {
                path: p,
                commit_id: self.commit_id,
                filter: self.filter,
                tree: tree_id,
            });
        }
        return Ok(Some(ws));
    }

    fn dirs(
        &self,
        at: Option<String>,
        depth: Option<i32>,
        context: &Context,
    ) -> FieldResult<Option<Vec<Path>>> {
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.commit_id)?;
        let tree = filter::apply(&transaction, self.filter, commit.tree()?)?;
        let tree_id = tree.id();

        let paths = find_paths(&transaction, tree, at, depth, git2::ObjectType::Tree)?;

        let mut ws = vec![];
        for p in paths {
            ws.push(Path {
                path: p,
                commit_id: self.commit_id,
                filter: self.filter,
                tree: tree_id,
            });
        }
        return Ok(Some(ws));
    }

    fn file(&self, path: String, context: &Context) -> FieldResult<Option<Path>> {
        let transaction = context.transaction.lock()?;
        let path = std::path::Path::new(&path).to_owned();
        let tree = transaction.repo().find_commit(self.commit_id)?.tree()?;

        let tree = filter::apply(&transaction, self.filter, tree)?;

        if let Some(git2::ObjectType::Blob) = tree.get_path(&path)?.kind() {
            Ok(Some(Path {
                path: path,
                commit_id: self.commit_id,
                filter: self.filter,
                tree: tree.id(),
            }))
        } else {
            Err(josh_error("not a blob"))?
        }
    }
}

pub struct Path {
    path: std::path::PathBuf,
    commit_id: git2::Oid,
    filter: filter::Filter,
    tree: git2::Oid,
}

pub fn linecount(repo: &git2::Repository, id: git2::Oid) -> usize {
    if let Ok(blob) = repo.find_blob(id) {
        return blob.content().iter().filter(|x| **x == '\n' as u8).count()
            + if blob.content().len() == 0 { 0 } else { 1 };
    }

    if let Ok(tree) = repo.find_tree(id) {
        let mut c = 0;
        for i in tree.iter() {
            c += linecount(repo, i.id());
        }
        return c;
    }
    return 0;
}

struct Markers {
    path: std::path::PathBuf,
    commit_id: git2::Oid,
    filter: filter::Filter,
    topic: String,
}

#[graphql_object(context = Context)]
impl Markers {
    fn list(&self, context: &Context) -> FieldResult<Vec<Marker>> {
        let transaction = context.transaction.lock()?;

        let refname = transaction.refname("refs/josh/meta");

        let r = transaction.repo().revparse_single(&refname);
        let tree = if let Ok(r) = r {
            let commit = transaction.repo().find_commit(r.id())?;
            commit.tree()?
        } else {
            filter::tree::empty(&transaction.repo())
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
            .split("\n")
            .filter(|x| *x != "")
            .map(|x| {
                let mut s = x.splitn(2, ": ");
                Marker {
                    position: s.next().unwrap_or("").to_owned(),
                    text: s.next().unwrap_or("").to_owned(),
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
            filter::tree::empty(&transaction.repo())
        };

        let commit = self.commit_id.to_string();
        let mtree = mtree
            .get_path(&marker_path(&commit, &self.topic))
            .map(|p| transaction.repo().find_tree(p.id()).ok())
            .ok()
            .flatten()
            .unwrap_or(filter::tree::empty(transaction.repo()));

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
        }
        return Ok(0);
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

    fn markers(&self, topic: String) -> Markers {
        Markers {
            path: self.path.clone(),
            commit_id: self.commit_id,
            filter: self.filter,
            topic: topic,
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
        let transaction = context.transaction.lock()?;
        let id = transaction
            .repo()
            .find_tree(self.tree)?
            .get_path(&self.path)?
            .id();
        Ok(format!("{}", id))
    }
    fn text(&self, context: &Context) -> FieldResult<Option<String>> {
        let transaction = context.transaction.lock()?;
        let id = transaction
            .repo()
            .find_tree(self.tree)?
            .get_path(&self.path)?
            .id();
        let blob = transaction.repo().find_blob(id)?;

        Ok(Some(std::str::from_utf8(blob.content())?.to_string()))
    }

    fn toml(&self, context: &Context) -> FieldResult<Document> {
        let transaction = context.transaction.lock()?;
        let id = transaction
            .repo()
            .find_tree(self.tree)?
            .get_path(&self.path)?
            .id();
        let blob = transaction.repo().find_blob(id)?;
        let value = toml::de::from_str::<serde_json::Value>(std::str::from_utf8(blob.content())?)
            .unwrap_or(json!({}));

        Ok(Document { value: value })
    }

    fn json(&self, context: &Context) -> FieldResult<Document> {
        let transaction = context.transaction.lock()?;
        let id = transaction
            .repo()
            .find_tree(self.tree)?
            .get_path(&self.path)?
            .id();
        let blob = transaction.repo().find_blob(id)?;
        let value = serde_json::from_str::<serde_json::Value>(std::str::from_utf8(blob.content())?)
            .unwrap_or(json!({}));

        Ok(Document { value: value })
    }

    fn yaml(&self, context: &Context) -> FieldResult<Document> {
        let transaction = context.transaction.lock()?;
        let id = transaction
            .repo()
            .find_tree(self.tree)?
            .get_path(&self.path)?
            .id();
        let blob = transaction.repo().find_blob(id)?;
        let value = serde_yaml::from_str::<serde_json::Value>(std::str::from_utf8(blob.content())?)
            .unwrap_or(json!({}));

        Ok(Document { value: value })
    }
}

pub struct Document {
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
                v.push(Document { value: x.clone() });
            }
        } else {
            return None;
        }
        return Some(v);
    }

    fn value(&self, at: String) -> Option<Document> {
        self.value.pointer(&at).map(|x| Document {
            value: x.to_owned(),
        })
    }
}

pub struct Reference {
    refname: String,
}

#[graphql_object(context = Context)]
impl Reference {
    fn name(&self) -> FieldResult<String> {
        Ok(UpstreamRef::from_str(&self.refname)
            .ok_or(josh_error("not a ns"))?
            .reference)
    }

    fn rev(&self, context: &Context, filter: Option<String>) -> FieldResult<Revision> {
        let transaction = context.transaction.lock()?;
        let id = transaction
            .repo()
            .find_reference(&self.refname)?
            .target()
            .unwrap_or(git2::Oid::zero());

        Ok(Revision {
            filter: filter::parse(&filter.unwrap_or(":/".to_string()))?,
            commit_id: id,
        })
    }
}

pub struct Context {
    transaction: std::sync::Arc<std::sync::Mutex<cache::Transaction>>,
}

impl juniper::Context for Context {}

pub struct Repository {
    name: String,
}

pub struct RepositoryMut {}

fn marker_path(commit: &str, topic: &str) -> std::path::PathBuf {
    std::path::Path::new(topic)
        .join("markers")
        .join(&commit[..1])
        .join(&commit[1..3])
        .join(&commit[3..6])
        .join(&commit)
}

#[derive(juniper::GraphQLInputObject)]
struct MarkersInput {
    path: String,
    list: Vec<MarkerInput>,
}

#[derive(juniper::GraphQLInputObject)]
struct MarkerInput {
    position: String,
    text: String,
}

#[derive(juniper::GraphQLObject)]
struct Marker {
    position: String,
    text: String,
}

pub struct MarkersMut {
    commit: String,
    topic: String,
}

#[graphql_object(context = Context)]
impl MarkersMut {
    fn add(&self, context: &Context, markers: Vec<MarkersInput>) -> FieldResult<bool> {
        let transaction = context.transaction.lock()?;
        let rev = transaction.refname("refs/josh/meta");

        transaction
            .repo()
            .find_commit(git2::Oid::from_str(&self.commit)?)?;

        let r = transaction.repo().revparse_single(&rev);
        let (tree, parent) = if let Ok(r) = r {
            let commit = transaction.repo().find_commit(r.id())?;
            let tree = commit.tree()?;
            (tree, Some(commit))
        } else {
            (filter::tree::empty(&transaction.repo()), None)
        };

        let mut tree = tree;

        for mm in markers {
            let path = mm.path;
            let path = &marker_path(&self.commit, &self.topic).join(&path);
            let prev = if let Ok(e) = tree.get_path(&path) {
                let blob = transaction.repo().find_blob(e.id())?;
                std::str::from_utf8(blob.content())?.to_owned()
            } else {
                "".to_owned()
            };

            let mm = mm
                .list
                .iter()
                .map(|x| format!("{}: {}", &x.position, &x.text))
                .collect::<Vec<String>>();

            let mut lines = prev.split("\n").filter(|x| *x != "").collect::<Vec<_>>();
            for marker in mm.iter() {
                lines.push(marker);
            }
            lines.sort();
            lines.dedup();

            let blob = transaction.repo().blob(&lines.join("\n").as_bytes())?;

            tree = filter::tree::insert(transaction.repo(), &tree, &path, blob, 0o0100644)?;
        }

        transaction.repo().commit(
            Some(&rev),
            &transaction.repo().signature()?,
            &transaction.repo().signature()?,
            "marker",
            &tree,
            &if let Some(parent) = parent.as_ref() {
                vec![parent]
            } else {
                vec![]
            },
        )?;

        Ok(true)
    }
}

#[graphql_object(context = Context)]
impl RepositoryMut {
    fn meta(&self, commit: String, topic: String) -> FieldResult<MarkersMut> {
        Ok(MarkersMut {
            commit: commit,
            topic: topic,
        })
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
            "refs/josh/upstream/{}.git/{}",
            to_ns(&self.name),
            pattern.unwrap_or("refs/heads/*".to_string())
        );

        log::debug!("refname: {:?}", refname);

        let mut refs = vec![];

        for reference in transaction.repo().references_glob(&refname)? {
            let r = reference?;
            let name = r.name().ok_or(josh_error("reference without name"))?;

            refs.push(Reference {
                refname: name.to_string(),
            });
        }

        Ok(refs)
    }

    fn rev(&self, context: &Context, at: String, filter: Option<String>) -> FieldResult<Revision> {
        let rev = format!("refs/josh/upstream/{}.git/{}", to_ns(&self.name), at);

        let transaction = context.transaction.lock()?;
        let id = if let Ok(id) = git2::Oid::from_str(&at) {
            id
        } else {
            transaction.repo().revparse_single(&rev)?.id()
        };

        Ok(Revision {
            filter: filter::parse(&filter.unwrap_or(":/".to_string()))?,
            commit_id: id,
        })
    }
}

pub struct Query;

#[graphql_object(context = Context)]
impl Query {
    fn version() -> &str {
        option_env!("GIT_DESCRIBE").unwrap_or(std::env!("CARGO_PKG_VERSION"))
    }

    fn repos(context: &Context, name: Option<String>) -> FieldResult<Vec<Repository>> {
        let transaction = context.transaction.lock()?;

        let refname = format!("refs/josh/upstream/*.git/refs/heads/*");

        let mut repos = vec![];

        for reference in transaction.repo().references_glob(&refname)? {
            let r = reference?;
            let n = r.name().ok_or(josh_error("reference without name"))?;
            let n = UpstreamRef::from_str(n).ok_or(josh_error("not a ns"))?.ns;
            let n = from_ns(&n);

            if let Some(nn) = &name {
                if nn == &n {
                    repos.push(n);
                }
            } else {
                repos.push(n);
            }
        }

        repos.dedup();

        return Ok(repos.into_iter().map(|x| Repository { name: x }).collect());
    }
}

regex_parsed!(
    UpstreamRef,
    r"refs/josh/upstream/(?P<ns>.*)[.]git/(?P<reference>refs/heads/.*)",
    [ns, reference]
);

pub type Schema =
    juniper::RootNode<'static, Query, EmptyMutation<Context>, EmptySubscription<Context>>;

pub fn context(transaction: cache::Transaction) -> Context {
    Context {
        transaction: std::sync::Arc::new(std::sync::Mutex::new(transaction)),
    }
}

pub fn schema() -> Schema {
    Schema::new(Query, EmptyMutation::new(), EmptySubscription::new())
}

pub type CommitSchema =
    juniper::RootNode<'static, Revision, EmptyMutation<Context>, EmptySubscription<Context>>;

pub fn commit_schema(id: git2::Oid) -> CommitSchema {
    CommitSchema::new(
        Revision {
            commit_id: id,
            filter: filter::nop(),
        },
        EmptyMutation::new(),
        EmptySubscription::new(),
    )
}

pub type RepoSchema =
    juniper::RootNode<'static, Repository, RepositoryMut, EmptySubscription<Context>>;

pub fn repo_schema(name: &str) -> RepoSchema {
    RepoSchema::new(
        Repository {
            name: name.to_string(),
        },
        RepositoryMut {},
        EmptySubscription::new(),
    )
}
