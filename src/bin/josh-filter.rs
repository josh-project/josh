#![deny(warnings)]
#![warn(unused_extern_crates)]

#[macro_use]
extern crate rs_tracing;

use std::fs::read_to_string;
use std::io::Write;

fn run_filter(args: Vec<String>) -> josh::JoshResult<i32> {
    let args = clap::App::new("josh-filter")
        .arg(
            clap::Arg::with_name("filter")
                .help("Filter to apply")
                .default_value(":/")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("input")
                .help("Ref to apply filter to")
                .default_value("HEAD")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("file")
                .long("file")
                .help("Read filter spec from file")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("update")
                .long("update")
                .help("reference to update with the result")
                .default_value("FILTERED_HEAD")
                .takes_value(true),
        )
        .arg(
            clap::Arg::with_name("squash")
                .help("Only output one commit, without history")
                .long("squash"),
        )
        .arg(
            clap::Arg::with_name("discover")
                .help("Populate the cache with probable filters")
                .short("d"),
        )
        .arg(
            clap::Arg::with_name("trace")
                .help("Write a trace in chrome tracing format")
                .short("t"),
        )
        .arg(
            clap::Arg::with_name("print-filter")
                .help("Pretty print the filter and exit")
                .short("p"),
        )
        .arg(
            clap::Arg::with_name("cache-stats")
                .help("Show stats about cache content")
                .short("s"),
        )
        .arg(
            clap::Arg::with_name("no-cache")
                .help("Don't load cache")
                .short("n"),
        )
        .arg(
            clap::Arg::with_name("pack")
                .help("Write a packfile instead of loose objects")
                .long("pack"),
        )
        .arg(
            clap::Arg::with_name("query")
                .long("query")
                .short("q")
                .takes_value(true),
        )
        .arg(clap::Arg::with_name("reverse").long("reverse"))
        .arg(
            clap::Arg::with_name("check-permission")
                .long("check-permission")
                .short("c")
                .takes_value(true),
        )
        .arg(clap::Arg::with_name("version").long("version").short("v"))
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
        josh::cache::load(&repo.path())?;
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
            mempack.dump(&repo, &mut buf).unwrap();
            if buf.len() > 32 {
                let mut w = odb.packwriter().unwrap();
                w.write(&buf).unwrap();
                w.commit().unwrap();
            }
        }
    });

    let input_ref = args.value_of("input").unwrap();

    if args.is_present("discover") {
        let r = repo.revparse_single(&input_ref)?;
        let hs = josh::housekeeping::find_all_workspaces_and_subdirectories(&r.peel_to_tree()?)?;
        for i in hs {
            if i.contains(":workspace=") {
                continue;
            }
            josh::filter_refs(
                &transaction,
                josh::filter::parse(&i)?,
                &[(input_ref.to_string(), "refs/JOSH_TMP".to_string())],
            )?;
        }
    }

    let update_target = args.value_of("update").unwrap();

    let src = input_ref;
    let target = update_target;

    let reverse = args.is_present("reverse");
    let check_permissions = args.is_present("check-permission");

    if check_permissions {
        filterobj = josh::filter::chain(josh::filter::parse(":PATHS")?, filterobj);
        filterobj = josh::filter::chain(filterobj, josh::filter::parse(":FOLD")?);
    }

    let t = if reverse {
        "refs/JOSH_TMP".to_owned()
    } else {
        target.to_string()
    };
    let src = repo
        .revparse_ext(&src)?
        .1
        .ok_or(josh::josh_error("reference not found"))?
        .name()
        .unwrap()
        .to_string();

    josh::filter_refs(&transaction, filterobj, &[(src.clone(), t.clone())])?;

    let mut all_paths = vec![];

    if check_permissions {
        let result_tree = repo.find_reference(&t)?.peel_to_tree()?;

        result_tree.walk(git2::TreeWalkMode::PreOrder, |_, entry| {
            let name = entry.name().unwrap();
            if name.starts_with("JOSH_ORIG_PATH_") {
                let pathname = format!(
                    "{}",
                    josh::from_ns(&name.replacen("JOSH_ORIG_PATH_", "", 1))
                );
                all_paths.push(pathname);
            }
            git2::TreeWalkResult::Ok
        })?;
    }

    let mut dedup = vec![];

    for w in all_paths.as_slice().windows(2) {
        if let [a, b, ..] = w {
            if !b.starts_with(a) {
                dedup.push(a.to_owned());
            }
        }
    }

    let dedup = all_paths;

    let options = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    };

    if let Some(cp) = args.value_of("check-permission") {
        let pattern = glob::Pattern::new(cp)?;

        let mut allowed = dedup.len() != 0;
        for d in dedup.iter() {
            let d = std::path::PathBuf::from(d);
            let m = pattern.matches_path_with(&d, options.clone());
            if !m {
                allowed = false;
                println!("missing permission for: {:?}", &d);
            }
        }
        println!("Allowed = {:?}", allowed);
    }

    if reverse {
        let new = repo.revparse_single(&target).unwrap().id();
        let old = repo.revparse_single("JOSH_TMP").unwrap().id();
        let unfiltered_old = repo.revparse_single(&input_ref).unwrap().id();

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
            _ => {
                return Ok(1);
            }
        }
    }

    std::mem::drop(finish);

    if let Some(query) = args.value_of("query") {
        print!(
            "{}",
            josh::query::render(
                &git2::Repository::open_from_env()?,
                "",
                &update_target.to_string(),
                &query,
            )?
            .unwrap_or("File not found".to_string())
        );
    }

    return Ok(0);
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
        println!("ERROR: {:?}", e);
        1
    } else {
        0
    })
}
