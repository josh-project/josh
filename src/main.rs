extern crate centralgithook;
extern crate git2;
extern crate clap;

use std::env;
use std::process::exit;
use std::path::Path;
use std::path::PathBuf;
use centralgithook::migrate;

const GERRIT_PORT: &'static str = "29418";
const AUTOMATION_USER: &'static str = "automation";
// const MODULE_PATH_PREFIX: &'static str = "bsw";
const CENTRAL_NAME: &'static str = "central";

struct Gerrit
{
    path: PathBuf,
}

impl Gerrit
{
    pub fn new() -> Self
    {
        let git_dir = env::var("GIT_DIR").expect("GIT_DIR not set");
        let mut p = Path::new(&git_dir);
        while !p.join("bin").join("gerrit.sh").exists() {
            p = p.parent().expect("can't find gerrit root");
        }

        Gerrit { path: p.to_path_buf() }
    }
}

impl migrate::RepoHost for Gerrit
{
    // create module project on gerrit (if not existing)
    fn create_project(&self, module: &str) -> String
    {
        let shell = migrate::Shell { cwd: Path::new("/").to_path_buf() };
        shell.command(&format!("ssh -p {} {} gerrit create-project {}",
                               GERRIT_PORT,
                               "gerrit-test-git",
                               module))
    }

    fn fetch_url(&self, module_path: &str) -> String
    {
        if let Some(root) = self.path.as_os_str().to_str() {
            format!("{}/git/{}.git", root, module_path)
        }
        else {
            self.remote_url(module_path)
        }
    }

    fn remote_url(&self, module_path: &str) -> String
    {
        format!("ssh://{}@gerrit-test-git:{}/{}",
                AUTOMATION_USER,
                GERRIT_PORT,
                module_path)
    }
}

fn main()
{
    exit(main_ret());
}

fn main_ret() -> i32
{

    let args = clap::App::new("centralgithook")
        .arg(clap::Arg::with_name("oldrev").long("oldrev").takes_value(true))
        .arg(clap::Arg::with_name("newrev").long("newrev").takes_value(true))
        .arg(clap::Arg::with_name("project").long("project").takes_value(true))
        .arg(clap::Arg::with_name("refname").long("refname").takes_value(true))
        .arg(clap::Arg::with_name("uploader").long("uploader").takes_value(true))
        .arg(clap::Arg::with_name("commit").long("commit").takes_value(true))
        .arg(clap::Arg::with_name("change").long("change").takes_value(true))
        .arg(clap::Arg::with_name("change-url").long("change-url").takes_value(true))
        .arg(clap::Arg::with_name("change-owner").long("change-owner").takes_value(true))
        .arg(clap::Arg::with_name("branch").long("branch").takes_value(true))
        .arg(clap::Arg::with_name("submitter").long("submitter").takes_value(true))
        .arg(clap::Arg::with_name("topic").long("topic").takes_value(true))
        .get_matches();

    let newrev = args.value_of("newrev").unwrap_or("");
    let oldrev = args.value_of("oldrev").unwrap_or("");
    let project = args.value_of("project").unwrap_or("");
    let refname = args.value_of("refname").unwrap_or("");
    let commit = args.value_of("commit").unwrap_or("");

    let gerrit = Gerrit::new();

    if let Some(hook) = env::args().nth(0) {

        // ref-update: fired after push
        // change-merged: fired after gerrit-submit
        let is_update = hook.ends_with("ref-update");
        let is_submit = hook.ends_with("change-merged");

        let is_review = refname == "refs/for/master";
        let is_module = project != CENTRAL_NAME;
        let is_initial = oldrev == "0000000000000000000000000000000000000000";

        let uploader = args.value_of("uploader").unwrap_or("");
        if is_update && !is_review && !uploader.contains("Automation") {
            println!("==== uploader: {}", uploader);
            println!("==== only push to refs/for/master");
            return 1;
        }

        let scratch_dir = Path::new("/tmp/scratchme");
        let scratch = migrate::Scratch::new(&scratch_dir, &gerrit);
        if is_submit {
            // submit to central
            migrate::central_submit(&scratch, scratch.transfer(commit, &Path::new("."))).unwrap();
        }
        else if is_module && is_update && is_review {
            // module was pushed, get changes to central
            migrate::module_review_upload(&scratch,
                                          scratch.transfer(newrev, Path::new(".")),
                                          project,
                                          CENTRAL_NAME)
                .unwrap();
            // stop gerrit from allowing push to module directly
            return 1;
        }
        else if !is_module && is_update && !is_review {
            // direct push to master-branch of central
            if is_initial {
                println!(".\n\n##### INITIAL IMPORT ######");
                migrate::central_submit(&scratch, scratch.transfer(newrev, &Path::new(".")))
                    .expect("initial import failed");
                return 0;
            }
            else {
                println!(".\n\n###########################################");
                println!("##### INITIAL IMPORT already happened #####");
                println!("###########################################");
                return 1;
            }
        }
    }
    return 0;
}
