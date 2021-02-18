use super::*;

struct GraphQLHelper {
    repo_path: std::path::PathBuf,
    headref: String,
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

        let transaction = cache::Transaction::open(&self.repo_path)?;

        let reference = transaction.repo().find_reference(&self.headref)?;
        let tree = reference.peel_to_tree()?;

        let blob = tree
            .get_path(&path)?
            .to_object(&transaction.repo())?
            .peel_to_blob()
            .map(|x| x.content().to_vec())
            .unwrap_or(vec![]);
        let query = String::from_utf8(blob)?;

        let transaction = cache::Transaction::open(&self.repo_path)?;
        let (res, _errors) = juniper::execute_sync(
            &query,
            None,
            &graphql::commit_schema(
                reference.target().ok_or(josh_error("missing target"))?,
            ),
            &juniper::Variables::new(),
            &graphql::context(transaction),
        )?;

        let j = serde_json::to_string(&res)?;
        let j: serde_json::Value = serde_json::from_str(&j)?;

        let j = if let Some(at) = hash.get("at") {
            j.pointer(&at.render()).unwrap_or(&json!({})).to_owned()
        } else {
            j
        };

        return Ok(j);
    }
}

impl handlebars::HelperDef for GraphQLHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &handlebars::Helper,
        _: &handlebars::Handlebars,
        _: &handlebars::Context,
        rc: &mut handlebars::RenderContext,
    ) -> Result<
        Option<handlebars::ScopedJson<'reg, 'rc>>,
        handlebars::RenderError,
    > {
        return Ok(Some(handlebars::ScopedJson::Derived(
            self.josh_helper(
                h.hash(),
                &rc.get_current_template_name().unwrap_or(&"/".to_owned()),
            )
            .map_err(|_| handlebars::RenderError::new("josh"))?,
        )));
    }
}

mod helpers {
    handlebars_helper!(concat_helper: |x: str, y: str| format!("{}{}", x, y) );
}

pub fn render(
    repo: &git2::Repository,
    headref: &str,
    query_and_params: &str,
) -> JoshResult<Option<String>> {
    let mut parameters = query_and_params.split("&");
    let query = parameters
        .next()
        .ok_or(josh_error(&format!("invalid query {:?}", query_and_params)))?;
    let mut split = query.splitn(2, "=");
    let cmd = split
        .next()
        .ok_or(josh_error(&format!("invalid query {:?}", query_and_params)))?;
    let path = split
        .next()
        .ok_or(josh_error(&format!("invalid query {:?}", query_and_params)))?;
    let tree = repo.find_reference(&headref)?.peel_to_tree()?;

    let obj = ok_or!(
        tree.get_path(&std::path::PathBuf::from(path))?
            .to_object(&repo),
        {
            return Ok(None);
        }
    );

    let template = if let Ok(blob) = obj.peel_to_blob() {
        let template = std::str::from_utf8(blob.content())?;
        if cmd == "get" {
            return Ok(Some(template.to_string()));
        }
        if cmd == "render" {
            template.to_string()
        } else {
            return Err(josh_error("no such cmd"));
        }
    } else {
        return Ok(Some("".to_string()));
    };

    std::mem::drop(obj);
    std::mem::drop(tree);

    let mut handlebars = handlebars::Handlebars::new();
    handlebars.register_template_string(&path, template)?;
    handlebars.register_helper("concat", Box::new(helpers::concat_helper));
    handlebars.register_helper(
        "graphql",
        Box::new(GraphQLHelper {
            repo_path: repo.path().to_owned(),
            headref: headref.to_string(),
        }),
    );

    let mut params = std::collections::BTreeMap::new();
    for p in parameters {
        let mut split = p.splitn(2, "=");
        let name = split.next().ok_or(josh_error(&format!(
            "invalid query {:?}",
            query_and_params
        )))?;
        let value = split.next().ok_or(josh_error(&format!(
            "invalid query {:?}",
            query_and_params
        )))?;
        params.insert(name.to_string(), value.to_string());
    }

    return Ok(Some(format!(
        "{}",
        handlebars.render(&path, &json!(params))?
    )));
}
