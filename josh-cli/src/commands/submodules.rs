use anyhow::{Context, anyhow};
use gix::bstr::ByteSlice;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
#[derive(Default)]
struct SubmoduleConfig {
    path: Option<PathBuf>,
    url: Option<String>,
    branch: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct ConfiguredSubmodule {
    name: String,
    path: PathBuf,
    url: String,
    branch: Option<String>,
}

fn parse_configured_submodules(contents: &[u8]) -> anyhow::Result<Vec<ConfiguredSubmodule>> {
    let contents = std::str::from_utf8(contents).context(".gitmodules is not valid UTF-8")?;
    let config = gix::config::File::try_from(contents).context("Failed to parse .gitmodules")?;
    let mut configs: BTreeMap<String, SubmoduleConfig> = BTreeMap::new();

    let Some(sections) = config.sections_by_name("submodule") else {
        return Ok(Vec::new());
    };
    for section in sections {
        let name = section
            .header()
            .subsection_name()
            .context("Submodule section has no name")?;
        let name = std::str::from_utf8(name.as_ref())
            .context("Submodule name is not valid UTF-8")?
            .to_string();
        let body = section.body();
        let config = configs.entry(name).or_default();
        if let Some(value) = body.value("path") {
            config.path = Some(PathBuf::from(
                std::str::from_utf8(value.as_ref()).context("Submodule path is not valid UTF-8")?,
            ));
        }
        if let Some(value) = body.value("url") {
            config.url = Some(
                std::str::from_utf8(value.as_ref())
                    .context("Submodule URL is not valid UTF-8")?
                    .to_string(),
            );
        }
        if let Some(value) = body.value("branch") {
            config.branch = Some(
                std::str::from_utf8(value.as_ref())
                    .context("Submodule branch is not valid UTF-8")?
                    .to_string(),
            );
        }
    }

    configs
        .into_iter()
        .map(|(name, config)| {
            let path = config
                .path
                .with_context(|| format!("Submodule '{name}' has no path"))?;
            let url = config
                .url
                .with_context(|| format!("Submodule '{name}' has no URL"))?;
            Ok(ConfiguredSubmodule {
                name,
                path,
                url,
                branch: config.branch,
            })
        })
        .collect()
}

fn configured_submodules(
    transaction: &josh_core::cache::Transaction,
    tree: gix_hash::ObjectId,
) -> anyhow::Result<Vec<ConfiguredSubmodule>> {
    let odb = transaction.odb();
    let entry =
        josh_core::filter::tree::get_path_entry(transaction, odb, tree, Path::new(".gitmodules"))?
            .context("HEAD has no .gitmodules")?;
    if !entry.mode.is_blob() {
        return Err(anyhow!(".gitmodules in HEAD is not a file"));
    }

    let contents = josh_core::filter::tree::blob_bytes(odb, entry.oid)
        .context(".gitmodules in HEAD is not a blob")?;
    parse_configured_submodules(&contents)
}

fn normalize_relative_git_path(base: &str, relative: &str) -> String {
    let absolute = base.starts_with('/');
    let joined = format!("{}/{}", base.trim_end_matches('/'), relative);
    let mut components = Vec::new();
    for component in joined.split('/') {
        match component {
            "" | "." => {}
            ".." if components
                .last()
                .is_some_and(|component| *component != "..") =>
            {
                components.pop();
            }
            ".." if !absolute => components.push(component),
            ".." => {}
            component => components.push(component),
        }
    }
    let path = components.join("/");
    if absolute { format!("/{path}") } else { path }
}

fn serialize_git_url(url: &gix::Url) -> anyhow::Result<String> {
    let serialized = url.to_bstring();
    Ok(serialized
        .to_str()
        .context("Git URL is not valid UTF-8")?
        .to_owned())
}

fn resolve_submodule_url(superproject_url: &str, url: &str) -> anyhow::Result<gix::Url> {
    if !url.starts_with("./") && !url.starts_with("../") {
        return gix::Url::from_bytes(url.as_bytes().as_bstr())
            .with_context(|| format!("Invalid submodule URL '{url}'"));
    }

    let mut resolved = gix::Url::from_bytes(superproject_url.as_bytes().as_bstr())
        .with_context(|| format!("Invalid superproject URL '{superproject_url}'"))?;
    let base = resolved
        .path
        .to_str()
        .context("Superproject URL path is not valid UTF-8")?;
    resolved.path = normalize_relative_git_path(base, url).into();
    Ok(resolved)
}

#[derive(Debug, Clone)]
pub struct SubmoduleRemote {
    pub name: String,
    pub url: gix::Url,
}

impl SubmoduleRemote {
    pub fn serialized_url(&self) -> anyhow::Result<String> {
        serialize_git_url(&self.url)
    }
}

pub fn submodule_remotes(
    transaction: &josh_core::cache::Transaction,
    commit: gix_hash::ObjectId,
    superproject_url: &str,
) -> anyhow::Result<Vec<SubmoduleRemote>> {
    let commit = josh_core::objects::CommitData::read(transaction.odb(), commit)?;
    let modules = configured_submodules(transaction, commit.tree_id()?)?;
    modules
        .into_iter()
        .map(|module| {
            josh_view::validate_link_id(&module.name)
                .with_context(|| format!("Invalid submodule name '{}'", module.name))?;
            Ok(SubmoduleRemote {
                name: module.name,
                url: resolve_submodule_url(superproject_url, &module.url)?,
            })
        })
        .collect()
}

fn submodule_tracked_ref(
    transaction: &josh_core::cache::Transaction,
    name: &str,
    branch: Option<&str>,
    inherited_branch: Option<&str>,
    url: &gix::Url,
) -> anyhow::Result<String> {
    let branch = match branch {
        Some(".") => inherited_branch
            .with_context(|| format!("Submodule '{name}' inherits an unknown branch"))?
            .to_string(),
        Some(branch) => branch.to_string(),
        None => {
            let url = serialize_git_url(url)?;
            crate::remote_ops::get_head_branch(&url, transaction.path(), name)
                .with_context(|| format!("Failed to determine default branch of '{url}'"))?
        }
    };
    Ok(format!("refs/heads/{branch}"))
}

fn combined_submodule_filter(
    dereferences: Vec<josh_core::filter::Filter>,
) -> josh_core::filter::Filter {
    let all_dereferences = match dereferences.as_slice() {
        [dereference] => *dereference,
        _ => josh_core::filter::to_filter(josh_core::filter::Op::Compose(dereferences.clone())),
    };
    josh_core::filter::to_filter(josh_core::filter::Op::Compose(
        std::iter::once(josh_core::filter::to_filter(
            josh_core::filter::Op::Exclude(all_dereferences),
        ))
        .chain(dereferences)
        .collect(),
    ))
    .with_meta("history", "embed")
}

pub fn add_submodule_links_at_commit(
    transaction: &josh_core::cache::Transaction,
    superproject_url: &str,
    commit: gix_hash::ObjectId,
    inherited_branch: Option<&str>,
) -> anyhow::Result<josh_core::filter::Filter> {
    let commit = josh_core::objects::CommitData::read(transaction.odb(), commit)?;
    let tree = commit.tree_id()?;
    let modules = configured_submodules(transaction, tree)?;
    if modules.is_empty() {
        return Err(anyhow!("Commit has no configured submodules"));
    }

    struct Prepared {
        name: String,
        oid: gix_hash::ObjectId,
        view: josh_view::View,
        deref_filter: josh_core::filter::Filter,
    }

    let mut prepared = Vec::with_capacity(modules.len());
    for module in modules {
        josh_view::validate_link_id(&module.name)
            .with_context(|| format!("Invalid submodule name '{}'", module.name))?;
        let entry = josh_core::filter::tree::get_path_entry(
            transaction,
            transaction.odb(),
            tree,
            &module.path,
        )?
        .with_context(|| {
            format!(
                "Submodule '{}' path '{}' does not exist in HEAD",
                module.name,
                module.path.display()
            )
        })?;
        if !entry.mode.is_commit() {
            return Err(anyhow!(
                "Submodule '{}' path '{}' is not a gitlink in HEAD",
                module.name,
                module.path.display()
            ));
        }

        let url = resolve_submodule_url(superproject_url, &module.url)?;
        let tracked_ref = submodule_tracked_ref(
            transaction,
            &module.name,
            module.branch.as_deref(),
            inherited_branch,
            &url,
        )?;
        let url = serialize_git_url(&url)?;
        let deref_filter =
            josh_core::filter::to_filter(josh_core::filter::Op::ObjectDeref(module.path.clone()));
        let filter = josh_core::filter::to_filter(josh_core::filter::Op::Chain(vec![
            josh_core::filter::to_filter(josh_core::filter::Op::Subdir(module.path)),
            josh_core::filter::to_filter(josh_core::filter::Op::Export),
        ]));
        let view = josh_view::View {
            filter,
            link: Some(josh_view::Link {
                forge: crate::forge::guess_forge(&url),
                tracked_ref,
                url,
            }),
        };

        if let Some(existing) = josh_view::read_view(transaction, &module.name)?
            && existing != view
        {
            return Err(anyhow!(
                "Link '{}' already exists with different configuration",
                module.name
            ));
        }

        prepared.push(Prepared {
            name: module.name,
            oid: entry.oid,
            view,
            deref_filter,
        });
    }

    let combined_filter =
        combined_submodule_filter(prepared.iter().map(|module| module.deref_filter).collect());

    for module in prepared {
        if !matches!(
            transaction.odb().try_kind(module.oid),
            Ok(Some(gix::object::Kind::Commit))
        ) {
            let oid = module.oid.to_string();
            let url = &module.view.link.as_ref().expect("link prepared above").url;
            transaction
                .spawn_git(
                    &[
                        "fetch",
                        "--no-tags",
                        "--no-write-fetch-head",
                        "--no-recurse-submodules",
                        url,
                        &oid,
                    ],
                    &[],
                )
                .with_context(|| {
                    format!(
                        "Failed to fetch submodule '{}' commit {} from {}",
                        module.name, module.oid, url
                    )
                })?;
        }

        let object_ref = josh_changes::JoshRef::Submodule {
            name: module.name.clone(),
        }
        .ref_name();
        transaction.update_ref(
            &object_ref,
            josh_core::cache::Expected::Any,
            module.oid,
            "josh remote add --submodules",
        )?;

        if josh_view::read_view(transaction, &module.name)?.is_none() {
            josh_view::write_view(
                transaction,
                &module.name,
                &module.view,
                &format!("josh remote add --submodules {}", module.name),
            )?;
        }

        let link = module.view.link.as_ref().expect("link prepared above");
        println!(
            "Added submodule link '{}': {} {} {}",
            module.name,
            link.url,
            link.tracked_ref,
            josh_core::filter::spec(module.view.filter)
        );
    }

    Ok(combined_filter)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_submodules_with_git_config_semantics() {
        let modules = parse_configured_submodules(
            br#"
                [submodule "zeta"]
                    path = "modules/zeta dir"
                    url = "../zeta repo.git"
                    branch = release
                [submodule "alpha"]
                    path = modules/alpha
                    url = https://example.com/alpha.git
                [submodule "zeta"]
                    branch = next
            "#,
        )
        .unwrap();

        assert_eq!(
            modules,
            vec![
                ConfiguredSubmodule {
                    name: "alpha".to_string(),
                    path: PathBuf::from("modules/alpha"),
                    url: "https://example.com/alpha.git".to_string(),
                    branch: None,
                },
                ConfiguredSubmodule {
                    name: "zeta".to_string(),
                    path: PathBuf::from("modules/zeta dir"),
                    url: "../zeta repo.git".to_string(),
                    branch: Some("next".to_string()),
                },
            ]
        );
    }

    #[test]
    fn derives_combined_filter_for_all_submodules() {
        let single = vec![josh_core::filter::to_filter(
            josh_core::filter::Op::ObjectDeref(PathBuf::from("libs")),
        )];
        assert_eq!(
            josh_core::filter::spec(combined_submodule_filter(single)),
            r#":~(history="embed")[:[:exclude[:#libs],:#libs]]"#
        );

        let dereferences = ["libs", "vendor"]
            .into_iter()
            .map(|path| {
                josh_core::filter::to_filter(josh_core::filter::Op::ObjectDeref(PathBuf::from(
                    path,
                )))
            })
            .collect();
        assert_eq!(
            josh_core::filter::spec(combined_submodule_filter(dereferences)),
            r#":~(history="embed")[:[:exclude[:[:#libs,:#vendor]],:#libs,:#vendor]]"#
        );
    }

    #[test]
    fn resolves_relative_git_urls_without_losing_transport_syntax() {
        let https = resolve_submodule_url("https://example.com/org/super.git", "../dependency.git")
            .unwrap();
        assert_eq!(
            serialize_git_url(&https).unwrap(),
            "https://example.com/org/dependency.git"
        );

        let scp =
            resolve_submodule_url("git@example.com:org/super.git", "../dependency.git").unwrap();
        assert_eq!(
            serialize_git_url(&scp).unwrap(),
            "git@example.com:org/dependency.git"
        );
    }

    #[test]
    fn preserves_absolute_scp_like_git_urls() {
        let url = resolve_submodule_url(
            "https://example.com/org/super.git",
            "git@example.com:other/dependency.git",
        )
        .unwrap();
        assert_eq!(
            serialize_git_url(&url).unwrap(),
            "git@example.com:other/dependency.git"
        );
    }
}
