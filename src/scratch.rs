extern crate git2;
const TMP_NAME: &'static str = "refs/centralgit/tmp_fd2db5f8_bac2_4a1e_9487_4ac3414788aa";

use super::SubdirView;
use super::UnapplyView;
use super::View;
use super::replace_subtree;
use git2::*;
use shell::Shell;
use std::collections::HashMap;
use std::path::Path;


pub fn view_ref_root(module: &str) -> String
{
    format!(
        "refs/{}/#{}#/refs",
        "centralgit_0ee845b3_9c3f_41ee_9149_9e98a65ecf35",
        module
    )
}

pub fn view_ref(module: &str, branch: &str) -> String
{
    format!("{}/heads/{}", view_ref_root(module), branch)
}

// pub fn view_base_ref(module: &str, branch: &str) -> String
// {
//     format!("{}/central_base/{}", view_ref_root(module), branch)
// }
//

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
        Scratch {
            repo: Repository::init_bare(&path).expect("could not init scratch"),
        }
    }

    pub fn unapply_view(&self, current: Oid, view: &View, old: Oid, new: Oid) -> UnapplyView
    {
        if old == new {
            return UnapplyView::NoChanges;
        }

        match self.repo.graph_descendant_of(new, old) {
            Err(_) | Ok(false) => {
                debug!("graph_descendant_of({},{})", new, old);
                return UnapplyView::RejectNoFF;
            }
            Ok(true) => (),
        }

        debug!("==== walking commits from {} to {}", old, new);

        let walk = {
            let mut walk = self.repo.revwalk().expect("walk: can't create revwalk");
            walk.set_sorting(SORT_REVERSE | SORT_TOPOLOGICAL);
            let range = format!("{}..{}", old, new);
            walk.push_range(&range)
                .expect(&format!("walk: invalid range: {}", range));;
            walk.hide(old).expect("walk: can't hide");
            walk
        };

        let mut current = current;
        for rev in walk {
            let rev = rev.expect("walk: invalid rev");

            debug!("==== walking commit {}", rev);

            let module_commit = self.repo
                .find_commit(rev)
                .expect("walk: object is not actually a commit");

            if module_commit.parents().count() > 1 {
                // TODO: invectigate the possibility of allowing merge commits
                return UnapplyView::RejectMerge;
            }

            debug!("==== Rewriting commit {}", rev);

            let tree = module_commit.tree().expect("walk: commit has no tree");
            let parent = self.repo
                .find_commit(current)
                .expect("walk: current object is no commit");

            let new_tree = view.unapply(
                &self.repo,
                &tree,
                &parent.tree().expect("walk: parent has no tree"),
            );

            current = self.rewrite(
                &module_commit,
                &vec![&parent],
                &self.repo
                    .find_tree(new_tree)
                    .expect("can't find rewritten tree"),
            );
        }
        return UnapplyView::Done(current);
    }


    // force push of the new revision-object to temp repo
    pub fn transfer(&self, rev: &str, source: &Path) -> Object
    {
        // TODO: implement using libgit
        let target = &self.repo.path();
        let shell = Shell {
            cwd: source.to_path_buf(),
        };
        shell.command(&format!("git update-ref {} {}", TMP_NAME, rev));
        shell.command(&format!(
            "git push --force {} {}",
            &target.to_string_lossy(),
            TMP_NAME
        ));

        let obj = self.repo
            .revparse_single(rev)
            .expect("can't find transfered ref");
        return obj;
    }

    // takes everything from base except it's tree and replaces it with the tree
    // given
    pub fn rewrite(&self, base: &Commit, parents: &[&Commit], tree: &Tree) -> Oid
    {
        if parents.len() == 0 {
            ::std::fs::remove_file(self.repo.path().join("HEAD")).expect("can't remove HEAD");
        } else {
            self.repo
                .set_head_detached(parents[0].id())
                .expect("rewrite: can't detach head");
        }
        self.repo
            .commit(
                Some("HEAD"),
                &base.author(),
                &base.committer(),
                &base.message().unwrap_or("no message"),
                tree,
                parents,
            )
            .expect("rewrite: can't commit")
    }

    // pub fn push(&self, host: &RepoHost, oid: Oid, module: &str, target: &str) ->
    // String {
    //     self.repo.set_head_detached(oid).expect("can't detach HEAD");
    // let cmd = format!("if git push {} HEAD:{};then echo \"====\n====
    // SUCCESS!\n==== Ignore \
    // the error message below.\n====\";else echo
    // \"####\n#### \                        FAILED\n####\n\";fi",
    //                       host.remote_url(module),
    //                       target);
    //     let shell = Shell { cwd: self.repo.path().to_path_buf() };
    //     let (stdout, stderr) = shell.command(&cmd);
    //     debug!("push: {}\n{}\n{}", cmd, stdout, stderr);
    //     format!("{}\n\n{}", stderr, stdout)
    // }

    pub fn apply_view_to_branch(&self, branchname: &str, view: &str)
    {
        if view == "." {
            return;
        }

        debug!("apply_view_to_branch {}", branchname);
        if let Ok(branch) = self.repo.find_branch(branchname, git2::BranchType::Local) {
            let r = branch.into_reference().target().expect("no ref");

            let viewobj = SubdirView::new(&Path::new(&view));
            if let Some(view_commit) = self.apply_view(&viewobj, r) {
                println!("applied view to branch {}", branchname);
                self.repo
                    .reference(
                        &view_ref(&view, &branchname),
                        view_commit,
                        true,
                        "apply_view",
                    )
                    .expect("can't create reference");
            } else {
                println!("can't apply view to branch {}", branchname);
            };
        };
    }


    pub fn apply_view(&self, view: &View, newrev: Oid) -> Option<Oid>
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

            let new_tree = if let Some(tree_id) = view.apply(&tree) {
                self.repo
                    .find_tree(tree_id)
                    .expect("central_submit: can't find tree")
            } else {
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
                _ => panic!(
                    "commit with {} parents: {}",
                    commit.parents().count(),
                    commit.id()
                ),
            } {
                CommitKind::Merge(parent1, parent2) => {
                    let parent1 = self.repo.find_commit(parent1).unwrap();
                    let parent2 = self.repo.find_commit(parent2).unwrap();
                    map.insert(
                        commit.id(),
                        self.rewrite(&commit, &[&parent1, &parent2], &new_tree),
                    );
                }
                CommitKind::Normal(parent) => {
                    let parent = self.repo.find_commit(parent).unwrap();
                    if new_tree.id() == parent.tree().unwrap().id() {
                        map.insert(commit.id(), parent.id());
                    } else {
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

        let empty = self.repo
            .find_tree(self.repo.treebuilder(None).unwrap().write().unwrap())
            .unwrap();
        let mut map = HashMap::<Oid, Oid>::new();

        'walk: for commit in walk {
            let commit = self.repo.find_commit(commit.unwrap()).unwrap();
            let tree = commit.tree().expect("commit has no tree");
            let new_tree = self.repo
                .find_tree(replace_subtree(&self.repo, path, &tree, &empty))
                .expect("can't find tree");

            match commit.parents().count() {
                2 => {
                    let parent1 = commit.parents().nth(0).unwrap().id();
                    let parent2 = commit.parents().nth(1).unwrap().id();
                    if let (Some(&parent1), Some(&parent2)) = (map.get(&parent1), map.get(&parent2))
                    {
                        let parent1 = self.repo.find_commit(parent1).unwrap();
                        let parent2 = self.repo.find_commit(parent2).unwrap();

                        map.insert(
                            commit.id(),
                            self.rewrite(&commit, &[&parent1, &parent2], &new_tree),
                        );
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
                _ => panic!(
                    "commit with {} parents: {}",
                    commit.parents().count(),
                    commit.id()
                ),
            }

            map.insert(commit.id(), self.rewrite(&commit, &[], &new_tree));
        }

        let final_tree = self.repo
            .find_tree(replace_subtree(
                &self.repo,
                path,
                &src.tree().unwrap(),
                &dst.tree().unwrap(),
            ))
            .expect("can't find tree");

        let parents = [&dst, &self.repo.find_commit(map[&src.id()]).unwrap()];
        self.repo
            .set_head_detached(parents[0].id())
            .expect("join: can't detach head");

        let join_commit = self.repo
            .commit(
                Some("HEAD"),
                signature,
                signature,
                &format!("join repo into {:?}", path),
                &final_tree,
                &parents,
            )
            .unwrap();
        return join_commit;
    }
}
