extern crate git2;
extern crate clap;

use git2::*;
use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::exit;

const CENTRAL_NAME:    &'static str = "bsw/central";
const AUTOMATION_USER: &'static str = "automation";

// FIXME: hardcoded path
const TMP_REPO_DIR:    &'static str = "/home/christian/gerrit_testsite/tmp_automation_repo";

fn module_review_upload(project: &str, newrev: &str) -> Result<(),git2::Error> {

  let tmp_repo = try!(Repository::init_bare(TMP_REPO_DIR));
  let _ = in_tmp_repo("fetch --all");

  transfer_to_tmp(newrev);
  let parent_commit_obj = try!(tmp_repo.revparse_single(CENTRAL_NAME));
  let mut parent_commit_oid: git2::Oid = try!(parent_commit_obj
                                              .as_commit()
                                              .map(|x| x.id())
                                              .ok_or(git2::Error::from_str("could not get commit from obj")));

  let module_name: &str = try!(Path::new(project)
                               .components()
                               .last()
                               .map(|x| x.as_os_str().to_str().expect("not a valid unicode string"))
                               .ok_or(git2::Error::from_str("needs to be valid name")));
  let module_path = format!("remotes/modules/{}/master", module_name);
  let object = try!(tmp_repo.revparse_single(&module_path).map(|x| x.id()));
  let oldrev = format!("{}", object);

  {
    let old = try!(tmp_repo.revparse_single(&oldrev).map(|x| x.id()));
    let new = try!(tmp_repo.revparse_single(&newrev).map(|x| x.id()));

    if !try!(tmp_repo.graph_descendant_of(new,old)) {
      println!(".");
      println!("===========================================================");
      println!("======== Commit not based on master, rebase first! ========");
      println!("===========================================================");
      return Ok(());
    }
  }

  let walk = {
    let mut walk = try!(tmp_repo.revwalk());
    walk.set_sorting(git2::SORT_REVERSE | git2::SORT_TIME);
    try!(walk.push_range(&format!("{}..{}", oldrev, newrev)));
    walk
  };


  println!("===== project path: {}", project);
  println!("===== Apply commits from {} to {}", oldrev, newrev);

  for rev in walk {
    let newrev = format!("{}", try!(rev));
    if oldrev == newrev { continue; }
    println!("===== Apply commit {}", newrev);

    let module_commit_obj = try!(tmp_repo.revparse_single(&newrev));
    let module_commit = try!(module_commit_obj.as_commit()
                             .ok_or(git2::Error::from_str("object is not actually a commit")));
    let module_tree = try!(module_commit.tree());

    let parent_commit = try!(tmp_repo.find_commit(parent_commit_oid));

    let new_tree = {
      let master_tree: Tree = try!(parent_commit.tree());
      let new_tree_oid = try!(module_to_subfolder(
          Path::new(module_name),
          &module_tree,
          &master_tree));
      try!(tmp_repo.find_tree(new_tree_oid))
    };

    parent_commit_oid = try!(make_commit(&tmp_repo, &new_tree, module_commit, &vec!(&parent_commit)));

  }

  println!(""); println!("");
  println!("===================== Doing actual upload in central git ========================");
  let parent_commit = try!(tmp_repo.find_commit(parent_commit_oid));
  let x = try!(push_from_tmp(
      &tmp_repo,
      &parent_commit,
      CENTRAL_NAME,
      "refs/for/master"
      ));
  println!("{}", x);
  println!("==== The review upload may have worked, even if it says error below. Look UP! ====");
  Ok(())
}

fn central_submit(newrev: &str) {
  println!("central_submit");

  let module_names = get_module_names(newrev).unwrap();
  let tmp_repo = setup_tmp_repo(&module_names);
  transfer_to_tmp(newrev);

  let central_commit_obj = tmp_repo.revparse_single(newrev).unwrap();
  let central_commit = central_commit_obj.as_commit().unwrap();
  let central_tree = central_commit.tree().unwrap();
  let _ = tmp_repo.branch(CENTRAL_NAME,central_commit,true);

  for module_name in module_names {
    let module_master_commit_obj =
      tmp_repo.revparse_single(&format!("remotes/modules/{}/master",module_name)).unwrap();
    let module_master_commit =
      module_master_commit_obj.as_commit().unwrap();
    let _ = tmp_repo.branch(&format!("modules/{}",module_name),module_master_commit,true);

    let parents = vec!(module_master_commit);
    let old_tree_oid = module_master_commit.tree().unwrap().id();

    let module_path = { let mut p = PathBuf::new(); p.push("modules"); p.push(&module_name); p };

    let new_tree_oid = central_tree.get_path(&module_path).unwrap().id();

    if new_tree_oid != old_tree_oid {

      let new_tree = tmp_repo.find_tree(new_tree_oid).unwrap();

      let module_commit = make_commit(&tmp_repo, &new_tree, central_commit, &parents);
      let x = push_from_tmp(
        &tmp_repo,
        &tmp_repo.find_commit(module_commit.unwrap()).unwrap(),
        &format!("bsw/modules/{}",module_name),
        "master"
        ).unwrap();
      println!("{}", x);
    }
  }
}

fn transfer_to_tmp(rev: &str) {
  Command::new("git")
    .arg("branch").arg("-f").arg("tmp").arg(rev)
    .output().expect("failed to call git");

  Command::new("git")
    .arg("push").arg("--force").arg(TMP_REPO_DIR).arg("tmp")
    .output().expect("failed to call git");

  Command::new("git")
    .arg("branch").arg("-D").arg("tmp")
    .output().expect("failed to call git");
}

fn in_tmp_repo(cmd: &str) -> Result<String, git2::Error> {
  let args: Vec<&str> = cmd.split(" ").collect();
  match Command::new("git")
    .env("GIT_DIR",TMP_REPO_DIR)
    .args(&args).output().map(|output|
                              format!("{}", String::from_utf8_lossy(&output.stderr))) {
      Ok(value) => Ok(value),
      Err(_) => Err(git2::Error::from_str("could not fire git command")),
    }
}

fn setup_tmp_repo(modules: &Vec<String>) -> Repository {
  let repo = Repository::init_bare(TMP_REPO_DIR).unwrap();

  if !repo.find_remote("central_repo").is_ok() {
    let _ = repo.remote("central_repo",
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

    println!("create-project: {}", String::from_utf8_lossy(&output.stderr));

    let remote_url = format!("ssh://{}@gerrit-test-git:29418/bsw/modules/{}.git",
                             AUTOMATION_USER,
                             module
                            );

    let remote_name = format!("modules/{}",module);
    if !repo.find_remote(&remote_name).is_ok() {
      let _ = repo.remote(&remote_name, &remote_url);
    }
  }

  let _ = in_tmp_repo("fetch --all");

  return repo;
}

fn module_to_subfolder(path: &Path, module_tree: &Tree, master_tree: &Tree) -> Result<Oid, git2::Error> {
  let mpath = Path::new("modules");
  let modules_oid = try!(master_tree.get_path(mpath).map(|x| x.id()));
  let tmp_repo = try!(Repository::init_bare(TMP_REPO_DIR));

  let modules_tree = try!(tmp_repo.find_tree(modules_oid));
  let mut mbuilder = try!(tmp_repo.treebuilder(Some(&modules_tree)));
  mbuilder.insert(path, module_tree.id(), 0o0040000).expect("mbuilder insert failed"); // GIT_FILEMODE_TREE
  let mtree = try!(mbuilder.write());

  let mut builder = try!(tmp_repo.treebuilder(Some(master_tree)));
  builder.insert(mpath, mtree, 0o0040000).expect("builder insert failed"); // GIT_FILEMODE_TREE
  let r = try!(builder.write());
  println!("module_to_subfolder {}", r);
  Ok(r)
}

fn get_module_names(rev: &str) -> Result<Vec<String>, git2::Error> {
  let central_repo = try!(Repository::open("."));

  let object = try!(central_repo.revparse_single(rev));
  let commit = try!(object
                    .as_commit()
                    .ok_or(git2::Error::from_str("could not get commit from obj")));
  let tree: git2::Tree = try!(commit.tree());

  let modules_o = try!(tree.get_path(&Path::new("modules")).unwrap()
                   .to_object(&central_repo));
  let modules = try!(modules_o.as_tree()
                    .ok_or(git2::Error::from_str("could not get tree from path")));

  let mut names = Vec::<String>::new();
  for module in modules.iter() {
    names.push(module.name().unwrap().to_string());
  }
  Ok(names)
}

fn push_from_tmp(tmp_repo: &Repository,
                 commit: &Commit,
                 repo: &str ,to: &str)
  -> Result<String, git2::Error> {
    let _ = tmp_repo.set_head_detached(commit.id());
    in_tmp_repo(
      &format!("push ssh://{}@gerrit-test-git:29418/{}.git HEAD:{}",
               AUTOMATION_USER,
               repo,
               to
              )
      )
  }

fn make_commit(repo: &Repository, tree: &Tree, base: &Commit, parents: &[&Commit]) -> Result<Oid, git2::Error> {
  if parents.len() != 0 {
    let _ = repo.set_head_detached(parents[0].id());
  }
  repo.commit(
    Some("HEAD"),
    &base.author(),
    &base.committer(),
    &base.message().unwrap_or("no message"),
    tree,
    parents
    )
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
  let project = args.value_of("project").unwrap_or("");
  let refname = args.value_of("refname").unwrap_or("");
  let commit = args.value_of("commit").unwrap_or("");


  if let Some(hook) = env::args().nth(0) {
    let is_review = refname == "refs/for/master";
    let is_module = project != CENTRAL_NAME;
    let is_update = hook.ends_with("ref-update");
    let is_submit = hook.ends_with("change-merged");

    // // TODO
    // let uploader = args.value_of("uploader").unwrap_or("");
    // if !is_review && !uploader.contains("Automation") {
    //   println!("only push to refs/for/master");
    //   return 1;
    // }

    if is_submit { central_submit(commit); }
    else if !is_module && is_update && !is_review { central_submit(newrev); }
    else if is_module && is_update && is_review { let _ = module_review_upload(project,newrev); return 1; }
  }

  return 0;
}


