use super::*;
use pest::Parser;
use std::path::Path;
mod opt;
mod parse;
pub mod tree;

pub use parse::parse;

lazy_static! {
    static ref FILTERS: std::sync::Mutex<std::collections::HashMap<Filter, Op>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
}

/// Filters are represented as `git2::Oid`, however they are not ever stored
/// inside the repo.
#[derive(Clone, Hash, PartialEq, Eq, Debug, Copy, PartialOrd, Ord)]
pub struct Filter(git2::Oid);

impl Filter {
    pub fn id(&self) -> git2::Oid {
        self.0
    }
}

pub fn nop() -> Filter {
    to_filter(Op::Nop)
}

fn to_filter(op: Op) -> Filter {
    let s = format!("{:?}", op);
    let f = Filter(
        git2::Oid::hash_object(git2::ObjectType::Blob, s.as_bytes())
            .expect("hash_object filter"),
    );
    FILTERS.lock().unwrap().insert(f, op);
    return f;
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
    Squash,
    Paths,

    File(std::path::PathBuf),
    Prefix(std::path::PathBuf),
    Subdir(std::path::PathBuf),
    Workspace(std::path::PathBuf),

    Glob(String),

    Compose(Vec<Filter>),
    Chain(Filter, Filter),
    Subtract(Filter, Filter),
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
    return pretty2(&to_op(filter), indent, true);
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
        Op::Subtract(af, bf) => match (to_op(*af), to_op(*bf)) {
            (Op::Nop, Op::Compose(filters)) => ff(&filters, "exclude", indent),
            (Op::Nop, b) => format!(":exclude[{}]", pretty2(&b, indent, false)),
            _ => ff(&vec![*af, *bf], "subtract", indent + 4),
        },
        Op::Chain(a, b) => match (to_op(*a), to_op(*b)) {
            (Op::Subdir(p1), Op::Prefix(p2)) if p1 == p2 => {
                format!("::{}/", p1.to_string_lossy().to_string())
            }
            (a, Op::Prefix(p)) if compose => {
                format!(
                    "{} = {}",
                    p.to_string_lossy().to_string(),
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
        Op::Workspace(path) => {
            format!(":workspace={}", path.to_string_lossy())
        }

        Op::Chain(a, b) => match (to_op(*a), to_op(*b)) {
            (Op::Subdir(p1), Op::Prefix(p2)) if p1 == p2 => {
                format!("::{}/", p1.to_string_lossy().to_string())
            }
            (a, b) => format!("{}{}", spec2(&a), spec2(&b)),
        },

        Op::Nop => ":nop".to_string(),
        Op::Empty => ":empty".to_string(),
        Op::Paths => ":PATHS".to_string(),
        Op::Fold => ":FOLD".to_string(),
        Op::Squash => ":SQUASH".to_string(),
        Op::Subdir(path) => format!(":/{}", path.to_string_lossy()),
        Op::File(path) => format!("::{}", path.to_string_lossy()),
        Op::Prefix(path) => format!(":prefix={}", path.to_string_lossy()),
        Op::Glob(pattern) => format!("::{}", pattern),
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
    .to_owned()
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
    .to_owned()
}

/// Calculate the filtered commit for `commit`. This can take some time if done
/// for the first time and thus should generally be done asynchronously.
pub fn apply_to_commit(
    filter: Filter,
    commit: &git2::Commit,
    transaction: &cache::Transaction,
) -> JoshResult<git2::Oid> {
    for _ in 0..10000 {
        let filtered = apply_to_commit2(&to_op(filter), commit, transaction)?;

        if let Some(id) = filtered {
            return Ok(id);
        }

        for (f, i) in transaction.get_missing() {
            history::walk2(f, i, transaction)?;
        }
    }

    Err(josh_error("apply_to_commit did not finish"))
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
    let filter = opt::optimize(to_filter(op.clone()));
    let repo = transaction.repo();

    match &to_op(filter) {
        Op::Nop => return Ok(Some(commit.id())),
        Op::Empty => return Ok(Some(git2::Oid::zero())),

        Op::Chain(a, b) => {
            let r = some_or!(
                apply_to_commit2(&to_op(*a), &commit, transaction)?,
                { return Ok(None) }
            );
            if let Ok(r) = repo.find_commit(r) {
                return apply_to_commit2(&to_op(*b), &r, transaction);
            } else {
                return Ok(Some(git2::Oid::zero()));
            }
        }
        Op::Squash => {
            return Some(history::rewrite_commit(
                &repo,
                &commit,
                &vec![],
                &commit.tree()?,
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
        Op::Compose(filters) => {
            let filtered = filters
                .iter()
                .map(|f| apply_to_commit2(&to_op(*f), &commit, transaction))
                .collect::<JoshResult<Vec<_>>>()?
                .into_iter()
                .collect::<Option<Vec<_>>>();

            let filtered = some_or!(filtered, { return Ok(None) });

            let filtered: Vec<_> =
                filters.iter().zip(filtered.into_iter()).collect();

            let filtered = filtered
                .into_iter()
                .filter(|(_, id)| *id != git2::Oid::zero());

            let filtered = filtered
                .into_iter()
                .map(|(f, id)| Ok((f, repo.find_commit(id)?.tree()?)))
                .collect::<JoshResult<Vec<_>>>()?;

            tree::compose(&transaction, filtered)?
        }
        Op::Workspace(ws_path) => {
            let normal_parents = commit
                .parent_ids()
                .map(|parent| transaction.get(filter, parent))
                .collect::<Option<Vec<git2::Oid>>>();

            let normal_parents = some_or!(normal_parents, { return Ok(None) });

            let cw = parse::parse(&tree::get_blob(
                &repo,
                &commit.tree()?,
                &ws_path.join("workspace.josh"),
            ))
            .unwrap_or(to_filter(Op::Empty));

            let extra_parents = commit
                .parents()
                .map(|parent| {
                    rs_tracing::trace_scoped!("parent", "id": parent.id().to_string());
                    let pcw = parse::parse(&tree::get_blob(
                        &repo,
                        &parent.tree().unwrap_or(tree::empty(&repo)),
                        &ws_path.join("workspace.josh"),
                    )).unwrap_or(to_filter(Op::Empty));

                    apply_to_commit2(
                        &Op::Subtract(cw, pcw),
                        &parent,
                        transaction,
                    )
                })
                .collect::<JoshResult<Vec<_>>>()?.into_iter()
                .collect::<Option<Vec<_>>>();

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
            ))
            .transpose();
        }
        Op::Fold => {
            let filtered_parent_ids = commit
                .parents()
                .map(|x| transaction.get(filter, x.id()))
                .collect::<Option<Vec<_>>>();

            let filtered_parent_ids =
                some_or!(filtered_parent_ids, { return Ok(None) });

            let trees: Vec<git2::Oid> = filtered_parent_ids
                .iter()
                .map(|x| Ok(repo.find_commit(*x)?.tree_id()))
                .collect::<JoshResult<_>>()?;

            let mut filtered_tree = commit.tree_id();

            for t in trees {
                filtered_tree = tree::overlay(&repo, filtered_tree, t)?;
            }

            repo.find_tree(filtered_tree)?
        }
        Op::Subtract(a, b) => {
            let af = {
                transaction
                    .repo()
                    .find_commit(some_or!(
                        apply_to_commit2(&to_op(*a), &commit, transaction)?,
                        { return Ok(None) }
                    ))
                    .map(|x| x.tree_id())
                    .unwrap_or(tree::empty_id())
            };
            let bf = {
                transaction
                    .repo()
                    .find_commit(some_or!(
                        apply_to_commit2(&to_op(*b), &commit, transaction)?,
                        { return Ok(None) }
                    ))
                    .map(|x| x.tree_id())
                    .unwrap_or(tree::empty_id())
            };
            let bf = repo.find_tree(bf)?;
            let bu = unapply(&transaction, *b, bf, tree::empty(&repo))?;
            let ba = apply(transaction, *a, bu)?;

            repo.find_tree(tree::subtract(&repo, af, ba.id())?)?
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

    let filtered_parent_ids =
        some_or!(filtered_parent_ids, { return Ok(None) });

    return Some(history::create_filtered_commit(
        commit,
        filtered_parent_ids,
        filtered_tree,
        transaction,
        filter,
    ))
    .transpose();
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
        Op::Nop => return Ok(tree),
        Op::Empty => return Ok(tree::empty(&repo)),
        Op::Fold => return Ok(tree),
        Op::Squash => return Ok(tree),

        Op::Glob(pattern) => {
            let pattern = glob::Pattern::new(pattern)?;
            let options = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            };
            tree::remove_pred(
                &repo,
                "",
                tree.id(),
                &|path, isblob| {
                    isblob && (pattern.matches_path_with(&path, options))
                },
                git2::Oid::zero(),
                &mut std::collections::HashMap::new(),
            )
        }
        Op::File(path) => {
            let (file, mode) = tree
                .get_path(&path)
                .map(|x| (x.id(), x.filemode()))
                .unwrap_or((git2::Oid::zero(), 0o0100644));
            if let Ok(_) = repo.find_blob(file) {
                tree::insert(&repo, &tree::empty(&repo), &path, file, mode)
            } else {
                Ok(tree::empty(&repo))
            }
        }

        Op::Subdir(path) => {
            return Ok(tree
                .get_path(&path)
                .and_then(|x| repo.find_tree(x.id()))
                .unwrap_or(tree::empty(&repo)));
        }
        Op::Prefix(path) => tree::insert(
            &repo,
            &tree::empty(&repo),
            &path,
            tree.id(),
            0o0040000,
        ),

        Op::Subtract(a, b) => {
            let af = apply(transaction, *a, tree.clone())?;
            let bf = apply(transaction, *b, tree.clone())?;
            let bu = unapply(transaction, *b, bf, tree::empty(&repo))?;
            let ba = apply(transaction, *a, bu)?;
            Ok(repo.find_tree(tree::subtract(&repo, af.id(), ba.id())?)?)
        }

        Op::Paths => tree::pathstree("", tree.id(), transaction),

        Op::Workspace(path) => {
            let base = to_filter(Op::Subdir(path.to_owned()));
            if let Ok(cw) = parse::parse(&tree::get_blob(
                &repo,
                &tree,
                &path.join("workspace.josh"),
            )) {
                apply(transaction, compose(base, cw), tree)
            } else {
                apply(transaction, base, tree)
            }
        }

        Op::Compose(filters) => {
            let filtered: Vec<_> = filters
                .iter()
                .map(|f| Ok(apply(transaction, *f, tree.clone())?))
                .collect::<JoshResult<_>>()?;
            let filtered: Vec<_> =
                filters.iter().zip(filtered.into_iter()).collect();
            return tree::compose(transaction, filtered);
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
    unapply2(transaction, &to_op(filter), tree, parent_tree)
}

fn unapply2<'a>(
    transaction: &'a cache::Transaction,
    op: &Op,
    tree: git2::Tree<'a>,
    parent_tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    return match op {
        Op::Nop => Ok(tree),
        Op::Empty => Ok(parent_tree),

        Op::Chain(a, b) => {
            let p = apply(transaction, *a, parent_tree.clone())?;
            let x = unapply(transaction, *b, tree, p)?;
            unapply(transaction, *a, x, parent_tree)
        }
        Op::Workspace(path) => {
            let root = to_filter(Op::Subdir(path.to_owned()));
            let mapped = parse(&tree::get_blob(
                &transaction.repo(),
                &tree,
                &Path::new("workspace.josh"),
            ))?;

            let tree = tree::insert(
                &transaction.repo(),
                &tree,
                &Path::new("workspace.josh"),
                transaction
                    .repo()
                    .blob(&format!("{}\n", pretty(mapped, 0)).as_bytes())?,
                0o0100644, // Should this handle filemode?
            )?;

            return unapply(
                transaction,
                compose(root, mapped),
                tree,
                parent_tree,
            );
        }
        Op::Compose(filters) => {
            let mut remaining = tree.clone();
            let mut result = parent_tree.clone();

            for other in filters.iter().rev() {
                let from_empty = unapply(
                    transaction,
                    *other,
                    remaining.clone(),
                    tree::empty(&transaction.repo()),
                )?;
                if tree::empty_id() == from_empty.id() {
                    continue;
                }
                result =
                    unapply(transaction, *other, remaining.clone(), result)?;
                let reapply = apply(transaction, *other, from_empty.clone())?;

                remaining = transaction.repo().find_tree(tree::subtract(
                    &transaction.repo(),
                    remaining.id(),
                    reapply.id(),
                )?)?;
            }

            return Ok(result);
        }

        Op::File(path) => {
            let (file, mode) = tree
                .get_path(&path)
                .map(|x| (x.id(), x.filemode()))
                .unwrap_or((git2::Oid::zero(), 0o0100644));
            if let Ok(_) = transaction.repo().find_blob(file) {
                tree::insert(
                    &transaction.repo(),
                    &parent_tree,
                    &path,
                    file,
                    mode,
                )
            } else {
                Ok(tree::empty(&transaction.repo()))
            }
        }

        Op::Subtract(a, b) => match (to_op(*a), to_op(*b)) {
            (Op::Nop, b) => {
                let subtracted = tree::subtract(
                    &transaction.repo(),
                    tree.id(),
                    unapply2(
                        transaction,
                        &b,
                        tree,
                        tree::empty(&transaction.repo()),
                    )?
                    .id(),
                )?;
                Ok(transaction.repo().find_tree(tree::overlay(
                    &transaction.repo(),
                    parent_tree.id(),
                    subtracted,
                )?)?)
            }
            _ => return Err(josh_error("filter not reversible")),
        },
        Op::Glob(pattern) => {
            let pattern = glob::Pattern::new(pattern)?;
            let options = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            };
            let subtracted = tree::remove_pred(
                &transaction.repo(),
                "",
                tree.id(),
                &|path, isblob| {
                    isblob && (pattern.matches_path_with(&path, options))
                },
                git2::Oid::zero(),
                &mut std::collections::HashMap::new(),
            )?;
            Ok(transaction.repo().find_tree(tree::overlay(
                &transaction.repo(),
                parent_tree.id(),
                subtracted.id(),
            )?)?)
        }
        Op::Prefix(path) => Ok(tree
            .get_path(&path)
            .and_then(|x| transaction.repo().find_tree(x.id()))
            .unwrap_or(tree::empty(&transaction.repo()))),
        Op::Subdir(path) => tree::insert(
            &transaction.repo(),
            &parent_tree,
            &path,
            tree.id(),
            0o0040000,
        ),
        _ => return Err(josh_error("filter not reversible")),
    };
}

/// Create a filter that is the result of feeding the output of `first` into `second`
pub fn chain(first: Filter, second: Filter) -> Filter {
    opt::optimize(to_filter(Op::Chain(first, second)))
}

/// Create a filter that is the result of overlaying the output of `first` onto `second`
pub fn compose(first: Filter, second: Filter) -> Filter {
    opt::optimize(to_filter(Op::Compose(vec![first, second])))
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
