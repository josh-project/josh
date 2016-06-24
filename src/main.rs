extern crate centralgithook;
extern crate git2;
extern crate clap;

use std::env;
use std::process::exit;
use std::path::Path;
use std::process::Command;
use centralgithook::migrate;

const GERRIT_PORT: &'static str = "29418";
const AUTOMATION_USER: &'static str = "automation";
// const MODULE_PATH_PREFIX: &'static str = "bsw";
const CENTRAL_NAME: &'static str = "central";

struct Gerrit { }

impl migrate::RepoHost for Gerrit {
    // create module project on gerrit (if not existing)
    fn create_project(&self, module: &str) -> Result<(), git2::Error> {
        match Command::new("ssh")
            .arg("-p").arg(GERRIT_PORT)
            .arg("gerrit-test-git")
            .arg("gerrit")
            .arg("create-project")
            .arg(module)
            // .arg(format!("{}/{}", MODULE_PATH_PREFIX, module))
            .output() {
                Ok(output) => {
                    println!("create-project: {}", String::from_utf8_lossy(&output.stderr));
                    Ok(())
                },
                Err(_) => Err(git2::Error::from_str("failed to create project")),
            }
    }

    fn remote_url(&self, module_path: &str) -> String {
        format!("ssh://{}@gerrit-test-git:{}/{}",
                AUTOMATION_USER,
                GERRIT_PORT,
                module_path)
    }

}

fn main() { exit(main_ret()); } fn main_ret() -> i32 {

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

  let gerrit = Gerrit{};


  // ref-update: fired after push
  // change-merged: fired after gerrit-submit
  if let Some(hook) = env::args().nth(0) {
    let is_review = refname == "refs/for/master";
    let is_module = project != CENTRAL_NAME;
    let is_update = hook.ends_with("ref-update");
    let is_submit = hook.ends_with("change-merged");
    let is_initial = oldrev == "0000000000000000000000000000000000000000";

    // // TODO
    // let uploader = args.value_of("uploader").unwrap_or("");
    // if !is_review && !uploader.contains("Automation") {
    //   println!("only push to refs/for/master");
    //   return 1;
    // }

    let scratch_dir = Path::new("/tmp/scratchme");
    let scratch = migrate::Scratch::new(&scratch_dir,&gerrit);
    if is_submit {
      // submit to central
      migrate::central_submit(commit,
                              CENTRAL_NAME,
                              &Path::new("."),
                              &scratch).unwrap();
    }
    else if is_module && is_update && is_review {
      // module was pushed, get changes to central
      migrate::module_review_upload(
        project,
        // Path::new(&env::var("GIT_DIR").expect("GIT_DIR needs to be set")),
        &scratch,
        newrev,
        CENTRAL_NAME,
        ).unwrap();
      // stop gerrit from allowing push to module directly
      return 1;
    }
    else if !is_module && is_update && !is_review {
      // direct push to master-branch of central
      if is_initial {
          println!("##### INITIAL IMPORT {} {} ######",oldrev,newrev);
          migrate::initial_import(newrev,
                                  CENTRAL_NAME,
                                  &scratch).unwrap();
          return 1;
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



