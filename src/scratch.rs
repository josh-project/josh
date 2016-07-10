extern crate git2;
const TMP_NAME: &'static str = "refs/centralgit/tmp_fd2db5f8_bac2_4a1e_9487_4ac3414788aa";

use git2::*;
use std::path::Path;
use shell::Shell;
use super::RepoHost;
use std::collections::HashMap;
use super::ModuleToSubdir;

pub struct Scratch
{
    pub repo: Repository,
}

enum CommitKind
{
    Normal(Oid),
    Merge(Oid, Oid),
    Orphan,
}

impl Scratch
{
    pub fn new(path: &Path) -> Scratch
    {
        Scratch { repo: Repository::init_bare(&path).expect("could not init scratch") }
    }

    pub fn module_to_subdir(&self,
                            current: Oid,
                            path: Option<&Path>,
                            module_current: Option<Oid>,
                            new: Oid)
        -> ModuleToSubdir
    {
        let mut current = current;
        let (walk, initial) = if let Some(old) = module_current {

            if old == new {
                return ModuleToSubdir::NoChanges;
            }

            match self.repo.graph_descendant_of(new, old) {
                Err(_) | Ok(false) => {
                    debug!("graph_descendant_of({},{})", new, old);
                    return ModuleToSubdir::RejectNoFF;
                }
                Ok(true) => (),
            }

            debug!("==== walking commits from {} to {}", old, new);

            let mut walk = self.repo.revwalk().expect("walk: can't create revwalk");
            walk.set_sorting(SORT_REVERSE | SORT_TOPOLOGICAL);
            let range = format!("{}..{}", old, new);
            walk.push_range(&range).expect(&format!("walk: invalid range: {}", range));;
            walk.hide(old).expect("walk: can't hide");
            (walk, false)
        }
        else {
            let mut walk = self.repo.revwalk().expect("walk: can't create revwalk");
            walk.set_sorting(SORT_REVERSE | SORT_TOPOLOGICAL);
            walk.push(new).expect("walk: can't push");
            (walk, true)
        };

        for rev in walk {
            let rev = rev.expect("walk: invalid rev");

            debug!("==== walking commit {}", rev);

            let module_commit = self.repo
                .find_commit(rev)
                .expect("walk: object is not actually a commit");

            if module_commit.parents().count() > 1 {
                // TODO: invectigate the possibility of allowing merge commits
                return ModuleToSubdir::RejectMerge;
            }

            if let Some(path) = path {
                debug!("==== Rewriting commit {}", rev);

                let tree = module_commit.tree().expect("walk: commit has no tree");
                let parent =
                    self.repo.find_commit(current).expect("walk: current object is no commit");

                let new_tree = self.replace_subtree(path,
                                                    tree.id(),
                                                    &parent.tree()
                                                        .expect("walk: parent has no tree"));

                current = self.rewrite(&module_commit, &vec![&parent], &new_tree);
            }
        }
        return ModuleToSubdir::Done(current, initial);
    }


    pub fn tracking(&self, host: &RepoHost, module: &str, branch: &str) -> Option<Object>
    {
        let remote_name = format!("{}", module);
        let fetch_url = host.local_path(&module);
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

    // force push of the new revision-object to temp repo
    pub fn transfer(&self, rev: &str, source: &Path) -> Object
    {
        // TODO: implement using libgit
        let target = &self.repo.path();
        let shell = Shell { cwd: source.to_path_buf() };
        shell.command(&format!("git update-ref {} {}", TMP_NAME, rev));
        shell.command(&format!("git push --force {} {}",
                               &target.to_string_lossy(),
                               TMP_NAME));

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

    pub fn push(&self, host: &RepoHost, oid: Oid, module: &str, target: &str) -> String
    {
        self.repo.set_head_detached(oid).expect("can't detach HEAD");
        let cmd = format!("if git push {} HEAD:{};then echo \"====\n==== SUCCESS!\n==== Ignore \
                           the error message below.\n====\";else echo \"####\n#### \
                           FAILED\n####\n\";fi",
                          host.remote_url(module),
                          target);
        let shell = Shell { cwd: self.repo.path().to_path_buf() };
        let (stdout, stderr) = shell.command(&cmd);
        debug!("push: {}\n{}\n{}", cmd, stdout, stderr);
        format!("{}\n\n{}", stderr, stdout)
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

    fn replace_child(&self, child: &Path, subtree: Oid, full_tree: &Tree) -> Tree
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

    pub fn replace_subtree(&self, path: &Path, subtree: Oid, full_tree: &Tree) -> Tree
    {
        if path.components().count() == 1 {
            return self.replace_child(path, subtree, full_tree);
        }
        else {
            let name = Path::new(path.file_name().expect("no module name"));
            let path = path.parent().expect("module not in subdir");

            let st = if let Some(st) = self.subtree(&full_tree, path) {
                st
            }
            else {
                let empty = self.repo.treebuilder(None).unwrap().write().unwrap();
                self.repo.find_tree(empty).unwrap()
            };

            let tree = self.replace_child(name, subtree, &st);

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
                continue 'walk;
            };

            match match commit.parents().count() {
                2 => {
                    let parent1 = commit.parents().nth(0).unwrap().id();
                    let parent2 = commit.parents().nth(1).unwrap().id();
                    match (map.get(&parent1), map.get(&parent2)) {
                        (Some(&parent1), Some(&parent2)) => CommitKind::Merge(parent1, parent2),
                        (Some(&parent), None) => CommitKind::Normal(parent),
                        (None, Some(&parent)) => CommitKind::Normal(parent),
                        _ => CommitKind::Orphan,
                    }
                }
                1 => {
                    let parent = commit.parents().nth(0).unwrap().id();
                    match map.get(&parent) {
                        Some(&parent) => CommitKind::Normal(parent),
                        _ => CommitKind::Orphan,
                    }
                }
                0 => CommitKind::Orphan,
                _ => {
                    panic!("commit with {} parents: {}",
                           commit.parents().count(),
                           commit.id())
                }
            } {
                CommitKind::Merge(parent1, parent2) => {
                    let parent1 = self.repo.find_commit(parent1).unwrap();
                    let parent2 = self.repo.find_commit(parent2).unwrap();
                    map.insert(commit.id(),
                               self.rewrite(&commit, &[&parent1, &parent2], &new_tree));
                }
                CommitKind::Normal(parent) => {
                    let parent = self.repo.find_commit(parent).unwrap();
                    if new_tree.id() == parent.tree().unwrap().id() {
                        map.insert(commit.id(), parent.id());
                    }
                    else {
                        map.insert(commit.id(), self.rewrite(&commit, &[&parent], &new_tree));
                    }
                }
                CommitKind::Orphan => {
                    map.insert(commit.id(), self.rewrite(&commit, &[], &new_tree));
                }
            }
        }

        return map.get(&newrev).map(|&id| id);
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

    pub fn join_to_subdir(&self, dst: Oid, path: &Path, src: Oid, signature: &Signature) -> Oid
    {
        let dst = self.repo.find_commit(dst).unwrap();
        let src = self.repo.find_commit(src).unwrap();

        let walk = {
            let mut walk = self.repo.revwalk().expect("walk: can't create revwalk");
            walk.set_sorting(SORT_REVERSE | SORT_TOPOLOGICAL);
            walk.push(src.id()).expect("walk.push");
            walk
        };

        let empty =
            self.repo.find_tree(self.repo.treebuilder(None).unwrap().write().unwrap()).unwrap();
        let mut map = HashMap::<Oid, Oid>::new();

        'walk: for commit in walk {
            let commit = self.repo.find_commit(commit.unwrap()).unwrap();
            let tree = commit.tree().expect("commit has no tree");
            let new_tree = self.replace_subtree(path, tree.id(), &empty);

            match commit.parents().count() {
                2 => {
                    let parent1 = commit.parents().nth(0).unwrap().id();
                    let parent2 = commit.parents().nth(1).unwrap().id();
                    if let (Some(&parent1), Some(&parent2)) = (map.get(&parent1),
                                                               map.get(&parent2)) {
                        let parent1 = self.repo.find_commit(parent1).unwrap();
                        let parent2 = self.repo.find_commit(parent2).unwrap();

                        map.insert(commit.id(),
                                   self.rewrite(&commit, &[&parent1, &parent2], &new_tree));
                        continue 'walk;
                    }
                }
                1 => {
                    let parent = commit.parents().nth(0).unwrap().id();
                    let parent = *map.get(&parent).unwrap();
                    let parent = self.repo.find_commit(parent).unwrap();
                    map.insert(commit.id(), self.rewrite(&commit, &[&parent], &new_tree));
                    continue 'walk;
                }
                0 => {}
                _ => {
                    panic!("commit with {} parents: {}",
                           commit.parents().count(),
                           commit.id())
                }
            }

            map.insert(commit.id(), self.rewrite(&commit, &[], &new_tree));
        }

        let final_tree = self.replace_subtree(path, src.tree().unwrap().id(), &dst.tree().unwrap());

        let parents = [&dst, &self.repo.find_commit(map[&src.id()]).unwrap()];
        self.repo.set_head_detached(parents[0].id()).expect("join: can't detach head");

        let join_commit = self.repo
            .commit(Some("HEAD"),
                    signature,
                    signature,
                    &format!("join repo into {:?}", path),
                    &final_tree,
                    &parents)
            .unwrap();
        return join_commit;

    }
}
