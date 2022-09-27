use super::*;
use pest::Parser;
use std::path::Path;
mod opt;
mod parse;
pub mod tree;

pub use opt::invert;
pub use parse::get_comments;
pub use parse::parse;

lazy_static! {
    static ref FILTERS: std::sync::Mutex<std::collections::HashMap<Filter, Op>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
}

/// Filters are represented as `git2::Oid`, however they are not ever stored
/// inside the repo.
#[derive(
    Clone, Hash, PartialEq, Eq, Copy, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct Filter(git2::Oid);

impl std::convert::TryFrom<String> for Filter {
    type Error = JoshError;
    fn try_from(s: String) -> JoshResult<Filter> {
        parse(&s)
    }
}

impl Into<String> for Filter {
    fn into(self) -> String {
        spec(self)
    }
}

impl Default for Filter {
    fn default() -> Filter {
        nop()
    }
}

impl Filter {
    pub fn id(&self) -> git2::Oid {
        self.0
    }
}

impl std::fmt::Debug for Filter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        to_op(*self).fmt(f)
    }
}

pub fn nop() -> Filter {
    to_filter(Op::Nop)
}

pub fn empty() -> Filter {
    to_filter(Op::Empty)
}

pub fn squash(ids: Option<(&str, &str, &[(git2::Oid, String)])>) -> Filter {
    if let Some((author, email, ids)) = ids {
        to_filter(Op::Squash(Some(
            ids.iter()
                .map(|(x, y)| (*x, (y.clone(), author.to_string(), email.to_string())))
                .collect(),
        )))
    } else {
        to_filter(Op::Squash(None))
    }
}

fn to_filter(op: Op) -> Filter {
    let s = format!("{:?}", op);
    let f = Filter(
        git2::Oid::hash_object(git2::ObjectType::Blob, s.as_bytes()).expect("hash_object filter"),
    );
    FILTERS.lock().unwrap().insert(f, op);
    f
}

fn to_op(filter: Filter) -> Op {
    FILTERS
        .lock()
        .unwrap()
        .get(&filter)
        .expect("unknown filter")
        .clone()
}

#[derive(Clone, Debug)]
enum Op {
    Nop,
    Empty,
    Fold,
    Paths,
    Squash(Option<std::collections::HashMap<git2::Oid, (String, String, String)>>),
    Linear,

    RegexReplace(regex::Regex, String),

    #[cfg(feature = "search")]
    Index,
    Invert,

    File(std::path::PathBuf),
    Prefix(std::path::PathBuf),
    Subdir(std::path::PathBuf),
    Workspace(std::path::PathBuf),

    Glob(String),

    Compose(Vec<Filter>),
    Chain(Filter, Filter),
    Subtract(Filter, Filter),
    Exclude(Filter),
}

/// Pretty print the filter on multiple lines with initial indentation level.
/// Nested filters will be indented with additional 4 spaces per nesting level.
pub fn pretty(filter: Filter, indent: usize) -> String {
    let filter = opt::simplify(filter);

    if let Op::Compose(filters) = to_op(filter) {
        if indent == 0 {
            let i = format!("\n{}", " ".repeat(indent));
            return filters
                .iter()
                .map(|x| pretty2(&to_op(*x), indent + 4, true))
                .collect::<Vec<_>>()
                .join(&i);
        }
    }
    pretty2(&to_op(filter), indent, true)
}

fn pretty2(op: &Op, indent: usize, compose: bool) -> String {
    let ff = |filters: &Vec<_>, n, ind| {
        let ind2 = std::cmp::max(ind, 4);
        let i = format!("\n{}", " ".repeat(ind2));
        let joined = filters
            .iter()
            .map(|x| pretty2(&to_op(*x), ind + 4, true))
            .collect::<Vec<_>>()
            .join(&i);

        format!(
            ":{}[{}{}{}]",
            n,
            &i,
            joined,
            &format!("\n{}", " ".repeat(ind2 - 4))
        )
    };
    match op {
        Op::Compose(filters) => ff(filters, "", indent),
        Op::Subtract(af, bf) => ff(&vec![*af, *bf], "subtract", indent + 4),
        Op::Exclude(bf) => match to_op(*bf) {
            Op::Compose(filters) => ff(&filters, "exclude", indent),
            b => format!(":exclude[{}]", pretty2(&b, indent, false)),
        },
        Op::Chain(a, b) => match (to_op(*a), to_op(*b)) {
            (Op::Subdir(p1), Op::Prefix(p2)) if p1 == p2 => {
                format!("::{}/", parse::quote(&p1.to_string_lossy()))
            }
            (a, Op::Prefix(p)) if compose => {
                format!(
                    "{} = {}",
                    parse::quote(&p.to_string_lossy()),
                    pretty2(&a, indent, false)
                )
            }
            (a, b) => format!(
                "{}{}",
                pretty2(&a, indent, false),
                pretty2(&b, indent, false)
            ),
        },
        _ => spec2(op),
    }
}

pub fn nesting(filter: Filter) -> usize {
    nesting2(&to_op(filter))
}

fn nesting2(op: &Op) -> usize {
    match op {
        Op::Compose(filters) => 1 + filters.iter().map(|f| nesting(*f)).fold(0, |a, b| a.max(b)),
        Op::Exclude(filter) => 1 + nesting(*filter),
        Op::Workspace(_) => usize::MAX,
        Op::Chain(a, b) => 1 + nesting(*a).max(nesting(*b)),
        Op::Subtract(a, b) => 1 + nesting(*a).max(nesting(*b)),
        _ => 0,
    }
}

/// Compact, single line string representation of a filter so that `parse(spec(F)) == F`
/// Note that this is will not be the best human readable representation. For that see `pretty(...)`
pub fn spec(filter: Filter) -> String {
    let filter = opt::simplify(filter);
    spec2(&to_op(filter))
}

fn spec2(op: &Op) -> String {
    match op {
        Op::Compose(filters) => {
            format!(
                ":[{}]",
                filters
                    .iter()
                    .map(|x| spec(*x))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Op::Subtract(a, b) => {
            format!(":subtract[{},{}]", spec(*a), spec(*b))
        }
        Op::Exclude(b) => {
            format!(":exclude[{}]", spec(*b))
        }
        Op::Workspace(path) => {
            format!(":workspace={}", parse::quote(&path.to_string_lossy()))
        }
        Op::RegexReplace(regex, replacement) => {
            format!(
                ":replace={},{}",
                parse::quote(&regex.to_string()),
                parse::quote(&replacement)
            )
        }

        Op::Chain(a, b) => match (to_op(*a), to_op(*b)) {
            (Op::Subdir(p1), Op::Prefix(p2)) if p1 == p2 => {
                format!("::{}/", parse::quote(&p1.to_string_lossy()))
            }
            (a, b) => format!("{}{}", spec2(&a), spec2(&b)),
        },

        Op::Nop => ":/".to_string(),
        Op::Empty => ":empty".to_string(),
        Op::Paths => ":PATHS".to_string(),
        Op::Invert => ":INVERT".to_string(),
        #[cfg(feature = "search")]
        Op::Index => ":INDEX".to_string(),
        Op::Fold => ":FOLD".to_string(),
        Op::Squash(None) => ":SQUASH".to_string(),
        Op::Squash(Some(hs)) => {
            let mut v = hs
                .iter()
                .map(|(x, y)| format!("{}:{}:{}:{}", x, y.0, y.1, y.2))
                .collect::<Vec<String>>();
            v.sort();
            let s = v.join(",");
            let s = git2::Oid::hash_object(git2::ObjectType::Blob, s.as_bytes())
                .expect("hash_object filter");
            format!(":SQUASH={}", s)
        }
        Op::Linear => ":linear".to_string(),
        Op::Subdir(path) => format!(":/{}", parse::quote(&path.to_string_lossy())),
        Op::File(path) => format!("::{}", parse::quote(&path.to_string_lossy())),
        Op::Prefix(path) => format!(":prefix={}", parse::quote(&path.to_string_lossy())),
        Op::Glob(pattern) => format!("::{}", parse::quote(pattern)),
    }
}

pub fn src_path(filter: Filter) -> std::path::PathBuf {
    src_path2(&to_op(filter))
}

fn src_path2(op: &Op) -> std::path::PathBuf {
    normalize_path(&match op {
        Op::Subdir(path) => path.to_owned(),
        Op::File(path) => path.to_owned(),
        Op::Chain(a, b) => src_path(*a).join(src_path(*b)),
        _ => std::path::PathBuf::new(),
    })
}

pub fn dst_path(filter: Filter) -> std::path::PathBuf {
    dst_path2(&to_op(filter))
}

fn dst_path2(op: &Op) -> std::path::PathBuf {
    normalize_path(&match op {
        Op::Prefix(path) => path.to_owned(),
        Op::File(path) => path.to_owned(),
        Op::Chain(a, b) => dst_path(*b).join(dst_path(*a)),
        _ => std::path::PathBuf::new(),
    })
}

/// Calculate the filtered commit for `commit`. This can take some time if done
/// for the first time and thus should generally be done asynchronously.
pub fn apply_to_commit(
    filter: Filter,
    commit: &git2::Commit,
    transaction: &cache::Transaction,
) -> JoshResult<git2::Oid> {
    let filter = opt::optimize(filter);
    loop {
        let filtered = apply_to_commit2(&to_op(filter), commit, transaction)?;

        if let Some(id) = filtered {
            return Ok(id);
        }

        let missing = transaction.get_missing();

        // Since 'missing' is sorted by nesting, the first is always the minimal
        let minimal_nesting = missing.get(0).map(|(f, _)| nesting(*f)).unwrap_or(0);

        for (f, i) in missing {
            if nesting(f) != minimal_nesting {
                break;
            }
            history::walk2(f, i, transaction)?;
        }
    }
}

fn get_workspace<'a>(repo: &'a git2::Repository, tree: &'a git2::Tree<'a>, path: &Path) -> Filter {
    let f = parse::parse(&tree::get_blob(repo, tree, &path.join("workspace.josh")))
        .unwrap_or_else(|_| to_filter(Op::Empty));

    if invert(f).is_ok() {
        f
    } else {
        to_filter(Op::Empty)
    }
}

pub fn apply_to_commit3(
    filter: Filter,
    commit: &git2::Commit,
    transaction: &cache::Transaction,
) -> JoshResult<bool> {
    Ok(apply_to_commit2(&to_op(filter), commit, transaction)?.is_some())
}

fn apply_to_commit2(
    op: &Op,
    commit: &git2::Commit,
    transaction: &cache::Transaction,
) -> JoshResult<Option<git2::Oid>> {
    let repo = transaction.repo();
    let filter = to_filter(op.clone());

    match &op {
        Op::Nop => return Ok(Some(commit.id())),
        Op::Empty => return Ok(Some(git2::Oid::zero())),

        Op::Chain(a, b) => {
            let r = some_or!(apply_to_commit2(&to_op(*a), commit, transaction)?, {
                return Ok(None);
            });
            return if let Ok(r) = repo.find_commit(r) {
                apply_to_commit2(&to_op(*b), &r, transaction)
            } else {
                Ok(Some(git2::Oid::zero()))
            };
        }
        Op::Squash(None) => {
            return Some(history::rewrite_commit(
                repo,
                commit,
                &[],
                &commit.tree()?,
                None,
            ))
            .transpose()
        }
        _ => {
            if let Some(oid) = transaction.get(filter, commit.id()) {
                return Ok(Some(oid));
            }
        }
    };

    rs_tracing::trace_scoped!("apply_to_commit", "spec": spec(filter), "commit": commit.id().to_string());

    let filtered_tree = match &to_op(filter) {
        Op::Squash(Some(ids)) => {
            if let Some(_) = ids.get(&commit.id()) {
                commit.tree()?
            } else {
                for parent in commit.parents() {
                    return Ok(
                        if let Some(fparent) = transaction.get(filter, parent.id()) {
                            Some(history::drop_commit(
                                commit,
                                vec![fparent],
                                transaction,
                                filter,
                            )?)
                        } else {
                            None
                        },
                    );
                }
                tree::empty(repo)
            }
        }
        Op::Linear => {
            let p: Vec<_> = commit.parent_ids().collect();
            if p.is_empty() {
                transaction.insert(filter, commit.id(), commit.id(), true);
                return Ok(Some(commit.id()));
            }
            let parent = some_or!(transaction.get(filter, p[0]), {
                return Ok(None);
            });

            return Some(history::create_filtered_commit(
                commit,
                vec![parent],
                commit.tree()?,
                transaction,
                filter,
                None,
            ))
            .transpose();
        }
        Op::Compose(filters) => {
            let filtered = filters
                .iter()
                .map(|f| apply_to_commit2(&to_op(*f), commit, transaction))
                .collect::<Vec<_>>()
                .into_iter()
                .collect::<JoshResult<Option<Vec<_>>>>()?;

            let filtered = some_or!(filtered, { return Ok(None) });

            let inverted = invert(filter)?;

            if filter == inverted {
                // If the filter is symetric it does not change any paths and uniqueness of
                // mappings is already guaranteed.
                let filtered = filtered
                    .into_iter()
                    .filter(|id| *id != git2::Oid::zero())
                    .into_iter()
                    .map(|id| Ok(repo.find_commit(id)?.tree_id()))
                    .collect::<JoshResult<Vec<_>>>()?;

                tree::compose_fast(transaction, filtered)?
            } else {
                let filtered = filters
                    .iter()
                    .zip(filtered.into_iter())
                    .filter(|(_, id)| *id != git2::Oid::zero())
                    .into_iter()
                    .map(|(f, id)| Ok((f, repo.find_commit(id)?.tree()?)))
                    .collect::<JoshResult<Vec<_>>>()?;

                tree::compose(transaction, filtered)?
            }
        }
        Op::Workspace(ws_path) => {
            let normal_parents = commit
                .parent_ids()
                .map(|parent| transaction.get(filter, parent))
                .collect::<Option<Vec<git2::Oid>>>();

            let normal_parents = some_or!(normal_parents, { return Ok(None) });

            let cw = get_workspace(repo, &commit.tree()?, ws_path);

            let extra_parents = commit
                .parents()
                .map(|parent| {
                    rs_tracing::trace_scoped!("parent", "id": parent.id().to_string());

                    let pcw = get_workspace(
                        repo,
                        &parent.tree().unwrap_or_else(|_| tree::empty(repo)),
                        ws_path,
                    );

                    apply_to_commit2(
                        &to_op(opt::optimize(to_filter(Op::Subtract(cw, pcw)))),
                        &parent,
                        transaction,
                    )
                })
                .collect::<JoshResult<Option<Vec<_>>>>()?;

            let extra_parents = some_or!(extra_parents, { return Ok(None) });

            let filtered_parent_ids = normal_parents
                .into_iter()
                .chain(extra_parents.into_iter())
                .collect();

            let filtered_tree = apply(transaction, filter, commit.tree()?)?;

            return Some(history::create_filtered_commit(
                commit,
                filtered_parent_ids,
                filtered_tree,
                transaction,
                filter,
                None,
            ))
            .transpose();
        }
        Op::Fold => {
            let filtered_parent_ids = commit
                .parents()
                .map(|x| transaction.get(filter, x.id()))
                .collect::<Option<Vec<_>>>();

            let filtered_parent_ids = some_or!(filtered_parent_ids, { return Ok(None) });

            let trees: Vec<git2::Oid> = filtered_parent_ids
                .iter()
                .map(|x| Ok(repo.find_commit(*x)?.tree_id()))
                .collect::<JoshResult<_>>()?;

            let mut filtered_tree = commit.tree_id();

            for t in trees {
                filtered_tree = tree::overlay(repo, filtered_tree, t)?;
            }

            repo.find_tree(filtered_tree)?
        }
        Op::Subtract(a, b) => {
            let af = {
                transaction
                    .repo()
                    .find_commit(some_or!(
                        apply_to_commit2(&to_op(*a), commit, transaction)?,
                        { return Ok(None) }
                    ))
                    .map(|x| x.tree_id())
                    .unwrap_or_else(|_| tree::empty_id())
            };
            let bf = {
                transaction
                    .repo()
                    .find_commit(some_or!(
                        apply_to_commit2(&to_op(*b), commit, transaction)?,
                        { return Ok(None) }
                    ))
                    .map(|x| x.tree_id())
                    .unwrap_or_else(|_| tree::empty_id())
            };
            let bf = repo.find_tree(bf)?;
            let bu = apply(transaction, invert(*b)?, bf)?;
            let ba = apply(transaction, *a, bu)?.id();
            repo.find_tree(tree::subtract(transaction, af, ba)?)?
        }
        Op::Exclude(b) => {
            let bf = {
                transaction
                    .repo()
                    .find_commit(some_or!(
                        apply_to_commit2(&to_op(*b), commit, transaction)?,
                        { return Ok(None) }
                    ))
                    .map(|x| x.tree_id())
                    .unwrap_or_else(|_| tree::empty_id())
            };
            repo.find_tree(tree::subtract(transaction, commit.tree_id(), bf)?)?
        }
        _ => apply(transaction, filter, commit.tree()?)?,
    };

    let filtered_parent_ids = {
        rs_tracing::trace_scoped!("filtered_parent_ids", "n": commit.parent_ids().len());
        commit
            .parents()
            .map(|x| transaction.get(filter, x.id()))
            .collect::<Option<_>>()
    };

    let filtered_parent_ids = some_or!(filtered_parent_ids, { return Ok(None) });

    let message = match to_op(filter) {
        Op::Squash(Some(ids)) => ids.get(&commit.id()).map(|x| x.clone()),
        _ => None,
    };

    Some(history::create_filtered_commit(
        commit,
        filtered_parent_ids,
        filtered_tree,
        transaction,
        filter,
        message,
    ))
    .transpose()
}

/// Filter a single tree. This does not involve walking history and is thus fast in most cases.
pub fn apply<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    apply2(transaction, &to_op(filter), tree)
}

fn apply2<'a>(
    transaction: &'a cache::Transaction,
    op: &Op,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    let repo = transaction.repo();
    match op {
        Op::Nop => Ok(tree),
        Op::Empty => return Ok(tree::empty(repo)),
        Op::Fold => Ok(tree),
        Op::Squash(None) => Ok(tree),
        Op::Squash(Some(_)) => Err(josh_error("not applicable to tree")),
        Op::Linear => Ok(tree),

        Op::RegexReplace(regex, replacement) => {
            tree::regex_replace(tree.id(), &regex, &replacement, transaction)
        }

        Op::Glob(pattern) => {
            let pattern = glob::Pattern::new(pattern)?;
            let options = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            };
            tree::remove_pred(
                transaction,
                "",
                tree.id(),
                &|path, isblob| isblob && (pattern.matches_path_with(path, options)),
                to_filter(op.clone()).id(),
            )
        }
        Op::File(path) => {
            let (file, mode) = tree
                .get_path(path)
                .map(|x| (x.id(), x.filemode()))
                .unwrap_or((git2::Oid::zero(), 0o0100644));
            if repo.find_blob(file).is_ok() {
                tree::insert(repo, &tree::empty(repo), path, file, mode)
            } else {
                Ok(tree::empty(repo))
            }
        }

        Op::Subdir(path) => {
            return Ok(tree
                .get_path(path)
                .and_then(|x| repo.find_tree(x.id()))
                .unwrap_or_else(|_| tree::empty(repo)));
        }
        Op::Prefix(path) => tree::insert(repo, &tree::empty(repo), path, tree.id(), 0o0040000),

        Op::Subtract(a, b) => {
            let af = apply(transaction, *a, tree.clone())?;
            let bf = apply(transaction, *b, tree.clone())?;
            let bu = apply(transaction, invert(*b)?, bf)?;
            let ba = apply(transaction, *a, bu)?.id();
            Ok(repo.find_tree(tree::subtract(transaction, af.id(), ba)?)?)
        }
        Op::Exclude(b) => {
            let bf = apply(transaction, *b, tree.clone())?.id();
            Ok(repo.find_tree(tree::subtract(transaction, tree.id(), bf)?)?)
        }

        Op::Paths => tree::pathstree("", tree.id(), transaction),
        #[cfg(feature = "search")]
        Op::Index => tree::trigram_index(transaction, tree),

        Op::Invert => tree::invert_paths(transaction, "", tree),

        Op::Workspace(path) => {
            let wsj_file = to_filter(Op::File(Path::new("workspace.josh").to_owned()));
            let base = to_filter(Op::Subdir(path.to_owned()));
            let wsj_file = chain(base, wsj_file);
            apply(
                transaction,
                compose(wsj_file, compose(get_workspace(repo, &tree, path), base)),
                tree,
            )
        }

        Op::Compose(filters) => {
            let filtered: Vec<_> = filters
                .iter()
                .map(|f| apply(transaction, *f, tree.clone()))
                .collect::<JoshResult<_>>()?;
            let filtered: Vec<_> = filters.iter().zip(filtered.into_iter()).collect();
            tree::compose(transaction, filtered)
        }

        Op::Chain(a, b) => {
            return apply(transaction, *b, apply(transaction, *a, tree)?);
        }
    }
}

/// Calculate a tree with minimal differences from `parent_tree`
/// such that `apply(unapply(tree, parent_tree)) == tree`
pub fn unapply<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    tree: git2::Tree<'a>,
    parent_tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    if let Ok(inverted) = invert(filter) {
        let matching = apply(transaction, chain(filter, inverted), parent_tree.clone())?;
        let stripped = tree::subtract(transaction, parent_tree.id(), matching.id())?;
        let new_tree = apply(transaction, inverted, tree)?;

        return Ok(transaction.repo().find_tree(tree::overlay(
            transaction.repo(),
            new_tree.id(),
            stripped,
        )?)?);
    }

    if let Some(ws) = unapply_workspace(
        transaction,
        &to_op(filter),
        tree.clone(),
        parent_tree.clone(),
    )? {
        return Ok(ws);
    }

    if let Op::Chain(a, b) = to_op(filter) {
        let p = apply(transaction, a, parent_tree.clone())?;
        return unapply(
            transaction,
            a,
            unapply(transaction, b, tree, p)?,
            parent_tree,
        );
    }

    Err(josh_error("filter cannot be unapplied"))
}

fn unapply_workspace<'a>(
    transaction: &'a cache::Transaction,
    op: &Op,
    tree: git2::Tree<'a>,
    parent_tree: git2::Tree<'a>,
) -> JoshResult<Option<git2::Tree<'a>>> {
    return match op {
        Op::Workspace(path) => {
            let tree = pre_process_tree(transaction.repo(), tree)?;
            let workspace = get_workspace(transaction.repo(), &tree, Path::new(""));
            let original_workspace = get_workspace(transaction.repo(), &parent_tree, path);

            let root = to_filter(Op::Subdir(path.to_owned()));
            let wsj_file = to_filter(Op::File(Path::new("workspace.josh").to_owned()));
            let wsj_file = chain(root, wsj_file);
            let filter = compose(wsj_file, compose(workspace, root));
            let original_filter = compose(wsj_file, compose(original_workspace, root));
            let matching = apply(
                transaction,
                chain(original_filter, invert(original_filter)?),
                parent_tree.clone(),
            )?;
            let stripped = tree::subtract(transaction, parent_tree.id(), matching.id())?;
            let new_tree = apply(transaction, invert(filter)?, tree)?;

            let result = transaction.repo().find_tree(tree::overlay(
                transaction.repo(),
                new_tree.id(),
                stripped,
            )?)?;

            return Ok(Some(result));
        }
        _ => Ok(None),
    };
}

fn pre_process_tree<'a>(
    repo: &'a git2::Repository,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    let path = Path::new("workspace.josh");
    let ws_file = tree::get_blob(repo, &tree, path);
    let parsed = filter::parse(&ws_file)?;

    if invert(parsed).is_err() {
        return Err(josh_error("Invalid workspace: not reversible"));
    }

    let mut blob = String::new();
    if let Ok(c) = get_comments(&ws_file) {
        if !c.is_empty() {
            blob = c;
        }
    }
    let blob = &format!("{}{}\n", &blob, pretty(parsed, 0));

    let tree = tree::insert(
        repo,
        &tree,
        path,
        repo.blob(blob.as_bytes())?,
        0o0100644, // Should this handle filemode?
    )?;

    Ok(tree)
}

/// Create a filter that is the result of feeding the output of `first` into `second`
pub fn chain(first: Filter, second: Filter) -> Filter {
    opt::optimize(to_filter(Op::Chain(first, second)))
}

/// Create a filter that is the result of overlaying the output of `first` onto `second`
pub fn compose(first: Filter, second: Filter) -> Filter {
    opt::optimize(to_filter(Op::Compose(vec![first, second])))
}

/// Compute the warnings (filters not matching anything) for the filter applied to the tree
pub fn compute_warnings<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    tree: git2::Tree<'a>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut filter = filter;

    if let Op::Workspace(path) = to_op(filter) {
        let workspace_filter = &tree::get_blob(
            transaction.repo(),
            &tree,
            &path.join(&Path::new("workspace.josh")),
        );
        if let Ok(res) = parse(workspace_filter) {
            filter = res;
        } else {
            warnings.push("couldn't parse workspace\n".to_string());
            return warnings;
        }
    }

    let filter = opt::flatten(filter);
    if let Op::Compose(filters) = to_op(filter) {
        for f in filters {
            let tree = transaction.repo().find_tree(tree.id());
            if let Ok(tree) = tree {
                warnings.append(&mut compute_warnings2(transaction, f, tree));
            }
        }
    } else {
        warnings.append(&mut compute_warnings2(transaction, filter, tree));
    }
    warnings
}

fn compute_warnings2<'a>(
    transaction: &'a cache::Transaction,
    filter: Filter,
    tree: git2::Tree<'a>,
) -> Vec<String> {
    let mut warnings = Vec::new();

    let tree = apply(transaction, filter, tree);
    if let Ok(tree) = tree {
        if tree.is_empty() {
            warnings.push(format!("No match for \"{}\"", pretty(filter, 2)));
        }
    }
    warnings
}

pub fn make_permissions_filter(filter: Filter, whitelist: Filter, blacklist: Filter) -> Filter {
    rs_tracing::trace_scoped!("make_permissions_filter");

    let filter = chain(to_filter(Op::Paths), filter);
    let filter = chain(filter, to_filter(Op::Invert));
    let filter = chain(
        filter,
        compose(blacklist, to_filter(Op::Subtract(nop(), whitelist))),
    );
    opt::optimize(filter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn src_path_test() {
        assert_eq!(PathBuf::from("x"), src_path(parse(":/x").unwrap()));
        assert_eq!(PathBuf::from("x/y"), src_path(parse(":/x/y").unwrap()));
        assert_eq!(PathBuf::from("x/y"), src_path(parse(":/x::y").unwrap()));
    }

    #[test]
    fn dst_path_test() {
        assert_eq!(PathBuf::from(""), dst_path(parse(":/x").unwrap()));
        assert_eq!(PathBuf::from(""), dst_path(parse(":/x/y").unwrap()));
        assert_eq!(PathBuf::from("y"), dst_path(parse(":/x::y").unwrap()));
        assert_eq!(
            PathBuf::from("a/y"),
            dst_path(parse(":[a=:/x::y/]").unwrap())
        );

        assert_eq!(
            PathBuf::from("c/a"),
            dst_path(parse(":[a=:/x::y/,a/b=:/i]:prefix=c").unwrap())
        );
    }
}

pub fn is_linear(filter: Filter) -> bool {
    match to_op(filter) {
        Op::Linear => true,
        Op::Chain(a, b) => is_linear(a) || is_linear(b),
        _ => false,
    }
}
