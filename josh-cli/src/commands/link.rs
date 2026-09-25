use anyhow::{Context, anyhow};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, clap::Parser)]
pub struct LinkArgs {
    /// Link subcommand
    #[command(subcommand)]
    pub command: LinkCommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum LinkCommand {
    /// Add a link to a view of a remote repository
    Add(LinkAddArgs),
    /// Remove a link
    Rm(LinkRmArgs),
    /// List links
    List,
}

#[derive(Debug, clap::Parser)]
pub struct LinkAddArgs {
    /// Remote repository URL
    #[arg()]
    pub url: String,

    /// Filter defining the view (e.g. :/subfolder)
    #[arg()]
    pub filter: String,

    /// Link id (defaults to the repository name derived from the URL)
    #[arg(long = "id")]
    pub id: Option<String>,

    /// Forge hosting the linked remote (defaults to guessing from the URL)
    #[arg(long = "forge", conflicts_with = "no_forge")]
    pub forge: Option<josh_view::Forge>,

    /// Disable forge integration for this link (refs-only publishing)
    #[arg(long = "no-forge")]
    pub no_forge: bool,
}

#[derive(Debug, clap::Parser)]
pub struct LinkRmArgs {
    /// Link id
    #[arg()]
    pub id: String,
}

pub fn handle_link(
    args: &LinkArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    josh_core::filter::check_experimental_features_enabled("josh link")?;
    match &args.command {
        LinkCommand::Add(add_args) => handle_link_add(add_args, transaction),
        LinkCommand::Rm(rm_args) => handle_link_rm(rm_args, transaction),
        LinkCommand::List => handle_link_list(transaction),
    }
}

fn handle_link_add(
    args: &LinkAddArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    let id = match &args.id {
        Some(id) => id.clone(),
        None => josh_view::derive_id_from_url(&args.url).ok_or_else(|| {
            anyhow!(
                "Could not derive a link id from URL '{}'; pass --id",
                args.url
            )
        })?,
    };
    josh_view::validate_link_id(&id)?;

    if transaction
        .resolve_ref(&josh_view::link_ref_name(&id))?
        .is_some()
    {
        return Err(anyhow!("Link '{id}' already exists"));
    }

    let filter = josh_core::filter::parse(&args.filter)
        .with_context(|| format!("Failed to parse filter '{}'", args.filter))?;

    let short_ref =
        crate::remote_ops::get_head_branch(&args.url, &transaction.path().to_path_buf(), &id)
            .with_context(|| format!("Failed to determine default branch of '{}'", args.url))?;
    let tracked_ref = format!("refs/heads/{short_ref}");

    let forge = if args.no_forge {
        None
    } else {
        args.forge.or_else(|| crate::forge::guess_forge(&args.url))
    };

    let view = josh_view::View {
        filter,
        link: Some(josh_view::Link {
            url: args.url.clone(),
            tracked_ref: tracked_ref.clone(),
            forge,
        }),
    };

    josh_view::write_view(transaction, &id, &view, &format!("josh link add {id}"))?;

    println!(
        "Added link '{id}': {} {} {}",
        args.url,
        tracked_ref,
        josh_core::filter::spec(filter)
    );
    Ok(())
}

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

fn resolve_submodule_url(superproject_url: &str, url: &str) -> String {
    if !url.starts_with("./") && !url.starts_with("../") {
        return url.to_string();
    }

    format!("{}/{}", superproject_url.trim_end_matches('/'), url)
}

fn submodule_tracked_ref(
    transaction: &josh_core::cache::Transaction,
    name: &str,
    branch: Option<&str>,
    url: &str,
) -> anyhow::Result<String> {
    let branch = match branch {
        Some(".") => transaction
            .head()
            .context("Failed to get HEAD")?
            .short_branch()
            .with_context(|| format!("Submodule '{name}' inherits the branch of a detached HEAD"))?
            .to_string(),
        Some(branch) => branch.to_string(),
        None => crate::remote_ops::get_head_branch(url, transaction.path(), name)
            .with_context(|| format!("Failed to determine default branch of '{url}'"))?,
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

pub fn add_submodule_links(
    transaction: &josh_core::cache::Transaction,
    superproject_url: &str,
) -> anyhow::Result<josh_core::filter::Filter> {
    let head = transaction.head().context("Failed to get HEAD")?;
    let commit = josh_core::objects::CommitData::read(transaction.odb(), head.commit)?;
    let tree = commit.tree_id()?;
    let modules = configured_submodules(transaction, tree)?;
    if modules.is_empty() {
        return Err(anyhow!("HEAD has no configured submodules"));
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

        let url = resolve_submodule_url(superproject_url, &module.url);
        let tracked_ref =
            submodule_tracked_ref(transaction, &module.name, module.branch.as_deref(), &url)?;
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

        let object_ref = format!("refs/josh/submodules/{}", module.name);
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

fn handle_link_rm(
    args: &LinkRmArgs,
    transaction: &josh_core::cache::Transaction,
) -> anyhow::Result<()> {
    josh_view::delete_view(transaction, &args.id)?;
    println!("Removed link '{}'", args.id);
    Ok(())
}

fn handle_link_list(transaction: &josh_core::cache::Transaction) -> anyhow::Result<()> {
    for (id, view) in josh_view::list_views(transaction)? {
        match &view.link {
            Some(link) => println!(
                "{}\t{}\t{}\t{}\t{}",
                id,
                link.url,
                link.tracked_ref,
                link.forge.map(|f| f.to_string()).unwrap_or_default(),
                josh_core::filter::spec(view.filter)
            ),
            None => println!("{}\t{}", id, josh_core::filter::spec(view.filter)),
        }
    }
    Ok(())
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
}
