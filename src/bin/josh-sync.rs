/* #![deny(warnings)] */
#![warn(unused_extern_crates)]

#[macro_use]
extern crate lazy_static;

use std::fs::read_to_string;

lazy_static! {
    static ref FILE_REGEX: regex::Regex = regex::Regex::new(
        r"\[(?P<remote>.*)\((?P<src_ref>.*)\)\](?P<spec>[^\[]*)"
    )
    .expect("can't compile regex");
}

fn run_filter(args: Vec<String>) -> josh::JoshResult<i32> {
    let args = clap::App::new("josh-sync")
        .arg(clap::Arg::with_name("file").long("file").takes_value(true))
        .arg(clap::Arg::with_name("version").long("version"))
        .arg(clap::Arg::with_name("message").takes_value(true).short("m"))
        .get_matches_from(args);

    if args.is_present("version") {
        let v = option_env!("GIT_DESCRIBE")
            .unwrap_or(std::env!("CARGO_PKG_VERSION"));
        println!("Version: {}", v);
        return Ok(0);
    }

    let repo = git2::Repository::open_from_env()?;

    josh::filter_cache::load(&repo.path());
    let filename = args.value_of("file").unwrap_or("");
    let filestr = read_to_string(&filename)?;

    let head = repo.head()?.peel_to_commit()?;
    let mut new_tree = head.tree()?;

    let mut msg = format!(
        "{}\n\nSync-Config: {}",
        args.value_of("message").unwrap_or("sync"),
        filename
    );

    for caps in FILE_REGEX.captures_iter(&filestr) {
        let remote = caps.name("remote").unwrap().as_str().trim().to_owned();
        let src_ref = caps.name("src_ref").unwrap().as_str().trim().to_owned();

        let filter_spec = caps.name("spec").unwrap().as_str().trim().to_owned();

        let filter = josh::parse(&filter_spec)?;

        let src = repo
            .revparse_ext(&format!("refs/remotes/{}/{}", remote, src_ref))?
            .0
            .peel_to_commit()?;

        let state_in_head = josh::filters::unapply(
            &repo,
            &filter,
            head.tree()?,
            josh::empty_tree(&repo),
        )?;
        let head_cleaned = josh::treeops::substract_fast(
            &repo,
            new_tree.id(),
            josh::filters::apply(&repo, &filter, state_in_head)?.id(),
        )?;

        let merged = josh::treeops::overlay(
            &repo,
            head_cleaned,
            josh::filters::apply(&repo, &filter, src.tree()?)?.id(),
        )?;
        new_tree = repo.find_tree(merged)?;

        msg =
            format!("{}\nSynced: [{}({}) {}]", msg, remote, src_ref, src.id());
    }

    let new_tree = josh::treeops::replace_subtree(
        &repo,
        &std::path::PathBuf::from(filename),
        repo.blob(filestr.as_bytes())?,
        &new_tree,
    )?;

    let commit = repo.commit(
        Some("HEAD"),
        &repo.signature()?,
        &repo.signature()?,
        &msg,
        &new_tree,
        &[&head],
    )?;

    repo.reset(
        &repo.find_object(commit, None)?,
        git2::ResetType::Hard,
        None,
    )?;
    josh::filter_cache::persist(&repo.path());

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
