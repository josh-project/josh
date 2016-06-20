extern crate git2;

use git2::*;
use std::process::Command;
use std::path::Path;
use std::path::PathBuf;

pub fn module_review_upload(project: &str,
                            project_path: &Path,
                            scratch_repo_path: &Path,
                            newrev: &str,
                            central_name: &str,
                            central_git_remote: &str) -> Result<(), git2::Error> {
    println!("in module_review_upload for project {}", &project);
    let scratch_repo = try!(Repository::init_bare(&scratch_repo_path));
    try!(in_tmp_repo(&scratch_repo, "fetch --all"));

    let scratch_repo_path = get_repo_path(&scratch_repo);

    transfer_to_scratch(newrev,
                        &project_path.to_path_buf(),
                        &scratch_repo_path.to_path_buf());
    let mut parent_commit_oid: git2::Oid = try!(try!(scratch_repo.revparse_single(central_name))
                                                .as_commit()
                                                .map(|x| x.id())
                                                .ok_or(git2::Error::from_str("could not get commit from obj")));

    let module_name: &str = try!(Path::new(project)
                                 .components()
                                 .last()
                                 .map(|x| x.as_os_str().to_str().expect("not a valid unicode string"))
                                 .ok_or(git2::Error::from_str("needs to be valid name")));

    let object = try!(scratch_repo.revparse_single(&remote_ref_name(&module_name))
                      .map(|x| x.id()));
    let oldrev = format!("{}", object);

    {
        let old = try!(scratch_repo.revparse_single(&oldrev).map(|x| x.id()));
        let new = try!(scratch_repo.revparse_single(&newrev).map(|x| x.id()));

        if !try!(scratch_repo.graph_descendant_of(new, old)) {
            println!(".");
            println!("===========================================================");
            println!("======== Commit not based on master, rebase first! ========");
            println!("===========================================================");
            return Ok(());
        }
    }

    let walk = {
        let mut walk = try!(scratch_repo.revwalk());
        walk.set_sorting(git2::SORT_REVERSE | git2::SORT_TIME);
        try!(walk.push_range(&format!("{}..{}", oldrev, newrev)));
        walk
    };

    println!("===== project path: {}", project);
    println!("===== Apply commits from {} to {}", oldrev, newrev);

    for rev in walk {
        let newrev = format!("{}", try!(rev));
        if oldrev == newrev {
            continue;
        }
        println!("===== Apply commit {}", newrev);

        let module_commit_obj = try!(scratch_repo.revparse_single(&newrev));
        let module_commit = try!(module_commit_obj.as_commit()
                                 .ok_or(git2::Error::from_str("object is not actually a commit")));
        let module_tree = try!(module_commit.tree());

        let parent_commit = try!(scratch_repo.find_commit(parent_commit_oid));

        let new_tree = {
            let master_tree: Tree = try!(parent_commit.tree());
            let new_tree_oid = try!(module_to_subfolder(&scratch_repo,
                                                        Path::new(module_name),
                                                        &module_tree,
                                                        &master_tree));
            try!(scratch_repo.find_tree(new_tree_oid))
        };

        parent_commit_oid =
            try!(make_commit(&scratch_repo, &new_tree, module_commit, &vec![&parent_commit]));
    }

    println!("");
    println!("");
    println!("===================== Doing actual upload in central git ========================");

    let commit = &try!(scratch_repo.find_commit(parent_commit_oid));
    try!(scratch_repo.set_head_detached(commit.id()));
    try!(in_tmp_repo(&scratch_repo, &format!("push {} HEAD:{}", &central_git_remote, "refs/for/master")));
    println!("==== The review upload may have worked, even if it says error below. Look UP! ====");
    Ok(())
}

pub fn central_submit(remote_addr: &str,
                      newrev: &str,//sha1 of refered commit
                      remote_url: &Fn(&str) -> String,
                      check: &Fn(&str) -> Result<(), git2::Error>, // create git repo (if not existing)
                      central_name: &str,
                      central_repo_path: &Path,
                      scratch_dir: &Path) -> Result<(), git2::Error> {
    println!(" ---> central_submit (remote addr:{}, sha1 of commit: {})", &remote_addr, &newrev);

    let central_repo = git2::Repository::open(&central_repo_path)
        .expect(&format!("central repo should exist at {:?}", &central_repo_path));
    let module_names = try!(get_module_names(&central_repo, newrev));

    println!("    ########### SCRATCH: create scratch repo ########### ");
    let central_remote_url = remote_url("central");
    let scratch_repo = try!(setup_scratch_repo(&scratch_dir,
                                               &central_remote_url,
                                               &module_names,
                                               &check,
                                               &remote_url));

    let scratch_repo_path = get_repo_path(&scratch_repo);
    transfer_to_scratch(newrev,
                        &get_repo_path(&central_repo).to_path_buf(),
                        &scratch_repo_path.to_path_buf());

    let central_commit_obj = try!(scratch_repo.revparse_single(newrev));
    let central_commit = try!(central_commit_obj.as_commit()
                              .ok_or(git2::Error::from_str("could not get commit from obj")));
    let central_tree = try!(central_commit.tree());
    println!("    ########### create {} branch in scratch_repo and point to the central_commit ########### ", &central_name);
    // create central_name branch in scratch_repo and point to the central_commit
    // is identical to master in central
    // marker for other hooks
    try!(scratch_repo.branch(central_name, central_commit, true));
    let mut _p = scratch_repo_path.to_path_buf();
    call_command("cat", &["config"], Some(&_p), None);
    call_command("git", &["branch","--all"], Some(&_p), None);

    for module_name in module_names {
        println!(" ####### prepare commit from scratch_repo to module {}", &module_name);
        let module_master_commit_obj =
            try!(scratch_repo.revparse_single(&remote_ref_name(&module_name)));
        let module_master_commit = try!(module_master_commit_obj.as_commit()
                                        .ok_or(git2::Error::from_str("could not get commit from obj")));
        // maybe not needed: branch
        try!(scratch_repo.branch(&format!("modules/{}", module_name), module_master_commit, true));

        let parents = vec![module_master_commit];

        let module_path = {
            let mut p = PathBuf::new();
            p.push("modules");
            p.push(&module_name);
            p
        };
        println!("\tpath for module in central repository... => {:?}", module_path);

        // new tree is sub-tree of complete central tree
        let new_tree_oid = try!(central_tree.get_path(&module_path)).id();
        let old_tree_oid = try!(module_master_commit.tree()).id();

        // if sha1's are equal the content is equal
        if new_tree_oid != old_tree_oid {
            println!("\tmodule repository for {} is behind, => updating", &module_name);

            let new_tree = try!(scratch_repo.find_tree(new_tree_oid));

            let module_commit = try!(make_commit(&scratch_repo, &new_tree, central_commit, &parents));
            // do the push to the module git
            let commit = &try!(scratch_repo.find_commit(module_commit));
            try!(scratch_repo.set_head_detached(commit.id()));

            let remote_url = &remote_url(&module_name);
            try!(in_tmp_repo(&scratch_repo, &format!("push {} HEAD:{}", remote_url, "master")));
        }
    }
    Ok(())
}

fn _oid_to_sha1(oid: &[u8]) -> String {
    oid.iter().fold(Vec::new(), |mut acc, x| {
        acc.push(format!("{0:>02x}", x)); acc
    }).concat()
}

fn call_command(command: &str, args: &[&str], mpath: Option<&PathBuf>, menv: Option<(&str, &str)>)
{
    let mut c = Command::new(&command);
    c.args(args);
    println!("call {:?} (in {:?})", c, mpath);
    if let Some(path) = mpath {
        c.current_dir(path);
    }
    if let Some((k, v)) = menv {
        c.env(k, v);
    }
    let output = c
        .output()
        .unwrap_or_else(|e| panic!("failed to execute process: {}", e));
    println!("{}{}",
             String::from_utf8_lossy(&output.stdout),
             String::from_utf8_lossy(&output.stderr));
}

fn show_status(path: &PathBuf) {
    call_command("git", &["status"], Some(&path), None);
    call_command("git",
                 &["log",
                 "--graph",
                 "--pretty=format:'%Cred%h%Creset -%C(yellow)%d%Creset %s %Cgreen(%cr) \
                 %C(bold blue)<%an>%Creset'",
                 "--abbrev-commit",
                 "--date=relative"],
                 Some(&path), None);

}

fn remote_ref_name(module_name: &str) -> String {
    format!("remotes/modules/{}/master", &module_name)
}

// force push of the new revision-object to temp repo
fn transfer_to_scratch(rev: &str, source_path: &PathBuf, target_path: &PathBuf) {
    println!("---> transfer_to_scratch in {}", source_path.display());
    //" create tmp branch
    call_command("git", &["branch", "-f", "tmp", rev], Some(source_path), None);
    // force push
    call_command("git", &["push", "--force", &target_path.to_string_lossy(), "tmp"], Some(source_path), None);
    // delete tmp branch
    call_command("git", &["branch", "-D", "tmp"], Some(source_path), None);
    println!("<--- transfer_to_scratch done...");
}

fn in_tmp_repo(repo: &Repository, cmd: &str) -> Result<(), git2::Error> {
    println!("in_tmp_repo ({:?}): git {}", &repo.path(), &cmd);
    // call_command("git", &["branch", "-D", "tmp"], None, Some("GIT_DIR", repo_path.as_os_str()));
    let args: Vec<&str> = cmd.split(" ").collect();
    let repo_path = get_repo_path(&repo);
    let mut c = Command::new("git");
    c.env("GIT_DIR", repo_path.as_os_str());
    c.args(&args);

    println!("call {:?} command in {:?}", c, repo_path);
    let output = c
        .output()
        .unwrap_or_else(|e| panic!("failed to execute process: {}", e));
    println!("{}{}",
             String::from_utf8_lossy(&output.stdout),
             String::from_utf8_lossy(&output.stderr));
    Ok(())
        // Err(_) => Err(git2::Error::from_str("could not fire git command")),
}

fn setup_scratch_repo(scratch_dir: &Path,
                      central_remote_url: &str,
                      modules: &Vec<String>,
                      check: &Fn(&str) -> Result<(), git2::Error>,
                      remote_url: &Fn(&str) -> String) -> Result<Repository, git2::Error> {

    println!(" ####### setup scratch repo for remote: {}\n #######\tlocation: {:?}", &central_remote_url, &scratch_dir);
    let scratch_repo = try!(Repository::init_bare(scratch_dir));

    // point remote to central
    if !scratch_repo.find_remote("central").is_ok() {
        try!(scratch_repo.remote("central", &central_remote_url));
    }

    // create remote for each module
    for module in modules.iter() {
        try!(check(&module));

        // create remote for each module
        let remote_name = format!("modules/{}", module);
        let module_remote_url = remote_url(&module);
        println!("  create remote (remote_name:{}, remote_url:{})", &remote_name, &module_remote_url);
        if !scratch_repo.find_remote(&remote_name).is_ok() {
            try!(scratch_repo.remote(&remote_name, &module_remote_url));
        }
    }

    // fetch all branches from remotes
    // FIXME remote branches missing
    // try!(in_tmp_repo(&scratch_repo, "fetch --all"));
    call_command("git", &["fetch", "--all"], None,
                 Some(("GIT_DIR", &scratch_dir.to_path_buf().to_str().expect("path as string"))));
    println!("  fetched all branches from remotes...");
    call_command("git", &["branch","--all"], Some(&scratch_dir.to_path_buf()), None);

    Ok(scratch_repo)
}

fn module_to_subfolder(tmp_repo: &Repository,
                       path: &Path,
                       module_tree: &Tree,
                       master_tree: &Tree)
    -> Result<Oid, git2::Error> {
        let mpath = Path::new("modules");
        let modules_oid = try!(master_tree.get_path(mpath).map(|x| x.id()));

        let modules_tree = try!(tmp_repo.find_tree(modules_oid));
        let mut mbuilder = try!(tmp_repo.treebuilder(Some(&modules_tree)));
        try!(mbuilder.insert(path, module_tree.id(), 0o0040000)); // GIT_FILEMODE_TREE
        let mtree = try!(mbuilder.write());

        let mut builder = try!(tmp_repo.treebuilder(Some(master_tree)));
        try!(builder.insert(mpath, mtree, 0o0040000)); // GIT_FILEMODE_TREE
        let r = try!(builder.write());
        println!("module_to_subfolder {}", r);
        Ok(r)
    }

fn get_module_names(central_repo: &Repository, rev: &str) -> Result<Vec<String>, git2::Error> {

    show_status(&get_repo_path(&central_repo).to_path_buf());

    let object = try!(central_repo.revparse_single(rev));
    let commit = try!(object.as_commit()
                      .ok_or(git2::Error::from_str("could not get commit from obj")));
    let tree: git2::Tree = try!(commit.tree());

    let tree_object = try!(tree.get_path(&Path::new("modules")));
    let modules_o = try!(tree_object.to_object(&central_repo));
    let modules = try!(modules_o.as_tree()
                       .ok_or(git2::Error::from_str("could not get tree from path")));

    let mut names = Vec::<String>::new();
    for module in modules.iter() {
        names.push(try!(module.name()
                        .ok_or(git2::Error::from_str("could not get name for module")))
                   .to_string());
    }
    println!("<--- get_module_names returns: {:?}", names);
    Ok(names)
}

// takes everything from base except it's tree and replaces it with the tree given
fn make_commit(repo: &Repository,
               tree: &Tree,
               base: &Commit,
               parents: &[&Commit]) -> Result<Oid, git2::Error> {
    if parents.len() != 0 {
        try!(repo.set_head_detached(parents[0].id()));
    }
    repo.commit(Some("HEAD"),
    &base.author(),
    &base.committer(),
    &base.message().unwrap_or("no message"),
    tree,
    parents)
}

fn get_repo_path(repo: &Repository) -> &Path {
    if repo.is_bare() {
        return repo.path();
    }
    return repo.workdir().expect("get workdir from repo");
}

#[cfg(test)]
mod tests {
    extern crate tempdir;
    extern crate git2;
    use super::central_submit;
    use super::call_command;
    use super::_oid_to_sha1;
    use super::get_repo_path;
    use self::tempdir::TempDir;
    use std::path::{Path, PathBuf};
    use std::fs::File;
    use std::fs;
    use std::str;
    use std::io::Write;
    use std::io::Read;

    #[test]
    fn test_commit_to_central() {
        let td = TempDir::new("play").expect("folder play should be created");
        let temp_path = td.path();
        let central_repo_dir = temp_path.join("central");
        let central_repo_path = &Path::new(&central_repo_dir);
        println!("    ");
        println!("    ########### SETUP: create central repository ########### ");
        let central_repo = create_repository(&central_repo_dir);
        commit_files(&central_repo,
                     &central_repo_path,
                     &vec!["modules/moduleA/added_in_central.txt",
                     "modules/moduleB/added_in_central.txt",
                     "modules/moduleC/added_in_central.txt"]);

        println!("    ########### SETUP: create module repositories ########### ");

        let module_names = vec!["moduleA", "moduleB", "moduleC"];
        let _module_repos: Vec<git2::Repository> = module_names
            .iter()
            .map(|m| {
                let x = temp_path.join(&m);
                let repo = create_repository(&x);
                // point remote to central
                let ref_name = "modules/".to_string() + &m;
                central_repo.remote(&ref_name, &m).expect("remote as to be added");
                commit_files(&repo, &Path::new(&x), &vec!["test/a.txt", "b.txt", "c.txt"]);
                call_command("git", &["checkout","-b", "not_needed"], Some(&x), None);
                repo
            })
        .collect();

        let remote_url = move |module_path: &str| -> String {
            temp_path.join(&module_path).to_string_lossy().to_string()
        };
        let make_sure_module_exists = move |module_name: &str| -> Result<(), git2::Error> {
            let repo_dir = temp_path.join(&module_name);
            if git2::Repository::open(&repo_dir).is_err() {
                //TODO only create repository if this is intended
                println!("+++++++++ {:?} did not exist => creating it", repo_dir);
                let _ = create_repository(&repo_dir);
            }
            Ok(())
        };
        let fp = central_repo_dir.clone().join(".git").join("refs").join("heads").join("master");
        let mut f = File::open(&fp).expect("open file master");
        let mut s = String::new();
        f.read_to_string(&mut s).expect("read file shoud work");
        println!("master file in {:?}: {}", central_repo_dir, s);

        let h = central_repo.head().expect("get head of central repo");
        let t = h.target().expect("get oid of head reference");
        let central_repo_head_oid = t.as_bytes();

        let central_head_sha1 = _oid_to_sha1(&central_repo_head_oid);

        println!("    ########### START: calling central_submit ########### ");
        let td = TempDir::new("scratch").expect("folder scratch should be created");
        let scratch_dir = td.path();

        let _t = temp_path.join("moduleA").join("added_in_central.txt");
        println!("file in question:{:?}", _t);
        assert!(!temp_path.join("moduleA").join("added_in_central.txt").exists());
        central_submit("central",
                       &central_head_sha1[..],
                       &remote_url,
                       &make_sure_module_exists,
                       "central",
                       &central_repo_path,
                       &scratch_dir).expect("call central_submit");
        for m in module_names {
            let x = temp_path.join(&m);
            call_command("git", &["checkout", "master"], Some(&x), None);
            assert!(temp_path.join(m).join("added_in_central.txt").exists());
        }
    }

    fn commit_file(repo: &git2::Repository, file: &Path, parents: &[&git2::Commit]) -> git2::Oid {
        let mut index = repo.index().expect("get index of repo");
        index.add_path(file).expect("file should be added");
        index.write().expect("write index");
        let tree_id = index.write_tree().expect("got tree_id");
        let tree = repo.find_tree(tree_id).expect("got tree");
        let sig = git2::Signature::now("foo", "bar").expect("created signature");
        repo.commit(Some("HEAD"),
        &sig,
        &sig,
        &format!("commit for {:?}", &file.as_os_str()),
        &tree,
        &parents)
            .expect("commit to repo")
    }

    fn commit_files(repo: &git2::Repository, pb: &Path, content: &Vec<&str>) {
        let mut parent_commit = None;
        for file_name in content {
            let foo_file = pb.join(file_name);
            create_dummy_file(&foo_file);
            let oid = match parent_commit {
                Some(parent) => commit_file(&repo, &Path::new(file_name), &[&parent]),
                None => commit_file(&repo, &Path::new(file_name), &[]),
            };
            parent_commit = repo.find_commit(oid).ok();
        }
    }

    fn create_repository(temp: &Path) -> git2::Repository {
        let repo = git2::Repository::init(temp).expect("init should succeed");

        let path = get_repo_path(&repo).to_path_buf();
        println!("Initialized empty Git repository in {}", path.display());
        repo
    }

    fn create_dummy_file(f: &PathBuf) {
        let parent_dir = f.as_path().parent().expect("need to get parent");
        fs::create_dir_all(parent_dir).expect("create directories");

        let mut file = File::create(&f.as_path()).expect("create file");
        file.write_all("test content".as_bytes()).expect("write to file");
    }
}
