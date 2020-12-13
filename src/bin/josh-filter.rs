#![deny(warnings)]
#![warn(unused_extern_crates)]

#[macro_use]
extern crate lazy_static;

use std::sync::{Arc, RwLock};

use std::fs::read_to_string;

lazy_static! {
    static ref FILE_REGEX: regex::Regex =
        regex::Regex::new(r"\[(?P<src>.*)\](?P<spec>[^\[]*)")
            .expect("can't compile regex");
    static ref STR_REGEX: regex::Regex =
        regex::Regex::new(r"(?P<src>[^:]*)(?P<spec>:[^\[]*)")
            .expect("can't compile regex");
}

fn run_filter(args: Vec<String>) -> josh::JoshResult<i32> {
    let args = clap::App::new("josh-filter")
        .arg(clap::Arg::with_name("input_ref").takes_value(true))
        .arg(clap::Arg::with_name("spec").takes_value(true))
        .arg(clap::Arg::with_name("file").long("file").takes_value(true))
        .arg(
            clap::Arg::with_name("update")
                .long("update")
                .takes_value(true),
        )
        .arg(clap::Arg::with_name("squash").long("squash"))
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
                .takes_value(true),
        )
        .arg(clap::Arg::with_name("infofile").long("infofile"))
        .arg(clap::Arg::with_name("version").long("version"))
        .arg(
            clap::Arg::with_name("trace")
                .long("trace")
                .takes_value(true),
        )
        .get_matches_from(args);

    if args.is_present("version") {
        let v = option_env!("GIT_DESCRIBE")
            .unwrap_or(std::env!("CARGO_PKG_VERSION"));
        println!("Version: {}", v);
        return Ok(0);
    }

    let repo = git2::Repository::open_from_env()?;
    let forward_maps = Arc::new(RwLock::new(josh::filter_cache::try_load(
        &repo.path().join("josh_forward_maps"),
    )));
    let backward_maps = Arc::new(RwLock::new(josh::filter_cache::try_load(
        &repo.path().join("josh_backward_maps"),
    )));

    let input_ref = args.value_of("input_ref").unwrap_or("");
    let specstr = args.value_of("spec").unwrap_or("");
    let update_target = args.value_of("update").unwrap_or("refs/JOSH_HEAD");
    let srcstr = format!("{}:{}", input_ref, update_target);

    let filestr = args
        .value_of("file")
        .and_then(|f| read_to_string(f).ok())
        .unwrap_or(format!("[{}]{}", srcstr, specstr));

    for caps in FILE_REGEX.captures_iter(&filestr) {
        let from_to = caps.name("src").unwrap().as_str().trim().to_owned();
        let mut splitted = from_to.splitn(2, ":");

        let src = splitted
            .next()
            .ok_or(josh::josh_error("from_to must contain \":\""))?
            .to_owned();
        let target = splitted
            .next()
            .ok_or(josh::josh_error("from_to must contain \":\""))?
            .to_owned();

        let filter_spec = caps.name("spec").unwrap().as_str().trim().to_owned();

        let mut filterobj = josh::filters::parse(&filter_spec)?;

        /* if args.is_present("infofile") { */
        /*     let pres = filterobj.prefixes(); */
        /*     for (p, v) in pres.iter() { */
        /*         filterobj = josh::build_chain( */
        /*             filterobj, */
        /*             josh::filters::parse(&format!( */
        /*                 ":INFO={},commit=#sha1,tree=#tree,src={},filter={}", */
        /*                 p, */
        /*                 &src, */
        /*                 v.replace(":", "<colon>").replace(",", "<comma>") */
        /*             ))?, */
        /*         ); */
        /*     } */
        /* } */

        let reverse = args.is_present("reverse");
        let check_permissions = args.is_present("check-permission");

        if args.is_present("squash") {
            filterobj = josh::build_chain(
                josh::filters::parse(&format!(":CUTOFF={}", &src))?,
                filterobj,
            );
        }

        if check_permissions {
            filterobj =
                josh::build_chain(josh::filters::parse(":DIRS")?, filterobj);
            filterobj =
                josh::build_chain(filterobj, josh::filters::parse(":FOLD")?);
        }

        let t = if reverse {
            "refs/JOSH_TMP".to_owned()
        } else {
            target.clone()
        };
        let src = repo
            .revparse_ext(&src)?
            .1
            .ok_or(josh::josh_error("reference not found"))?
            .name()
            .unwrap()
            .to_string();

        josh::apply_filter_to_refs(
            &repo,
            &*filterobj,
            &[(src.clone(), t.clone())],
            &mut forward_maps.write().unwrap(),
            &mut backward_maps.write().unwrap(),
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

        if let Some(cp) = args.value_of("check-permission") {
            let permission_regex = regex::Regex::new(cp)?;

            let mut allowed = dedup.len() != 0;
            for d in dedup.iter() {
                let m = permission_regex.is_match(&d);
                if !m {
                    allowed = false;
                    println!("missing permission for: {}", &d);
                }
            }
            println!("Allowed = {:?}", allowed);
        }

        if let Some(query) = args.value_of("query") {
            print!(
                "{}",
                josh::query::render(
                    git2::Repository::open_from_env()?,
                    &update_target.to_string(),
                    &query,
                    josh::filter_cache::new_downstream(&forward_maps),
                    josh::filter_cache::new_downstream(&backward_maps),
                )?
                .unwrap_or("File not found".to_string())
            );
        }

        if reverse {
            let new = repo.revparse_single(&target).unwrap().id();
            let old = repo.revparse_single("JOSH_TMP").unwrap().id();
            let unfiltered_old = repo.revparse_single(&input_ref).unwrap().id();

            match josh::unapply_filter(
                &repo,
                backward_maps.clone(),
                &*filterobj,
                unfiltered_old,
                old,
                new,
            )? {
                josh::UnapplyFilter::Done(rewritten) => {
                    repo.reference(&src, rewritten, true, "unapply_filter")?;
                }
                _ => {
                    return Ok(1);
                }
            }
        }
    }

    let bm = backward_maps.read().unwrap();

    josh::filter_cache::persist(&*bm, &repo.path().join("josh_backward_maps"))
        .ok();
    josh::filter_cache::persist(
        &forward_maps.read().unwrap(),
        &repo.path().join("josh_forward_maps"),
    )
    .ok();

    return Ok(0);
}

fn main() {
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
