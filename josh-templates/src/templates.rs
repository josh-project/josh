use josh_core::cache::TransactionContext;
use josh_core::cache_stack::CacheStack;
use josh_core::{JoshResult, cache, josh_error};
use serde_json::json;

struct GraphQLHelper {
    repo_path: std::path::PathBuf,
    cache: std::sync::Arc<CacheStack>,
    ref_prefix: String,
    commit_id: git2::Oid,
}

impl GraphQLHelper {
    fn transaction_context(&self, path: impl AsRef<std::path::Path>) -> TransactionContext {
        TransactionContext::new(path, self.cache.clone())
    }
}

impl GraphQLHelper {
    fn josh_helper(
        &self,
        hash: &std::collections::BTreeMap<&str, handlebars::PathAndJson>,
        template_name: &str,
    ) -> JoshResult<serde_json::Value> {
        let mirror_path = self.repo_path.join("mirror");
        let overlay_path = self.repo_path.join("overlay");

        let path = if let Some(f) = hash.get("file") {
            f.render()
        } else {
            return Err(josh_error("missing pattern"));
        };

        let path = std::path::PathBuf::from(template_name)
            .join("..")
            .join(path);

        let path = josh_core::normalize_path(&path);
        let transaction = if let Ok(to) = self
            .transaction_context(&mirror_path)
            .open(Some(&self.ref_prefix))
        {
            to.repo().odb()?.add_disk_alternate(
                self.repo_path
                    .join("overlay")
                    .join("objects")
                    .to_str()
                    .unwrap(),
            )?;
            to
        } else {
            self.transaction_context(&self.repo_path)
                .open(Some(&self.ref_prefix))?
        };

        let tree = transaction.repo().find_commit(self.commit_id)?.tree()?;

        let blob = tree
            .get_path(&path)?
            .to_object(transaction.repo())?
            .peel_to_blob()
            .map(|x| x.content().to_vec())
            .unwrap_or_default();
        let query = String::from_utf8(blob)?;

        let mut variables = juniper::Variables::new();

        for (k, v) in hash.iter() {
            variables.insert(k.to_string(), juniper::InputValue::scalar(v.render()));
        }

        let (transaction, transaction_mirror) =
            if let Ok(to) = self.transaction_context(&overlay_path).open(None) {
                to.repo().odb()?.add_disk_alternate(
                    self.repo_path
                        .join("mirror")
                        .join("objects")
                        .to_str()
                        .unwrap(),
                )?;
                (to, self.transaction_context(&mirror_path).open(None)?)
            } else {
                (
                    self.transaction_context(&self.repo_path).open(None)?,
                    self.transaction_context(&self.repo_path).open(None)?,
                )
            };

        let (res, _errors) = juniper::execute_sync(
            &query,
            None,
            &josh_graphql::graphql::commit_schema(self.commit_id),
            &variables,
            &josh_graphql::context(transaction, transaction_mirror),
        )?;

        let j = serde_json::to_string(&res)?;
        let j: serde_json::Value = serde_json::from_str(&j)?;

        let j = if let Some(at) = hash.get("at") {
            j.pointer(&at.render()).unwrap_or(&json!({})).to_owned()
        } else {
            j
        };

        Ok(j)
    }
}

impl handlebars::HelperDef for GraphQLHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &handlebars::Helper,
        _: &handlebars::Handlebars,
        _: &handlebars::Context,
        rc: &mut handlebars::RenderContext,
    ) -> Result<handlebars::ScopedJson<'rc>, handlebars::RenderError> {
        Ok(handlebars::ScopedJson::Derived(
            self.josh_helper(
                h.hash(),
                rc.get_current_template_name().unwrap_or(&"/".to_owned()),
            )
            .map_err(|e| handlebars::RenderErrorReason::Other(format!("{}", e)))?,
        ))
    }
}

mod helpers {
    handlebars::handlebars_helper!(concat_helper: |x: str, y: str| format!("{}{}", x, y) );
}

pub fn render(
    transaction: &cache::Transaction,
    cache: std::sync::Arc<CacheStack>,
    ref_prefix: &str,
    commit_id: git2::Oid,
    query_and_params: &str,
    split_odb: bool,
) -> JoshResult<Option<(String, std::collections::BTreeMap<String, String>)>> {
    let repo_path = transaction.repo().path();
    let overlay_path = transaction
        .repo()
        .path()
        .parent()
        .ok_or(josh_error("parent"))?
        .join("overlay");

    let params = form_urlencoded::parse(query_and_params.as_bytes())
        .map(|(x, y)| (x.to_string(), y.to_string()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let (cmd, path) = if let Some(path) = params.get("get") {
        ("get", path)
    } else if let Some(path) = params.get("graphql") {
        ("graphql", path)
    } else if let Some(path) = params.get("render") {
        ("render", path)
    } else {
        return Err(josh_error("no command"));
    };

    let tree = transaction.repo().find_commit(commit_id)?.tree()?;
    let obj = tree
        .get_path(&std::path::PathBuf::from(path))?
        .to_object(transaction.repo());

    let obj = if let Ok(obj) = obj {
        obj
    } else {
        return Ok(None);
    };

    let template = if let Ok(blob) = obj.peel_to_blob() {
        let file = std::str::from_utf8(blob.content())?;
        if cmd == "get" {
            return Ok(Some((file.to_string(), params)));
        }
        if cmd == "graphql" {
            let mut variables = juniper::Variables::new();

            for (k, v) in params.iter() {
                variables.insert(k.to_string(), juniper::InputValue::scalar(v.clone()));
            }

            let (transaction, transaction_mirror) =
                if let Ok(to) = TransactionContext::new(&overlay_path, cache.clone()).open(None) {
                    to.repo().odb()?.add_disk_alternate(
                        transaction
                            .repo()
                            .path()
                            .parent()
                            .ok_or(josh_error("parent"))?
                            .join("mirror")
                            .join("objects")
                            .to_str()
                            .unwrap(),
                    )?;
                    (
                        to,
                        TransactionContext::new(repo_path, cache.clone()).open(None)?,
                    )
                } else {
                    (
                        TransactionContext::new(repo_path, cache.clone()).open(None)?,
                        TransactionContext::new(repo_path, cache.clone()).open(None)?,
                    )
                };
            let (res, _errors) = juniper::execute_sync(
                file,
                None,
                &josh_graphql::commit_schema(commit_id),
                &variables,
                &josh_graphql::context(transaction, transaction_mirror),
            )?;

            let j = serde_json::to_string_pretty(&res)?;
            return Ok(Some((j, params)));
        }
        if cmd == "render" {
            file.to_string()
        } else {
            return Err(josh_error("no such cmd"));
        }
    } else {
        return Ok(Some(("".to_string(), params)));
    };

    drop(obj);
    drop(tree);

    let repo_path = if split_odb {
        transaction
            .repo()
            .path()
            .parent()
            .ok_or(josh_error("parent"))?
            .to_owned()
    } else {
        transaction.repo().path().to_owned()
    };

    let mut handlebars = handlebars::Handlebars::new();
    handlebars.register_template_string(path, template)?;
    handlebars.register_helper("concat", Box::new(helpers::concat_helper));
    handlebars.register_helper(
        "graphql",
        Box::new(GraphQLHelper {
            repo_path,
            cache,
            ref_prefix: ref_prefix.to_owned(),
            commit_id,
        }),
    );
    handlebars.set_strict_mode(true);

    let rendered = match handlebars.render(path, &json!(params)) {
        Ok(res) => res,
        Err(res) => return Err(josh_error(&format!("{}", res))),
    };

    Ok(Some((rendered, params)))
}
