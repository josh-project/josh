#![warn(unused_extern_crates)]

use anyhow::{Context, anyhow};
use std::fs::read_to_string;
use std::str::FromStr;

fn resolve_input_ref(
    transaction: &josh_core::cache::Transaction,
    input_ref: &str,
) -> anyhow::Result<(String, gix_hash::ObjectId)> {
    let oid = josh_core::git::resolve_snapshot_input(transaction, input_ref)?;
    let ref_string = if input_ref == "+" || input_ref == "." {
        oid.to_string()
    } else if gix_hash::ObjectId::from_str(input_ref).is_ok() {
        input_ref.to_string()
    } else if let Some(name) = transaction.expand_ref_name(input_ref)? {
        name
    } else {
        oid.to_string()
    };
    Ok((ref_string, oid))
}

fn make_app() -> clap::Command {
    let app = clap::Command::new("josh-filter");

    let app = { app.arg(clap::Arg::new("search").long("search")) };

    app
        .arg(
            clap::Arg::new("filter")
                .help("Filter to apply")
                .default_value(":/"),
        )
        .arg(
            clap::Arg::new("input")
                .help("Ref or SHA to apply filter to, '.' for the working tree, or '+' for the index (staged changes)")
                .default_value("HEAD"),
        )
        .arg(
            clap::Arg::new("file")
                .long("file")
                .help("Read filter spec from file"),
        )
        .arg(
            clap::Arg::new("update")
                .long("update")
                .help("reference to update with the result")
                .default_value("FILTERED_HEAD"),
        )
        .arg(
            clap::Arg::new("squash-pattern")
                .help("Produce a history that contains only commits pointed to by references matching the given pattern")
                .long("squash-pattern")
        )
        .arg(
            clap::Arg::new("squash-file")
                .help("Produce a history that contains only commits listed in the given file")
                .long("squash-file")
        )
        .arg(
            clap::Arg::new("single")
                .action(clap::ArgAction::SetTrue)
                .help("Produce a history that contains only one single commit")
                .long("single"),
        )
        .arg(
            clap::Arg::new("discover")
                .action(clap::ArgAction::SetTrue)
                .help("Populate the cache with probable filters")
                .short('d'),
        )
        .arg(
            clap::Arg::new("print-filter")
                .action(clap::ArgAction::SetTrue)
                .help("Pretty print the filter and exit")
                .short('p'),
        )
        .arg(
            clap::Arg::new("filter-id")
                .action(clap::ArgAction::SetTrue)
                .help("Print the filter id and exit")
                .short('i'),
        )
        .arg(
            clap::Arg::new("cache-stats")
                .action(clap::ArgAction::SetTrue)
                .help("Show stats about cache content")
                .short('s'),
        )
        .arg(
            clap::Arg::new("no-cache")
                .action(clap::ArgAction::SetTrue)
                .help("Don't load cache")
                .short('n'),
        )
        .arg(
            clap::Arg::new("distributed-cache")
                .action(clap::ArgAction::SetTrue)
                .help("Enables distributed cache")
                .long("distributed-cache"),
        )
        .arg(clap::Arg::new("query").long("query").short('q'))
        .arg(
            clap::Arg::new("graphql")
                .long("graphql")
                .short('g'),
        )
        .arg(
            clap::Arg::new("reverse").action(clap::ArgAction::SetTrue).long("reverse").help(
                "reverse-apply the filter to the output reference to update the input reference",
            ),
        )
        .arg(
            clap::Arg::new("check-roundtrip").action(clap::ArgAction::SetTrue).long("check-roundtrip").help(
                "If --reverse is also set, check if applying the filter to the result of the reverse filter gives back the input",
            ),
        )
        .arg(
            clap::Arg::new("force").action(clap::ArgAction::SetTrue).long("force").help(
                "Allow --reverse to move the input reference to a non-fast-forward result, discarding commits made on it since the filtered reference was created",
            ),
        )
        .arg(
            clap::Arg::new("whitelist")
                .long("whitelist")
                .short('w'),
        )
        .arg(
            clap::Arg::new("blacklist")
                .long("blacklist")
                .short('b'),
        )
        .arg(clap::Arg::new("users").long("users"))
        .arg(clap::Arg::new("groups").long("groups"))
        .arg(clap::Arg::new("user").long("user").short('u'))
        .arg(clap::Arg::new("repo").long("repo").short('r'))
        .arg(clap::Arg::new("version").action(clap::ArgAction::SetTrue).long("version").short('v'))
}

struct GitNotesFilterHook {
    notes: josh_core::git::NoteReader,
}

impl josh_core::cache::FilterHook for GitNotesFilterHook {
    fn filter_for_commit(
        &self,
        commit_oid: gix_hash::ObjectId,
        arg: &str,
    ) -> anyhow::Result<josh_core::filter::Filter> {
        let notes_ref = if arg.starts_with("refs/") {
            arg.to_string()
        } else {
            format!("refs/notes/{}", arg)
        };
        let msg = self.notes.message(&notes_ref, commit_oid)?;
        josh_core::filter::parse(&msg)
    }
}

fn run_filter(args: Vec<String>) -> anyhow::Result<i32> {
    let args = make_app().get_matches_from(args);

    if args.get_flag("version") {
        println!("Version: {}", josh_core::VERSION);
        return Ok(0);
    }
    let specstr = args.get_one::<String>("filter").unwrap();
    let is_from_file = args.get_one::<String>("file").is_some();
    let specstr = args
        .get_one::<String>("file")
        .and_then(|f| read_to_string(f).ok())
        .unwrap_or(specstr.to_string());

    let repo_path = josh_core::git::discover_repository_paths()?.git_dir;

    let cache = std::sync::Arc::new({
        let mut cache = josh_core::cache::CacheStack::new();

        if !args.get_flag("no-cache") {
            cache = cache.with_backend(josh_core::cache::SledCacheBackend::new(&repo_path));
        }

        if args.get_flag("distributed-cache") {
            cache.with_backend(josh_core::cache::DistributedCacheBackend::new(&repo_path)?)
        } else {
            cache
        }
    });

    let mut transaction = josh_core::cache::TransactionContext::from_env(cache.clone())?
        .with_mem_odb_limit(josh_cli::MAX_MEM_PACK_SIZE)
        .open()?;

    let hook = GitNotesFilterHook {
        notes: josh_core::git::NoteReader::open(transaction.path())?,
    };
    transaction = transaction.with_filter_hook(std::sync::Arc::new(hook));

    // If the filter spec doesn't contain a colon and it's not from a file,
    // treat it as a SHA and read from tree
    let mut filterobj = if specstr.contains(':') || is_from_file {
        josh_core::filter::parse(&specstr)?
    } else {
        // Try to parse as SHA and read filter from tree
        let tree_oid = gix_hash::ObjectId::from_str(specstr.trim())
            .with_context(|| format!("Invalid filter spec or SHA: {}", specstr))?;
        josh_core::filter::from_tree(&transaction, tree_oid)?
    };

    let input_ref = args.get_one::<String>("input").unwrap();

    let mut refs = vec![];
    let mut ids = vec![];

    let (input_ref, oid) = resolve_input_ref(&transaction, input_ref)?;
    refs.push((input_ref.clone(), oid));

    if args.get_flag("single") {
        filterobj = josh_core::filter::Filter::new()
            .squash(None)
            .chain(filterobj);
    }

    if let Some(pattern) = args.get_one::<String>("squash-pattern") {
        // Iterate over the pattern's literal prefix before its first metacharacter.
        // `glob`'s default `MatchOptions` let `*` and `?` match across `/`.
        let matcher = glob::Pattern::new(pattern)?;
        let literal_len = pattern.find(['*', '?', '[', '\\']).unwrap_or(pattern.len());
        transaction.for_each_ref_prefixed(&pattern[..literal_len], |name, oid| {
            if !matcher.matches(name) {
                return Ok(());
            }
            let target = josh_core::objects::peel_to_commit(transaction.odb(), oid)?;
            ids.push((target, josh_core::filter::Filter::new().message(name)));
            refs.push((name.to_string(), target));
            Ok(())
        })?;
        filterobj = josh_core::filter::Filter::new()
            .squash(Some(&ids))
            .chain(filterobj);
    };

    if let Some(filename) = args.get_one::<String>("squash-file") {
        let reflist = read_to_string(filename)?;

        for line in reflist.lines() {
            let split = line.split(' ').collect::<Vec<_>>();
            if let [sha, name] = split.as_slice() {
                let target = gix_hash::ObjectId::from_str(sha)?;
                let target = josh_core::objects::peel_to_commit(transaction.odb(), target)?;
                ids.push((target, josh_core::filter::Filter::new().message(name)));
                refs.push((name.to_string(), target));
            } else if !split.is_empty() {
                eprintln!("Warning: malformed line: {:?}", line);
            }
        }
        filterobj = josh_core::filter::Filter::new()
            .squash(Some(&ids))
            .chain(filterobj);
    };

    if args.get_flag("print-filter") {
        let filterobj = if args.get_flag("reverse") {
            josh_core::filter::invert(filterobj)?
        } else {
            filterobj
        };
        println!(
            "{}",
            josh_core::filter::pretty(filterobj, if args.contains_id("file") { 0 } else { 4 })
        );
        return Ok(0);
    }

    if args.get_flag("filter-id") {
        let filterobj = if args.get_flag("reverse") {
            josh_core::filter::invert(filterobj)?
        } else {
            filterobj
        };
        println!("{}", josh_core::filter::as_tree(&transaction, filterobj)?);
        return Ok(0);
    }

    let finish = defer::defer(|| {
        if args.get_flag("cache-stats") {
            josh_core::cache::sled_print_stats().expect("failed to collect cache stats");
        }
    });

    if args.get_flag("discover") {
        let odb = transaction.odb();
        let head = transaction
            .rev_parse(&input_ref)?
            .ok_or_else(|| anyhow!("no such revision: {}", input_ref))?;
        let tree = josh_core::objects::CommitData::read(odb, head)?.tree_id()?;
        let hs = josh_core::housekeeping::find_all_workspaces_and_subdirectories(odb, tree)?;
        for i in hs {
            let (mut updated_refs, _) = josh_core::filter_refs(
                &transaction,
                josh_core::filter::parse(&i)?,
                &[(input_ref.to_string(), head)],
            );
            updated_refs[0].0 = "refs/JOSH_TMP".to_string();
            josh_core::update_refs(&transaction, updated_refs);
        }
    }

    let update_target = args.get_one::<String>("update").unwrap();

    let target = update_target;

    let reverse = args.get_flag("reverse");

    let old_oid = transaction
        .resolve_ref(target)?
        .unwrap_or(gix_hash::ObjectId::null(gix_hash::Kind::Sha1));

    let (mut updated_refs, errors) = josh_core::filter_refs(&transaction, filterobj, &refs);

    if let Some(error) = errors.into_iter().next() {
        return Err(error.1);
    }
    for item in &mut updated_refs {
        if item.0 == input_ref {
            if reverse {
                item.0 = "refs/JOSH_TMP".to_string();
            } else {
                item.0 = target.to_string();
            }
        } else {
            item.0 = item.0.replacen("refs/heads/", "refs/heads/filtered/", 1);
            item.0 = item.0.replacen("refs/tags/", "refs/tags/filtered/", 1);
        }
    }
    josh_core::update_refs(&transaction, updated_refs.clone());

    if let Some(searchstring) = args.get_one::<String>("search") {
        let commit = transaction
            .rev_parse(&input_ref)?
            .ok_or_else(|| anyhow!("no such revision: {}", input_ref))?;

        let odb = transaction.odb();
        let tree = josh_core::objects::CommitData::read(
            odb,
            josh_core::filter_commit(&transaction, filterobj, commit)?,
        )?
        .tree_id()?;

        // The trigram index is experimental; without it every file is a candidate and
        // search_matches does all the filtering, so results are identical, just slower.
        let candidates = if josh_core::filter::experimental_features_enabled() {
            let ifilterobj = filterobj.chain(josh_core::filter::parse(":SQUASH:INDEX")?);
            let index_commit = josh_core::filter_commit(&transaction, ifilterobj, commit)?;
            let index_tree = josh_core::objects::CommitData::read(odb, index_commit)?.tree_id()?;
            josh_search::search_candidates(odb, index_tree, tree, searchstring)?
        } else {
            let mut scan = vec![];
            josh_core::objects::walk_tree_preorder(odb, tree, &mut |parent, entry| {
                if !entry.mode.is_tree()
                    && !entry.mode.is_commit()
                    && let Ok(name) = std::str::from_utf8(entry.filename)
                {
                    let separator = if parent.is_empty() { "" } else { "/" };
                    scan.push(format!("{}{}{}", parent, separator, name));
                }
                Ok(())
            })?;
            scan
        };
        let matches = josh_search::search_matches(odb, tree, searchstring, &candidates)?;

        for r in matches {
            for l in r.1 {
                println!("{}:{}: {}", r.0, l.0, l.1);
            }
        }
    }

    if reverse {
        // The refs just written point at filtered objects that are still buffered, and
        // rev-parse reads them through the repository handle, which only sees disk.
        transaction.flush_mem_odb()?;

        let rev = |spec: &str| -> anyhow::Result<gix_hash::ObjectId> {
            transaction
                .rev_parse(spec)?
                .ok_or_else(|| anyhow!("no such revision: {}", spec))
        };
        let new = rev(target)?;
        let old = rev("JOSH_TMP")?;
        let unfiltered_old = rev(&input_ref)?;

        let ret = match josh_core::history::unapply_filter(
            &transaction,
            filterobj,
            unfiltered_old,
            old,
            new,
            josh_core::history::OrphansMode::Keep,
            None,
        ) {
            Ok(rewritten) => {
                // Concurrent commits on the input reference that are not
                // represented in the pushed filtered state would be silently
                // discarded by the ref update: require fast-forward like git.
                if rewritten != unfiltered_old
                    && !josh_core::objects::is_descendant_of(
                        transaction.odb(),
                        rewritten,
                        unfiltered_old,
                    )?
                    && !args.get_flag("force")
                {
                    return Err(anyhow!(
                        "refusing non-fast-forward update of {} -- it contains commits that \
                         the reverse apply would discard. Re-apply the filter and rebase the \
                         filtered changes onto the result, or pass --force",
                        input_ref
                    ));
                }
                transaction.update_ref(
                    &input_ref,
                    josh_core::cache::Expected::At(unfiltered_old),
                    rewritten,
                    "unapply_filter",
                )?;
                rewritten
            }
            Err(e) => {
                eprintln!("{}", e);
                return Ok(1);
            }
        };

        let roundtripped = if args.get_flag("check-roundtrip") {
            josh_core::filter_commit(&transaction, filterobj, ret)?
        } else {
            new
        };

        return if roundtripped != new {
            println!("Roundtrip failed");
            Ok(1)
        } else {
            println!("{}", ret);
            Ok(0)
        };
    }

    if !reverse
        && args.get_one::<String>("update") != Some(&"FILTERED_HEAD".to_string())
        && updated_refs.len() == 1
        && updated_refs[0].1 == old_oid
    {
        eprintln!(
            "Warning: reference {} wasn't updated",
            args.get_one::<String>("update").unwrap()
        );
    }

    println!("{}", updated_refs[0].1);

    // The queries below run in separate transactions whose stores only see on-disk objects, so
    // flush the filtered objects out of this transaction first.
    transaction.flush_mem_odb()?;

    if let Some(gql_query) = args.get_one::<String>("graphql") {
        let context = josh_graphql::context(transaction.try_clone()?, transaction.try_clone()?);

        // Since we're running on local repo, mark fetch as already completed
        // so that graphql code doesn't request it again
        context.fetch_state.complete();

        let (res, _errors) = juniper::execute_sync(
            gql_query,
            None,
            &josh_graphql::repo_schema(".".to_string(), true),
            &std::collections::HashMap::new(),
            &context,
        )?;

        let j = serde_json::to_string_pretty(&res)?;
        println!("{}", j);
    }

    std::mem::drop(finish);

    if let Some(query) = args.get_one::<String>("query") {
        let transaction = josh_core::cache::TransactionContext::from_env(cache.clone())?
            .with_mem_odb_limit(josh_cli::MAX_MEM_PACK_SIZE)
            .open()?;
        let commit_id = transaction
            .resolve_ref(update_target)?
            .ok_or_else(|| anyhow!("update target '{}' not found", update_target))?;

        print!(
            "{}",
            josh_templates::render(&transaction, cache.clone(), "", commit_id, query, false)?
                .map(|x| x.0)
                .unwrap_or("File not found".to_string())
        );
    }

    Ok(0)
}

fn main() -> std::process::ExitCode {
    let _flush_guard = josh_core::memodb::FlushGuard::new();
    env_logger::init();
    let args = {
        let mut args = vec![];
        for arg in std::env::args() {
            args.push(arg);
        }
        args
    };

    let code = if let Err(e) = run_filter(args) {
        eprintln!("ERROR: {}", e);
        1
    } else {
        0
    };

    std::process::ExitCode::from(code as u8)
}

#[test]
fn verify_app() {
    make_app().debug_assert();
}
