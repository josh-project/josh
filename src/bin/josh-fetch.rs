#![deny(warnings)]

extern crate josh;

#[macro_use]
extern crate rs_tracing;

extern crate clap;
extern crate git2;
extern crate regex;

#[macro_use]
extern crate lazy_static;

use josh::scratch;
use josh::shell;
use josh::view_maps;
use regex::Regex;
use std::env;
use std::process::exit;

use std::fs::read_to_string;

lazy_static! {
    static ref INFO_REGEX: Regex =
        Regex::new(r"\[(?P<target>.*):(?P<remote>.*)@(?P<rev>.*)\](?P<spec>[^\[]*)")
            .expect("can't compile regex");
}

fn run_fetch(args: Vec<String>) -> i32 {
    let args = clap::App::new("josh-proxy")
        .arg(clap::Arg::with_name("file").long("file").takes_value(true))
        .arg(
            clap::Arg::with_name("trace")
                .long("trace")
                .takes_value(true),
        )
        .get_matches_from(args);

    if let Some(tf) = args.value_of("trace") {
        open_trace_file!(tf).expect("can't open tracefile");
    }

    let repo = git2::Repository::open_from_env().unwrap();
    let shell = shell::Shell {
        cwd: repo.path().to_owned(),
    };

    for caps in INFO_REGEX
        .captures_iter(&read_to_string(args.value_of("file").unwrap()).expect("read_to_string"))
    {
        let remote = caps.name("remote").unwrap().as_str().to_string();
        let rev = caps.name("rev").unwrap().as_str().trim().to_owned();
        let target = caps.name("target").unwrap().as_str().trim().to_owned();
        let viewstr = caps.name("spec").unwrap().as_str().trim().to_owned();

        let mut viewobj = josh::build_view(&viewstr);

        let pres = viewobj.prefixes();

        for p in pres {
            viewobj = josh::build_chain(
                viewobj,
                josh::build_view(&format!(":info={}/joshinfo", p.to_str().unwrap())),
            );
        }

        let cmd = format!("git fetch {} '{}'", &remote, &rev);

        let (_stdout, stderr) = shell.command(&cmd);
        println!("{}", stderr);

        let mut fm = view_maps::ViewMaps::new();
        let mut bm = view_maps::ViewMaps::new();
        scratch::transform_commit(&repo, &*viewobj, "FETCH_HEAD", &target, &mut fm, &mut bm);
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
