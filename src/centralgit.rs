extern crate git2;

use git2::*;
use std::path::Path;
use scratch::Scratch;
use super::Hooks;
use super::RepoHost;
use super::ProjectList;
use super::ModuleToSubdir;

pub struct CentralGit
{
    branch: String,
}

impl CentralGit
{
    pub fn new(branch: &str) -> Self
    {
        CentralGit { branch: branch.to_string() }
    }
}

pub fn module_ref(module: &str, branch: &str) -> String
{
    format!("refs/{}/{}/refs/heads/{}",
            "centralgit_0ee845b3_9c3f_41ee_9149_9e98a65ecf35",
            module,
            branch)
}

impl Hooks for CentralGit
{
    fn branch(&self) -> &str
    {
        return &self.branch;
    }

    fn review_upload(&self,
                     scratch: &Scratch,
                     host: &RepoHost,
                     project_list: &ProjectList,
                     newrev: Object,
                     module: &str)
        -> ModuleToSubdir
    {
        debug!(".\n\n==== Doing review upload for module {}", &module);

        let new = newrev.id();
        let current = scratch.tracking(host, project_list.central(), &self.branch)
            .expect("no central tracking")
            .id();
        let module_current = scratch.tracking(host, &module, &self.branch());

        return scratch.module_to_subdir(current,
                                        Some(Path::new(module)),
                                        module_current.map(|x| x.id()),
                                        new);
    }

    fn pre_create_project(&self, scratch: &Scratch, rev: Oid, project: &str)
    {
        if let Ok(_) = scratch.repo.refname_to_id(&module_ref(project, &self.branch())) {
            debug!("=== module ref for {} already exists", project);
        }
        else {
            if let Some(commit) = scratch.split_subdir(&project, rev) {
                scratch.repo
                    .reference(&module_ref(project, &self.branch()),
                               commit,
                               true,
                               "subtree_split")
                    .expect("can't create reference");
            }
            else {
                debug!("=== subdir empty: {}", project);
            }
        }
    }

    fn project_created(&self,
                       scratch: &Scratch,
                       host: &RepoHost,
                       project_list: &ProjectList,
                       _project: &str)
    {
        if let Some(rev) = scratch.tracking(host, project_list.central(), &self.branch()) {
            self.central_submit(scratch, host, project_list, rev);
        }
    }

    fn central_submit(&self,
                      scratch: &Scratch,
                      host: &RepoHost,
                      project_list: &ProjectList,
                      newrev: Object)
    {
        debug!(" ---> central_submit (sha1 of commit: {})", &newrev.id());

        let central_commit = newrev.as_commit().expect("could not get commit from obj");
        let central_tree = central_commit.tree().expect("commit has no tree");

        let mut changed = vec![];

        for module in project_list.projects() {
            if module == project_list.central() {
                continue;
            }
            debug!("");
            debug!("==== fetching tracking branch for module: {}", &module);
            match scratch.tracking(host, &module, &self.branch()) {
                Some(_) => (),
                None => {
                    debug!("====    no tracking branch for module {} => project does not exist \
                            or is empty",
                           &module);
                    debug!("====    initializing with subdir history");

                    changed.push(module.to_string());
                }
            };
            self.pre_create_project(scratch, newrev.id(), &module);

            let module_commit_obj = if let Ok(rev) = scratch.repo
                .revparse_single(&module_ref(&module, &self.branch())) {
                debug!("=== OK module ref : {}", module);
                rev
            }
            else {
                debug!("=== NO module ref : {}", module);
                continue;
            };

            let parents = vec![module_commit_obj.as_commit()
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
                changed.push(module.to_string());
                let new_tree =
                    scratch.repo.find_tree(new_tree_id).expect("central_submit: can't find tree");
                debug!("====    commit changes module => make commit on module");
                let module_commit = scratch.rewrite(central_commit, &parents, &new_tree);
                scratch.repo
                    .reference(&module_ref(&module, &self.branch()),
                               module_commit,
                               true,
                               "rewrite")
                    .expect("can't create reference");
            }
            else {
                debug!("====    commit does not change module => skipping");
            }
        }

        for module in changed {
            if let Ok(module_commit) = scratch.repo
                .refname_to_id(&module_ref(&module, &self.branch())) {
                let output = scratch.push(host,
                                          module_commit,
                                          &module,
                                          &format!("refs/heads/{}", self.branch()));
                debug!("{}", output);
            }
        }
    }
}
