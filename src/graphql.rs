#![allow(unused_variables)]

use super::*;
use juniper::{graphql_object, EmptyMutation, EmptySubscription, FieldResult};

pub struct Commit {
    filter: filter::Filter,
    rev: git2::Oid,
}

#[graphql_object(context = Context)]
impl Commit {
    fn filter(&self) -> String {
        filter::spec(self.filter)
    }

    fn id(&self, context: &Context) -> FieldResult<String> {
        let transaction = cache::Transaction::open(&context.path)?;
        let commit = transaction.repo().find_commit(self.rev)?;
        tracing::trace!("Commit::rev: {:?}", self.filter);
        let filter_commit =
            filter::apply_to_commit(self.filter, &commit, &transaction)?;
        Ok(format!("{}", filter_commit))
    }

    fn summary(&self, context: &Context) -> FieldResult<String> {
        let transaction = cache::Transaction::open(&context.path)?;
        let commit = transaction.repo().find_commit(self.rev)?;
        tracing::trace!("Commit::rev: {:?}", self.filter);
        let filter_commit = transaction.repo().find_commit(
            filter::apply_to_commit(self.filter, &commit, &transaction)?,
        )?;
        Ok(filter_commit.summary().unwrap_or("").to_owned())
    }

    fn apply(&self, filter: String) -> FieldResult<Commit> {
        Ok(Commit {
            filter: filter::chain(self.filter, filter::parse(&filter)?),
            rev: self.rev,
        })
    }

    fn original(&self, context: &Context) -> FieldResult<Commit> {
        let transaction = cache::Transaction::open(&context.path)?;
        let commit = transaction.repo().find_commit(self.rev)?;
        let filter_commit = transaction.repo().find_commit(
            filter::apply_to_commit(self.filter, &commit, &transaction)?,
        )?;
        Ok(Commit {
            filter: filter::parse(":nop")?,
            rev: history::find_original(
                &transaction,
                self.filter,
                self.rev,
                filter_commit.id(),
            )
            .unwrap_or(git2::Oid::zero()),
        })
    }

    fn parents(&self, context: &Context) -> FieldResult<Vec<Commit>> {
        let transaction = cache::Transaction::open(&context.path)?;
        let commit = transaction.repo().find_commit(self.rev)?;
        let filter_commit = transaction.repo().find_commit(
            filter::apply_to_commit(self.filter, &commit, &transaction)?,
        )?;

        let parents = filter_commit
            .parent_ids()
            .map(|id| Commit {
                filter: self.filter,
                rev: history::find_original(
                    &transaction,
                    self.filter,
                    self.rev,
                    id,
                )
                .unwrap_or(git2::Oid::zero()),
            })
            .collect();

        Ok(parents)
    }

    fn files(&self, context: &Context) -> FieldResult<Vec<File>> {
        let transaction = cache::Transaction::open(&context.path)?;
        let commit = transaction.repo().find_commit(self.rev)?;

        let tree = filter::apply(&transaction, self.filter, commit.tree()?)?;

        /* let tree = transaction.repo().find_commit(filter_commit)?.tree()?; */
        let mut ws = vec![];
        tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            if let Some(git2::ObjectType::Blob) = entry.kind() {
                if let Some(name) = entry.name() {
                    ws.push(File {
                        name: format!("{}{}", root, name),
                        id: entry.id(),
                    });
                }
            }
            0
        })?;
        return Ok(ws);
    }

    fn workspaces(&self, context: &Context) -> FieldResult<Vec<Commit>> {
        let transaction = cache::Transaction::open(&context.path)?;
        let tree = transaction.repo().find_commit(self.rev)?.tree()?;

        let mut ws = vec![];
        tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            if entry.name() == Some(&"workspace.josh") {
                if let Ok(filter) = filter::parse(&format!(
                    ":workspace={}",
                    root.trim_matches('/').to_owned()
                )) {
                    ws.push(Commit {
                        rev: self.rev,
                        filter: filter,
                    });
                }
            }
            0
        })?;
        return Ok(ws);
    }
}

pub struct File {
    name: String,
    id: git2::Oid,
}

#[graphql_object(context = Context)]
impl File {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn id(&self) -> String {
        format!("{}", self.id)
    }

    fn content(&self, context: &Context) -> FieldResult<String> {
        let transaction = cache::Transaction::open(&context.path)?;
        let blob = transaction.repo().find_blob(self.id)?;

        Ok(std::str::from_utf8(blob.content())?.to_string())
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

    fn commit(
        &self,
        context: &Context,
        filter: Option<String>,
    ) -> FieldResult<Commit> {
        let transaction = cache::Transaction::open(&context.path)?;
        let rev = transaction
            .repo()
            .find_reference(&self.refname)?
            .target()
            .unwrap_or(git2::Oid::zero());

        Ok(Commit {
            filter: filter::parse(&filter.unwrap_or(":nop".to_string()))?,
            rev: rev,
        })
    }
}

pub struct Context {
    path: std::path::PathBuf,
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
        name: Option<String>,
    ) -> FieldResult<Vec<Reference>> {
        let transaction = cache::Transaction::open(&context.path)?;
        let refname = format!(
            "refs/josh/upstream/{}.git/{}",
            to_ns(&self.name),
            name.unwrap_or("refs/heads/*".to_string())
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
        let transaction = cache::Transaction::open(&context.path)?;

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

pub fn context(path: &std::path::Path) -> Context {
    Context {
        path: path.to_owned(),
    }
}

pub fn schema() -> Schema {
    Schema::new(Query, EmptyMutation::new(), EmptySubscription::new())
}
