extern crate git2;

use git2::*;
use std::process::Command;
use std::path::Path;
use std::path::PathBuf;

// FIXME: hardcoded path
const TMP_REPO_DIR: &'static str = "/home/christian/gerrit_testsite/tmp_automation_repo";

pub fn module_review_upload(project: &str,
                            newrev: &str,
                            remote_module_url: &Fn(&str) -> String,
                            central_name: &str)
                            -> Result<(), git2::Error> {

    let tmp_repo = try!(Repository::init_bare(TMP_REPO_DIR));
    try!(in_tmp_repo("fetch --all"));

    transfer_to_tmp(newrev);
    let mut parent_commit_oid: git2::Oid = try!(try!(tmp_repo.revparse_single(central_name))
        .as_commit()
        .map(|x| x.id())
        .ok_or(git2::Error::from_str("could not get commit from obj")));

    let module_name: &str = try!(Path::new(project)
        .components()
        .last()
        .map(|x| x.as_os_str().to_str().expect("not a valid unicode string"))
        .ok_or(git2::Error::from_str("needs to be valid name")));

    let object = try!(tmp_repo.revparse_single(&remote_ref_name(&module_name))
        .map(|x| x.id()));
    let oldrev = format!("{}", object);

    {
        let old = try!(tmp_repo.revparse_single(&oldrev).map(|x| x.id()));
        let new = try!(tmp_repo.revparse_single(&newrev).map(|x| x.id()));

        if !try!(tmp_repo.graph_descendant_of(new, old)) {
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
        if oldrev == newrev {
            continue;
        }
        println!("===== Apply commit {}", newrev);

        let module_commit_obj = try!(tmp_repo.revparse_single(&newrev));
        let module_commit = try!(module_commit_obj.as_commit()
            .ok_or(git2::Error::from_str("object is not actually a commit")));
        let module_tree = try!(module_commit.tree());

        let parent_commit = try!(tmp_repo.find_commit(parent_commit_oid));

        let new_tree = {
            let master_tree: Tree = try!(parent_commit.tree());
            let new_tree_oid =
                try!(module_to_subfolder(Path::new(module_name), &module_tree, &master_tree));
            try!(tmp_repo.find_tree(new_tree_oid))
        };

        parent_commit_oid =
            try!(make_commit(&tmp_repo, &new_tree, module_commit, &vec![&parent_commit]));
    }

    println!("");
    println!("");
    println!("===================== Doing actual upload in central git ========================");
    let x = try!(push_from_tmp(&tmp_repo,
                               &try!(tmp_repo.find_commit(parent_commit_oid)),
                               central_name,
                               "refs/for/master",
                               &remote_module_url));
    println!("{}", x);
    println!("==== The review upload may have worked, even if it says error below. Look UP! ====");
    Ok(())
}

fn remote_ref_name(module_name: &str) -> String {
    format!("remotes/modules/{}/master", &module_name)
}

pub fn central_submit(remote_addr: &str,
                      newrev: &str,
                      remote_module_url: &Fn(&str) -> String,
                      check: &Fn(&str) -> Result<(), git2::Error>,
                      module_path_prefix: &str,
                      central_name: &str)
                      -> Result<(), git2::Error> {
    println!("central_submit");

    let module_names = try!(get_module_names(newrev));

    let tmp_repo = try!(setup_tmp_repo(&remote_addr,
                                       &module_names,
                                       &check,
                                       &remote_module_url,
                                       module_path_prefix));
    transfer_to_tmp(newrev);

    let central_commit_obj = try!(tmp_repo.revparse_single(newrev));
    let central_commit = try!(central_commit_obj.as_commit()
        .ok_or(git2::Error::from_str("could not get commit from obj")));
    let central_tree = try!(central_commit.tree());
    // create central_name branch in tmp_repo and point to the central_commit
    // is identical to master in central
    // marker for other hooks
    try!(tmp_repo.branch(central_name, central_commit, true));

    for module_name in module_names {
        // get master branches from module
        let module_master_commit_obj =
            try!(tmp_repo.revparse_single(&remote_ref_name(&module_name)));
        let module_master_commit = try!(module_master_commit_obj.as_commit()
            .ok_or(git2::Error::from_str("could not get commit from obj")));
        // maybe not needed: branch
        try!(tmp_repo.branch(&format!("modules/{}", module_name),
                             module_master_commit,
                             true));

        let parents = vec![module_master_commit];

        // get path for module in central repository
        let module_path = {
            let mut p = PathBuf::new();
            p.push("modules");
            p.push(&module_name);
            p
        };

        // new tree is sub-tree of complete central tree
        let new_tree_oid = try!(central_tree.get_path(&module_path)).id();
        let old_tree_oid = try!(module_master_commit.tree()).id();

        // if sha1's are equal the content is equal
        if new_tree_oid != old_tree_oid {
            // need to update module git

            let new_tree = try!(tmp_repo.find_tree(new_tree_oid));

            let module_commit = try!(make_commit(&tmp_repo, &new_tree, central_commit, &parents));
            // do the push to the module git
            let x = try!(push_from_tmp(&tmp_repo,
                                       &try!(tmp_repo.find_commit(module_commit)),
                                       &format!("{}/{}", module_path_prefix, module_name),
                                       "master",
                                       &remote_module_url));
            println!("{}", x);
        }
    }
    Ok(())
}

// force push of the new revision-object to temp repo
fn transfer_to_tmp(rev: &str) {
    // create tmp branch
    Command::new("git")
        .arg("branch")
        .arg("-f")
        .arg("tmp")
        .arg(rev)
        .output()
        .expect("failed to call git");

    // force push
    Command::new("git")
        .arg("push")
        .arg("--force")
        .arg(TMP_REPO_DIR)
        .arg("tmp")
        .output()
        .expect("failed to call git");

    // delete tmp branch
    Command::new("git")
        .arg("branch")
        .arg("-D")
        .arg("tmp")
        .output()
        .expect("failed to call git");
}

fn in_tmp_repo(cmd: &str) -> Result<String, git2::Error> {
    let args: Vec<&str> = cmd.split(" ").collect();
    match Command::new("git")
        .env("GIT_DIR", TMP_REPO_DIR)
        .args(&args)
        .output()
        .map(|output| format!("{}", String::from_utf8_lossy(&output.stderr))) {
        Ok(value) => Ok(value),
        Err(_) => Err(git2::Error::from_str("could not fire git command")),
    }
}

fn setup_tmp_repo(remote_addr: &str,
                  modules: &Vec<String>,
                  check: &Fn(&str) -> Result<(), git2::Error>,
                  remote_module_url: &Fn(&str) -> String,
                  module_path_prefix: &str)
                  -> Result<Repository, git2::Error> {
    let repo = try!(Repository::init_bare(TMP_REPO_DIR));

    // point remote to central
    if !repo.find_remote("central_repo").is_ok() {
        try!(repo.remote("central_repo", &remote_addr));
    }

    // create remote for each module
    for module in modules.iter() {
        try!(check(module));

        // create remote for each module
        let remote_name = format!("modules/{}", module);
        if !repo.find_remote(&remote_name).is_ok() {
            try!(repo.remote(&remote_name,
                             &remote_module_url(&format!("{}/{}.git",
                                                         module_path_prefix,
                                                         module))));
        }
    }

    // fetch all branches from remotes
    try!(in_tmp_repo("fetch --all"));

    Ok(repo)
}

fn module_to_subfolder(path: &Path,
                       module_tree: &Tree,
                       master_tree: &Tree)
                       -> Result<Oid, git2::Error> {
    let mpath = Path::new("modules");
    let modules_oid = try!(master_tree.get_path(mpath).map(|x| x.id()));
    let tmp_repo = try!(Repository::init_bare(TMP_REPO_DIR));

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

fn get_module_names(rev: &str) -> Result<Vec<String>, git2::Error> {
    let central_repo = try!(Repository::open("."));

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
    Ok(names)
}

fn push_from_tmp(tmp_repo: &Repository,
                 commit: &Commit,
                 repo: &str,
                 to: &str,
                 remote_module_url: &Fn(&str) -> String)
                 -> Result<String, git2::Error> {
    try!(tmp_repo.set_head_detached(commit.id()));

    let remote_url = &remote_module_url(&format!("{}.git", repo));
    in_tmp_repo(&format!("push {} HEAD:{}", remote_url, to))
}

// takes everything from base except it's tree and replaces it with the tree given
fn make_commit(repo: &Repository,
               tree: &Tree,
               base: &Commit,
               parents: &[&Commit])
               -> Result<Oid, git2::Error> {
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

#[cfg(test)]
mod tests {
    extern crate tempdir;
    extern crate git2;
    use super::central_submit;
    use self::tempdir::TempDir;
    use std::path::{Path, PathBuf};
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_commit_to_central() {
        let td = TempDir::new("play").expect("folder play should be created");
        let repo = git2::Repository::init(td.path()).expect("init should succeed");

        let pb = Path::new(td.path());
        // Print a message to stdout like "git init" does
        let path = repo.workdir().unwrap().to_path_buf();
        println!("Initialized empty Git repository in {}", path.display());


        let file_name = "foo.txt";
        let foo_file = pb.join(file_name);
        create_dummy_file(&foo_file);
        let oid = commit_file(&repo, &Path::new(file_name), &[]);

        let parent_commit = repo.find_commit(oid).expect("find parent_commit");

        let file_name = "bar.txt";
        let bar_file = pb.join(file_name);
        create_dummy_file(&bar_file);
        commit_file(&repo, &Path::new(file_name), &vec![&parent_commit]);
        assert!(true);
    }

    fn commit_file(repo: &git2::Repository, file: &Path, parents: &[&git2::Commit]) -> git2::Oid {
        let mut index = repo.index().expect("get index of repo");
        index.add_path(file).expect("file should be added");
        index.write().expect("write index");
        let tree_id = index.write_tree().expect("got tree_id");
        let tree = repo.find_tree(tree_id).expect("got tree");
        let sig = git2::Signature::now("foo", "bar").expect("created signature");
        repo.commit(Some("HEAD"), &sig, &sig, "commit A", &tree, &parents)
            .expect("commit to repo")
    }

    fn create_dummy_file(f: &PathBuf) {
        let mut file = File::create(&f.as_path()).expect("create file");
        file.write_all("test content".as_bytes()).expect("write to file");
    }
}
