use std::sync::{Arc, RwLock};
struct BlobHelper {
    repo_path: std::path::PathBuf,
    headref: String,
}

impl BlobHelper {
    fn josh_helper(
        &self,
        params: &[handlebars::PathAndJson],
    ) -> super::JoshResult<serde_json::Value> {
        let path = if let [f, ..] = params {
            f.render()
        } else {
            return Err(super::josh_error("missing pattern"));
        };

        let repo = git2::Repository::init(&self.repo_path)?;
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
            self.josh_helper(h.params().as_slice())
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
        params: &[handlebars::PathAndJson],
        hash: &std::collections::BTreeMap<&str, handlebars::PathAndJson>,
    ) -> super::JoshResult<serde_json::Value> {
        let filename = if let Some(f) = hash.get("glob") {
            glob::Pattern::new(&f.render())?
        } else {
            return Err(super::josh_error("missing pattern"));
        };
        let repo = git2::Repository::init(&self.repo_path)?;
        let tree = repo.find_reference(&self.headref)?.peel_to_tree()?;

        let mut names = vec![];

        tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            let name = entry.name().unwrap_or("INVALID_FILENAME");
            let path = std::path::PathBuf::from(root).join(name);
            let path_str = path.to_string_lossy();

            if filename.matches(&path_str) {
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
            self.josh_helper(h.params().as_slice(), h.hash())
                .map_err(|_| handlebars::RenderError::new("josh"))?,
        )));
    }
}

struct FilterHelper {
    repo_path: std::path::PathBuf,
    headref: String,
    forward_maps:
        std::sync::Arc<std::sync::RwLock<super::filter_cache::FilterCache>>,
    backward_maps:
        std::sync::Arc<std::sync::RwLock<super::filter_cache::FilterCache>>,
}

impl FilterHelper {
    fn josh_helper(
        &self,
        params: &[handlebars::PathAndJson],
    ) -> super::JoshResult<serde_json::Value> {
        let filter_spec = if let [f, ..] = params {
            f.render()
        } else {
            return Err(super::josh_error("missing spec"));
        };
        let repo = git2::Repository::init(&self.repo_path)?;
        let original_commit =
            repo.find_reference(&self.headref)?.peel_to_commit()?;
        let filterobj = super::filters::parse(&filter_spec);
        let filter_commit = filterobj.apply_to_commit(
            &repo,
            &original_commit,
            &mut *self.forward_maps.write()?,
            &mut *self.backward_maps.write()?,
            &mut std::collections::HashMap::new(),
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
            self.josh_helper(h.params().as_slice())
                .map_err(|_| handlebars::RenderError::new("josh"))?,
        )));
    }
}

struct KvHelper {
    kv_store: Arc<RwLock<std::collections::HashMap<String, serde_json::Value>>>,
}

impl KvHelper {
    fn josh_helper(
        &self,
        params: &[handlebars::PathAndJson],
    ) -> super::JoshResult<serde_json::Value> {
        let key = if let [f, ..] = params {
            f.render()
        } else {
            return Err(super::josh_error("missing spec"));
        };

        if let Some(v) = self.kv_store.read()?.get(&key) {
            return Ok(v.to_owned());
        } else {
            return Ok(json!(""));
        }
    }
}

impl handlebars::HelperDef for KvHelper {
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
            self.josh_helper(h.params().as_slice())
                .map_err(|_| handlebars::RenderError::new("josh"))?,
        )));
    }
}

handlebars_helper!(concat_helper: |x: str, y: str| format!("{}{}", x, y) );

handlebars_helper!(toml_helper: |x: str| toml::de::from_str::<serde_json::Value>(x).unwrap_or(json!({})) );

pub fn render(
    repo: &git2::Repository,
    headref: &str,
    query: &str,
    kv_store: Arc<RwLock<std::collections::HashMap<String, serde_json::Value>>>,
    backward_maps: std::sync::Arc<
        std::sync::RwLock<super::filter_cache::FilterCache>,
    >,
    forward_maps: std::sync::Arc<
        std::sync::RwLock<super::filter_cache::FilterCache>,
    >,
) -> super::JoshResult<String> {
    let mut split = query.splitn(2, "=");
    let cmd = split
        .next()
        .ok_or(super::josh_error(&format!("invalid query {:?}", query)))?;
    let path = split
        .next()
        .ok_or(super::josh_error(&format!("invalid query {:?}", query)))?;
    let tree = repo.find_reference(&headref)?.peel_to_tree()?;

    let obj = tree
        .get_path(&std::path::PathBuf::from(path))?
        .to_object(&repo)?;

    let template = if let Ok(blob) = obj.peel_to_blob() {
        let template = std::str::from_utf8(blob.content())?;
        if cmd == "get" {
            return Ok(template.to_string());
        }
        if cmd == "render" {
            template.to_string()
        } else {
            return Err(super::josh_error("no such cmd"));
        }
    } else {
        return Ok("".to_string());
    };

    let mut handlebars = handlebars::Handlebars::new();
    handlebars.register_template_string("template", template)?;
    handlebars.register_helper("concat", Box::new(concat_helper));
    handlebars.register_helper("toml", Box::new(toml_helper));
    handlebars.register_helper(
        "git-find",
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
            forward_maps: forward_maps.clone(),
            backward_maps: backward_maps.clone(),
        }),
    );

    handlebars.register_helper(
        "db-lookup",
        Box::new(KvHelper { kv_store: kv_store }),
    );
    return Ok(format!("{}", handlebars.render("template", &json!({}))?));
}
