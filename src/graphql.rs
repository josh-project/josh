#![allow(unused_variables)]

use super::*;
use juniper::{graphql_object, EmptyMutation, EmptySubscription, FieldResult};

pub struct Revision {
    filter: filter::Filter,
    id: git2::Oid,
}

#[graphql_object(context = Context)]
impl Revision {
    fn filter(&self) -> String {
        filter::spec(self.filter)
    }

    fn hash(&self, context: &Context) -> FieldResult<String> {
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.id)?;
        let filter_commit =
            filter::apply_to_commit(self.filter, &commit, &transaction)?;
        Ok(format!("{}", filter_commit))
    }

    fn summary(&self, context: &Context) -> FieldResult<String> {
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.id)?;
        let filter_commit = transaction.repo().find_commit(
            filter::apply_to_commit(self.filter, &commit, &transaction)?,
        )?;
        Ok(filter_commit.summary().unwrap_or("").to_owned())
    }

    fn date(&self, format: String, context: &Context) -> FieldResult<String> {
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.id)?;

        let ts = commit.time().seconds();

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
            let commit = transaction.repo().find_commit(self.id)?;
            let filter_commit = transaction.repo().find_commit(
                filter::apply_to_commit(self.filter, &commit, &transaction)?,
            )?;

            history::find_original(
                &transaction,
                self.filter,
                self.id,
                filter_commit.id(),
            )?
        } else {
            self.id
        };

        Ok(Some(Revision {
            filter: filter::parse(&filter.unwrap_or(":nop".to_string()))?,
            id: id,
        }))
    }

    fn parents(&self, context: &Context) -> FieldResult<Vec<Revision>> {
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.id)?;
        let filter_commit = transaction.repo().find_commit(
            filter::apply_to_commit(self.filter, &commit, &transaction)?,
        )?;

        let parents = filter_commit
            .parent_ids()
            .map(|id| Revision {
                filter: self.filter,
                id: history::find_original(
                    &transaction,
                    self.filter,
                    self.id,
                    id,
                )
                .unwrap_or(git2::Oid::zero()),
            })
            .collect();

        Ok(parents)
    }

    fn files(&self, context: &Context) -> FieldResult<Vec<Path>> {
        let transaction = context.transaction.lock()?;
        let commit = transaction.repo().find_commit(self.id)?;

        let tree = filter::apply(&transaction, self.filter, commit.tree()?)?;

        let mut ws = vec![];
        tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            if let Some(git2::ObjectType::Blob) = entry.kind() {
                if let Some(name) = entry.name() {
                    ws.push(Path {
                        path: std::path::Path::new(root).join(name),
                        id: self.id,
                        tree: tree.id(),
                    });
                }
            }
            0
        })?;
        return Ok(ws);
    }

    fn file(&self, path: String, context: &Context) -> FieldResult<Path> {
        let transaction = context.transaction.lock()?;
        let path = std::path::Path::new(&path).to_owned();
        let tree = transaction.repo().find_commit(self.id)?.tree()?;

        let tree = filter::apply(&transaction, self.filter, tree)?;

        if let Some(git2::ObjectType::Blob) = tree.get_path(&path)?.kind() {
            Ok(Path {
                path: path,
                id: self.id,
                tree: tree.id(),
            })
        } else {
            Err(josh_error("not a blob"))?
        }
    }
}

pub struct Path {
    path: std::path::PathBuf,
    id: git2::Oid,
    tree: git2::Oid,
}

#[graphql_object(context = Context)]
impl Path {
    fn path(&self) -> String {
        self.path.to_string_lossy().to_string()
    }

    fn dir(&self, relative: String) -> FieldResult<Path> {
        Ok(Path {
            path: normalize_path(&self.path.join(&relative)),
            id: self.id,
            tree: self.tree,
        })
    }

    fn rev(&self, filter: String) -> FieldResult<Revision> {
        let hm: std::collections::HashMap<String, String> =
            [("path".to_string(), self.path.to_string_lossy().to_string())]
                .iter()
                .cloned()
                .collect();
        Ok(Revision {
            filter: filter::parse(&strfmt::strfmt(&filter, &hm)?)?,
            id: self.id,
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
        let value = toml::de::from_str::<serde_json::Value>(
            std::str::from_utf8(blob.content())?,
        )
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
        let value = serde_json::from_str::<serde_json::Value>(
            std::str::from_utf8(blob.content())?,
        )
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
        let value = serde_yaml::from_str::<serde_json::Value>(
            std::str::from_utf8(blob.content())?,
        )
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
    fn string(
        &self,
        at: Option<String>,
        default: Option<String>,
    ) -> Option<String> {
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

    fn rev(
        &self,
        context: &Context,
        filter: Option<String>,
    ) -> FieldResult<Revision> {
        let transaction = context.transaction.lock()?;
        let id = transaction
            .repo()
            .find_reference(&self.refname)?
            .target()
            .unwrap_or(git2::Oid::zero());

        Ok(Revision {
            filter: filter::parse(&filter.unwrap_or(":nop".to_string()))?,
            id: id,
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

#[graphql_object(context = Context)]
impl Repository {
    fn name(&self) -> &str {
        &self.name
    }

    fn refs(
        &self,
        context: &Context,
        pattern: Option<String>,
    ) -> FieldResult<Vec<Reference>> {
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

    fn rev(
        &self,
        context: &Context,
        at: String,
        filter: Option<String>,
    ) -> FieldResult<Revision> {
        let rev =
            format!("refs/josh/upstream/{}.git/{}", to_ns(&self.name), at);

        let transaction = context.transaction.lock()?;
        let id = transaction.repo().revparse_single(&rev)?.id();

        Ok(Revision {
            filter: filter::parse(&filter.unwrap_or(":nop".to_string()))?,
            id: id,
        })
    }
}

pub struct Query;

#[graphql_object(context = Context)]
impl Query {
    fn version() -> &str {
        option_env!("GIT_DESCRIBE").unwrap_or(std::env!("CARGO_PKG_VERSION"))
    }

    fn repos(
        context: &Context,
        name: Option<String>,
    ) -> FieldResult<Vec<Repository>> {
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

pub type Schema = juniper::RootNode<
    'static,
    Query,
    EmptyMutation<Context>,
    EmptySubscription<Context>,
>;

pub fn context(transaction: cache::Transaction) -> Context {
    Context {
        transaction: std::sync::Arc::new(std::sync::Mutex::new(transaction)),
    }
}

pub fn schema() -> Schema {
    Schema::new(Query, EmptyMutation::new(), EmptySubscription::new())
}

pub type CommitSchema = juniper::RootNode<
    'static,
    Revision,
    EmptyMutation<Context>,
    EmptySubscription<Context>,
>;

pub fn commit_schema(id: git2::Oid) -> CommitSchema {
    CommitSchema::new(
        Revision {
            id: id,
            filter: filter::nop(),
        },
        EmptyMutation::new(),
        EmptySubscription::new(),
    )
}

pub type RepoSchema = juniper::RootNode<
    'static,
    Repository,
    EmptyMutation<Context>,
    EmptySubscription<Context>,
>;

pub fn repo_schema(name: &str) -> RepoSchema {
    RepoSchema::new(
        Repository {
            name: name.to_string(),
        },
        EmptyMutation::new(),
        EmptySubscription::new(),
    )
}
