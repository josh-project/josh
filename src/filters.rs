use super::empty_tree;
use super::empty_tree_id;
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

fn is_empty_root(repo: &git2::Repository, tree: &git2::Tree) -> bool {
    if tree.id() == empty_tree_id() {
        return true;
    }

    let mut all_empty = true;

    for e in tree.iter() {
        if let Ok(Ok(t)) = e.to_object(&repo).map(|x| x.into_tree()) {
            all_empty = all_empty && is_empty_root(&repo, &t);
        } else {
            return false;
        }
    }
    return all_empty;
}

fn create_filtered_commit<'a>(
    repo: &'a git2::Repository,
    original_commmit: &'a git2::Commit,
    filtered_parent_ids: Vec<git2::Oid>,
    filtered_tree: git2::Tree<'a>,
) -> super::JoshResult<git2::Oid> {
    let is_initial_merge = filtered_parent_ids.len() > 1
        && !repo.merge_base_many(&filtered_parent_ids).is_ok();

    let filtered_parent_commits: std::result::Result<Vec<_>, _> =
        filtered_parent_ids
            .iter()
            .filter(|x| **x != git2::Oid::zero())
            .map(|x| repo.find_commit(*x))
            .collect();

    let mut filtered_parent_commits = filtered_parent_commits?;

    if is_initial_merge {
        filtered_parent_commits.retain(|x| x.tree_id() != empty_tree_id());
    }

    let selected_filtered_parent_commits: Vec<&_> = select_parent_commits(
        &original_commmit,
        filtered_tree.id(),
        filtered_parent_commits.iter().collect(),
    );

    if selected_filtered_parent_commits.len() == 0
        && !(original_commmit.parents().len() == 0
            && is_empty_root(&repo, &original_commmit.tree()?))
    {
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
        transaction: &mut super::filter_cache::Transaction,
    ) -> super::JoshResult<git2::Oid> {
        let filtered_tree = self.apply(&repo, commit.tree()?)?;

        let filtered_parent_ids = commit
            .parents()
            .map(|x| {
                apply_filter_cached_impl(repo, self.get(), x.id(), transaction)
            })
            .collect::<super::JoshResult<_>>()?;

        return create_filtered_commit(
            repo,
            commit,
            filtered_parent_ids,
            filtered_tree,
        );
    }

    fn apply<'a>(
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

    fn apply<'a>(
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
    fn apply<'a>(
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

pub fn overlay(
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
                    overlay(repo, entry.id(), e.id())?,
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
        transaction: &mut super::filter_cache::Transaction,
    ) -> super::JoshResult<git2::Oid> {
        let filtered_parent_ids: Vec<git2::Oid> = commit
            .parents()
            .map(|x| {
                apply_filter_cached_impl(repo, self.get(), x.id(), transaction)
            })
            .collect::<super::JoshResult<_>>()?;

        let mut trees = vec![];
        for parent_id in &filtered_parent_ids {
            trees.push(repo.find_commit(*parent_id)?.tree_id());
        }

        let mut filtered_tree = commit.tree_id();

        for t in trees {
            filtered_tree = overlay(repo, filtered_tree, t)?;
        }

        return create_filtered_commit(
            repo,
            commit,
            filtered_parent_ids,
            repo.find_tree(filtered_tree)?,
        );
    }

    fn apply<'a>(
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

struct SquashFilter;

impl Filter for SquashFilter {
    fn get(&self) -> &dyn Filter {
        self
    }

    fn apply_to_commit(
        &self,
        repo: &git2::Repository,
        commit: &git2::Commit,
        _transaction: &mut super::filter_cache::Transaction,
    ) -> super::JoshResult<git2::Oid> {
        return scratch::rewrite(&repo, &commit, &vec![], &commit.tree()?);
    }

    fn apply<'a>(
        &self,
        _repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        Ok(tree)
    }

    fn filter_spec(&self) -> String {
        "SQUASH".to_owned()
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
        transaction: &mut super::filter_cache::Transaction,
    ) -> super::JoshResult<git2::Oid> {
        let r = apply_filter_cached(repo, &*self.first, commit.id())?;

        let commit = ok_or!(repo.find_commit(r), {
            return Ok(git2::Oid::zero());
        });
        return apply_filter_cached(repo, &*self.second, commit.id());
    }

    fn apply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let t = self.first.apply(&repo, tree)?;
        return self.second.apply(&repo, t);
    }

    fn unapply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
        parent_tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let p = self.first.apply(&repo, parent_tree.clone())?;
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
    fn apply<'a>(
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
    fn apply<'a>(
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
    fn apply<'a>(
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
    fn apply<'a>(
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
        Ok(repo.find_tree(overlay(
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
    substract_cache: std::cell::RefCell<
        std::collections::HashMap<(git2::Oid, git2::Oid), git2::Oid>,
    >,
}

pub fn substract<'a>(
    repo: &'a git2::Repository,
    a: git2::Tree<'a>,
    b: git2::Tree<'a>,
) -> super::JoshResult<git2::Tree<'a>> {
    substract_tree(
        &repo,
        "",
        a.id(),
        &|path, _| !b.get_path(path).is_ok(),
        b.id(),
        &mut std::collections::HashMap::new(),
    )
}

impl Filter for CombineFilter {
    fn get(&self) -> &dyn Filter {
        self
    }

    fn apply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let mut result = empty_tree(&repo);
        let mut taken = empty_tree(&repo);

        for other in self.others.iter() {
            let applied = other.apply(&repo, tree.clone())?;
            let taken_applied = other.apply(&repo, taken.clone())?;

            let substracted = substract_tree(
                &repo,
                "",
                applied.id(),
                &|path, _| !taken_applied.get_path(path).is_ok(),
                taken_applied.id(),
                &mut self.substract_cache.borrow_mut(),
            )?;

            taken = other.unapply(&repo, applied.clone(), taken.clone())?;
            result =
                repo.find_tree(overlay(&repo, result.id(), substracted.id())?)?;
        }

        return Ok(result);
    }

    fn unapply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
        parent_tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        let mut remaining = tree.clone();
        let mut result = parent_tree.clone();

        for other in self.others.iter().rev() {
            let from_empty =
                other.unapply(&repo, remaining.clone(), empty_tree(&repo))?;
            if empty_tree_id() == from_empty.id() {
                continue;
            }
            result = other.unapply(&repo, remaining.clone(), result)?;
            let reapply = other.apply(&repo, from_empty.clone())?;
            remaining = substract_tree(
                &repo,
                "",
                remaining.id(),
                &|path, _| !reapply.get_path(path).is_ok(),
                reapply.id(),
                &mut std::collections::HashMap::new(),
            )?;
        }

        return Ok(result);
    }

    fn filter_spec(&self) -> String {
        return self
            .others
            .iter()
            .map(|x| x.filter_spec())
            .collect::<Vec<_>>()
            .join("\n");
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
        repo: &'a git2::Repository,
        tree_and_parents: (git2::Tree<'a>, Vec<git2::Oid>),
        transaction: &mut super::filter_cache::Transaction,
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
            let p = apply_filter_cached_impl(repo, self, *parent, transaction)?;
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
        in_this.retain(|x| !in_parents.contains(x));
        let s = in_this.join("\n");

        let pcw: Box<dyn Filter> = if in_this.len() == 1 {
            parse(&in_this[0])?
        } else {
            build_combine_filter(&s, None)?
        };

        for parent in parents {
            // TODO: maybe consider doing this for the parents individually
            // -> move this into the loop above
            if let Ok(parent) = repo.find_commit(parent) {
                let p = apply_filter_cached(&*repo, &*pcw, parent.id())?;
                if p != git2::Oid::zero() {
                    filtered_parent_ids.push(p);
                }
            }
            break;
        }

        return Ok((cw.apply(repo, full_tree)?, filtered_parent_ids));
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
        transaction: &mut super::filter_cache::Transaction,
    ) -> super::JoshResult<git2::Oid> {
        let (filtered_tree, filtered_parent_ids) = self
            .ws_apply_to_tree_and_parents(
                repo,
                (commit.tree()?, commit.parents().map(|x| x.id()).collect()),
                transaction,
            )?;

        return create_filtered_commit(
            repo,
            commit,
            filtered_parent_ids,
            filtered_tree,
        );
    }

    fn apply<'a>(
        &self,
        repo: &'a git2::Repository,
        tree: git2::Tree<'a>,
    ) -> super::JoshResult<git2::Tree<'a>> {
        if let Ok(cw) = combine_filter_from_ws(repo, &tree, &self.ws_path) {
            cw
        } else {
            build_combine_filter("", Some(SubdirFilter::new(&self.ws_path)))?
        }
        .apply(repo, tree)
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
        ["SQUASH"] => Ok(Box::new(SquashFilter)),
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

pub fn replace_child<'a>(
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
        if oid == git2::Oid::zero() {
            builder.remove(child).ok();
        } else if oid == empty_tree_id() {
            builder.remove(child).ok();
        } else {
            builder.insert(child, oid, mode).ok();
        }
        builder.write()?
    };
    return Ok(repo.find_tree(full_tree_id)?);
}

pub fn replace_subtree<'a>(
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

#[tracing::instrument(skip(repo))]
pub fn apply_filter_cached(
    repo: &git2::Repository,
    filter: &dyn Filter,
    input: git2::Oid,
) -> super::JoshResult<git2::Oid> {
    rs_tracing::trace_scoped!("apply_filter_cached","spec":filter.filter_spec());
    apply_filter_cached_impl(
        repo,
        filter,
        input,
        &mut super::filter_cache::Transaction::new(filter.filter_spec()),
    )
}

#[tracing::instrument(skip(repo, transaction))]
fn apply_filter_cached_impl(
    repo: &git2::Repository,
    filter: &dyn Filter,
    input: git2::Oid,
    transaction: &mut super::filter_cache::Transaction,
) -> super::JoshResult<git2::Oid> {
    rs_tracing::trace_scoped!("apply_filter_cached_impl","spec":filter.filter_spec());
    if filter.filter_spec() == "" {
        return Ok(git2::Oid::zero());
    }

    if transaction.has(repo, input) {
        return Ok(transaction.get(input));
    }

    let walk = {
        let mut walk = repo.revwalk()?;
        walk.set_sorting(git2::Sort::REVERSE | git2::Sort::TOPOLOGICAL)?;
        walk.push(input)?;
        walk
    };

    let mut in_commit_count = 0;
    let mut out_commit_count = 0;
    let mut empty_tree_count = 0;
    for original_commit_id in walk {
        in_commit_count += 1;

        let original_commit = repo.find_commit(original_commit_id?)?;


        let filtered_commit = ok_or!(
            filter.apply_to_commit(&repo, &original_commit, transaction),
            {
                tracing::error!("cannot apply_to_commit");
                git2::Oid::zero()
            }
        );

        if filtered_commit == git2::Oid::zero() {
            empty_tree_count += 1;
        }
        transaction.insert(original_commit.id(), filtered_commit);
        out_commit_count += 1;
    }

    if !transaction.has(&repo, input) {
        transaction.insert(input, git2::Oid::zero());
    }
    let rewritten = transaction.get(input);
    tracing::event!(
        tracing::Level::TRACE,
        ?in_commit_count,
        ?out_commit_count,
        ?empty_tree_count,
        original = ?input,
        rewritten = ?rewritten,
    );
    return Ok(rewritten);
}
