#![warn(unused_extern_crates)]

#[macro_use]
extern crate rs_tracing;

use josh_core::JoshError;
use std::fs::read_to_string;
use std::io::Write;

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
                .help("Ref to apply filter to")
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
            clap::Arg::new("trace")
                .action(clap::ArgAction::SetTrue)
                .help("Write a trace in chrome tracing format")
                .short('t'),
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
            clap::Arg::new("notes-cache")
                .action(clap::ArgAction::SetTrue)
                .help("Enables notes based cache")
        )
        .arg(
            clap::Arg::new("pack")
                .action(clap::ArgAction::SetTrue)
                .help("Write a packfile instead of loose objects")
                .long("pack"),
        )
        .arg(clap::Arg::new("query").long("query").short('q'))
        .arg(
            clap::Arg::new("graphql")
                .long("graphql")
                .short('g'),
        )
        .arg(
            clap::Arg::new("max_comp")
                .long("max_comp")
                .short('m'),
        )
        .arg(
            clap::Arg::new("reverse").action(clap::ArgAction::SetTrue).long("reverse").help(
                "reverse-apply the filter to the output reference to update the input reference",
            ),
        )
        .arg(
            clap::Arg::new("check-permission")
                .action(clap::ArgAction::SetTrue)
                .long("check-permission")
                .short('c'),
        )
        .arg(clap::Arg::new("missing-permission").long("missing-permission")
                .action(clap::ArgAction::SetTrue))
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
    repo: std::sync::Mutex<git2::Repository>,
}

impl josh_core::cache::FilterHook for GitNotesFilterHook {
    fn filter_for_commit(
        &self,
        commit_oid: git2::Oid,
        arg: &str,
    ) -> josh_core::JoshResult<josh_core::filter::Filter> {
        let notes_ref = if arg.starts_with("refs/") {
            arg.to_string()
        } else {
            format!("refs/notes/{}", arg)
        };
        let repo = self.repo.lock().unwrap();
        let note = repo
            .find_note(Some(notes_ref.as_str()), commit_oid)
            .map_err(|_| josh_core::josh_error("missing git note for commit"))?;
        let msg = note
            .message()
            .ok_or_else(|| josh_core::josh_error("empty git note"))?;
        josh_core::filter::parse(msg)
    }
}

fn run_filter(args: Vec<String>) -> josh_core::JoshResult<i32> {
    let args = make_app().get_matches_from(args);

    if args.get_flag("trace") {
        rs_tracing::open_trace_file!(".").unwrap();
    }

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

    let repo = git2::Repository::open_from_env()?;
    let repo_path = repo.path().to_path_buf();

    // If the filter spec doesn't contain a colon and it's not from a file,
    // treat it as a SHA and read from tree
    let mut filterobj = if specstr.contains(':') || is_from_file {
        josh_core::filter::parse(&specstr)?
    } else {
        // Try to parse as SHA and read filter from tree
        let tree_oid = git2::Oid::from_str(&specstr.trim()).map_err(|_| {
            josh_core::josh_error(&format!("Invalid filter spec or SHA: {}", specstr))
        })?;
        josh_core::filter::persist::from_tree(&repo, tree_oid)?
    };

    if !args.get_flag("no-cache") {
        josh_core::cache::sled_load(&repo_path)?;
    }

    let cache = std::sync::Arc::new({
        let cache = josh_core::cache::CacheStack::new()
            .with_backend(josh_core::cache::SledCacheBackend::default());

        if args.get_flag("notes-cache") {
            cache.with_backend(josh_core::cache::NotesCacheBackend::new(&repo_path)?)
        } else {
            cache
        }
    });

    let mut transaction =
        josh_core::cache::TransactionContext::from_env(cache.clone())?.open(None)?;

    let repo_for_hook = git2::Repository::open_ext(
        transaction.repo().path(),
        git2::RepositoryOpenFlags::NO_SEARCH,
        &[] as &[&std::ffi::OsStr],
    )?;
    let hook = GitNotesFilterHook {
        repo: std::sync::Mutex::new(repo_for_hook),
    };
    transaction = transaction.with_filter_hook(std::sync::Arc::new(hook));

    let repo = transaction.repo();
    let input_ref = args.get_one::<String>("input").unwrap();

    let mut refs = vec![];
    let mut ids: Vec<(git2::Oid, josh_core::filter::Filter)> = vec![];

    let reference = repo.resolve_reference_from_short_name(input_ref).unwrap();
    let input_ref = reference.name().unwrap().to_string();
    refs.push((input_ref.clone(), reference.target().unwrap()));

    if args.get_flag("single") {
        filterobj = josh_core::filter::chain(josh_core::filter::squash(None), filterobj);
    }

    if let Some(pattern) = args.get_one::<String>("squash-pattern") {
        let pattern = pattern.to_string();
        for reference in repo.references_glob(&pattern).unwrap() {
            let reference = reference?;
            let target = reference.peel_to_commit()?.id();
            ids.push((
                target,
                josh_core::filter::message(reference.name().unwrap()),
            ));
            refs.push((reference.name().unwrap().to_string(), target));
        }
        filterobj = josh_core::filter::chain(josh_core::filter::squash(Some(&ids)), filterobj);
    };

    if let Some(filename) = args.get_one::<String>("squash-file") {
        let reflist = read_to_string(filename)?;

        for line in reflist.lines() {
            let split = line.split(' ').collect::<Vec<_>>();
            if let [sha, name] = split.as_slice() {
                let target = git2::Oid::from_str(sha)?;
                let target = repo.find_object(target, None)?.peel_to_commit()?.id();
                ids.push((target, josh_core::filter::message(name)));
                refs.push((name.to_string(), target));
            } else if !split.is_empty() {
                eprintln!("Warning: malformed line: {:?}", line);
            }
        }
        filterobj = josh_core::filter::chain(josh_core::filter::squash(Some(&ids)), filterobj);
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
        println!("{}", josh_core::filter::as_tree(repo, filterobj)?);
        return Ok(0);
    }

    let odb = repo.odb()?;
    let mp = if args.get_flag("pack") {
        let mempack = odb.add_new_mempack_backend(1000)?;
        Some(mempack)
    } else {
        None
    };

    let finish = defer::defer(|| {
        if args.get_flag("trace") {
            rs_tracing::close_trace_file!();
        }
        if args.get_flag("cache-stats") {
            josh_core::cache::sled_print_stats().expect("failed to collect cache stats");
        }
        if let Some(mempack) = mp {
            let mut buf = git2::Buf::new();
            mempack.dump(repo, &mut buf).unwrap();
            if buf.len() > 32 {
                let mut w = odb.packwriter().unwrap();
                w.write(&buf).unwrap();
                w.commit().unwrap();
            }
        }
    });

    if args.get_flag("discover") {
        let r = repo.revparse_single(&input_ref)?;
        let hs =
            josh_core::housekeeping::find_all_workspaces_and_subdirectories(&r.peel_to_tree()?)?;
        for i in hs {
            let (mut updated_refs, _) = josh_core::filter_refs(
                &transaction,
                josh_core::filter::parse(&i)?,
                &[(input_ref.to_string(), r.id())],
                josh_core::filter::empty(),
            );
            updated_refs[0].0 = "refs/JOSH_TMP".to_string();
            josh_core::update_refs(&transaction, &mut updated_refs, "");
        }
    }

    let update_target = args.get_one::<String>("update").unwrap();

    let target = update_target;

    let reverse = args.get_flag("reverse");

    let check_permissions = args.get_flag("check-permission");
    let mut permissions_filter = josh_core::filter::empty();
    if check_permissions {
        let whitelist;
        let blacklist;
        if args.contains_id("users")
            && args.contains_id("groups")
            && args.contains_id("user")
            && args.contains_id("repo")
        {
            let users = args.get_one::<String>("users").unwrap();
            let groups = args.get_one::<String>("groups").unwrap();
            let user = args.get_one::<String>("user").unwrap();
            let repo = args.get_one::<String>("repo").unwrap();

            let acl = josh_core::get_acl(users, groups, user, repo)?;
            whitelist = acl.0;
            blacklist = acl.1;
        } else {
            whitelist = match args.get_one::<String>("whitelist") {
                Some(s) => josh_core::filter::parse(s)?,
                _ => josh_core::filter::nop(),
            };
            blacklist = match args.get_one::<String>("blacklist") {
                Some(s) => josh_core::filter::parse(s)?,
                _ => josh_core::filter::empty(),
            };
        }
        permissions_filter =
            josh_core::filter::make_permissions_filter(filterobj, whitelist, blacklist)
    }

    let missing_permissions = args.get_flag("missing-permission");
    if missing_permissions {
        filterobj = permissions_filter;
        permissions_filter = josh_core::filter::empty();
    }

    let old_oid = if let Ok(id) = transaction.repo().refname_to_id(target) {
        id
    } else {
        git2::Oid::zero()
    };

    let (mut updated_refs, errors) =
        josh_core::filter_refs(&transaction, filterobj, &refs, permissions_filter);

    for error in errors {
        return Err(error.1);
    }
    for i in 0..updated_refs.len() {
        if updated_refs[i].0 == input_ref {
            if reverse {
                updated_refs[i].0 = "refs/JOSH_TMP".to_string();
            } else {
                updated_refs[i].0 = target.to_string();
            }
        } else {
            updated_refs[i].0 =
                updated_refs[i]
                    .0
                    .replacen("refs/heads/", "refs/heads/filtered/", 1);
            updated_refs[i].0 = updated_refs[i]
                .0
                .replacen("refs/tags/", "refs/tags/filtered/", 1);
        }
    }
    josh_core::update_refs(&transaction, &mut updated_refs, "");

    if let Some(searchstring) = args.get_one::<String>("search") {
        let ifilterobj =
            josh_core::filter::chain(filterobj, josh_core::filter::parse(":SQUASH:INDEX")?);

        let max_complexity: usize = args
            .get_one::<String>("max_comp")
            .unwrap_or(&"6".to_string())
            .parse()?;

        let commit = repo.find_reference(&input_ref)?.peel_to_commit()?;

        let index_commit =
            josh_core::filter_commit(&transaction, ifilterobj, commit.id(), permissions_filter)?;
        let tree = repo
            .find_commit(josh_core::filter_commit(
                &transaction,
                filterobj,
                commit.id(),
                permissions_filter,
            )?)?
            .tree()?;
        let index_tree = repo.find_commit(index_commit)?.tree()?;

        /* let start = std::time::Instant::now(); */
        let candidates = josh_core::filter::tree::search_candidates(
            &transaction,
            &index_tree,
            searchstring,
            max_complexity,
        )?;
        let matches = josh_core::filter::tree::search_matches(
            &transaction,
            &tree,
            searchstring,
            &candidates,
        )?;
        /* let duration = start.elapsed(); */

        for r in matches {
            for l in r.1 {
                println!("{}:{}: {}", r.0, l.0, l.1);
            }
        }
        /* eprintln!("\n Search took {:?}", duration); */
    }

    if reverse {
        let new = repo.revparse_single(target).unwrap().id();
        let old = repo.revparse_single("JOSH_TMP").unwrap().id();
        let unfiltered_old = repo.revparse_single(&input_ref).unwrap().id();

        match josh_core::history::unapply_filter(
            &transaction,
            filterobj,
            unfiltered_old,
            old,
            new,
            josh_core::history::OrphansMode::Keep,
            None,
            &mut None,
        ) {
            Ok(rewritten) => {
                repo.reference(&input_ref, rewritten, true, "unapply_filter")?;
                println!("{}", rewritten);
            }
            Err(JoshError(msg)) => {
                eprintln!("{}", msg);
                return Ok(1);
            }
        }
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

    if let Some(gql_query) = args.get_one::<String>("graphql") {
        let context = josh_graphql::context(transaction.try_clone()?, transaction.try_clone()?);
        *context.allow_refs.lock()? = true;
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
        let transaction =
            josh_core::cache::TransactionContext::from_env(cache.clone())?.open(None)?;
        let commit_id = transaction.repo().refname_to_id(update_target)?;

        print!(
            "{}",
            josh_templates::render(&transaction, cache.clone(), "", commit_id, query, false)?
                .map(|x| x.0)
                .unwrap_or("File not found".to_string())
        );
    }

    Ok(0)
}

fn main() {
    env_logger::init();
    let args = {
        let mut args = vec![];
        for arg in std::env::args() {
            args.push(arg);
        }
        args
    };

    std::process::exit(if let Err(e) = run_filter(args) {
        eprintln!(
            "ERROR: {}",
            match e {
                JoshError(s) => s,
            }
        );
        1
    } else {
        0
    })
}

#[test]
fn verify_app() {
    make_app().debug_assert();
}
