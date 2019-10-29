#![deny(warnings)]

extern crate josh;

extern crate rs_tracing;

extern crate git2;
extern crate regex;
extern crate structopt;

#[macro_use]
extern crate lazy_static;

use josh::view_maps;
use std::env;
use std::fs::File;
use std::io::Read;
use std::process::exit;
use structopt::StructOpt;

fn parse_file_argument(path: &str) -> Result<File, std::io::Error> {
    File::open(path)
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "josh-filter",
    about = "Runs josh operations in a local git repository"
)]
#[structopt(global_settings(&[structopt::clap::AppSettings::ColoredHelp]))]
pub struct JoshFilter {
    #[structopt(long = "squash")]
    #[structopt(help = "all commit leading to the specified view")]
    pub squash: bool,

    #[structopt(long = "info-file")]
    #[structopt(
        help = "create an additional info file which contains information how the workspace have been constructed"
    )]
    pub info_file: bool,

    #[structopt(long = "reverse")]
    pub reverse: bool,

    #[structopt(long = "file")]
    #[structopt(parse(try_from_str = parse_file_argument))]
    #[structopt(
        help = "overwrites what have been specified for from_to with the contents of the specified file"
    )]
    pub file: Option<File>,

    #[structopt(name = "from_to")]
    #[structopt(help = "Format string specifying source and destionation for the workspace")]
    pub from_to: String,

    #[structopt(name = "spec")]
    #[structopt(help = "Format string defineing the workspace")]
    pub spec: String,
}

lazy_static! {
    static ref FILE_REGEX: regex::Regex =
        regex::Regex::new(r"\[(?P<src>.*)\](?P<spec>[^\[]*)").expect("can't compile regex");
    static ref STR_REGEX: regex::Regex =
        regex::Regex::new(r"(?P<src>[^:]*)(?P<spec>:[^\[]*)").expect("can't compile regex");
}

fn run_filter(args: Vec<String>) -> i32 {
    let args = JoshFilter::from_iter(args.iter());

    let repo = git2::Repository::open_from_env().unwrap();
    let mut fm = view_maps::ViewMaps::new();
    let mut bm = view_maps::ViewMaps::new();

    let srcstr = args.from_to;
    let specstr = args.spec;

    let filestr = args.file.map_or_else(
        || format!("[{}]{}", srcstr, specstr),
        |mut f| {
            let mut s = String::new();
            f.read_to_string(&mut s).unwrap();
            s
        },
    );

    for caps in FILE_REGEX.captures_iter(&filestr) {
        let from_to = caps.name("src").unwrap().as_str().trim().to_owned();
        let mut splitted = from_to.splitn(2, ":");

        let src = splitted
            .next()
            .expect("from_to must contain \":\"")
            .to_owned();
        let target = splitted
            .next()
            .expect("from_to must contain \":\"")
            .to_owned();

        let viewstr = caps.name("spec").unwrap().as_str().trim().to_owned();

        let mut viewobj = josh::build_view(&repo, &viewstr);

        let pres = viewobj.prefixes();

        if args.info_file {
            for (p, v) in pres.iter() {
                viewobj = josh::build_chain(
                    viewobj,
                    josh::build_view(
                        &repo,
                        &format!(
                            ":info={},commit=#sha1,tree=#tree,src={},view={}",
                            p,
                            &src,
                            v.replace(":", "<colon>").replace(",", "<comma>")
                        ),
                    ),
                );
            }
        }

        if args.squash {
            viewobj = josh::build_chain(
                josh::build_view(&repo, &format!(":cutoff={}", &src)),
                viewobj,
            );
        }

        let t = if args.reverse {
            "refs/JOSH_TMP".to_owned()
        } else {
            target.clone()
        };
        let src = repo
            .revparse_ext(&src)
            .expect("reference not found 1")
            .1
            .expect("reference not found")
            .name()
            .unwrap()
            .to_string();

        josh::apply_view_to_refs(&repo, &*viewobj, &[(src.clone(), t)], &mut fm, &mut bm);

        if args.reverse {
            let new = repo.revparse_single(&target).unwrap().id();
            let old = repo.revparse_single("JOSH_TMP").unwrap().id();

            match josh::unapply_view(&repo, &bm, &*viewobj, old, new) {
                josh::UnapplyView::Done(rewritten) => {
                    repo.reference(&src, rewritten, true, "unapply_view")
                        .expect("can't create reference");
                }
                _ => {
                    /* debug!("rewritten ERROR"); */
                    return 1;
                }
            }
        }
    }

    return 0;
}

fn main() {
    let args = {
        let mut args = vec![];
        for arg in env::args() {
            args.push(arg);
        }
        args
    };

    exit(run_filter(args));
}
