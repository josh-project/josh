extern crate centralgithook;
extern crate git2;
extern crate clap;
extern crate env_logger;

use std::env;
use std::process::exit;
use std::path::Path;
use centralgithook::CentralGit;
use centralgithook::Scratch;
use centralgithook::Gerrit;

const GERRIT_PORT: &'static str = "29418";
const AUTOMATION_USER: &'static str = "automation";
const CENTRAL_NAME: &'static str = "central";

fn main()
{
    ::std::env::set_var("RUST_LOG", "centralgithook=debug");
    env_logger::init().expect("can't init logger");

    let git_dir = env::var("GIT_DIR").expect("GIT_DIR not set");
    let (gerrit_path, gerrit) = Gerrit::new(&Path::new(&git_dir),
                                            CENTRAL_NAME,
                                            AUTOMATION_USER,
                                            GERRIT_PORT);

    let scratch_dir = gerrit_path.join("centralgithook_scratch");
    let scratch = Scratch::new(&scratch_dir, &gerrit);

    let mut args = vec!();
    for arg in env::args() {
        args.push(arg);
    }
    let hooks = CentralGit;
    exit(centralgithook::dispatch(args, &hooks, &gerrit, &scratch));
}

