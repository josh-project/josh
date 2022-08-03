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

        let transaction = cache::Transaction::open(&self.repo_path, Some(&self.ref_prefix))?;

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

        let transaction = cache::Transaction::open(&self.repo_path, None)?;
        let (res, _errors) = juniper::execute_sync(
            &query,
            None,
            &graphql::commit_schema(self.commit_id),
            &variables,
            &graphql::context(transaction),
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
            .map_err(|_| handlebars::RenderError::new("josh"))?,
        ));
    }
}

mod helpers {
    handlebars_helper!(concat_helper: |x: str, y: str| format!("{}{}", x, y) );
}

pub fn render(
    repo: &git2::Repository,
    ref_prefix: &str,
    headref: &str,
    query_and_params: &str,
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

    let reference = repo.find_reference(headref)?;
    let commit_id = reference.peel_to_commit()?.id();

    let tree = repo.find_commit(commit_id)?.tree()?;

    let obj = ok_or!(
        tree.get_path(&std::path::PathBuf::from(path))?
            .to_object(repo),
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
            let transaction = cache::Transaction::open(repo.path(), None)?;
            let (res, _errors) = juniper::execute_sync(
                template,
                None,
                &graphql::commit_schema(commit_id),
                &variables,
                &graphql::context(transaction),
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

    let mut handlebars = handlebars::Handlebars::new();
    handlebars.register_template_string(path, template)?;
    handlebars.register_helper("concat", Box::new(helpers::concat_helper));
    handlebars.register_helper(
        "graphql",
        Box::new(GraphQLHelper {
            repo_path: repo.path().to_owned(),
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
