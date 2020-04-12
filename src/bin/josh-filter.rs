#![deny(warnings)]

extern crate josh;

extern crate rs_tracing;

extern crate clap;
extern crate git2;
extern crate regex;

#[macro_use]
extern crate lazy_static;

use josh::view_maps;
use std::env;
use std::process::exit;
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
        .arg(clap::Arg::with_name("file").long("file").takes_value(true))
        .arg(clap::Arg::with_name("from_to").takes_value(true))
        .arg(clap::Arg::with_name("spec").takes_value(true))
        .arg(clap::Arg::with_name("squash").long("squash"))
        .arg(clap::Arg::with_name("reverse").long("reverse"))
        .arg(clap::Arg::with_name("infofile").long("infofile"))
        .arg(clap::Arg::with_name("version").long("version"))
        .arg(
            clap::Arg::with_name("trace")
                .long("trace")
                .takes_value(true),
        )
        .get_matches_from(args);

    if args.is_present("version") {
        let v =
            option_env!("GIT_DESCRIBE").unwrap_or(env!("CARGO_PKG_VERSION"));
        println!("Version: {}", v);
        return Ok(0);
    }

    let repo = git2::Repository::open_from_env()?;
    let mut fm = view_maps::try_load(&repo.path().join("josh_forward_maps"));
    let backward_maps = Arc::new(RwLock::new(view_maps::try_load(
        &repo.path().join("josh_backward_maps"),
    )));

    let srcstr = args.value_of("from_to").unwrap_or("");
    let specstr = args.value_of("spec").unwrap_or("");

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

        let mut viewobj = josh::build_filter(&filter_spec);

        let pres = viewobj.prefixes();

        if args.is_present("infofile") {
            for (p, v) in pres.iter() {
                viewobj = josh::build_chain(
                    viewobj,
                    josh::build_filter(&format!(
                        ":info={},commit=#sha1,tree=#tree,src={},view={}",
                        p,
                        &src,
                        v.replace(":", "<colon>").replace(",", "<comma>")
                    )),
                );
            }
        }

        let reverse = args.is_present("reverse");

        if args.is_present("squash") {
            viewobj = josh::build_chain(
                josh::build_filter(&format!(":cutoff={}", &src)),
                viewobj,
            );
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

        josh::apply_view_to_refs(
            &repo,
            &*viewobj,
            &[(src.clone(), t)],
            &mut fm,
            &mut backward_maps.write().unwrap(),
        );

        if reverse {
            let new = repo.revparse_single(&target).unwrap().id();
            let old = repo.revparse_single("JOSH_TMP").unwrap().id();

            match josh::unapply_view(
                &repo,
                backward_maps.clone(),
                &*viewobj,
                old,
                new,
            )? {
                josh::UnapplyView::Done(rewritten) => {
                    repo.reference(&src, rewritten, true, "unapply_view")?;
                }
                _ => {
                    /* debug!("rewritten ERROR"); */
                    return Ok(1);
                }
            }
        }
    }

    let bm = backward_maps.read().unwrap();

    view_maps::persist(&*bm, &repo.path().join("josh_backward_maps"));
    view_maps::persist(&fm, &repo.path().join("josh_forward_maps"));

    return Ok(0);
}

fn main() {
    let args = {
        let mut args = vec![];
        for arg in env::args() {
            args.push(arg);
        }
        args
    };

    exit(run_filter(args).unwrap_or(1));
}
