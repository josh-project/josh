use super::empty_tree;
use super::empty_tree_id;
use super::filter_cache;
use super::filter_cache::FilterCache;
use super::scratch;
use pest::Parser;
use std::collections::HashMap;
use std::path::Path;

fn select_parent_commits<'a>(
    original_commit: &'a git2::Commit,
    filtered_tree_id: git2::Oid,
    filtered_parent_commits: Vec<&'a git2::Commit>,
) -> Vec<&'a git2::Commit<'a>> {
    let affects_filtered = filtered_parent_commits
        .iter()
        .any(|x| filtered_tree_id != x.tree_id());

    let all_diffs_empty = original_commit
        .parents()
        .all(|x| x.tree_id() == original_commit.tree_id());

    return if affects_filtered || all_diffs_empty {
        filtered_parent_commits
    } else {
        vec![]
    };
}

fn create_filtered_commit(
    repo: &git2::Repository,
    original_commmit: &git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: &git2::Tree,
) -> super::JoshResult<git2::Oid> {
    let filtered_parent_commits: std::result::Result<Vec<_>, _> =
        filtered_parent_ids
            .iter()
            .filter(|x| **x != git2::Oid::zero())
            .map(|x| repo.find_commit(*x))
            .collect();

    let filtered_parent_commits = filtered_parent_commits?;

    let selected_filtered_parent_commits: Vec<&_> = select_parent_commits(
        &original_commmit,
        filtered_tree.id(),
        filtered_parent_commits.iter().collect(),
    );

    if selected_filtered_parent_commits.len() == 0 {
        if filtered_parent_commits.len() != 0 {
            return Ok(filtered_parent_commits[0].id());
        }
        if filtered_tree.id() == empty_tree_id() {
            return Ok(git2::Oid::zero());
        }
    }

    return scratch::rewrite(
        &repo,
        &original_commmit,
        &selected_filtered_parent_commits,
        &filtered_tree,
    );
}

pub trait Filter {
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        if forward_maps.has(&repo, &self.filter_spec(), commit.id()) {
            return Ok(forward_maps.get(&self.filter_spec(), commit.id()));
        }
        let filtered_tree =
            self.apply_to_tree(&repo, &commit.tree()?, commit.id())?;

        let filtered_parent_ids =
            self.apply_to_parents(repo, commit, forward_maps, backward_maps)?;

        return create_filtered_commit(
            repo,
            commit,
            filtered_parent_ids,
            &find_tree_or_error(
                &repo,
                filtered_tree,
                Some(&commit),
                &self.filter_spec(),
            ),
        );
    }

    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>>;

    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid>;

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid>;

    fn prefixes(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    fn filter_spec(&self) -> String;
}

struct NopFilter;

impl Filter for NopFilter {
    fn apply_to_parents(
        &self,
        _repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut FilterCache,
        _backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return Ok(commit.parent_ids().collect());
    }
    fn apply_to_commit(
        &self,
        _repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut FilterCache,
        _backward_maps: &mut FilterCache,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        return Ok(commit.id());
    }

    fn apply_to_tree(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        Ok(tree.id())
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        Ok(tree.id())
    }

    fn filter_spec(&self) -> String {
        return ":nop".to_owned();
    }
}

struct DirsFilter {
    cache: std::cell::RefCell<
        std::collections::HashMap<(git2::Oid, String), git2::Oid>,
    >,
}

fn striped_tree(
    repo: &git2::Repository,
    root: &str,
    input: git2::Oid,
    cache: &mut std::collections::HashMap<(git2::Oid, String), git2::Oid>,
) -> super::JoshResult<git2::Oid> {
    if let Some(cached) = cache.get(&(input, root.to_string())) {
        return Ok(*cached);
    }

    let tree = repo.find_tree(input)?;
    let mut result = empty_tree(&repo);

    for entry in tree.iter() {
        if entry.kind() == Some(git2::ObjectType::Blob)
            && entry.name().ok_or(super::josh_error("no name"))?
                == "workspace.josh"
        {
            let r = replace_child(
                &repo,
                &Path::new(entry.name().ok_or(super::josh_error("no name"))?),
                entry.id(),
                &result,
            )?;

            result =
                repo.find_tree(r).expect("DIRS filter: can't find new tree");
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let r = replace_child(
                &repo,
                &Path::new(entry.name().ok_or(super::josh_error("no name"))?),
                striped_tree(
                    &repo,
                    &format!(
                        "{}/{}",
                        root,
                        entry.name().ok_or(super::josh_error("no name"))?
                    ),
                    entry.id(),
                    cache,
                )?,
                &result,
            )?;

            result =
                repo.find_tree(r).expect("DIRS filter: can't find new tree");
        }
    }

    if root != "" {
        let empty_blob = repo.blob("".as_bytes())?;

        let r = replace_child(
            &repo,
            &Path::new(&format!("JOSH_ORIG_PATH_{}", super::to_ns(&root))),
            empty_blob,
            &result,
        )?;

        result = repo.find_tree(r).expect("DIRS filter: can't find new tree");
    }
    let result_id = result.id();

    cache.insert((input, root.to_string()), result_id);
    return Ok(result_id);
}

impl Filter for DirsFilter {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }

    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        return striped_tree(
            &repo,
            "",
            tree.id(),
            &mut self.cache.borrow_mut(),
        );
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        _tree: &git2::Tree,
        _parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        Ok(empty_tree_id())
    }

    fn filter_spec(&self) -> String {
        return ":DIRS".to_owned();
    }
}

fn merged_tree(
    repo: &git2::Repository,
    input1: git2::Oid,
    input2: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    if input1 == input2 {
        return Ok(input1);
    }
    if input1 == empty_tree_id() {
        return Ok(input2);
    }
    if input2 == empty_tree_id() {
        return Ok(input1);
    }

    if let (Ok(tree1), Ok(tree2)) =
        (repo.find_tree(input1), repo.find_tree(input2))
    {
        let mut result_tree = tree1.clone();

        for entry in tree2.iter() {
            if let Some(e) = tree1
                .get_name(entry.name().ok_or(super::josh_error("no name"))?)
            {
                let r = replace_child(
                    &repo,
                    &Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    merged_tree(repo, entry.id(), e.id())?,
                    &result_tree,
                )?;

                result_tree = repo
                    .find_tree(r)
                    .expect("FOLD filter: can't find new tree");
            } else {
                let r = replace_child(
                    &repo,
                    &Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    entry.id(),
                    &result_tree,
                )?;

                result_tree = repo
                    .find_tree(r)
                    .expect("FOLD filter: can't find new tree");
            }
        }

        return Ok(result_tree.id());
    }

    return Ok(input1);
}

struct FoldFilter;

impl Filter for FoldFilter {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }

    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        if forward_maps.has(&repo, &self.filter_spec(), commit.id()) {
            return Ok(forward_maps.get(&self.filter_spec(), commit.id()));
        }

        let filtered_parent_ids =
            self.apply_to_parents(repo, commit, forward_maps, backward_maps)?;

        let mut trees = vec![];
        for parent_id in &filtered_parent_ids {
            trees.push(repo.find_commit(*parent_id)?.tree_id());
        }

        let mut filtered_tree = commit.tree_id();

        for t in trees {
            filtered_tree = merged_tree(repo, filtered_tree, t)?;
        }

        return create_filtered_commit(
            repo,
            commit,
            filtered_parent_ids,
            &find_tree_or_error(
                &repo,
                filtered_tree,
                Some(&commit),
                &self.filter_spec(),
            ),
        );
    }

    fn apply_to_tree(
        &self,
        _repo: &git2::Repository,
        _tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        Ok(empty_tree_id())
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        _tree: &git2::Tree,
        _parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        Ok(empty_tree_id())
    }

    fn filter_spec(&self) -> String {
        return ":FOLD".to_owned();
    }
}

struct EmptyFilter;

impl Filter for EmptyFilter {
    fn apply_to_commit(
        &self,
        _repo: &git2::Repository,
        _commit: &git2::Commit,
        _forward_maps: &mut FilterCache,
        _backward_maps: &mut FilterCache,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        return Ok(git2::Oid::zero());
    }
    fn apply_to_parents(
        &self,
        _repo: &git2::Repository,
        _commit: &git2::Commit,
        _forward_maps: &mut FilterCache,
        _backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return Ok(vec![]);
    }
    fn apply_to_tree(
        &self,
        _repo: &git2::Repository,
        _tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        Ok(empty_tree_id())
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        _tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        Ok(parent_tree.id())
    }

    fn filter_spec(&self) -> String {
        return ":empty".to_owned();
    }
}

struct CutoffFilter {
    /* rev: git2::Oid, */
    name: String,
    /*         rev: ok_or!( */
    /*             repo.revparse_single(&name) */
    /*                 .and_then(|r| r.peel_to_commit()) */
    /*                 .map(|r| r.id()), */
    /*             { */
    /*                 return Box::new(EmptyFilter); */
    /*             } */
    /*         ), */
}

impl Filter for CutoffFilter {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }

    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut FilterCache,
        _backward_maps: &mut FilterCache,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        return scratch::rewrite(&repo, &commit, &vec![], &commit.tree()?);
    }

    fn apply_to_tree(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        Ok(tree.id())
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        Ok(tree.id())
    }

    fn filter_spec(&self) -> String {
        return format!(":cutoff={}", self.name);
    }
}

struct ChainFilter {
    first: Box<dyn Filter>,
    second: Box<dyn Filter>,
}

impl Filter for ChainFilter {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        let r = self.first.apply_to_commit(
            repo,
            commit,
            forward_maps,
            backward_maps,
            _meta,
        )?;

        let commit = ok_or!(repo.find_commit(r), {
            return Ok(git2::Oid::zero());
        });
        return self.second.apply_to_commit(
            repo,
            &commit,
            forward_maps,
            backward_maps,
            _meta,
        );
    }

    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        let r = self.first.apply_to_tree(&repo, &tree, commit_id)?;
        if let Ok(t) = repo.find_tree(r) {
            return self.second.apply_to_tree(&repo, &t, commit_id);
        }
        return Ok(empty_tree_id());
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        let p =
            self.first
                .apply_to_tree(&repo, &parent_tree, git2::Oid::zero())?;
        let p = repo.find_tree(p)?;
        let a = self.second.unapply(&repo, &tree, &p)?;
        self.first.unapply(&repo, &repo.find_tree(a)?, &parent_tree)
    }

    fn filter_spec(&self) -> String {
        return format!(
            "{}{}",
            &self.first.filter_spec(),
            &self.second.filter_spec()
        )
        .replacen(":nop", "", 1);
    }
}

struct SubdirFilter {
    path: std::path::PathBuf,
}

impl SubdirFilter {
    fn new(path: &Path) -> Box<dyn Filter> {
        let mut components = path.iter();
        let mut chain: Box<dyn Filter> = if let Some(comp) = components.next() {
            Box::new(SubdirFilter {
                path: Path::new(comp).to_owned(),
            })
        } else {
            Box::new(NopFilter)
        };

        for comp in components {
            chain = Box::new(ChainFilter {
                first: chain,
                second: Box::new(SubdirFilter {
                    path: Path::new(comp).to_owned(),
                }),
            })
        }
        return chain;
    }
}

impl Filter for SubdirFilter {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn apply_to_tree(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        return Ok(tree
            .get_path(&self.path)
            .map(|x| x.id())
            .unwrap_or(empty_tree_id()));
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        replace_subtree(&repo, &self.path, tree.id(), &parent_tree)
    }

    fn filter_spec(&self) -> String {
        return format!(":/{}", &self.path.to_str().unwrap());
    }
}

struct PrefixFilter {
    prefix: std::path::PathBuf,
}

impl Filter for PrefixFilter {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        replace_subtree(&repo, &self.prefix, tree.id(), &empty_tree(&repo))
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        return Ok(tree
            .get_path(&self.prefix)
            .map(|x| x.id())
            .unwrap_or(empty_tree_id()));
    }

    fn filter_spec(&self) -> String {
        return format!(":prefix={}", &self.prefix.to_str().unwrap());
    }
}

struct HideFilter {
    path: std::path::PathBuf,
}

impl Filter for HideFilter {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        _commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        replace_subtree(&repo, &self.path, git2::Oid::zero(), &tree)
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        let hidden = parent_tree
            .get_path(&self.path)
            .map(|x| x.id())
            .unwrap_or(git2::Oid::zero());
        replace_subtree(&repo, &self.path, hidden, &tree)
    }

    fn filter_spec(&self) -> String {
        return format!(":hide={}", &self.path.to_str().unwrap());
    }
}

struct InfoFileFilter {
    values: std::collections::BTreeMap<String, String>,
}

impl Filter for InfoFileFilter {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        let mut s = "".to_owned();
        for (k, v) in self.values.iter() {
            let v = v.replace("<colon>", ":").replace("<comma>", ",");
            if k == "prefix" {
                continue;
            }
            s = format!(
                "{}{}: {}\n",
                &s,
                k,
                match v.as_str() {
                    "#sha1" => commit_id.to_string(),
                    "#tree" => tree
                        .get_path(&Path::new(&self.values["prefix"]))
                        .map(|x| x.id())
                        .unwrap_or(git2::Oid::zero())
                        .to_string(),
                    _ => v,
                }
            );
        }
        replace_subtree(
            repo,
            &Path::new(&self.values["prefix"]).join(".joshinfo"),
            repo.blob(s.as_bytes())?,
            &tree,
        )
    }

    fn unapply(
        &self,
        _repo: &git2::Repository,
        tree: &git2::Tree,
        _parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        Ok(tree.id())
    }

    fn filter_spec(&self) -> String {
        let mut s = format!(":info=");

        for (k, v) in self.values.iter() {
            s = format!("{}{}={},", s, k, v);
        }
        return s.trim_end_matches(",").to_string();
    }
}

struct CombineFilter {
    base: Box<dyn Filter>,
    others: Vec<Box<dyn Filter>>,
    prefixes: Vec<std::path::PathBuf>,
}

impl Filter for CombineFilter {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        if self.prefixes.len() == 0 {
            return self.base.apply_to_parents(
                &repo,
                &commit,
                forward_maps,
                backward_maps,
            );
        }
        return commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn prefixes(&self) -> HashMap<String, String> {
        let mut p = HashMap::new();
        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            p.insert(prefix.to_str().unwrap().to_owned(), other.filter_spec());
        }
        p
    }

    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        let mut base = self.base.apply_to_tree(&repo, &tree, commit_id)?;

        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            let otree = other.apply_to_tree(&repo, &tree, commit_id)?;
            if otree == empty_tree_id() {
                continue;
            }
            /* let otree = repo.find_tree(otree).expect("can't find tree"); */
            let otree = find_tree_or_error(
                &repo,
                otree,
                repo.find_commit(commit_id).ok().as_ref(),
                &self.filter_spec(),
            );
            base = replace_subtree(
                &repo,
                &prefix,
                otree.id(),
                &repo.find_tree(base)?,
            )?;
        }

        return Ok(base);
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        let mut base_wo = tree.id();

        for prefix in self.prefixes.iter() {
            base_wo = replace_subtree(
                repo,
                prefix,
                empty_tree_id(),
                &repo.find_tree(base_wo)?,
            )?;
        }

        let mut res =
            self.base
                .unapply(repo, &repo.find_tree(base_wo)?, parent_tree)?;

        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            let r = ok_or!(tree.get_path(&prefix).map(|x| x.id()), {
                continue;
            });
            if r == empty_tree_id() {
                continue;
            }
            let r = repo.find_tree(r)?;
            let ua = other.unapply(&repo, &r, &parent_tree)?;

            let merged = repo
                .merge_trees(
                    &parent_tree,
                    &repo.find_tree(res)?,
                    &repo.find_tree(ua)?,
                    Some(
                        git2::MergeOptions::new()
                            .file_favor(git2::FileFavor::Theirs),
                    ),
                )?
                .write_tree_to(&repo)?;

            res = merged;
        }

        return Ok(res);
    }

    fn filter_spec(&self) -> String {
        let mut s = format!("/ = {}", &self.base.filter_spec());

        for (other, prefix) in self.others.iter().zip(self.prefixes.iter()) {
            s = format!(
                "{}\n{} = {}",
                &s,
                prefix.to_str().unwrap(),
                other.filter_spec()
            );
        }
        return s;
    }
}

struct WorkspaceFilter {
    ws_path: std::path::PathBuf,
}

fn combine_filter_from_ws(
    repo: &git2::Repository,
    tree: &git2::Tree,
    ws_path: &Path,
) -> Box<CombineFilter> {
    let base = SubdirFilter::new(&ws_path);
    let wsp = ws_path.join("workspace.josh");
    let ws_config_oid = ok_or!(tree.get_path(&wsp).map(|x| x.id()), {
        return build_combine_filter("", base);
    });

    let ws_blob = ok_or!(repo.find_blob(ws_config_oid), {
        return build_combine_filter("", base);
    });

    let ws_content = ok_or!(std::str::from_utf8(ws_blob.content()), {
        return build_combine_filter("", base);
    });

    return build_combine_filter(ws_content, base);
}

impl WorkspaceFilter {
    fn ws_apply_to_tree_and_parents(
        &self,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
        repo: &git2::Repository,
        tree_and_parents: (git2::Oid, Vec<git2::Oid>),
        commit_id: git2::Oid,
    ) -> super::JoshResult<(git2::Oid, Vec<git2::Oid>)> {
        let (tree, parents) = tree_and_parents;
        let full_tree = repo.find_tree(tree)?;

        let mut in_this = std::collections::HashSet::new();

        let cw = combine_filter_from_ws(repo, &full_tree, &self.ws_path);

        for (other, prefix) in cw.others.iter().zip(cw.prefixes.iter()) {
            in_this.insert(format!(
                "{} = {}",
                prefix.to_str().ok_or(super::josh_error("prefix.to_str"))?,
                other.filter_spec()
            ));
        }

        let mut filtered_parent_ids = vec![];
        for parent in parents.iter() {
            let p = apply_filter_cached(
                repo,
                self,
                *parent,
                forward_maps,
                backward_maps,
            )?;
            if p != git2::Oid::zero() {
                filtered_parent_ids.push(p);
            }

            let parent_commit = repo.find_commit(*parent)?;

            let pcw = combine_filter_from_ws(
                repo,
                &parent_commit.tree()?,
                &self.ws_path,
            );

            for (other, prefix) in pcw.others.iter().zip(pcw.prefixes.iter()) {
                in_this.remove(&format!(
                    "{} = {}",
                    prefix
                        .to_str()
                        .ok_or(super::josh_error("prefix.to_str"))?,
                    other.filter_spec()
                ));
            }
        }
        let mut s = String::new();
        for x in in_this {
            s = format!("{}{}\n", s, x);
        }

        let pcw: Box<dyn Filter> =
            build_combine_filter(&s, Box::new(EmptyFilter));

        for parent in parents {
            if let Ok(parent) = repo.find_commit(parent) {
                let p = pcw.apply_to_commit(
                    &repo,
                    &parent,
                    forward_maps,
                    backward_maps,
                    &mut HashMap::new(),
                )?;
                if p != git2::Oid::zero() {
                    filtered_parent_ids.push(p);
                }
            }
            break;
        }

        return Ok((
            cw.apply_to_tree(repo, &full_tree, commit_id)?,
            filtered_parent_ids,
        ));
    }
}

fn find_tree_or_error<'a>(
    repo: &'a git2::Repository,
    filtered_tree: git2::Oid,
    commit: Option<&git2::Commit>,
    filter_spec: &str,
) -> git2::Tree<'a> {
    ok_or!(repo.find_tree(filtered_tree), {
        tracing::debug!(
                    "Filter.apply_to_commit: can't find tree: {:?} filter_spec: {:?}, original-commit: {:?}, message: {:?}, header: {:?}, obj.kind: {:?}",
                    filtered_tree,
                    filter_spec,
                    commit.map(|x| x.id()),
                    commit.map(|x| x.message()),
                    commit.map(|x| x.raw_header()),
                    repo.find_object(filtered_tree, None).ok().map(|x| x.kind()));
        return empty_tree(&repo);
    })
}

impl Filter for WorkspaceFilter {
    fn apply_to_parents(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<Vec<git2::Oid>> {
        return commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self,
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect();
    }
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
        _meta: &mut HashMap<String, String>,
    ) -> super::JoshResult<git2::Oid> {
        if forward_maps.has(repo, &self.filter_spec(), commit.id()) {
            return Ok(forward_maps.get(&self.filter_spec(), commit.id()));
        }

        let (filtered_tree, filtered_parent_ids) = self
            .ws_apply_to_tree_and_parents(
                forward_maps,
                backward_maps,
                repo,
                (commit.tree_id(), commit.parents().map(|x| x.id()).collect()),
                commit.id(),
            )?;

        return create_filtered_commit(
            repo,
            commit,
            filtered_parent_ids,
            &find_tree_or_error(
                &repo,
                filtered_tree,
                Some(&commit),
                &self.filter_spec(),
            ),
        );
    }

    fn apply_to_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        commit_id: git2::Oid,
    ) -> super::JoshResult<git2::Oid> {
        return combine_filter_from_ws(repo, tree, &self.ws_path)
            .apply_to_tree(repo, tree, commit_id);
    }

    fn unapply(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree,
        parent_tree: &git2::Tree,
    ) -> super::JoshResult<git2::Oid> {
        let mut cw =
            combine_filter_from_ws(repo, tree, &std::path::PathBuf::from(""));

        cw.base = SubdirFilter::new(&self.ws_path);
        return cw.unapply(repo, tree, parent_tree);
    }

    fn filter_spec(&self) -> String {
        return format!(":workspace={}", &self.ws_path.to_str().unwrap());
    }
}

#[derive(Parser)]
#[grammar = "filter_parser.pest"]
struct MyParser;

fn kvargs(args: &[&str]) -> std::collections::BTreeMap<String, String> {
    let mut s = std::collections::BTreeMap::new();
    for p in args {
        let x: Vec<_> = p.split("=").collect();
        if let [k, v] = x.as_slice() {
            s.insert(k.to_owned().to_string(), v.to_owned().to_string());
        } else if let [v] = x.as_slice() {
            s.insert("prefix".to_owned(), v.to_owned().to_string());
        }
    }
    return s;
}

fn make_filter(args: &[&str]) -> Box<dyn Filter> {
    match args {
        ["", arg] => SubdirFilter::new(&Path::new(arg)),
        ["empty", arg] => Box::new(EmptyFilter),
        ["nop"] => Box::new(NopFilter),
        ["info", iargs @ ..] => Box::new(InfoFileFilter {
            values: kvargs(iargs),
        }),
        ["prefix", arg] => Box::new(PrefixFilter {
            prefix: Path::new(arg).to_owned(),
        }),
        ["+", arg] => Box::new(PrefixFilter {
            prefix: Path::new(arg).to_owned(),
        }),
        ["hide", arg] => Box::new(HideFilter {
            path: Path::new(arg).to_owned(),
        }),
        ["cutoff", arg] => Box::new(CutoffFilter {
            name: arg.to_owned().to_string(),
        }),
        ["workspace", arg] => Box::new(WorkspaceFilter {
            ws_path: Path::new(arg).to_owned(),
        }),
        ["DIRS"] => Box::new(DirsFilter {
            cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        }),
        ["FOLD"] => Box::new(FoldFilter),
        _ => Box::new(EmptyFilter),
    }
}

fn parse_item(pair: pest::iterators::Pair<Rule>) -> Box<dyn Filter> {
    match pair.as_rule() {
        Rule::filter => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();
            make_filter(v.as_slice())
        }
        Rule::filter_noarg => {
            let mut inner = pair.into_inner();
            make_filter(&[inner.next().unwrap().as_str()])
        }
        _ => unreachable!(),
    }
}

fn parse_file_entry(
    pair: pest::iterators::Pair<Rule>,
    combine_filter: &mut CombineFilter,
) {
    match pair.as_rule() {
        Rule::file_entry => {
            let mut inner = pair.into_inner();
            let path = inner.next().unwrap().as_str();
            let filter = inner.next().unwrap().as_str();
            let filter = parse(filter);
            combine_filter.prefixes.push(Path::new(path).to_owned());
            combine_filter.others.push(filter);
        }
        _ => unreachable!(),
    }
}

fn build_combine_filter(
    filter_spec: &str,
    base: Box<dyn Filter>,
) -> Box<CombineFilter> {
    let mut combine_filter = Box::new(CombineFilter {
        base: base,
        others: vec![],
        prefixes: vec![],
    });

    if let Ok(r) = MyParser::parse(Rule::workspace_file, filter_spec) {
        let mut r = r;
        let r = r.next().unwrap();
        for pair in r.into_inner() {
            parse_file_entry(pair, &mut combine_filter);
        }
    };

    return combine_filter;
}

pub fn build_chain(
    first: Box<dyn Filter>,
    second: Box<dyn Filter>,
) -> Box<dyn Filter> {
    Box::new(ChainFilter {
        first: first,
        second: second,
    })
}

pub fn parse(filter_spec: &str) -> Box<dyn Filter> {
    if filter_spec == "" {
        return parse(":nop");
    }
    if filter_spec.starts_with("!") || filter_spec.starts_with(":") {
        let mut chain: Option<Box<dyn Filter>> = None;
        if let Ok(r) = MyParser::parse(Rule::filter_spec, filter_spec) {
            let mut r = r;
            let r = r.next().unwrap();
            for pair in r.into_inner() {
                let v = parse_item(pair);
                chain = Some(if let Some(c) = chain {
                    Box::new(ChainFilter {
                        first: c,
                        second: v,
                    })
                } else {
                    v
                });
            }
            return chain.unwrap_or(Box::new(NopFilter));
        };
    }

    return build_combine_filter(filter_spec, Box::new(EmptyFilter));
}

fn get_subtree(tree: &git2::Tree, path: &Path) -> Option<git2::Oid> {
    tree.get_path(path).map(|x| x.id()).ok()
}

fn replace_child(
    repo: &git2::Repository,
    child: &Path,
    oid: git2::Oid,
    full_tree: &git2::Tree,
) -> super::JoshResult<git2::Oid> {
    let mode = if let Ok(_) = repo.find_tree(oid) {
        0o0040000 // GIT_FILEMODE_TREE
    } else {
        0o0100644
    };

    let full_tree_id = {
        let mut builder = repo.treebuilder(Some(&full_tree))?;
        if oid == git2::Oid::zero() {
            builder.remove(child).ok();
        } else {
            builder.insert(child, oid, mode)?;
        }
        builder.write()?
    };
    return Ok(full_tree_id);
}

fn replace_subtree(
    repo: &git2::Repository,
    path: &Path,
    oid: git2::Oid,
    full_tree: &git2::Tree,
) -> super::JoshResult<git2::Oid> {
    if path.components().count() == 1 {
        return Ok(repo
            .find_tree(replace_child(&repo, path, oid, full_tree)?)?
            .id());
    } else {
        let name =
            Path::new(path.file_name().ok_or(super::josh_error("file_name"))?);
        let path = path.parent().ok_or(super::josh_error("path.parent"))?;

        let st = if let Some(st) = get_subtree(&full_tree, path) {
            repo.find_tree(st)?
        } else {
            empty_tree(&repo)
        };

        let tree = repo.find_tree(replace_child(&repo, name, oid, &st)?)?;

        return replace_subtree(&repo, path, tree.id(), full_tree);
    }
}

fn apply_filter_cached(
    repo: &git2::Repository,
    filter: &dyn Filter,
    newrev: git2::Oid,
    forward_maps: &mut filter_cache::FilterCache,
    backward_maps: &mut filter_cache::FilterCache,
) -> super::JoshResult<git2::Oid> {
    if forward_maps.has(repo, &filter.filter_spec(), newrev) {
        return Ok(forward_maps.get(&filter.filter_spec(), newrev));
    }

    let trace_s = tracing::span!(tracing::Level::TRACE, "apply_filter_cached", filter_spec = ?filter.filter_spec());

    let walk = {
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(newrev)?;
        walk
    };

    let mut in_commit_count = 0;
    let mut out_commit_count = 0;
    let mut empty_tree_count = 0;
    for original_commit_id in walk {
        in_commit_count += 1;

        let original_commit = repo.find_commit(original_commit_id?)?;

        let filtered_commit = ok_or!(
            filter.apply_to_commit(
                &repo,
                &original_commit,
                forward_maps,
                backward_maps,
                &mut HashMap::new(),
            ),
            {
                tracing::error!("cannot apply_to_commit");
                git2::Oid::zero()
            }
        );

        if filtered_commit == git2::Oid::zero() {
            empty_tree_count += 1;
        }
        forward_maps.set(
            &filter.filter_spec(),
            original_commit.id(),
            filtered_commit,
        );
        backward_maps.set(
            &filter.filter_spec(),
            filtered_commit,
            original_commit.id(),
        );
        out_commit_count += 1;
    }

    if !forward_maps.has(&repo, &filter.filter_spec(), newrev) {
        forward_maps.set(&filter.filter_spec(), newrev, git2::Oid::zero());
    }
    let rewritten = forward_maps.get(&filter.filter_spec(), newrev);
    tracing::event!(
        parent: &trace_s,
        tracing::Level::TRACE,
        ?in_commit_count,
        ?out_commit_count,
        ?empty_tree_count,
        original = ?newrev.to_string(),
        rewritten = ?rewritten.to_string(),
    );
    return Ok(rewritten);
}
