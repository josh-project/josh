extern crate git2;

use git2::*;
use std::process::Command;
use std::path::Path;

pub trait RepoHost  {
    fn remote_url(&self, &str) -> String;
    fn create_project(&self, &str) -> Result<(), Error>;

}

struct Scratch<'a> {
    repo: Repository,
    host: &'a RepoHost,
}

impl<'a> Scratch<'a> {
    pub fn new(path: &Path, host: &'a RepoHost) -> Scratch<'a> {
        Scratch {
            repo: Repository::init_bare(&path).expect("could not init scratch"),
            host: host,
        }
    }

    fn tracking(&self, module: &str, branch: &str) -> Object {
        println!("tracking_branch remotes/{}/{}",module,branch);
        self.repo.revparse_single(&format!("remotes/{}/{}",module,branch))
            .expect("no tracking branch")
    }

    fn call_git(self: &Scratch<'a>, cmd: &str) -> Result<(), Error> {
        println!("Scratch::call_git ({:?}): git {}", &self.repo.path(), &cmd);
        let args: Vec<&str> = cmd.split(" ").collect();
        let repo_path = get_repo_path(&self.repo);
        let mut c = Command::new("git");
        c.env("GIT_DIR", repo_path.as_os_str());
        c.args(&args);

        println!("call_git {:?} command in {:?}", c, repo_path);
        let output = c
            .output()
            .unwrap_or_else(|e| panic!("failed to execute process: {}", e));
        println!("{}{}",
                 String::from_utf8_lossy(&output.stdout),
                 String::from_utf8_lossy(&output.stderr));
        Ok(())
    }

    fn setup(&self,
             central: &str,
             modules: &Vec<String>)
        -> Result<(), Error> {

        println!(" ####### setup scratch repo for remote: {}\n #######\tlocation: {:?}",
            &self.host.remote_url(central),
            &self.repo.path()
        );

        // point remote to central
        if !self.repo.find_remote("central").is_ok() {
            try!(self.repo.remote("central", &self.host.remote_url(central)));
        }

        // create remote for each module
        for module in modules.iter() {
            try!(self.host.create_project(module));

            // create remote for each module
            let remote_name = format!("{}", module);
            let module_remote_url = self.host.remote_url(&module);
            println!("  create remote (remote_name:{}, remote_url:{})",
                &remote_name,
                &module_remote_url
            );
            if !self.repo.find_remote(&remote_name).is_ok() {
                try!(self.repo.remote(&remote_name, &module_remote_url));
            }
        }

        // fetch all branches from remotes
        // FIXME remote branches missing
        // try!(in_tmp_repo(&self.repo, "fetch --all"));
        try!(self.call_git("fetch --all"));
        println!("  fetched all branches from remotes...");
        Ok(())
    }

    // force push of the new revision-object to temp repo
    fn transfer(&self, rev: &str, source: &Path) {
        let target = get_repo_path(&self.repo);
        println!("---> transfer_to_scratch in {}", source.display());
        //" create tmp branch
        call_command("git", &["branch", "-f", "tmp", rev], Some(source));
        // force push
        call_command(
            "git", &["push", "--force", &target.to_string_lossy(), "tmp"], Some(source)
        );
        // delete tmp branch
        call_command("git", &["branch", "-D", "tmp"], Some(source));
        println!("<--- transfer_to_scratch done...");
    }

    fn subtree(&self, tree: &Tree, path: &Path) -> Tree {
        let oid = tree.get_path(path).map(|x| x.id()).expect("can't find subtree");
        return self.repo.find_tree(oid).expect("can't find oid");
    }

    fn module_to_subfolder(&self,
                           module_path: &Path,
                           module_tree: &Tree,
                           master_tree: &Tree)
        -> Result<Oid, Error>
    {
        assert!(module_path.components().count() == 2); // FIXME: drop this requirement
        let parent_path = module_path.parent().expect("module not in subdir");
        let module_name = module_path.file_name().expect("no module name");

        let modules_tree = {
            let mut builder =
                try!(self.repo.treebuilder(Some(&self.subtree(master_tree, parent_path))));
            try!(builder.insert(module_name, module_tree.id(), 0o0040000)); // GIT_FILEMODE_TREE
            try!(builder.write())
        };

        let full_tree = {
            let mut builder = try!(self.repo.treebuilder(Some(master_tree)));
            try!(builder.insert(parent_path, modules_tree, 0o0040000)); // GIT_FILEMODE_TREE
            try!(builder.write())
        };
        println!("module_to_subfolder {}", full_tree);
        Ok(full_tree)
    }
}

pub fn module_review_upload(module: &str,
                            scratch_path: &Path,
                            newrev: &str,
                            central: &str,
                            host: &RepoHost) -> Result<(), Error> {
    println!("in module_review_upload for module {}", &module);
    let scratch = Scratch::new(&scratch_path, host);
    try!(scratch.setup(central,&vec!(module.to_string())));
    try!(scratch.call_git("fetch --all"));

    scratch.transfer(newrev,Path::new("."));
    let mut parent_commit_oid: Oid = scratch.tracking(central, "master").id();

    let object = scratch.tracking(&module, "master").id();
    let oldrev = format!("{}", object);

    {
        let old = try!(scratch.repo.revparse_single(&oldrev).map(|x| x.id()));
        let new = try!(scratch.repo.revparse_single(&newrev).map(|x| x.id()));

        if !try!(scratch.repo.graph_descendant_of(new, old)) {
            println!(".");
            println!("===========================================================");
            println!("======== Commit not based on master, rebase first! ========");
            println!("===========================================================");
            return Ok(());
        }
    }

    let walk = {
        let mut walk = try!(scratch.repo.revwalk());
        walk.set_sorting(SORT_REVERSE | SORT_TIME);
        try!(walk.push_range(&format!("{}..{}", oldrev, newrev)));
        walk
    };

    println!("===== module path: {}", module);
    println!("===== Rewrite commits from {} to {}", oldrev, newrev);

    for rev in walk {
        let newrev = format!("{}", try!(rev));
        if oldrev == newrev {
            continue;
        }
        println!("===== Rewrite commit {}", newrev);

        let module_commit_obj = try!(scratch.repo.revparse_single(&newrev));
        let module_commit = try!(module_commit_obj.as_commit()
                                 .ok_or(Error::from_str("object is not actually a commit")));
        let module_tree = try!(module_commit.tree());

        let parent_commit = try!(scratch.repo.find_commit(parent_commit_oid));

        let new_tree = {
            let master_tree: Tree = try!(parent_commit.tree());
            let new_tree_oid = try!(
                scratch.module_to_subfolder(Path::new(module), &module_tree, &master_tree)
            );
            try!(scratch.repo.find_tree(new_tree_oid))
        };

        parent_commit_oid =
            try!(make_commit(&scratch.repo, &new_tree, module_commit, &vec![&parent_commit]));
    }

    println!("");
    println!("");
    println!("===================== Doing actual upload in central git ========================");

    let commit = &try!(scratch.repo.find_commit(parent_commit_oid));
    try!(scratch.repo.set_head_detached(commit.id()));
    try!(scratch.call_git(
        &format!("push {} HEAD:{}", &host.remote_url(central), "refs/for/master")
    ));
    println!("==== The review upload may have worked, even if it says error below. Look UP! ====");
    Ok(())
}

pub fn central_submit(newrev: &str,//sha1 of refered commit
                      host: &RepoHost,
                      central: &str,
                      repo_path: &Path,
                      scratch_dir: &Path) -> Result<(), Error> {
    println!(" ---> central_submit (remote addr:{}, sha1 of commit: {})",
        &host.remote_url(central),
        &newrev
    );

    let central_repo = Repository::open(&repo_path)
        .expect(&format!("central repo should exist at {:?}", &repo_path));
    let module_paths = try!(get_module_paths(&central_repo, newrev));

    println!("    ########### SCRATCH: create scratch repo ########### ");
    let scratch = Scratch::new(&scratch_dir,host);
    try!(scratch.setup(
        &central,
        &module_paths,
    ));



    scratch.transfer(newrev, &get_repo_path(&central_repo));
    let central_commit_obj = try!(scratch.repo.revparse_single(newrev));
    let central_commit = try!(central_commit_obj.as_commit()
                              .ok_or(Error::from_str("could not get commit from obj")));
    let central_tree = try!(central_commit.tree());
    println!("    ########### create {} branch in scratch.repo and point to the central_commit ########### ", &central);
    // create central branch in scratch.repo and point to the central_commit
    // is identical to master in central
    // marker for other hooks
    try!(scratch.repo.branch(central, central_commit, true));

    for module_path in module_paths {
        println!(" ####### prepare commit from scratch.repo to module {}", &module_path);
        let module_master_commit_obj = scratch.tracking(&module_path, "master");
        let module_master_commit = try!(module_master_commit_obj.as_commit()
                                        .ok_or(Error::from_str("could not get commit from obj")));

        let parents = vec![module_master_commit];

        println!("\tpath for module in central repository... => {:?}", module_path);

        // new tree is sub-tree of complete central tree
        let new_tree_oid = try!(central_tree.get_path(&Path::new(&module_path))).id();
        let old_tree_oid = try!(module_master_commit.tree()).id();

        // if sha1's are equal the content is equal
        if new_tree_oid != old_tree_oid {
            println!("\tmodule repository for {} is behind, => updating", &module_path);

            let new_tree = try!(scratch.repo.find_tree(new_tree_oid));

            let module_commit = try!(make_commit(&scratch.repo, &new_tree, central_commit, &parents));
            // do the push to the module git
            let commit = &try!(scratch.repo.find_commit(module_commit));
            try!(scratch.repo.set_head_detached(commit.id()));

            let remote = &host.remote_url(&module_path);
            try!(scratch.call_git(&format!("push {} HEAD:{}", remote, "master")));
        }
    }
    Ok(())
}

pub fn call_command(command: &str, args: &[&str], cwd: Option<&Path>)
{
    let mut c = Command::new(&command);
    c.args(args);
    println!("call_git {:?} (in {:?})", c, cwd);
    if let Some(path) = cwd {
        // c.env("GIT_DIR", format!("{}.git",path.to_str().unwrap()));
        c.current_dir(path);
    }
    let output = c
        .output()
        .unwrap_or_else(|e| panic!("failed to execute process: {}", e));
    println!("{}{}",
             String::from_utf8_lossy(&output.stdout),
             String::from_utf8_lossy(&output.stderr));
}

fn get_module_paths(central: &Repository, rev: &str) -> Result<Vec<String>, Error> {

    let object = try!(central.revparse_single(rev));
    let commit = try!(object.as_commit()
                      .ok_or(Error::from_str("could not get commit from obj")));
    let tree: Tree = try!(commit.tree());

    let tree_object = try!(tree.get_path(&Path::new("modules")));
    let modules_o = try!(tree_object.to_object(&central));
    let modules = try!(modules_o.as_tree()
                       .ok_or(Error::from_str("could not get tree from path")));

    let mut names = Vec::<String>::new();
    for module in modules.iter() {
        names.push(format!("modules/{}",try!(module.name()
                        .ok_or(Error::from_str("could not get name for module")))
                   .to_string()));
    }
    println!("<--- get_module_paths returns: {:?}", names);
    Ok(names)
}

// takes everything from base except it's tree and replaces it with the tree given
fn make_commit(repo: &Repository,
               tree: &Tree,
               base: &Commit,
               parents: &[&Commit]) -> Result<Oid, Error> {
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

pub fn get_repo_path(repo: &Repository) -> &Path {
    if repo.is_bare() {
        return repo.path();
    }
    return repo.workdir().expect("get workdir from repo");
}

