extern crate grib;
extern crate clap;
extern crate git2;
use grib::scratch;
use std::path::Path;


fn main()
{
    let args = clap::App::new("git-join")
        .arg(clap::Arg::with_name("output").long("output").takes_value(true))
        .arg(clap::Arg::with_name("branch").long("branch").takes_value(true))
        .arg(clap::Arg::with_name("subdir").long("subdir").takes_value(true))
        .get_matches();

    let branch = args.value_of("branch").expect("missing branch");
    let output = args.value_of("output").expect("missing source");
    let subdir = args.value_of("subdir").expect("missing subdir");

    let repo = git2::Repository::open_from_env().expect("can't open repo");
    let source = repo.revparse_single(branch).expect("can't find branch");

    let result = scratch::join_to_subdir(
        &repo,
        &Path::new(subdir),
        source.id(),
    );

    repo
        .reference(&format!("refs/heads/{}", output), result.1, true, "join")
        .ok();
}
