extern crate git2;

use git2::*;
use std::process::Command;
use std::path::Path;
use std::path::PathBuf;

const TMP_NAME: &'static str = "tmp_fd2db5f8_bac2_4a1e_9487_4ac3414788aa";

pub trait RepoHost
{
    fn remote_url(&self, &str) -> String;
    fn create_project(&self, &str) -> Result<(), Error>;
}

pub struct Scratch<'a>
{
    repo: Repository,
    host: &'a RepoHost,
}

impl<'a> Scratch<'a>
{
    pub fn new(path: &Path, host: &'a RepoHost) -> Scratch<'a>
    {
        Scratch {
            repo: Repository::init_bare(&path).expect("could not init scratch"),
            host: host,
        }
    }

    fn tracking(&self, module: &str, branch: &str) -> Option<Object>
    {
        self.host.create_project(&module).expect("tracking: can't create project");
        println!("tracking_branch remotes/{}/{}", module, branch);

        let remote_name = format!("{}", module);
        let remote_url = self.host.remote_url(&module);
        println!("  create remote (remote_name:{}, remote_url:{})",
                 &remote_name,
                 &remote_url);
        if !self.repo.find_remote(&remote_name).is_ok() {
            self.repo.remote(&remote_name, &remote_url).expect("can't create remote");
        }

        self.call_git("fetch --all").expect("could not fetch");
        return self.repo
            .revparse_single(&format!("remotes/{}/{}", module, branch)).ok();
    }

    fn call_git(self: &Scratch<'a>, cmd: &str) -> Result<(), Error>
    {
        println!("Scratch::call_git ({:?}): git {}",
                 &self.repo.path(),
                 &cmd);
        let args: Vec<&str> = cmd.split(" ").collect();
        let repo_path = &self.repo.path();
        let mut c = Command::new("git");
        c.env("GIT_DIR", repo_path.as_os_str());
        c.args(&args);

        println!("call_git {:?} command in {:?}", c, repo_path);
        let output = c.output()
            .unwrap_or_else(|e| panic!("failed to execute process: {}", e));
        println!("{}{}",
                 String::from_utf8_lossy(&output.stdout),
                 String::from_utf8_lossy(&output.stderr));
        Ok(())
    }

    // force push of the new revision-object to temp repo
    pub fn transfer(&self, rev: &str, source: &Path) -> Object
    {
        // TODO: implement using libgit
        let target = &self.repo.path();
        let shell = Shell { cwd: source.to_path_buf() };
        println!("---> transfer_to_scratch in {}", source.display());
        // " create tmp branch
        shell.command(&format!("git branch -f {} {}", TMP_NAME, rev));
        // force push
        shell.command(&format!("git push --force {} {}",
                               &target.to_string_lossy(),
                               TMP_NAME));
        // delete tmp branch
        shell.command(&format!("git branch -D {}", TMP_NAME));
        println!("<--- transfer_to_scratch done...");

        let obj = self.repo.revparse_single(rev).expect("can't find transfered ref");
        return obj;
    }

    // takes everything from base except it's tree and replaces it with the tree
    // given
    fn rewrite(&self, base: &Commit, parents: &[&Commit], tree: &Tree) -> Result<Oid, Error>
    {
        if parents.len() != 0 {
            try!(self.repo.set_head_detached(parents[0].id()));
        }
        self.repo.commit(Some("HEAD"),
                         &base.author(),
                         &base.committer(),
                         &base.message().unwrap_or("no message"),
                         tree,
                         parents)
    }

    fn push(&self, oid: Oid, module: &str, target: &str)
    {
        let commit = &self.repo.find_commit(oid).expect("can't find commit");
        self.repo.set_head_detached(commit.id()).expect("can't detach HEAD");
        self.call_git(&format!("push {} HEAD:{}",
                               self.host.remote_url(module),
                               target))
            .expect("can't push");
    }

    fn subtree(&self, tree: &Tree, path: &Path) -> Option<Tree>
    {
        if let Some(oid) = tree.get_path(path).map(|x| x.id()).ok() {
            return self.repo.find_tree(oid).ok();
        }
        else {
            return None;
        }
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
            let mut builder = try!(self.repo
                .treebuilder(Some(&self.subtree(master_tree, parent_path)
                    .unwrap())));
            try!(builder.insert(module_name, module_tree.id(), 0o0040000)); // GIT_FILEMODE_TREE
            try!(builder.write())
        };

        let full_tree = {
            let mut builder = try!(self.repo.treebuilder(Some(master_tree)));
            try!(builder.insert(parent_path, modules_tree, 0o0040000)); // GIT_FILEMODE_TREE
            try!(builder.write())
        };
        println!("module_to_subfolder {:?}", full_tree);
        Ok(full_tree)
    }

    fn module_paths(&self, object: &Object) -> Vec<String>
    {
        let mut names = Vec::<String>::new();
        // println!("---> get_module_paths");
        let commit = object.as_commit().expect("module_paths: object is not a commit");
        let tree: Tree = commit.tree().expect("module_paths: commit has no tree");

        if let Ok(tree_object) = tree.get_path(&Path::new("modules")) {
            let modules_o = tree_object.to_object(&self.repo).expect("module_paths: to_object");
            if let Some(modules) = modules_o.as_tree() {

                for module in modules.iter() {
                    names.push(format!("modules/{}",
                                       module.name().expect("module_paths: TreeItem has no name"))
                        .to_string());
                }
            }
        }
        return names;
        // println!("<--- get_module_paths returns: {:?}", names);
    }

    fn split_subdir(&self, module: &str, newrev: &Oid) -> Object
    {
        // TODO: implement using libgit
        let shell = Shell { cwd: self.repo.path().to_path_buf() };
        shell.command("rm -Rf refs/original");

        self.call_git(&format!("branch -f initial_{} {}", module, newrev))
            .expect("create branch");

        self.call_git(&format!("filter-branch -f --subdirectory-filter {}/ -- initial_{}",
                               module,
                               module))
            .expect("error in filter-branch");

        return self.repo
            .revparse_single(&format!("initial_{}", module))
            .expect("can't find rewritten branch");
    }
}

pub fn module_review_upload(scratch: &Scratch,
                            newrev: Object,
                            module: &str,
                            central: &str)
    -> Result<(), Error>
{
    println!("in module_review_upload for module {}", &module);

    let new = newrev.id();
    let old = scratch.tracking(&module, "master").expect("no tracking branch").id();

    if !try!(scratch.repo.graph_descendant_of(new, old)) {
        println!(".");
        println!("===========================================================");
        println!("======== Commit not based on master, rebase first! ========");
        println!("===========================================================");
        return Ok(());
    }

    let walk = {
        let mut walk = try!(scratch.repo.revwalk());
        walk.set_sorting(SORT_REVERSE | SORT_TIME);
        try!(walk.push_range(&format!("{}..{}", old, new)));
        walk
    };

    println!("===== module path: {}", module);
    println!("===== Rewrite commits from {} to {}", old, new);

    let mut current_oid = scratch.tracking(central, "master").expect("no central tracking").id();
    for rev in walk {
        let currev = format!("{}", try!(rev));
        let oldrev = format!("{}", old);
        if oldrev == currev {
            continue;
        }
        println!("===== Rewrite commit {}", currev);

        let module_commit_obj = try!(scratch.repo.revparse_single(&currev));
        let module_commit = try!(module_commit_obj.as_commit()
            .ok_or(Error::from_str("object is not actually a commit")));
        let module_tree = try!(module_commit.tree());

        let parent_commit = try!(scratch.repo.find_commit(current_oid));

        let new_tree = {
            let master_tree: Tree = try!(parent_commit.tree());
            let new_tree_oid =
                try!(scratch.module_to_subfolder(Path::new(module), &module_tree, &master_tree));
            try!(scratch.repo.find_tree(new_tree_oid))
        };

        current_oid = try!(scratch.rewrite(module_commit, &vec![&parent_commit], &new_tree));
    }

    println!("");
    println!("");
    println!("===================== Doing actual upload in central git ========================");

    scratch.push(current_oid, central, "refs/for/master");

    println!("==== The review upload may have worked, even if it says error below. Look UP! ====");
    Ok(())
}

pub fn central_submit(scratch: &Scratch, newrev: Object) -> Result<(), Error>
{
    println!(" ---> central_submit (sha1 of commit: {})",
             &newrev.id());

    println!("    ########### SCRATCH: create scratch repo ########### ");

    let central_commit = try!(newrev.as_commit()
        .ok_or(Error::from_str("could not get commit from obj")));
    let central_tree = try!(central_commit.tree());

    for module in scratch.module_paths(&newrev) {
        println!(" ####### prepare commit from scratch.repo to module {}",
                 &module);
        let module_master_commit_obj = match scratch.tracking(&module, "master") {
            Some(obj) => obj,
            None => {
                let commit = scratch.split_subdir(&module, &newrev.id());
                try!(scratch.host.create_project(&module));
                scratch.push(commit.id(), &module, "refs/heads/master");
                scratch.tracking(&module, "master").expect("no tracking branch")
            }
        };

        let parents = vec![module_master_commit_obj.as_commit()
                               .expect("could not get commit from obj")];

        println!("\tpath for module in central repository... => {:?}",
                 module);

        // new tree is sub-tree of complete central tree
        let old_tree = try!(parents[0].tree());
        let new_tree = try!(scratch.repo
            .find_tree(try!(central_tree.get_path(&Path::new(&module))).id()));

        // if sha1's are equal the content is equal
        if new_tree.id() != old_tree.id() {
            println!("\tmodule repository for {} is behind, => updating",
                     &module);
            let module_commit = try!(scratch.rewrite(central_commit, &parents, &new_tree));
            scratch.push(module_commit, &module, "master");
        }
    }
    Ok(())
}

pub struct Shell
{
    pub cwd: PathBuf,
}


impl Shell
{
    pub fn command(&self, cmd: &str) -> String
    {
        println!("Shell command {:?} (in {:?})", cmd, self.cwd);
        let output = Command::new("sh")
            .current_dir(&self.cwd)
            .arg("-c")
            .arg(&cmd)
            .output().unwrap_or_else(|e| panic!("failed to execute process: {}", e));

        println!("{}", String::from_utf8_lossy(&output.stderr));

        return String::from_utf8(output.stdout).expect("failed to decode utf8").trim().to_string();
    }
}
