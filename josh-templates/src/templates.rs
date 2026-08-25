use anyhow::anyhow;
use josh_core::cache;
use josh_core::cache::{CacheStack, TransactionContext};
use serde_json::json;

struct GraphQLHelper {
    repo_path: std::path::PathBuf,
    cache: std::sync::Arc<CacheStack>,
    ref_prefix: String,
    commit_id: gix_hash::ObjectId,
}

impl GraphQLHelper {
    fn transaction_context(&self, path: impl AsRef<std::path::Path>) -> TransactionContext {
        TransactionContext::new(path, self.cache.clone()).with_ref_prefix(&self.ref_prefix)
    }
}

impl GraphQLHelper {
    fn josh_helper(
        &self,
        hash: &std::collections::BTreeMap<&str, handlebars::PathAndJson>,
        template_name: &str,
    ) -> anyhow::Result<serde_json::Value> {
        let mirror_path = self.repo_path.join("mirror");
        let overlay_path = self.repo_path.join("overlay");

        let path = if let Some(f) = hash.get("file") {
            f.render()
        } else {
            return Err(anyhow!("missing pattern"));
        };

        let path = std::path::PathBuf::from(template_name)
            .join("..")
            .join(path);

        let path = josh_core::normalize_path(&path);
        let transaction = if let Ok(to) = self.transaction_context(&mirror_path).open() {
            to.add_disk_alternate(
                self.repo_path
                    .join("overlay")
                    .join("objects")
                    .to_str()
                    .unwrap(),
            )?;
            to
        } else {
            self.transaction_context(&self.repo_path).open()?
        };

        let odb = transaction.odb();
        let tree = josh_core::objects::CommitData::read(odb, self.commit_id)?.tree_id()?;
        let entry = josh_core::objects::path_entry(odb, tree, &path)?
            .ok_or_else(|| anyhow!("no such path: {}", path.display()))?;
        let query = josh_core::objects::blob_text(odb, entry.oid.to_owned());

        let mut variables = juniper::Variables::new();

        for (k, v) in hash.iter() {
            variables.insert(k.to_string(), juniper::InputValue::scalar(v.render()));
        }

        let (transaction, transaction_mirror) =
            if let Ok(to) = self.transaction_context(&overlay_path).open() {
                to.add_disk_alternate(
                    self.repo_path
                        .join("mirror")
                        .join("objects")
                        .to_str()
                        .unwrap(),
                )?;
                (to, self.transaction_context(&mirror_path).open()?)
            } else {
                (
                    self.transaction_context(&self.repo_path).open()?,
                    self.transaction_context(&self.repo_path).open()?,
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
    commit_id: gix_hash::ObjectId,
    query_and_params: &str,
    split_odb: bool,
) -> anyhow::Result<Option<(String, std::collections::BTreeMap<String, String>)>> {
    let repo_path = transaction.path();
    let overlay_path = transaction
        .path()
        .parent()
        .ok_or(anyhow!("parent"))?
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
        return Err(anyhow!("no command"));
    };

    let odb = transaction.odb();
    let tree = josh_core::objects::CommitData::read(odb, commit_id)?.tree_id()?;
    let entry = josh_core::objects::path_entry(odb, tree, &std::path::PathBuf::from(path))?;

    let entry = if let Some(entry) = entry {
        entry
    } else {
        return Ok(None);
    };

    let template = if entry.mode.is_blob() {
        let content = josh_core::objects::blob_text(odb, entry.oid.to_owned());
        let file = content.as_str();
        if cmd == "get" {
            return Ok(Some((file.to_string(), params)));
        }
        if cmd == "graphql" {
            let mut variables = juniper::Variables::new();

            for (k, v) in params.iter() {
                variables.insert(k.to_string(), juniper::InputValue::scalar(v.clone()));
            }

            let (transaction, transaction_mirror) =
                if let Ok(to) = TransactionContext::new(&overlay_path, cache.clone()).open() {
                    to.add_disk_alternate(
                        transaction
                            .path()
                            .parent()
                            .ok_or(anyhow!("parent"))?
                            .join("mirror")
                            .join("objects")
                            .to_str()
                            .unwrap(),
                    )?;
                    (
                        to,
                        TransactionContext::new(repo_path, cache.clone()).open()?,
                    )
                } else {
                    (
                        TransactionContext::new(repo_path, cache.clone()).open()?,
                        TransactionContext::new(repo_path, cache.clone()).open()?,
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
            return Err(anyhow!("no such cmd"));
        }
    } else {
        return Ok(Some(("".to_string(), params)));
    };

    let repo_path = if split_odb {
        transaction
            .path()
            .parent()
            .ok_or(anyhow!("parent"))?
            .to_owned()
    } else {
        transaction.path().to_owned()
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
        Err(res) => return Err(anyhow!("{}", res)),
    };

    Ok(Some((rendered, params)))
}
