#![deny(warnings)]
#![warn(unused_extern_crates)]

#[macro_use]
extern crate rs_tracing;

use std::fs::read_to_string;
use std::io::Write;

fn run_filter(args: Vec<String>) -> josh::JoshResult<i32> {
    let args = clap::App::new("josh-filter")
        .arg(clap::Arg::with_name("spec").takes_value(true))
        .arg(clap::Arg::with_name("input_ref").takes_value(true))
        .arg(clap::Arg::with_name("file").long("file").takes_value(true))
        .arg(
            clap::Arg::with_name("update")
                .long("update")
                .takes_value(true),
        )
        .arg(clap::Arg::with_name("squash").long("squash"))
        .arg(clap::Arg::with_name("discover").short("d"))
        .arg(clap::Arg::with_name("trace").short("t"))
        .arg(clap::Arg::with_name("print-filter").short("p"))
        .arg(clap::Arg::with_name("cache-stats").short("s"))
        .arg(clap::Arg::with_name("no-cache").short("n"))
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
        let v = option_env!("GIT_DESCRIBE")
            .unwrap_or(std::env!("CARGO_PKG_VERSION"));
        println!("Version: {}", v);
        return Ok(0);
    }
    let specstr = args.value_of("spec").unwrap_or(":nop");
    let specstr = args
        .value_of("file")
        .and_then(|f| read_to_string(f).ok())
        .unwrap_or(specstr.to_string());

    let mut filterobj = josh::filters::parse(&specstr)?;

    if args.is_present("squash") {
        filterobj =
            josh::build_chain(josh::filters::parse(":SQUASH")?, filterobj);
    }

    if args.is_present("print-filter") {
        println!(
            "{}",
            josh::filters::pretty(
                filterobj,
                if args.is_present("file") { 0 } else { 4 }
            )
        );
        return Ok(0);
    }

    let repo = git2::Repository::open_from_env()?;
    let transaction = josh::filter_cache::Transaction::new(repo);
    let repo = transaction.repo();

    let odb = repo.odb()?;
    let mempack = odb.add_new_mempack_backend(1000)?;

    if !args.is_present("no-cache") {
        josh::filter_cache::load(&repo.path())?;
    }

    let finish = defer::defer(|| {
        if args.is_present("trace") {
            rs_tracing::close_trace_file!();
        }
        if args.is_present("cache-stats") {
            josh::filter_cache::print_stats();
        }
        let mut buf = git2::Buf::new();
        mempack.dump(&repo, &mut buf).unwrap();
        if buf.len() > 32 {
            let mut w = odb.packwriter().unwrap();
            w.write(&buf).unwrap();
            w.commit().unwrap();
        }
    });

    let input_ref = args.value_of("input_ref").unwrap_or("HEAD");

    if args.is_present("discover") {
        let r = repo.revparse_single(&input_ref)?;
        let hs = josh::housekeeping::find_all_workspaces_and_subdirectories(
            &r.peel_to_tree()?,
        )?;
        for i in hs {
            if i.contains(":workspace=") {
                continue;
            }
            josh::apply_filter_to_refs(
                &transaction,
                josh::parse(&i)?,
                &[(input_ref.to_string(), "refs/JOSH_TMP".to_string())],
            )?;
        }
    }

    let update_target = args.value_of("update").unwrap_or("refs/JOSH_HEAD");

    let src = input_ref;
    let target = update_target;

    let reverse = args.is_present("reverse");
    let check_permissions = args.is_present("check-permission");

    if check_permissions {
        filterobj =
            josh::build_chain(josh::filters::parse(":DIRS")?, filterobj);
        filterobj =
            josh::build_chain(filterobj, josh::filters::parse(":FOLD")?);
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

    josh::apply_filter_to_refs(
        &transaction,
        filterobj,
        &[(src.clone(), t.clone())],
    )?;

    let mut all_dirs = vec![];

    if check_permissions {
        let result_tree = repo.find_reference(&t)?.peel_to_tree()?;

        result_tree.walk(git2::TreeWalkMode::PreOrder, |_, entry| {
            let name = entry.name().unwrap();
            if name.starts_with("JOSH_ORIG_PATH_") {
                let dirname = format!(
                    "{}",
                    josh::from_ns(&name.replacen("JOSH_ORIG_PATH_", "", 1))
                );
                all_dirs.push(dirname);
            }
            git2::TreeWalkResult::Ok
        })?;
    }

    let mut dedup = vec![];

    for w in all_dirs.as_slice().windows(2) {
        if let [a, b, ..] = w {
            if !b.starts_with(a) {
                dedup.push(a.to_owned());
            }
        }
    }

    let dedup = all_dirs;

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

        match josh::unapply_filter(
            &transaction,
            filterobj,
            unfiltered_old,
            old,
            new,
            false,
        )? {
            josh::UnapplyFilter::Done(rewritten) => {
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
