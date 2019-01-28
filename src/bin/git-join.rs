extern crate grib;
extern crate clap;
extern crate git2;
use grib::Shell;
use grib::scratch;
use git2::*;
use std::path::Path;
use grib::replace_subtree;
use std::collections::HashMap;

const TMP_NAME: &'static str = "refs/centralgit/tmp_fd2db5f8_bac2_4a1e_9487_4ac3414788aa";

// force push of the new revision-object to temp repo
fn transfer<'a>(repo: &'a Repository, rev: &str, source: &Path) -> Object<'a>
{
    // TODO: implement using libgit
    let target = &repo.path();
    let shell = Shell {
        cwd: source.to_path_buf(),
    };
    shell.command(&format!("git update-ref {} {}", TMP_NAME, rev));
    shell.command(&format!("git push --force {} {}", &target.to_string_lossy(), TMP_NAME));

    let obj = repo.revparse_single(rev)
        .expect("can't find transfered ref");
    return obj;
}


fn main()
{
    let args = clap::App::new("git-join")
        .arg(clap::Arg::with_name("source").long("source").takes_value(true))
        .arg(clap::Arg::with_name("output").long("output").takes_value(true))
        .arg(clap::Arg::with_name("branch").long("branch").takes_value(true))
        .arg(clap::Arg::with_name("subdir").long("subdir").takes_value(true))
        .get_matches();

    let branch = args.value_of("branch").expect("missing branch");
    let source = args.value_of("source").expect("missing source");
    let output = args.value_of("output").expect("missing source");
    let subdir = args.value_of("subdir").expect("missing subdir");

    let td = Path::new("/tmp/git-join2/");
    let scratch = scratch::new(&td.join("scratch"));
    let repo = git2::Repository::open(".").expect("can't open repo");
    let central_head = repo.revparse_single(branch).expect("can't find branch");
    let shell = Shell {
        cwd: scratch.path().to_path_buf(),
    };
    scratch
        .find_reference("refs/heads/join_source")
        .map(|mut r| {
            r.delete().ok();
        })
        .ok();
    shell.command(&format!("git fetch {} {}:join_source", source, branch));
    transfer(&scratch, &format!("{}", central_head.id()), &Path::new("."));
    let module_head = scratch
        .revparse_single("join_source")
        .expect("can'f find join_source");

    let signature = scratch.signature().unwrap();
    let result = scratch::join_to_subdir(
        &scratch,
        central_head.id(),
        &Path::new(subdir),
        module_head.id(),
        &signature,
    );

    scratch
        .find_reference("refs/heads/result")
        .map(|mut r| {
            r.delete().ok();
        })
        .ok();
    scratch
        .reference("refs/heads/join_result", result.0, true, "join")
        .ok();
    scratch
        .reference("refs/heads/join_tmp", result.1, true, "join")
        .ok();
    let shell = Shell {
        cwd: Path::new(".").to_path_buf(),
    };
    repo.find_reference("refs/heads/join")
        .map(|mut r| {
            r.delete().ok();
        })
        .ok();

    shell.command(&format!("git branch -D {}", output));
    shell.command(&format!("git fetch {:?} join_tmp:{}", scratch.path(), output));
}
