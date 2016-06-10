extern crate git2;
extern crate clap;
use git2::*;
use std::process::Command;
use std::process::exit;
use std::env;
use std::path::Path;

const CENTRAL_NAME:    &'static str = "bsw/central";
const AUTOMATION_USER: &'static str = "automation";
const TMP_REPO_DIR:    &'static str = "/home/christian/gerrit_testsite/tmp_automation_repo";

fn module_review_upload() { }

fn central_review_upload() { }
fn central_submit() { }
fn central_direct_push() { }

fn in_tmp_repo(cmd: &str) -> String {
  let args: Vec<&str> = cmd.split(" ").collect();
  let output = Command::new("git")
    .env("GIT_DIR",TMP_REPO_DIR)
    .args(&args).output().unwrap();
  return format!("{}", String::from_utf8_lossy(&output.stdout));
}

fn setup_tmp_repo(name: &str, modules: &Vec<String>) -> Repository {
  let repo = Repository::init_bare(name).unwrap();

  if !repo.find_remote("central_repo").is_ok() {
    repo.remote("central_repo", 
      &format!("ssh://{}@gerrit-test-git/{}.git",AUTOMATION_USER,CENTRAL_NAME)
    );
  }

  for module in modules.iter() {
    let output = Command::new("ssh")
      .arg("-p").arg("29418")
      .arg("gerrit-test-git")
        .arg("gerrit")
        .arg("create-project")
          .arg(format!("bsw/modules/{}",module))
          .arg("--empty-commit")
      .output()
      .expect("failed to create project");

    println!("create-project: {}", String::from_utf8_lossy(&output.stdout));

    let remote_url = format!("ssh://{}@gerrit-test-git:29418/bsw/modules/{}.git",
      AUTOMATION_USER,
      module
    );

    let remote_name = format!("modules/{}",module);
    if !repo.find_remote(&remote_name).is_ok() {
        repo.remote(&remote_name, &remote_url);
    }
  }

  in_tmp_repo("fetch --all");

  return repo;
}

fn module_to_subfolder(module: Tree, master: Tree) {
  let modules_oid = master.get_path(&Path::new("modules")).unwrap().id();
  let tmp_repo = Repository::init_bare(TMP_REPO_DIR).unwrap();

  let modules_tree = tmp_repo.find_tree(modules_oid).unwrap();
  // let mbuilder = TreeBuilder::new(modules_tree);
  // let mbuilder = tmp_repo.treebuilder(modules_tree);
}

fn get_module_names(rev: &str) -> Vec<String> {
  let central_repo = Repository::open(".").unwrap();

  let object = central_repo.revparse_single(rev).unwrap();
  let commit = object.as_commit().unwrap();
  let tree = commit.tree().unwrap();

  let modules_o = tree.get_path(&Path::new("modules")).unwrap()
                      .to_object(&central_repo).unwrap();
  let modules = modules_o.as_tree().unwrap();

  let mut names = Vec::<String>::new();
  for module in modules.iter() {
    names.push(module.name().unwrap().to_string());
  }
  return names;
}

fn main() {

  let args = clap::App::new("centralgithook")
      .arg(clap::Arg::with_name("newrev").long("newrev").takes_value(true))
      .arg(clap::Arg::with_name("project").long("project").takes_value(true))
      .arg(clap::Arg::with_name("refname").long("refname").takes_value(true))
      .arg(clap::Arg::with_name("uploader").long("uploader").takes_value(true))
      .arg(clap::Arg::with_name("commit").long("commit").takes_value(true))
      .get_matches();

  let newrev = args.value_of("newrev").unwrap_or(""); 
  let project = args.value_of("project").unwrap_or(""); 
  let refname = args.value_of("refname").unwrap_or(""); 
  let uploader = args.value_of("uploader").unwrap_or(""); 
  let commit = args.value_of("commit").unwrap_or(""); 


  if let Some(hook) = env::args().nth(0) {
    let is_review = refname == "refs/for/master";
    let is_automation = uploader.contains("Automation");
    let is_module = project != CENTRAL_NAME;
    let is_update = hook.ends_with("ref-update");
    let is_submit = hook.ends_with("change-merged");

    if !is_review && !is_automation {
      println!("only push to refs/for/master");
      exit(1);
    }

    if is_module && is_update && is_review { module_review_upload(); }
    if !is_module && is_update && is_review { central_review_upload(); }
    if !is_module && is_update && !is_review { central_direct_push(); }
    if !is_module && is_submit { central_submit(); }
  }

  let repo = match Repository::open(".") {
      Ok(repo) => repo,
      Err(e) => panic!("failed to init: {}", e),
  };
  println!("Hello, world!");
  exit(0);
}
