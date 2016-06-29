extern crate centralgithook;
extern crate git2;
extern crate clap;
extern crate env_logger;

use std::env;
use std::process::exit;
use std::path::Path;
use std::path::PathBuf;
use centralgithook::migrate;
use centralgithook::migrate::RepoHost;
use centralgithook::migrate::ReviewUploadResult;

const GERRIT_PORT: &'static str = "29418";
const AUTOMATION_USER: &'static str = "automation";
const CENTRAL_NAME: &'static str = "central";

struct Gerrit
{
    path: PathBuf,
    prefix: String,
}

impl Gerrit
{
    pub fn new() -> Self
    {
        let git_dir = env::var("GIT_DIR").expect("GIT_DIR not set");
        let mut p = Path::new(&git_dir);
        while !p.join(&format!("{}.git",CENTRAL_NAME)).exists() {
            p = p.parent().expect("can't find gerrit git root");
        }

        let root = p.to_path_buf();

        while !p.join("bin").join("gerrit.sh").exists() {
            p = p.parent().expect("can't find gerrit root");
        }

        let path = p.to_path_buf();
        let p = p.join("git");

        println!("Gerrit path: {:?}", path);
        println!("Gerrit root: {:?}", root);
        println!("Gerrit p: {:?}", p);

        let prefix = root.strip_prefix(&p).unwrap().to_path_buf();

        println!("Gerrit prefix: {:?}", prefix);

        Gerrit { path: path, prefix: format!("{}/",prefix.as_os_str().to_str().unwrap()) }
    }

}

impl migrate::RepoHost for Gerrit
{
    fn fetch_url(&self, module_path: &str) -> String
    {
        if let Some(root) = self.path.as_os_str().to_str() {
            format!("{}/git/{}{}.git", root, self.prefix, module_path)
        }
        else {
            self.remote_url(module_path)
        }
    }

    fn remote_url(&self, module_path: &str) -> String
    {
        format!("ssh://{}@gerrit-test-git:{}/{}{}.git",
                AUTOMATION_USER,
                GERRIT_PORT,
                self.prefix,
                module_path)
    }

    fn projects(&self) -> Vec<String>
    {
        let path = self.path.join("git").join(&self.prefix);
        migrate::find_repos(&path, &path, vec![])
    }

    fn central(&self) -> &str
    {
        CENTRAL_NAME
    }
}

fn main()
{
    ::std::env::set_var("RUST_LOG","centralgithook=debug");
    env_logger::init().expect("can't init logger");
    exit(main_ret());
}

fn main_ret() -> i32
{

    let args = clap::App::new("centralgithook")
        .arg(clap::Arg::with_name("branch").long("branch").takes_value(true))
        .arg(clap::Arg::with_name("change").long("change").takes_value(true))
        .arg(clap::Arg::with_name("change-owner").long("change-owner").takes_value(true))
        .arg(clap::Arg::with_name("change-url").long("change-url").takes_value(true))
        .arg(clap::Arg::with_name("commit").long("commit").takes_value(true))
        .arg(clap::Arg::with_name("head").long("head").takes_value(true))
        .arg(clap::Arg::with_name("newrev").long("newrev").takes_value(true))
        .arg(clap::Arg::with_name("oldrev").long("oldrev").takes_value(true))
        .arg(clap::Arg::with_name("project").long("project").takes_value(true))
        .arg(clap::Arg::with_name("refname").long("refname").takes_value(true))
        .arg(clap::Arg::with_name("submitter").long("submitter").takes_value(true))
        .arg(clap::Arg::with_name("topic").long("topic").takes_value(true))
        .arg(clap::Arg::with_name("uploader").long("uploader").takes_value(true))
        .get_matches();

    let commit = args.value_of("commit").unwrap_or("");
    let newrev = args.value_of("newrev").unwrap_or("");
    let oldrev = args.value_of("oldrev").unwrap_or("");
    let project = args.value_of("project").unwrap_or("");
    let refname = args.value_of("refname").unwrap_or("");


    let gerrit = Gerrit::new();

    let (_, project) = project.split_at(gerrit.prefix.len());
    println!("PJECT: {}", project);

    if let Some(hook) = env::args().nth(0) {

        // ref-update: fired after push
        // change-merged: fired after gerrit-submit
        let is_update = hook.ends_with("ref-update");
        let is_submit = hook.ends_with("change-merged");
        let is_project_created = hook.ends_with("project-created");

        let is_review = is_update && refname == "refs/for/master";
        let is_module = project != format!("{}{}", gerrit.prefix, gerrit.central());
        let is_initial = !is_module && oldrev == "0000000000000000000000000000000000000000";

        let uploader = args.value_of("uploader").unwrap_or("");
        if is_update && !is_review && !uploader.contains("Automation") {
            println!(".");
            // debug!("==== uploader: {}", uploader);
            println!("===================================================================");
            println!("================= Do not push directly to master! =================");
            println!("===================================================================");
            return 1;
        }

        let scratch_dir = gerrit.path.join("centralgithook_scratch");
        let scratch = migrate::Scratch::new(&scratch_dir, &gerrit);
        if is_submit {
            // submit to central
            migrate::central_submit(&scratch, scratch.transfer(commit, &Path::new(".")));
        }
        else if is_project_created {
            migrate::project_created(&scratch);
            println!("==== project_created");
        }
        else if is_review {
            // module was pushed, get changes to central
            match migrate::review_upload(&scratch,
                                         scratch.transfer(newrev, Path::new(".")),
                                         project) {
                ReviewUploadResult::RejectNoFF => {
                    println!(".");
                    println!("===================================================================");
                    println!("=========== Commit not based on master, rebase first! =============");
                    println!("===================================================================");
                }
                ReviewUploadResult::NoChanges => {}
                ReviewUploadResult::RejectMerge => {
                    println!(".");
                    println!("===================================================================");
                    println!("=================== Do not submit merge commits! ==================");
                    println!("===================================================================");
                }
                ReviewUploadResult::Uploaded(oid) => {
                    println!("================ Doing actual upload in central git ===============");

                    println!("{}", scratch.push(oid, gerrit.central(), "refs/for/master"));
                    println!("==== The review upload may have worked, even if it says error \
                              below. Look UP! ====")
                }
                ReviewUploadResult::Central => return 0,
            }

            // stop gerrit from allowing push to module directly
            return 1;
        }
        else if !is_module && is_update && !is_review {
            // direct push to master-branch of central
            if is_initial {
                println!(".\n\n##### INITIAL IMPORT ######");
                migrate::central_submit(&scratch, scratch.transfer(newrev, &Path::new(".")));
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
