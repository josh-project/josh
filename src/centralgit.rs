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
    pub fn new(scratch: &Scratch, branch: &str, project_list: &ProjectList, host: &RepoHost) -> Self
    {
        let cg = CentralGit { branch: branch.to_string() };
        cg.update_tracking_refs(scratch, project_list, host);
        return cg;
    }

    pub fn update_tracking_refs(&self,
                                scratch: &Scratch,
                                project_list: &ProjectList,
                                host: &RepoHost)
    {
        let central_rev = if let Ok(id) = scratch.repo
            .refname_to_id(&module_ref(project_list.central(), self.branch())) {
            id
        }
        else {
            let central_rev = scratch.tracking(host, &project_list.central(), &self.branch())
                .expect("CentralGit::new: no central tracking")
                .id();
            scratch.repo
                .reference(&module_ref(&project_list.central(), &self.branch()),
                           central_rev,
                           true,
                           "tracking")
                .expect("can't create reference");
            central_rev
        };

        for module in project_list.projects() {
            if module == project_list.central() {
                continue;
            }

            if let Ok(_) = scratch.repo.refname_to_id(&module_ref(&module, &self.branch())) {
                debug!("=== module ref for {} already exists", &module);
            }
            else if let Some(rev) = scratch.tracking(host, &module, &self.branch()) {
                scratch.repo
                    .reference(&module_ref(&module, &self.branch()),
                               rev.id(),
                               true,
                               "tracking")
                    .expect("can't create reference");

            }
            else if let Some(commit) = scratch.split_subdir(&module, central_rev) {
                scratch.repo
                    .reference(&module_ref(&module, &self.branch()),
                               commit,
                               true,
                               "subtree_split")
                    .expect("can't create reference");
            }
            else {
                debug!("=== subdir empty: {}", module);
            }
        }

        for module in project_list.projects() {
            let current_central = scratch.repo
                .refname_to_id(&module_ref(project_list.central(), &self.branch()))
                .expect("central tracking");
            let module_base_central = scratch.repo
                .refname_to_id(&module_central_base_ref(&module, self.branch()))
                .unwrap_or(Oid::from_str("0000000000000000000000000000000000000000").unwrap());
            if current_central != module_base_central {
                // panic!("current_central != module_base_central");
            }
            if let Ok(module_commit) = scratch.repo
                .refname_to_id(&module_ref(&module, &self.branch())) {
                let output = scratch.push(host,
                                          module_commit,
                                          &module,
                                          &format!("refs/heads/{}", self.branch()));

                scratch.repo
                    .reference(&module_central_base_ref(&module, &self.branch()),
                               current_central,
                               true,
                               "module_push")
                    .expect("can't create reference");
                debug!("{}", output);
            }
        }
    }
}

pub fn module_ref(module: &str, branch: &str) -> String
{
    format!("refs/{}/{}/refs/heads/{}",
            "centralgit_0ee845b3_9c3f_41ee_9149_9e98a65ecf35",
            module,
            branch)
}

pub fn module_central_base_ref(module: &str, branch: &str) -> String
{
    format!("refs/{}/{}/refs/central_base/{}",
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

    fn pre_create_project(&self, scratch: &Scratch, rev: git2::Oid, project: &str)
    {

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
        self.update_tracking_refs(scratch, project_list, host);

        let new = newrev.id();

        let current_central = scratch.repo
            .refname_to_id(&module_ref(project_list.central(), self.branch()))
            .expect("no central tracking");

        let module_current = scratch.repo.refname_to_id(&module_ref(module, self.branch())).ok();

        return scratch.module_to_subdir(current_central,
                                        Some(Path::new(module)),
                                        module_current,
                                        new);
    }

    fn project_created(&self,
                       scratch: &Scratch,
                       host: &RepoHost,
                       project_list: &ProjectList,
                       _project: &str)
    {
        self.update_tracking_refs(scratch, project_list, host);
    }

    fn central_submit(&self,
                      scratch: &Scratch,
                      host: &RepoHost,
                      project_list: &ProjectList,
                      newrev: Object)
    {
        debug!(" ---> central_submit (sha1 of commit: {})", &newrev.id());
        self.update_tracking_refs(scratch, project_list, host);

        scratch.repo
            .reference(&module_ref(&project_list.central(), &self.branch()),
                       newrev.id(),
                       true,
                       "central_submit")
            .expect("can't create reference");

        let central_commit = newrev.as_commit().expect("could not get commit from obj");
        let central_tree = central_commit.tree().expect("commit has no tree");

        for module in project_list.projects() {
            if module == project_list.central() {
                continue;
            }

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
        self.update_tracking_refs(scratch, project_list, host);
    }
}
