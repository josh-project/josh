#![deny(warnings)]

extern crate josh;

#[macro_use]
extern crate rs_tracing;

extern crate clap;
extern crate git2;
extern crate regex;

#[macro_use]
extern crate lazy_static;

use josh::view_maps;
use regex::Regex;
use std::env;
use std::process::exit;

use std::fs::read_to_string;

lazy_static! {
    static ref INFO_REGEX: Regex =
        Regex::new(r"\[(?P<target>.*):(?P<rev>.*)\](?P<spec>[^\[]*)").expect("can't compile regex");
}

fn run_fetch(args: Vec<String>) -> i32 {
    let args = clap::App::new("josh-fetch")
        .arg(clap::Arg::with_name("file").long("file").takes_value(true))
        .arg(
            clap::Arg::with_name("trace")
                .long("trace")
                .takes_value(true),
        )
        .get_matches_from(args);

    args.value_of("trace")
        .map(|tf| open_trace_file!(tf).expect("can't open tracefile"));

    let repo = git2::Repository::open_from_env().unwrap();
    let mut fm = view_maps::ViewMaps::new();
    let mut bm = view_maps::ViewMaps::new();

    for caps in INFO_REGEX
        .captures_iter(&read_to_string(args.value_of("file").unwrap()).expect("read_to_string"))
    {
        let rev = caps.name("rev").unwrap().as_str().trim().to_owned();
        let target = caps.name("target").unwrap().as_str().trim().to_owned();
        let viewstr = caps.name("spec").unwrap().as_str().trim().to_owned();

        let mut viewobj = josh::build_view(&viewstr);

        let pres = viewobj.prefixes();

        for p in pres {
            viewobj = josh::build_chain(
                viewobj,
                josh::build_view(&format!(":info={}/.joshinfo,{}", p.to_str().unwrap(), &rev)),
            );
        }

        josh::transform_commit(&repo, &*viewobj, &rev, &target, &mut fm, &mut bm);
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

    exit(run_fetch(args));
}
