use super::empty_tree;
use super::empty_tree_id;
use super::filter_cache;
use super::filter_cache::FilterCache;
use super::scratch;
use pest::Parser;
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

fn create_filtered_commit<'a>(
    repo: &'a git2::Repository,
    original_commmit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: git2::Tree<'a>,
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
    fn get(&self) -> &dyn Filter;
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<git2::Oid> {
        if forward_maps.has(&repo, &self.filter_spec(), commit.id()) {
            return Ok(forward_maps.get(&self.filter_spec(), commit.id()));
        }
        let filtered_tree = self.apply_to_tree(&repo, commit.tree()?)?;

        let filtered_parent_ids = commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self.get(),
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect::<super::JoshResult<_>>()?;

        return create_filtered_commit(
            repo,
            commit,
            filtered_parent_ids,
            filtered_tree,
        );
    }

    fn apply_to_tree<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>>;

    fn unapply<'a>(
        &self,
        _repo: &'a git2::Repository,
        _tree: git2::Tree<'a>,
        _parent_tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        Err(super::josh_error(&format!(
            "filter not reversible: {:?}",
            self.filter_spec()
        )))
    }

    fn filter_spec(&self) -> String;
}

impl std::fmt::Debug for &dyn Filter {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::result::Result<(), std::fmt::Error> {
        write!(f, "{}", self.filter_spec())
    }
}

struct NopFilter;

impl Filter for NopFilter {
    fn get(&self) -> &dyn Filter {
        self
    }

    fn apply_to_tree<'a>(
        &self,
        _repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        Ok(tree)
    }

    fn unapply<'a>(
        &self,
        _repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
        _parent_tree: git2::Tree,
    ) -> super::JoshResult<git2::Tree<'a>> {
        Ok(tree.clone())
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

fn dirtree<'a>(
    repo: &'a git2::Repository,
    root: &str,
    input: git2::Oid,
    cache: &mut std::collections::HashMap<(git2::Oid, String), git2::Oid>,
) -> super::JoshResult<git2::Tree<'a>> {
    if let Some(cached) = cache.get(&(input, root.to_string())) {
        return Ok(repo.find_tree(*cached)?);
    }

    let tree = repo.find_tree(input)?;
    let mut result = empty_tree(&repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or(super::josh_error("INVALID_FILENAME"))?;

        if entry.kind() == Some(git2::ObjectType::Blob) {
            if name == "workspace.josh" {
                result = replace_child(
                    &repo,
                    &Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    entry.id(),
                    &result,
                )?;
            }
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = dirtree(
                &repo,
                &format!(
                    "{}{}{}",
                    root,
                    if root == "" { "" } else { "/" },
                    entry.name().ok_or(super::josh_error("no name"))?
                ),
                entry.id(),
                cache,
            )?
            .id();

            if s != empty_tree_id() {
                result = replace_child(
                    &repo,
                    &Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    s,
                    &result,
                )?;
            }
        }
    }

    if root != "" {
        let empty_blob = repo.blob("".as_bytes())?;

        result = replace_child(
            &repo,
            &Path::new(&format!("JOSH_ORIG_PATH_{}", super::to_ns(&root))),
            empty_blob,
            &result,
        )?;
    }
    cache.insert((input, root.to_string()), result.id());
    return Ok(result);
}

fn substract_tree<'a>(
    repo: &'a git2::Repository,
    root: &str,
    input: git2::Oid,
    pred: &dyn Fn(&std::path::Path, bool) -> bool,
    key: git2::Oid,
    cache: &mut std::collections::HashMap<(git2::Oid, git2::Oid), git2::Oid>,
) -> super::JoshResult<git2::Tree<'a>> {
    if let Some(cached) = cache.get(&(input, key)) {
        return Ok(repo.find_tree(*cached)?);
    }

    let tree = repo.find_tree(input)?;
    let mut result = empty_tree(&repo);

    for entry in tree.iter() {
        let name = entry.name().ok_or(super::josh_error("INVALID_FILENAME"))?;
        let path = std::path::PathBuf::from(root).join(name);

        if entry.kind() == Some(git2::ObjectType::Blob) {
            if pred(&path, true) {
                result = replace_child(
                    &repo,
                    &Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    entry.id(),
                    &result,
                )?;
            }
        }

        if entry.kind() == Some(git2::ObjectType::Tree) {
            let s = if (root != "") && pred(&path, false) {
                entry.id()
            } else {
                substract_tree(
                    &repo,
                    &format!(
                        "{}{}{}",
                        root,
                        if root == "" { "" } else { "/" },
                        entry.name().ok_or(super::josh_error("no name"))?
                    ),
                    entry.id(),
                    &pred,
                    key,
                    cache,
                )?
                .id()
            };

            if s != empty_tree_id() {
                result = replace_child(
                    &repo,
                    &Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    s,
                    &result,
                )?;
            }
        }
    }

    cache.insert((input, key), result.id());
    return Ok(result);
}

impl Filter for DirsFilter {
    fn get(&self) -> &dyn Filter {
        self
    }
    fn apply_to_tree<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        dirtree(&repo, "", tree.id(), &mut self.cache.borrow_mut())
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
                result_tree = replace_child(
                    &repo,
                    &Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    merged_tree(repo, entry.id(), e.id())?,
                    &result_tree,
                )?;
            } else {
                result_tree = replace_child(
                    &repo,
                    &Path::new(
                        entry.name().ok_or(super::josh_error("no name"))?,
                    ),
                    entry.id(),
                    &result_tree,
                )?;
            }
        }

        return Ok(result_tree.id());
    }

    return Ok(input1);
}

struct FoldFilter;

impl Filter for FoldFilter {
    fn get(&self) -> &dyn Filter {
        self
    }
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<git2::Oid> {
        if forward_maps.has(&repo, &self.filter_spec(), commit.id()) {
            return Ok(forward_maps.get(&self.filter_spec(), commit.id()));
        }

        let filtered_parent_ids: Vec<git2::Oid> = commit
            .parents()
            .map(|x| {
                apply_filter_cached(
                    repo,
                    self.get(),
                    x.id(),
                    forward_maps,
                    backward_maps,
                )
            })
            .collect::<super::JoshResult<_>>()?;

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
            repo.find_tree(filtered_tree)?,
        );
    }

    fn apply_to_tree<'a>(
        &self,
        repo: &'a git2::Repository,
        _tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        Ok(empty_tree(&repo))
    }

    fn filter_spec(&self) -> String {
        return ":FOLD".to_owned();
    }
}

struct CutoffFilter {
    name: String,
}

impl Filter for CutoffFilter {
    fn get(&self) -> &dyn Filter {
        self
    }

    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        _forward_maps: &mut FilterCache,
        _backward_maps: &mut FilterCache,
    ) -> super::JoshResult<git2::Oid> {
        return scratch::rewrite(&repo, &commit, &vec![], &commit.tree()?);
    }

    fn apply_to_tree<'a>(
        &self,
        _repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        Ok(tree)
    }

    fn filter_spec(&self) -> String {
        return format!(":CUTOFF={}", self.name);
    }
}

struct ChainFilter {
    first: Box<dyn Filter>,
    second: Box<dyn Filter>,
}

impl Filter for ChainFilter {
    fn get(&self) -> &dyn Filter {
        self
    }
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<git2::Oid> {
        let r = self.first.apply_to_commit(
            repo,
            commit,
            forward_maps,
            backward_maps,
        )?;

        let commit = ok_or!(repo.find_commit(r), {
            return Ok(git2::Oid::zero());
        });
        return self.second.apply_to_commit(
            repo,
            &commit,
            forward_maps,
            backward_maps,
        );
    }

    fn apply_to_tree<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let t = self.first.apply_to_tree(&repo, tree)?;
        return self.second.apply_to_tree(&repo, t);
    }

    fn unapply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
        parent_tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let p = self.first.apply_to_tree(&repo, parent_tree.clone())?;
        let a = self.second.unapply(&repo, tree, p)?;
        Ok(repo.find_tree(self.first.unapply(&repo, a, parent_tree)?.id())?)
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
    fn get(&self) -> &dyn Filter {
        self
    }
    fn apply_to_tree<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        return Ok(tree
            .get_path(&self.path)
            .and_then(|x| repo.find_tree(x.id()))
            .unwrap_or(empty_tree(&repo)));
    }

    fn unapply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
        parent_tree: git2::Tree,
    ) -> super::JoshResult<git2::Tree<'a>> {
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
    fn get(&self) -> &dyn Filter {
        self
    }
    fn apply_to_tree<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        replace_subtree(&repo, &self.prefix, tree.id(), &empty_tree(&repo))
    }

    fn unapply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
        _parent_tree: git2::Tree,
    ) -> super::JoshResult<git2::Tree<'a>> {
        Ok(tree
            .get_path(&self.prefix)
            .and_then(|x| repo.find_tree(x.id()))
            .unwrap_or(empty_tree(&repo)))
    }

    fn filter_spec(&self) -> String {
        return format!(":prefix={}", &self.prefix.to_str().unwrap());
    }
}

struct HideFilter {
    path: std::path::PathBuf,
}

impl Filter for HideFilter {
    fn get(&self) -> &dyn Filter {
        self
    }
    fn apply_to_tree<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        replace_subtree(&repo, &self.path, git2::Oid::zero(), &tree)
    }

    fn unapply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
        parent_tree: git2::Tree,
    ) -> super::JoshResult<git2::Tree<'a>> {
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

struct GlobFilter {
    pattern: glob::Pattern,
    invert: bool,
    cache: std::cell::RefCell<
        std::collections::HashMap<(git2::Oid, git2::Oid), git2::Oid>,
    >,
}

impl Filter for GlobFilter {
    fn get(&self) -> &dyn Filter {
        self
    }
    fn apply_to_tree<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let options = glob::MatchOptions {
            case_sensitive: true,
            require_literal_separator: true,
            require_literal_leading_dot: true,
        };
        substract_tree(
            &repo,
            "",
            tree.id(),
            &|path, isblob| {
                isblob
                    && (self.invert
                        != self.pattern.matches_path_with(&path, options))
            },
            git2::Oid::zero(),
            &mut self.cache.borrow_mut(),
        )
    }

    fn unapply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
        parent_tree: git2::Tree,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let options = glob::MatchOptions {
            case_sensitive: true,
            require_literal_separator: true,
            require_literal_leading_dot: true,
        };
        let substracted = substract_tree(
            &repo,
            "",
            tree.id(),
            &|path, isblob| {
                isblob
                    && (self.invert
                        != self.pattern.matches_path_with(&path, options))
            },
            git2::Oid::zero(),
            &mut self.cache.borrow_mut(),
        )?;
        Ok(repo.find_tree(merged_tree(
            &repo,
            parent_tree.id(),
            substracted.id(),
        )?)?)
    }

    fn filter_spec(&self) -> String {
        if self.invert {
            return format!(":~glob={}", &self.pattern.as_str());
        } else {
            return format!(":glob={}", &self.pattern.as_str());
        }
    }
}

struct CombineFilter {
    others: Vec<Box<dyn Filter>>,
    cache: std::cell::RefCell<
        std::collections::HashMap<
            (String, git2::Oid, git2::Oid),
            (git2::Oid, git2::Oid),
        >,
    >,
    substract_cache: std::cell::RefCell<
        std::collections::HashMap<(git2::Oid, git2::Oid), git2::Oid>,
    >,
}

impl Filter for CombineFilter {
    fn get(&self) -> &dyn Filter {
        self
    }

    fn apply_to_tree<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let mut result = empty_tree(&repo);
        let mut tree = tree;

        for other in self.others.iter() {
            let rid = result.id();
            let tid = tree.id();

            if let Some((r, t)) =
                self.cache.borrow().get(&(other.filter_spec(), rid, tid))
            {
                result = repo.find_tree(*r)?;
                tree = repo.find_tree(*t)?;
                continue;
            }

            let t2 = other.apply_to_tree(&repo, tree.clone())?;

            let _unapplied =
                other.unapply(&repo, t2.clone(), empty_tree(&repo))?;

            tree = substract_tree(
                &repo,
                "",
                tree.id(),
                &|path, _| !_unapplied.get_path(path).is_ok(),
                _unapplied.id(),
                &mut self.substract_cache.borrow_mut()
            )?;

            result = repo.find_tree(merged_tree(&repo, rid, t2.id())?)?;

            self.cache.borrow_mut().insert(
                (other.filter_spec(), rid, tid),
                (result.id(), tree.id()),
            );
        }

        return Ok(result);
    }

    fn unapply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
        parent_tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let mut ws_tree = tree.clone();
        let mut result = parent_tree.clone();

        for other in self.others.iter().rev() {
            let from_empty =
                other.unapply(&repo, ws_tree.clone(), empty_tree(&repo))?;
            if empty_tree_id() == from_empty.id() {
                continue;
            }
            result = other.unapply(&repo, ws_tree.clone(), result)?;
            let reapply = other.apply_to_tree(&repo, from_empty.clone())?;
            ws_tree = substract_tree(
                &repo,
                "",
                ws_tree.id(),
                &|path, _| !reapply.get_path(path).is_ok(),
                reapply.id(),
                &mut std::collections::HashMap::new(),
            )?;
        }

        return Ok(result);
    }

    fn filter_spec(&self) -> String {
        let mut s = String::new();
        for other in self.others.iter() {
            s = format!("{}\n{}", &s, other.filter_spec());
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
) -> super::JoshResult<Box<CombineFilter>> {
    let base = SubdirFilter::new(&ws_path);
    let wsp = ws_path.join("workspace.josh");
    let ws_config_oid = ok_or!(tree.get_path(&wsp).map(|x| x.id()), {
        return build_combine_filter("", Some(base));
    });

    let ws_blob = ok_or!(repo.find_blob(ws_config_oid), {
        return build_combine_filter("", Some(base));
    });

    let ws_content = ok_or!(std::str::from_utf8(ws_blob.content()), {
        return build_combine_filter("", Some(base));
    });

    return build_combine_filter(ws_content, Some(base));
}

impl WorkspaceFilter {
    fn ws_apply_to_tree_and_parents<'a>(
        &self,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
        repo: &'a git2::Repository,
        tree_and_parents: (git2::Tree<'a>, Vec<git2::Oid>),
    ) -> super::JoshResult<(git2::Tree<'a>, Vec<git2::Oid>)> {
        let (full_tree, parents) = tree_and_parents;

        let mut in_this = vec![];

        let cw = if let Ok(cw) =
            combine_filter_from_ws(repo, &full_tree, &self.ws_path)
        {
            cw
        } else {
            build_combine_filter("", Some(SubdirFilter::new(&self.ws_path)))?
        };

        for other in cw.others.iter() {
            in_this.push(format!("{}", other.filter_spec()));
        }

        let mut in_parents = std::collections::HashSet::new();

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

            let pcw = if let Ok(pcw) = combine_filter_from_ws(
                repo,
                &parent_commit.tree()?,
                &self.ws_path,
            ) {
                pcw
            } else {
                build_combine_filter(
                    "",
                    Some(SubdirFilter::new(&self.ws_path)),
                )?
            };

            for other in pcw.others.iter() {
                in_parents.insert(format!("{}", other.filter_spec()));
            }
        }
        let mut s = String::new();
        in_this.retain(|x| !in_parents.contains(x));
        for x in in_this {
            s = format!("{}\n{}", s, x);
        }

        let pcw: Box<dyn Filter> = build_combine_filter(&s, None)?;

        for parent in parents {
            // TODO: maybe consider doing this for the parents individually
            // -> move this into the loop above
            if let Ok(parent) = repo.find_commit(parent) {
                let p = pcw.apply_to_commit(
                    &repo,
                    &parent,
                    forward_maps,
                    backward_maps,
                )?;
                if p != git2::Oid::zero() {
                    filtered_parent_ids.push(p);
                }
            }
            break;
        }

        return Ok((cw.apply_to_tree(repo, full_tree)?, filtered_parent_ids));
    }
}

impl Filter for WorkspaceFilter {
    fn get(&self) -> &dyn Filter {
        self
    }
    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        forward_maps: &mut FilterCache,
        backward_maps: &mut FilterCache,
    ) -> super::JoshResult<git2::Oid> {
        if forward_maps.has(repo, &self.filter_spec(), commit.id()) {
            return Ok(forward_maps.get(&self.filter_spec(), commit.id()));
        }

        let (filtered_tree, filtered_parent_ids) = self
            .ws_apply_to_tree_and_parents(
                forward_maps,
                backward_maps,
                repo,
                (commit.tree()?, commit.parents().map(|x| x.id()).collect()),
            )?;

        return create_filtered_commit(
            repo,
            commit,
            filtered_parent_ids,
            filtered_tree,
        );
    }

    fn apply_to_tree<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        if let Ok(cw) = combine_filter_from_ws(repo, &tree, &self.ws_path) {
            cw
        } else {
            build_combine_filter("", Some(SubdirFilter::new(&self.ws_path)))?
        }
        .apply_to_tree(repo, tree)
    }

    fn unapply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
        parent_tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let mut cw =
            combine_filter_from_ws(repo, &tree, &std::path::PathBuf::from(""))?;
        cw.others[0] = SubdirFilter::new(&self.ws_path);
        return cw.unapply(repo, tree, parent_tree);
    }

    fn filter_spec(&self) -> String {
        return format!(":workspace={}", &self.ws_path.to_str().unwrap());
    }
}

#[derive(Parser)]
#[grammar = "filter_parser.pest"]
struct MyParser;


fn make_filter(args: &[&str]) -> super::JoshResult<Box<dyn Filter>> {
    match args {
        ["", arg] => Ok(SubdirFilter::new(&Path::new(arg))),
        ["nop"] => Ok(Box::new(NopFilter)),
        ["prefix", arg] => Ok(Box::new(PrefixFilter {
            prefix: Path::new(arg).to_owned(),
        })),
        ["+", arg] => Ok(Box::new(PrefixFilter {
            prefix: Path::new(arg).to_owned(),
        })),
        ["hide", arg] => Ok(Box::new(HideFilter {
            path: Path::new(arg).to_owned(),
        })),
        ["~glob", arg] => Ok(Box::new(GlobFilter {
            pattern: glob::Pattern::new(arg).unwrap(),
            invert: true,
            cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        })),
        ["glob", arg] => Ok(Box::new(GlobFilter {
            pattern: glob::Pattern::new(arg).unwrap(),
            invert: false,
            cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        })),
        ["workspace", arg] => Ok(Box::new(WorkspaceFilter {
            ws_path: Path::new(arg).to_owned(),
        })),
        ["CUTOFF", arg] => Ok(Box::new(CutoffFilter {
            name: arg.to_owned().to_string(),
        })),
        ["DIRS"] => Ok(Box::new(DirsFilter {
            cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        })),
        ["FOLD"] => Ok(Box::new(FoldFilter)),
        _ => Err(super::josh_error("invalid filter")),
    }
}

fn parse_item(
    pair: pest::iterators::Pair<Rule>,
) -> super::JoshResult<Box<dyn Filter>> {
    match pair.as_rule() {
        Rule::filter => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();
            make_filter(v.as_slice())
        }
        Rule::filter_noarg => {
            let mut inner = pair.into_inner();
            make_filter(&[inner.next().unwrap().as_str()])
        }
        _ => Err(super::josh_error("parse_item: no match")),
    }
}

fn parse_file_entry(
    pair: pest::iterators::Pair<Rule>,
    combine_filter: &mut CombineFilter,
) -> super::JoshResult<()> {
    match pair.as_rule() {
        Rule::file_entry => {
            let mut inner = pair.into_inner();
            let path = inner.next().unwrap().as_str();
            let filter = inner
                .next()
                .map(|x| x.as_str().to_owned())
                .unwrap_or(format!(":/{}", path));
            let filter = parse(&filter)?;
            let filter = build_chain(
                filter,
                Box::new(PrefixFilter {
                    prefix: Path::new(path).to_owned(),
                }),
            );
            combine_filter.others.push(filter);
            Ok(())
        }
        Rule::filter_spec => {
            let filter = pair.as_str();
            let filter = parse(&filter)?;
            combine_filter.others.push(filter);
            Ok(())
        }
        Rule::EOI => Ok(()),
        _ => Err(super::josh_error(&format!(
            "invalid workspace file {:?}",
            pair
        ))),
    }
}

fn build_combine_filter(
    filter_spec: &str,
    base: Option<Box<dyn Filter>>,
) -> super::JoshResult<Box<CombineFilter>> {
    let mut combine_filter = Box::new(CombineFilter {
        others: if let Some(base) = base {
            vec![base]
        } else {
            vec![]
        },
        cache: std::cell::RefCell::new(std::collections::HashMap::new()),
        substract_cache: std::cell::RefCell::new(
            std::collections::HashMap::new(),
        ),
    });

    if let Ok(mut r) = MyParser::parse(Rule::workspace_file, filter_spec) {
        let r = r.next().unwrap();
        for pair in r.into_inner() {
            parse_file_entry(pair, &mut combine_filter)?;
        }

        return Ok(combine_filter);
    }
    return Err(super::josh_error(&format!(
        "Invalid workspace:\n----\n{}\n----",
        filter_spec
    )));
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

pub fn parse(filter_spec: &str) -> super::JoshResult<Box<dyn Filter>> {
    if filter_spec == "" {
        return parse(":nop");
    }
    if filter_spec.starts_with("!") || filter_spec.starts_with(":") {
        let mut chain: Option<Box<dyn Filter>> = None;
        if let Ok(r) = MyParser::parse(Rule::filter_spec, filter_spec) {
            let mut r = r;
            let r = r.next().unwrap();
            for pair in r.into_inner() {
                let v = parse_item(pair)?;
                chain = Some(if let Some(c) = chain {
                    Box::new(ChainFilter {
                        first: c,
                        second: v,
                    })
                } else {
                    v
                });
            }
            return Ok(chain.unwrap_or(Box::new(NopFilter)));
        };
    }

    return Ok(build_combine_filter(filter_spec, None)?);
}

fn get_subtree(tree: &git2::Tree, path: &Path) -> Option<git2::Oid> {
    tree.get_path(path).map(|x| x.id()).ok()
}

fn replace_child<'a>(
    repo: &'a git2::Repository,
    child: &Path,
    oid: git2::Oid,
    full_tree: &git2::Tree,
) -> super::JoshResult<git2::Tree<'a>> {
    let mode = if let Ok(_) = repo.find_tree(oid) {
        0o0040000 // GIT_FILEMODE_TREE
    } else {
        0o0100644
    };

    let full_tree_id = {
        let mut builder = repo.treebuilder(Some(&full_tree))?;
        if oid == git2::Oid::zero() || oid == empty_tree_id() {
            builder.remove(child).ok();
        } else {
            builder.insert(child, oid, mode).ok();
        }
        builder.write()?
    };
    return Ok(repo.find_tree(full_tree_id)?);
}

fn replace_subtree<'a>(
    repo: &'a git2::Repository,
    path: &Path,
    oid: git2::Oid,
    full_tree: &git2::Tree,
) -> super::JoshResult<git2::Tree<'a>> {
    if path.components().count() == 1 {
        return replace_child(&repo, path, oid, full_tree);
    } else {
        let name =
            Path::new(path.file_name().ok_or(super::josh_error("file_name"))?);
        let path = path.parent().ok_or(super::josh_error("path.parent"))?;

        let st = if let Some(st) = get_subtree(&full_tree, path) {
            repo.find_tree(st).unwrap_or(empty_tree(&repo))
        } else {
            empty_tree(&repo)
        };

        let tree = replace_child(&repo, name, oid, &st)?;

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
    if filter.filter_spec() == "" {
        return Ok(git2::Oid::zero());
    }

    if forward_maps.has(repo, &filter.filter_spec(), newrev) {
        return Ok(forward_maps.get(&filter.filter_spec(), newrev));
    }

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
        tracing::Level::TRACE,
        ?in_commit_count,
        ?out_commit_count,
        ?empty_tree_count,
        original = ?newrev.to_string(),
        rewritten = ?rewritten.to_string(),
    );
    return Ok(rewritten);
}
