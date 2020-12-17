struct BlobHelper {
    repo: std::sync::Arc<std::sync::Mutex<git2::Repository>>,
    headref: String,
}

impl BlobHelper {
    fn josh_helper(
        &self,
        hash: &std::collections::BTreeMap<&str, handlebars::PathAndJson>,
    ) -> super::JoshResult<serde_json::Value> {
        let path = if let Some(f) = hash.get("path") {
            f.render()
        } else {
            return Err(super::josh_error("missing pattern"));
        };

        let repo = self.repo.lock()?;
        let tree = repo.find_reference(&self.headref)?.peel_to_tree()?;

        let blob = tree
            .get_path(&std::path::PathBuf::from(path))?
            .to_object(&repo)?
            .peel_to_blob()?;
        return Ok(json!(String::from_utf8(blob.content().to_vec())?));
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
    repo: std::sync::Arc<std::sync::Mutex<git2::Repository>>,
    headref: String,
}

impl FindFilesHelper {
    fn josh_helper(
        &self,
        hash: &std::collections::BTreeMap<&str, handlebars::PathAndJson>,
    ) -> super::JoshResult<serde_json::Value> {
        let filename = if let Some(f) = hash.get("glob") {
            glob::Pattern::new(&f.render())?
        } else {
            return Err(super::josh_error("missing pattern"));
        };
        let repo = self.repo.lock()?;
        let tree = repo.find_reference(&self.headref)?.peel_to_tree()?;

        let mut names = vec![];

        tree.walk(

        git2::TreeWalkMode::PreOrder, |root, entry| {
            let name = entry.name().unwrap_or("INVALID_FILENAME");
            let path = std::path::PathBuf::from(root).join(name);
            let path_str = path.to_string_lossy();

            if filename.matches_path_with(&path, glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true
            }){
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
    repo: std::sync::Arc<std::sync::Mutex<git2::Repository>>,
    headref: String,
}

impl FilterHelper {
    fn josh_helper(
        &self,
        hash: &std::collections::BTreeMap<&str, handlebars::PathAndJson>,
    ) -> super::JoshResult<serde_json::Value> {
        let filter_spec = if let Some(f) = hash.get("spec") {
            f.render()
        } else {
            return Err(super::josh_error("missing spec"));
        };
        let repo = self.repo.lock()?;
        let original_commit =
            repo.find_reference(&self.headref)?.peel_to_commit()?;
        let filterobj = super::filters::parse(&filter_spec)?;

        let filter_commit = super::filters::apply_filter_cached(
            &repo,
            &*filterobj,
            original_commit.id(),
        )?;

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
    repo: git2::Repository,
    headref: &str,
    query_and_params: &str,
) -> super::JoshResult<Option<String>> {
    let mut parameters = query_and_params.split("&");
    let query = parameters.next().ok_or(super::josh_error(&format!(
        "invalid query {:?}",
        query_and_params
    )))?;
    let mut split = query.splitn(2, "=");
    let cmd = split.next().ok_or(super::josh_error(&format!(
        "invalid query {:?}",
        query_and_params
    )))?;
    let path = split.next().ok_or(super::josh_error(&format!(
        "invalid query {:?}",
        query_and_params
    )))?;
    let tree = repo.find_reference(&headref)?.peel_to_tree()?;

    let obj = super::ok_or!(
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
            return Err(super::josh_error("no such cmd"));
        }
    } else {
        return Ok(Some("".to_string()));
    };

    std::mem::drop(obj);
    std::mem::drop(tree);

    let repo = std::sync::Arc::new(std::sync::Mutex::new(repo));

    let mut handlebars = handlebars::Handlebars::new();
    handlebars.register_template_string("template", template)?;
    handlebars.register_helper("concat", Box::new(concat_helper));
    handlebars.register_helper("toml", Box::new(toml_helper));
    handlebars.register_helper(
        "git-find",
        Box::new(FindFilesHelper {
            repo: repo.clone(),
            headref: headref.to_string(),
        }),
    );
    handlebars.register_helper(
        "git-blob",
        Box::new(BlobHelper {
            repo: repo.clone(),
            headref: headref.to_string(),
        }),
    );
    handlebars.register_helper(
        "josh-filter",
        Box::new(FilterHelper {
            repo: repo.clone(),
            headref: headref.to_string(),
        }),
    );

    let mut params = std::collections::BTreeMap::new();
    for p in parameters {
        let mut split = p.splitn(2, "=");
        let name = split.next().ok_or(super::josh_error(&format!(
            "invalid query {:?}",
            query_and_params
        )))?;
        let value = split.next().ok_or(super::josh_error(&format!(
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
