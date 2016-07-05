extern crate clap;
extern crate centralgithook;
extern crate git2;
use centralgithook::Scratch;
use centralgithook::Shell;
use std::path::Path;

fn main()
{
    let args = clap::App::new("git-join")
        .arg(clap::Arg::with_name("source").long("source").takes_value(true))
        // .arg(clap::Arg::with_name("branch").long("source branch").takes_value(true))
        .arg(clap::Arg::with_name("subdir").long("subdir").takes_value(true))
        .get_matches();

    let source = args.value_of("source").expect("missing source");
    let subdir = args.value_of("subdir").expect("missing subdir");

    let td = Path::new("/tmp/git-join2/");
    let scratch = Scratch::new(&td.join("scratch"));
    let repo = git2::Repository::open(".").expect("can't open repo");
    let central_head = repo.revparse_single("master").expect("can't find master");
    let shell = Shell { cwd: scratch.repo.path().to_path_buf() };
    scratch.repo
        .find_reference("refs/heads/join_source")
        .map(|mut r| {
            r.delete().ok();
        })
        .ok();
    shell.command(&format!("git fetch {} master:join_source", source));
    scratch.transfer(&format!("{}", central_head.id()), &Path::new("."));
    let module_head = scratch.repo.revparse_single("join_source").expect("can'f find join_source");

    let signature = scratch.repo.signature().unwrap();
    let result = scratch.join_to_subdir(central_head.id(),
                                        &Path::new(subdir),
                                        module_head.id(),
                                        &signature);

    scratch.repo
        .find_reference("refs/heads/result")
        .map(|mut r| {
            r.delete().ok();
        })
        .ok();
    scratch.repo.reference("refs/heads/join_result", result, true, "join").ok();
    let shell = Shell { cwd: Path::new(".").to_path_buf() };
    repo.find_reference("refs/heads/join")
        .map(|mut r| {
            r.delete().ok();
        })
        .ok();
    shell.command(&format!("git fetch {:?} join_result:join", scratch.repo.path()));
}
