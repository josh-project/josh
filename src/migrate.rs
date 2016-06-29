extern crate git2;

use git2::*;
use std::process::Command;
use std::path::Path;
use std::path::PathBuf;

const TMP_NAME: &'static str = "tmp_fd2db5f8_bac2_4a1e_9487_4ac3414788aa";

pub trait RepoHost
{
    fn central(&self) -> &str;
    fn projects(&self) -> Vec<String>;

    fn remote_url(&self, &str) -> String;
    fn fetch_url(&self, module: &str) -> String
    {
        self.remote_url(module)
    }
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
        let remote_name = format!("{}", module);
        let fetch_url = self.host.fetch_url(&module);
        let mut remote = if let Ok(remote) = self.repo.find_remote(&remote_name) {
            remote
        }
        else {
            debug!("==== create remote (remote_name:{}, remote_url:{})",
                   &remote_name,
                   &fetch_url);
            self.repo.remote(&remote_name, &fetch_url).expect("can't create remote")
        };

        let rs = remote.get_refspec(0).unwrap().str().unwrap().to_string();
        remote.fetch(&[&rs], None, None).expect("fetch failed");
        return self.repo
            .revparse_single(&format!("remotes/{}/{}", module, branch))
            .ok();
    }

    fn call_git(self: &Scratch<'a>, cmd: &str) -> Result<String, Error>
    {
        let args: Vec<&str> = cmd.split(" ").collect();
        let repo_path = &self.repo.path();
        let mut c = Command::new("git");
        c.current_dir(&repo_path);
        c.env("GIT_DIR", repo_path.as_os_str());
        c.args(&args);

        let output = c.output()
            .unwrap_or_else(|e| panic!("failed to execute process: {}", e));
        Ok(String::from_utf8(output.stderr).expect("cannot decode utf8"))
    }

    // force push of the new revision-object to temp repo
    pub fn transfer(&self, rev: &str, source: &Path) -> Object
    {
        // TODO: implement using libgit
        let target = &self.repo.path();
        let shell = Shell { cwd: source.to_path_buf() };
        // create tmp branch
        shell.command(&format!("git branch -f {} {}", TMP_NAME, rev));
        // force push
        shell.command(&format!("git push --force {} {}",
                               &target.to_string_lossy(),
                               TMP_NAME));
        // delete tmp branch
        shell.command(&format!("git branch -D {}", TMP_NAME));

        let obj = self.repo.revparse_single(rev).expect("can't find transfered ref");
        return obj;
    }

    // takes everything from base except it's tree and replaces it with the tree
    // given
    fn rewrite(&self, base: &Commit, parents: &[&Commit], tree: &Tree) -> Oid
    {
        if parents.len() != 0 {
            self.repo.set_head_detached(parents[0].id()).expect("rewrite: can't detach head");
        }
        self.repo
            .commit(Some("HEAD"),
                    &base.author(),
                    &base.committer(),
                    &base.message().unwrap_or("no message"),
                    tree,
                    parents)
            .expect("rewrite: can't commit")
    }

    pub fn push(&self, oid: Oid, module: &str, target: &str) -> String
    {
        let commit = &self.repo.find_commit(oid).expect("can't find commit");
        self.repo.set_head_detached(commit.id()).expect("can't detach HEAD");
        let output =
            self.call_git(&format!("push {} HEAD:{}", self.host.remote_url(module), target))
                .expect("can't push");
        format!("{}", output)
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

    fn replace_child(&self, child: &Path, subtree: Oid, full_tree: Tree) -> Tree
    {
        let full_tree_id = {
            let mut builder = self.repo
                .treebuilder(Some(&full_tree))
                .expect("replace_child: can't create treebuilder");
            builder.insert(child, subtree, 0o0040000) // GIT_FILEMODE_TREE
                .expect("replace_child: can't insert tree");
            builder.write().expect("replace_child: can't write tree")
        };
        return self.repo.find_tree(full_tree_id).expect("replace_child: can't find new tree");
    }

    fn replace_subtree(&self, path: &Path, subtree: Oid, full_tree: Tree) -> Tree
    {
        if path.components().count() == 1 {
            return self.replace_child(path, subtree, full_tree);
        }
        else {
            let name = Path::new(path.file_name().expect("no module name"));
            let path = path.parent().expect("module not in subdir");

            let st = self.subtree(&full_tree, path).unwrap();
            let tree = self.replace_child(name, subtree, st);

            return self.replace_subtree(path, tree.id(), full_tree);
        }
    }

    fn split_subdir(&self, module: &str, newrev: Oid) -> Object
    {
        // TODO: implement using libgit
        let shell = Shell { cwd: self.repo.path().to_path_buf() };
        shell.command("rm -Rf refs/original");
        shell.command("rm -Rf .git-rewrite");

        self.repo.set_head_detached(newrev).expect("can't detatch head");;

        self.call_git(&format!("filter-branch --subdirectory-filter {}/ -- HEAD", module))
            .expect("error in filter-branch");

        return self.repo
            .revparse_single("HEAD")
            .expect("can't find rewritten branch");
    }
}

pub enum ReviewUploadResult
{
    Uploaded(Oid),
    RejectNoFF,
    RejectMerge,
    NoChanges,
    Central,
}

pub fn review_upload(scratch: &Scratch, newrev: Object, module: &str) -> ReviewUploadResult
{
    debug!(".\n\n==== Doing review upload for module {}", &module);

    let new = newrev.id();
    let old = scratch.tracking(&module, "master").expect("no tracking branch 1").id();

    if old == new {
        return ReviewUploadResult::NoChanges;
    }

    match scratch.repo.graph_descendant_of(new, old) {
        Err(_) => return ReviewUploadResult::RejectNoFF,
        Ok(false) => return ReviewUploadResult::RejectNoFF,
        Ok(true) => (),
    }

    debug!("==== walking commits from {} to {}", old, new);

    let walk = {
        let mut walk = scratch.repo.revwalk().expect("walk: can't create revwalk");
        walk.set_sorting(SORT_REVERSE | SORT_TIME);
        let range = format!("{}..{}", old, new);
        walk.push_range(&range).expect(&format!("walk: invalid range: {}", range));;
        walk
    };

    let mut current =
        scratch.tracking(scratch.host.central(), "master").expect("no central tracking").id();

    for rev in walk {
        let rev = rev.expect("walk: invalid rev");
        if old == rev {
            continue;
        }

        debug!("==== walking commit {}", rev);

        let module_commit = scratch.repo
            .find_commit(rev)
            .expect("walk: object is not actually a commit");

        if module_commit.parents().count() > 1 {
            // TODO: also do this check on pushes to cenral refs/for/master
            // TODO: invectigate the possibility of allowing merge commits
            return ReviewUploadResult::RejectMerge;
        }

        if module != scratch.host.central() {
            debug!("==== Rewriting commit {}", rev);

            let tree = module_commit.tree().expect("walk: commit has no tree");
            let parent =
                scratch.repo.find_commit(current).expect("walk: current object is no commit");

            let new_tree = scratch.replace_subtree(Path::new(module),
                                                   tree.id(),
                                                   parent.tree()
                                                       .expect("walk: parent has no tree"));

            current = scratch.rewrite(&module_commit, &vec![&parent], &new_tree);
        }
    }


    if module != scratch.host.central() {
        return ReviewUploadResult::Uploaded(current);
    }
    else {
        return ReviewUploadResult::Central;
    }
}

pub fn project_created(scratch: &Scratch)
{
    if let Some(rev) = scratch.tracking(scratch.host.central(), "master") {
        central_submit(scratch, rev);
    }
}

pub fn central_submit(scratch: &Scratch, newrev: Object)
{
    debug!(" ---> central_submit (sha1 of commit: {})", &newrev.id());

    let central_commit = newrev.as_commit().expect("could not get commit from obj");
    let central_tree = central_commit.tree().expect("commit has no tree");

    for module in scratch.host.projects() {
        if module == scratch.host.central() {
            continue;
        }
        debug!("");
        debug!("==== fetching tracking branch for module: {}", &module);
        let module_master_commit_obj = match scratch.tracking(&module, "master") {
            Some(obj) => obj,
            None => {
                debug!("====    no tracking branch for module {} => project does not exist or is \
                        empty",
                       &module);
                debug!("====    initializing with subdir history");
                let commit = scratch.split_subdir(&module, newrev.id());
                scratch.push(commit.id(), &module, "refs/heads/master");
                scratch.tracking(&module, "master").expect("no tracking branch 3")
            }
        };

        let parents = vec![module_master_commit_obj.as_commit()
                               .expect("could not get commit from obj")];

        debug!("==== checking for changes in module: {:?}", module);

        // new tree is sub-tree of complete central tree
        let old_tree_id = if let Ok(tree) = parents[0].tree() {
            tree.id()
        }
        else {
            Oid::from_str("0000000000000000000000000000000000000000").unwrap()
        };

        let new_tree_id = if let Ok(tree_entry) = central_tree.get_path(&Path::new(&module)) {
            tree_entry.id()
        }
        else {
            Oid::from_str("0000000000000000000000000000000000000000").unwrap()
        };


        // if sha1's are equal the content is equal
        if new_tree_id != old_tree_id && !new_tree_id.is_zero() {
            let new_tree = scratch.repo.find_tree(new_tree_id).expect("central_submit: can't find tree");
            debug!("====    commit changes module => make commit on module");
            let module_commit = scratch.rewrite(central_commit, &parents, &new_tree);
            let output = scratch.push(module_commit, &module, "master");
            debug!("{}",output);
        }
        else {
            debug!("====    commit does not change module => skipping");
        }
    }
}

pub fn find_repos(root: &Path, path: &Path, mut repos: Vec<String>) -> Vec<String>
{
    if let Ok(children) = path.read_dir() {
        for child in children {
            let path = child.unwrap().path();

            let name = format!("{}", &path.to_str().unwrap());
            if let Some(last) = path.extension() {
                if last == "git" {
                    repos.push(name.trim_right_matches(".git")
                        .trim_left_matches(root.to_str().unwrap())
                        .trim_left_matches("/")
                        .to_string());
                    continue;
                }
            }
            repos = find_repos(root, &path, repos);
        }
    }
    return repos;
}

pub struct Shell
{
    pub cwd: PathBuf,
}

impl Shell
{
    pub fn command(&self, cmd: &str) -> String
    {
        debug!("Shell::command: {}", cmd);
        let output = Command::new("sh")
            .current_dir(&self.cwd)
            .arg("-c")
            .arg(&cmd)
            .output()
            .unwrap_or_else(|e| panic!("failed to execute process: {}", e));

        return String::from_utf8(output.stdout).expect("failed to decode utf8").trim().to_string();
    }
}
