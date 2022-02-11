#![deny(warnings)]
#![warn(unused_extern_crates)]

#[macro_use]
extern crate rs_tracing;

use josh::JoshError;
use std::fs::read_to_string;
use std::io::Write;

fn run_filter(args: Vec<String>) -> josh::JoshResult<i32> {
    let app = clap::App::new("josh-filter");

    #[cfg(feature = "search")]
    let app = { app.arg(clap::Arg::new("search").long("search").takes_value(true)) };
    let args = app
        .arg(
            clap::Arg::new("filter")
                .help("Filter to apply")
                .default_value(":/")
                .takes_value(true),
        )
        .arg(
            clap::Arg::new("input")
                .help("Ref to apply filter to")
                .default_value("HEAD")
                .takes_value(true),
        )
        .arg(
            clap::Arg::new("file")
                .long("file")
                .help("Read filter spec from file")
                .takes_value(true),
        )
        .arg(
            clap::Arg::new("update")
                .long("update")
                .help("reference to update with the result")
                .default_value("FILTERED_HEAD")
                .takes_value(true),
        )
        .arg(
            clap::Arg::new("squash")
                .help("Only output one commit, without history")
                .long("squash"),
        )
        .arg(
            clap::Arg::new("discover")
                .help("Populate the cache with probable filters")
                .short('d'),
        )
        .arg(
            clap::Arg::new("trace")
                .help("Write a trace in chrome tracing format")
                .short('t'),
        )
        .arg(
            clap::Arg::new("print-filter")
                .help("Pretty print the filter and exit")
                .short('p'),
        )
        .arg(
            clap::Arg::new("cache-stats")
                .help("Show stats about cache content")
                .short('s'),
        )
        .arg(
            clap::Arg::new("no-cache")
                .help("Don't load cache")
                .short('n'),
        )
        .arg(
            clap::Arg::new("pack")
                .help("Write a packfile instead of loose objects")
                .long("pack"),
        )
        .arg(
            clap::Arg::new("query")
                .long("query")
                .short('q')
                .takes_value(true),
        )
        .arg(
            clap::Arg::new("graphql")
                .long("graphql")
                .short('g')
                .takes_value(true),
        )
        .arg(
            clap::Arg::new("max_comp")
                .long("max_comp")
                .short('m')
                .takes_value(true),
        )
        .arg(clap::Arg::new("reverse").long("reverse"))
        .arg(
            clap::Arg::new("check-permission")
                .long("check-permission")
                .short('c'),
        )
        .arg(clap::Arg::new("missing-permission").long("missing-permission"))
        .arg(
            clap::Arg::new("whitelist")
                .long("whitelist")
                .short('w')
                .takes_value(true),
        )
        .arg(
            clap::Arg::new("blacklist")
                .long("blacklist")
                .short('b')
                .takes_value(true),
        )
        .arg(clap::Arg::new("users").long("users").takes_value(true))
        .arg(clap::Arg::new("groups").long("groups").takes_value(true))
        .arg(
            clap::Arg::new("user")
                .long("user")
                .short('u')
                .takes_value(true),
        )
        .arg(
            clap::Arg::new("repo")
                .long("repo")
                .short('r')
                .takes_value(true),
        )
        .arg(clap::Arg::new("version").long("version").short('v'))
        .get_matches_from(args);

    if args.is_present("trace") {
        rs_tracing::open_trace_file!(".").unwrap();
    }

    if args.is_present("version") {
        let v = option_env!("GIT_DESCRIBE").unwrap_or(std::env!("CARGO_PKG_VERSION"));
        println!("Version: {}", v);
        return Ok(0);
    }
    let specstr = args.value_of("filter").unwrap();
    let specstr = args
        .value_of("file")
        .and_then(|f| read_to_string(f).ok())
        .unwrap_or(specstr.to_string());

    let mut filterobj = josh::filter::parse(&specstr)?;

    if args.is_present("squash") {
        filterobj = josh::filter::chain(josh::filter::parse(":SQUASH")?, filterobj);
    }

    if args.is_present("print-filter") {
        println!(
            "{}",
            josh::filter::pretty(filterobj, if args.is_present("file") { 0 } else { 4 })
        );
        return Ok(0);
    }

    let repo = git2::Repository::open_from_env()?;
    if !args.is_present("no-cache") {
        josh::cache::load(repo.path())?;
    }
    let transaction = josh::cache::Transaction::new(repo, None);
    let repo = transaction.repo();

    let odb = repo.odb()?;
    let mp = if args.is_present("pack") {
        let mempack = odb.add_new_mempack_backend(1000)?;
        Some(mempack)
    } else {
        None
    };

    let finish = defer::defer(|| {
        if args.is_present("trace") {
            rs_tracing::close_trace_file!();
        }
        if args.is_present("cache-stats") {
            josh::cache::print_stats();
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

    let input_ref = args.value_of("input").unwrap();

    if args.is_present("discover") {
        let r = repo.revparse_single(input_ref)?;
        let hs = josh::housekeeping::find_all_workspaces_and_subdirectories(&r.peel_to_tree()?)?;
        for i in hs {
            if i.contains(":workspace=") {
                continue;
            }
            josh::filter_ref(
                &transaction,
                josh::filter::parse(&i)?,
                input_ref,
                "refs/JOSH_TMP",
                josh::filter::empty(),
            )?;
        }
    }

    let update_target = args.value_of("update").unwrap();

    let src = input_ref;
    let target = update_target;

    let reverse = args.is_present("reverse");

    let t = if reverse {
        "refs/JOSH_TMP".to_owned()
    } else {
        target.to_string()
    };
    let src = repo
        .revparse_ext(src)?
        .1
        .ok_or(josh::josh_error("reference not found"))?
        .name()
        .unwrap()
        .to_string();

    let check_permissions = args.is_present("check-permission");
    let mut permissions_filter = josh::filter::empty();
    if check_permissions {
        let whitelist;
        let blacklist;
        if args.is_present("users")
            && args.is_present("groups")
            && args.is_present("user")
            && args.is_present("repo")
        {
            let users = args.value_of("users").unwrap();
            let groups = args.value_of("groups").unwrap();
            let user = args.value_of("user").unwrap();
            let repo = args.value_of("repo").unwrap();

            let acl = josh::get_acl(users, groups, user, repo)?;
            whitelist = acl.0;
            blacklist = acl.1;
        } else {
            whitelist = match args.value_of("whitelist") {
                Some(s) => josh::filter::parse(s)?,
                _ => josh::filter::nop(),
            };
            blacklist = match args.value_of("blacklist") {
                Some(s) => josh::filter::parse(s)?,
                _ => josh::filter::empty(),
            };
        }
        permissions_filter = josh::filter::make_permissions_filter(filterobj, whitelist, blacklist)
    }

    let missing_permissions = args.is_present("missing-permission");
    if missing_permissions {
        filterobj = permissions_filter;
        permissions_filter = josh::filter::empty();
    }

    let updated_refs = josh::filter_ref(&transaction, filterobj, &src, &t, permissions_filter)?;
    if args.value_of("update") != Some("FILTERED_HEAD") && updated_refs == 0 {
        println!(
            "Warning: reference {} wasn't updated",
            args.value_of("update").unwrap()
        );
    }

    #[cfg(feature = "search")]
    if let Some(searchstring) = args.value_of("search") {
        let ifilterobj = josh::filter::chain(filterobj, josh::filter::parse(":SQUASH:INDEX")?);

        let max_complexity: usize = args.value_of("max_comp").unwrap_or("6").parse()?;

        josh::filter_ref(
            &transaction,
            ifilterobj,
            src.clone(),
            "refs/JOSH_TMP".to_string(),
        )?;
        let tree = repo.find_reference(&src)?.peel_to_tree()?;
        let index_tree = repo.find_reference(&"refs/JOSH_TMP")?.peel_to_tree()?;

        /* let start = std::time::Instant::now(); */
        let candidates = josh::filter::tree::search_candidates(
            &transaction,
            &index_tree,
            &searchstring,
            max_complexity,
        )?;
        let matches =
            josh::filter::tree::search_matches(&transaction, &tree, &searchstring, &candidates)?;
        /* let duration = start.elapsed(); */

        for r in matches {
            for l in r.1 {
                println!("{}:{}: {}", r.0, l.0, l.1);
            }
        }
        /* println!("\n Search took {:?}", duration); */
    }

    if reverse {
        let new = repo.revparse_single(target).unwrap().id();
        let old = repo.revparse_single("JOSH_TMP").unwrap().id();
        let unfiltered_old = repo.revparse_single(input_ref).unwrap().id();

        match josh::history::unapply_filter(
            &transaction,
            filterobj,
            unfiltered_old,
            old,
            new,
            false,
            None,
            &std::collections::HashMap::new(),
        )? {
            josh::UnapplyResult::Done(rewritten) => {
                repo.reference(&src, rewritten, true, "unapply_filter")?;
            }
            josh::UnapplyResult::RejectMerge(msg) => {
                println!("{}", msg);
                return Ok(1);
            }
            _ => {
                return Ok(1);
            }
        }
    }

    if let Some(gql_query) = args.value_of("graphql") {
        let (res, _errors) = juniper::execute_sync(
            gql_query,
            None,
            &josh::graphql::repo_schema(".".to_string(), true),
            &std::collections::HashMap::new(),
            &josh::graphql::context(transaction.try_clone()?),
        )?;

        let j = serde_json::to_string(&res)?;
        println!("{}", j);
    }

    std::mem::drop(finish);

    if let Some(query) = args.value_of("query") {
        print!(
            "{}",
            josh::query::render(
                &git2::Repository::open_from_env()?,
                "",
                &update_target.to_string(),
                query,
            )?
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
        println!(
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
