extern crate git2;
const TMP_NAME: &'static str = "tmp_fd2db5f8_bac2_4a1e_9487_4ac3414788aa";


use git2::*;
use std::process::Command;
use std::path::Path;
use shell::Shell;
use super::RepoHost;
use std::collections::HashMap;

pub fn module_ref(module: &str) -> String
{
    format!("refs/centralgit/splited/{}/split", module)
}

pub struct Scratch<'a>
{
    pub repo: Repository,
    pub host: &'a RepoHost,
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

    pub fn tracking(&self, module: &str, branch: &str) -> Option<Object>
    {
        let remote_name = format!("{}", module);
        let fetch_url = self.host.local_path(&module);
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
        if let Ok(_) = remote.fetch(&[&rs], None, None) {
            return self.repo
                .revparse_single(&format!("remotes/{}/{}", module, branch))
                .ok();
        }
        else {
            return None;
        }
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
    pub fn rewrite(&self, base: &Commit, parents: &[&Commit], tree: &Tree) -> Oid
    {
        if parents.len() == 0 {
            ::std::fs::remove_file(self.repo.path().join("HEAD")).expect("can't remove HEAD");
        }
        else {
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
        let cmd = format!("push {} HEAD:{}", self.host.remote_url(module), target);
        let output = self.call_git(&cmd).expect("can't push");
        debug!("push: {}\n{}", cmd, output);
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

    pub fn replace_subtree(&self, path: &Path, subtree: Oid, full_tree: Tree) -> Tree
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

    pub fn split_subdir(&self, module: &str, newrev: Oid) -> Option<Oid>
    {
        let walk = {
            let mut walk = self.repo.revwalk().expect("walk: can't create revwalk");
            walk.set_sorting(SORT_REVERSE | SORT_TOPOLOGICAL);
            walk.push(newrev).expect("walk.push");
            walk
        };

        let mut map = HashMap::<Oid, Oid>::new();

        'walk: for commit in walk {
            let commit = self.repo.find_commit(commit.unwrap()).unwrap();
            let tree = commit.tree().expect("commit has no tree");

            let new_tree = if let Ok(tree_entry) = tree.get_path(&Path::new(&module)) {
                self.repo.find_tree(tree_entry.id()).expect("central_submit: can't find tree")
            }
            else {
                continue;
            };

            let mut parents = vec![];
            for parent in commit.parents() {
                let parent = parent.id();
                if let Some(&parent) = map.get(&parent) {

                    let parent = self.repo.find_commit(parent).unwrap();
                    if new_tree.id() == parent.tree().unwrap().id() {
                        map.insert(commit.id(), parent.id());
                        continue 'walk;
                    }
                    parents.push(parent);
                }
            }

            let mut p = vec![];
            for parent in &parents { p.push(parent); }

            map.insert(commit.id(), self.rewrite(&commit, &p, &new_tree));
        }

        return map.get(&newrev).map(|&id|{id});
    }

    pub fn find_all_subdirs(&self, tree: &Tree) -> Vec<String>
    {
        let mut sd = vec![];
        for item in tree {
            if let Ok(st) = self.repo.find_tree(item.id()) {
                let name = item.name().unwrap();
                if !name.starts_with(".") {
                    sd.push(name.to_string());
                    for r in self.find_all_subdirs(&st) {
                        sd.push(format!("{}/{}", name, r));
                    }
                }
            }
        }
        return sd;
    }
}
