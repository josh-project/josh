use super::*;

struct BlobHelper {
    repo_path: std::path::PathBuf,
    headref: String,
}

impl BlobHelper {
    fn josh_helper(
        &self,
        hash: &std::collections::BTreeMap<&str, handlebars::PathAndJson>,
    ) -> JoshResult<serde_json::Value> {
        let path = if let Some(f) = hash.get("path") {
            f.render()
        } else {
            return Err(josh_error("missing pattern"));
        };

        let transaction = cache::Transaction::open(&self.repo_path)?;
        let tree = transaction
            .repo()
            .find_reference(&self.headref)?
            .peel_to_tree()?;

        let blob = tree
            .get_path(&std::path::PathBuf::from(path))?
            .to_object(&transaction.repo())?
            .peel_to_blob()
            .map(|x| x.content().to_vec())
            .unwrap_or(vec![]);
        return Ok(json!(String::from_utf8(blob)?));
    }
}

impl handlebars::HelperDef for BlobHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &handlebars::Helper,
        _: &handlebars::Handlebars,
        _: &handlebars::Context,
        _rc: &mut handlebars::RenderContext,
    ) -> Result<
        Option<handlebars::ScopedJson<'reg, 'rc>>,
        handlebars::RenderError,
    > {
        return Ok(Some(handlebars::ScopedJson::Derived(
            self.josh_helper(h.hash())
                .map_err(|_| handlebars::RenderError::new("josh"))?,
        )));
    }
}

struct FindFilesHelper {
    repo_path: std::path::PathBuf,
    headref: String,
}

impl FindFilesHelper {
    fn josh_helper(
        &self,
        hash: &std::collections::BTreeMap<&str, handlebars::PathAndJson>,
    ) -> JoshResult<serde_json::Value> {
        let filterspec = if let Some(f) = hash.get("filter") {
            f.render()
        } else {
            return Err(josh_error("missing filter"));
        };
        let transaction = cache::Transaction::open(&self.repo_path)?;
        let tree = transaction
            .repo()
            .find_reference(&self.headref)?
            .peel_to_tree()?;

        let tree = filter::apply(
            transaction.repo(),
            filter::parse(&filterspec)?,
            tree,
        )?;

        let mut names = vec![];
        tree.walk(
        git2::TreeWalkMode::PreOrder, |root, entry| {
            let name = entry.name().unwrap_or("INVALID_FILENAME");
            let path = std::path::PathBuf::from(root).join(name);
            let path_str = path.to_string_lossy();

            if entry.kind() == Some(git2::ObjectType::Blob) {

            names.push(json!({
            "path": path_str,
            "name": path.file_name().map(|x|x.to_str()).flatten().unwrap_or("INVALID_FILE_NAME"),
            "base": path.parent().map(|x| x.to_str()).flatten().unwrap_or("NO PARENT"),
            "sha1": format!("{}", entry.id()),
            }));
            }
            git2::TreeWalkResult::Ok
        })?;

        return Ok(json!(names));
    }
}

impl handlebars::HelperDef for FindFilesHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &handlebars::Helper,
        _: &handlebars::Handlebars,
        _: &handlebars::Context,
        _rc: &mut handlebars::RenderContext,
    ) -> Result<
        Option<handlebars::ScopedJson<'reg, 'rc>>,
        handlebars::RenderError,
    > {
        return Ok(Some(handlebars::ScopedJson::Derived(
            self.josh_helper(h.hash())
                .map_err(|_| handlebars::RenderError::new("josh"))?,
        )));
    }
}

struct FilterHelper {
    repo_path: std::path::PathBuf,
    headref: String,
}

impl FilterHelper {
    fn josh_helper(
        &self,
        hash: &std::collections::BTreeMap<&str, handlebars::PathAndJson>,
    ) -> JoshResult<serde_json::Value> {
        let filter_spec = if let Some(f) = hash.get("spec") {
            f.render()
        } else {
            return Err(josh_error("missing spec"));
        };
        let transaction = cache::Transaction::open(&self.repo_path)?;
        let original_commit = transaction
            .repo()
            .find_reference(&self.headref)?
            .peel_to_commit()?;
        let filterobj = filter::parse(&filter_spec)?;

        let filter_commit =
            history::walk2(filterobj, original_commit.id(), &transaction)?;

        return Ok(json!({ "sha1": format!("{}", filter_commit) }));
    }
}

impl handlebars::HelperDef for FilterHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &handlebars::Helper,
        _: &handlebars::Handlebars,
        _: &handlebars::Context,
        _rc: &mut handlebars::RenderContext,
    ) -> Result<
        Option<handlebars::ScopedJson<'reg, 'rc>>,
        handlebars::RenderError,
    > {
        return Ok(Some(handlebars::ScopedJson::Derived(
            self.josh_helper(h.hash())
                .map_err(|_| handlebars::RenderError::new("josh"))?,
        )));
    }
}

handlebars_helper!(concat_helper: |x: str, y: str| format!("{}{}", x, y) );

handlebars_helper!(toml_helper: |x: str| toml::de::from_str::<serde_json::Value>(x).unwrap_or(json!({})) );

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
    handlebars.register_template_string("template", template)?;
    handlebars.register_helper("concat", Box::new(concat_helper));
    handlebars.register_helper("toml", Box::new(toml_helper));
    handlebars.register_helper(
        "git-ls",
        Box::new(FindFilesHelper {
            repo_path: repo.path().to_owned(),
            headref: headref.to_string(),
        }),
    );
    handlebars.register_helper(
        "git-blob",
        Box::new(BlobHelper {
            repo_path: repo.path().to_owned(),
            headref: headref.to_string(),
        }),
    );
    handlebars.register_helper(
        "josh-filter",
        Box::new(FilterHelper {
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
        handlebars.render("template", &json!(params))?
    )));
}
