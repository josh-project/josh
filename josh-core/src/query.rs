use super::*;

struct GraphQLHelper {
    repo_path: std::path::PathBuf,
    ref_prefix: String,
    commit_id: git2::Oid,
}

impl GraphQLHelper {
    fn josh_helper(
        &self,
        hash: &std::collections::BTreeMap<&str, handlebars::PathAndJson>,
        template_name: &str,
    ) -> JoshResult<serde_json::Value> {
        let path = if let Some(f) = hash.get("file") {
            f.render()
        } else {
            return Err(josh_error("missing pattern"));
        };

        let path = std::path::PathBuf::from(template_name)
            .join("..")
            .join(path);
        let path = normalize_path(&path);

        let transaction = if let Ok(to) =
            cache::Transaction::open(&self.repo_path.join("mirror"), Some(&self.ref_prefix))
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
            cache::Transaction::open(&self.repo_path, Some(&self.ref_prefix))?
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

        let (transaction, transaction_overlay) =
            if let Ok(to) = cache::Transaction::open(&self.repo_path.join("overlay"), None) {
                to.repo().odb()?.add_disk_alternate(
                    self.repo_path
                        .join("mirror")
                        .join("objects")
                        .to_str()
                        .unwrap(),
                )?;
                (
                    cache::Transaction::open(&self.repo_path.join("mirror"), None)?,
                    to,
                )
            } else {
                (
                    cache::Transaction::open(&self.repo_path, None)?,
                    cache::Transaction::open(&self.repo_path, None)?,
                )
            };

        let (res, _errors) = juniper::execute_sync(
            &query,
            None,
            &graphql::commit_schema(self.commit_id),
            &variables,
            &graphql::context(transaction, transaction_overlay),
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
    ) -> Result<handlebars::ScopedJson<'reg, 'rc>, handlebars::RenderError> {
        return Ok(handlebars::ScopedJson::Derived(
            self.josh_helper(
                h.hash(),
                rc.get_current_template_name().unwrap_or(&"/".to_owned()),
            )
            .map_err(|e| handlebars::RenderError::new(format!("{}", e)))?,
        ));
    }
}

mod helpers {
    handlebars_helper!(concat_helper: |x: str, y: str| format!("{}{}", x, y) );
}

pub fn render(
    transaction: &cache::Transaction,
    ref_prefix: &str,
    commit_id: git2::Oid,
    query_and_params: &str,
    split_odb: bool,
) -> JoshResult<Option<String>> {
    let mut parameters = query_and_params.split('&');
    let query = parameters
        .next()
        .ok_or_else(|| josh_error(&format!("invalid query {:?}", query_and_params)))?;
    let mut split = query.splitn(2, '=');
    let cmd = split
        .next()
        .ok_or_else(|| josh_error(&format!("invalid query {:?}", query_and_params)))?;
    let path = split
        .next()
        .ok_or_else(|| josh_error(&format!("invalid query {:?}", query_and_params)))?;

    let tree = transaction.repo().find_commit(commit_id)?.tree()?;

    let obj = ok_or!(
        tree.get_path(&std::path::PathBuf::from(path))?
            .to_object(transaction.repo()),
        {
            return Ok(None);
        }
    );

    let mut params = std::collections::BTreeMap::new();
    for p in parameters {
        let mut split = p.splitn(2, '=');
        let name = split
            .next()
            .ok_or_else(|| josh_error(&format!("invalid query {:?}", query_and_params)))?;
        let value = split
            .next()
            .ok_or_else(|| josh_error(&format!("invalid query {:?}", query_and_params)))?;
        params.insert(name.to_string(), value.to_string());
    }

    let template = if let Ok(blob) = obj.peel_to_blob() {
        let template = std::str::from_utf8(blob.content())?;
        if cmd == "get" {
            return Ok(Some(template.to_string()));
        }
        if cmd == "graphql" {
            let mut variables = juniper::Variables::new();

            for (k, v) in params {
                variables.insert(k.to_string(), juniper::InputValue::scalar(v));
            }
            let transaction = cache::Transaction::open(transaction.repo().path(), None)?;
            let transaction_overlay = cache::Transaction::open(transaction.repo().path(), None)?;
            let (res, _errors) = juniper::execute_sync(
                template,
                None,
                &graphql::commit_schema(commit_id),
                &variables,
                &graphql::context(transaction, transaction_overlay),
            )?;

            let j = serde_json::to_string_pretty(&res)?;
            return Ok(Some(j));
        }
        if cmd == "render" {
            template.to_string()
        } else {
            return Err(josh_error("no such cmd"));
        }
    } else {
        return Ok(Some("".to_string()));
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
            repo_path: repo_path,
            ref_prefix: ref_prefix.to_owned(),
            commit_id,
        }),
    );
    handlebars.set_strict_mode(true);

    match handlebars.render(path, &json!(params)) {
        Ok(res) => Ok(Some(res)),
        Err(res) => return Err(josh_error(&format!("{}", res))),
    }
}
