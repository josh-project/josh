use super::*;
use pest::Parser;
use std::path::Path;

lazy_static! {
    static ref OPTIMIZED: std::sync::Mutex<std::collections::HashMap<Filter, Filter>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
    static ref PRETTIFYED: std::sync::Mutex<std::collections::HashMap<Filter, Filter>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
    static ref FILTERS: std::sync::Mutex<std::collections::HashMap<Filter, Op>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
}

#[derive(Clone, Hash, PartialEq, Eq, Debug, Copy)]
pub struct Filter(git2::Oid);

impl Filter {
    pub fn id(&self) -> git2::Oid {
        self.0
    }

    pub fn is_nop(&self) -> bool {
        let s = format!("{:?}", Op::Nop);
        let nop_id =
            git2::Oid::hash_object(git2::ObjectType::Blob, s.as_bytes())
                .expect("hash_object filter");

        return self.0 == nop_id;
    }
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
    Dirs,

    File(std::path::PathBuf),
    Prefix(std::path::PathBuf),
    Subdir(std::path::PathBuf),
    Workspace(std::path::PathBuf),

    Glob(String),

    Compose(Vec<Filter>),
    Chain(Filter, Filter),
    Subtract(Filter, Filter),
}

pub fn pretty(filter: Filter, indent: usize) -> String {
    let filter = simplify(filter);
    pretty2(&to_op(filter), indent, true)
}

fn pretty2(op: &Op, indent: usize, compose: bool) -> String {
    let i = format!("\n{}", " ".repeat(indent));

    let ff = |filters: &Vec<_>, n| {
        let joined = filters
            .iter()
            .map(|x| pretty(*x, indent + 4))
            .collect::<Vec<_>>()
            .join(&i);
        if indent == 0 {
            joined
        } else {
            format!(
                ":{}[{}{}{}]",
                n,
                &i,
                joined,
                &format!("\n{}", " ".repeat(indent - 4))
            )
        }
    };
    match op {
        Op::Compose(filters) => ff(filters, ""),
        Op::Subtract(a, b) => match (to_op(*a), to_op(*b)) {
            (Op::Nop, Op::Compose(filters)) => ff(&filters, "exclude"),
            (Op::Nop, b) => format!(":exclude[{}]", spec2(&b)),
            (a, b) => format!(":SUBTRACT[\n{}\n ~{}\n]", spec2(&a), spec2(&b)),
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

pub fn spec(filter: Filter) -> String {
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
            format!(":SUBTRACT[{}~{}]", spec(*a), spec(*b))
        }
        Op::Workspace(path) => {
            format!(":workspace={}", path.to_string_lossy())
        }

        Op::Nop => ":nop".to_string(),
        Op::Empty => ":empty".to_string(),
        Op::Dirs => ":DIRS".to_string(),
        Op::Fold => ":FOLD".to_string(),
        Op::Squash => ":SQUASH".to_string(),
        Op::Chain(a, b) => format!("{}{}", spec(*a), spec(*b)),
        Op::Subdir(path) => format!(":/{}", path.to_string_lossy()),
        Op::File(path) => format!("::{}", path.to_string_lossy()),
        Op::Prefix(path) => format!(":prefix={}", path.to_string_lossy()),
        Op::Glob(pattern) => format!("::{}", pattern),
    }
}

pub fn apply_to_commit(
    filter: Filter,
    commit: &git2::Commit,
    transaction: &filter_cache::Transaction,
) -> JoshResult<git2::Oid> {
    apply_to_commit2(&to_op(filter), commit, transaction)
}

fn apply_to_commit2(
    op: &Op,
    commit: &git2::Commit,
    transaction: &filter_cache::Transaction,
) -> JoshResult<git2::Oid> {
    let filter = optimize(to_filter(op.clone()));

    match &to_op(filter) {
        Op::Nop => return Ok(commit.id()),
        Op::Empty => return Ok(git2::Oid::zero()),

        Op::Chain(a, b) => {
            let r = apply_to_commit(*a, &commit, transaction)?;
            if let Ok(r) = transaction.repo().find_commit(r) {
                return apply_to_commit(*b, &r, transaction);
            } else {
                return Ok(git2::Oid::zero());
            }
        }
        Op::Squash => {
            return history::rewrite_commit(
                &transaction.repo(),
                &commit,
                &vec![],
                &commit.tree()?,
            )
        }
        _ => {
            if let Some(oid) = transaction.get(filter, commit.id()) {
                return Ok(oid);
            }
        }
    };

    rs_tracing::trace_scoped!("apply_to_commit", "spec": spec(filter), "commit": commit.id().to_string());

    let filtered_tree = match &to_op(filter) {
        Op::Compose(filters) => {
            let filtered = filters
                .iter()
                .map(|f| apply_to_commit(*f, &commit, transaction))
                .collect::<JoshResult<Vec<_>>>()?;

            let filtered: Vec<_> =
                filters.iter().zip(filtered.into_iter()).collect();

            let filtered = filtered
                .into_iter()
                .filter(|(_, id)| *id != git2::Oid::zero());

            let filtered = filtered
                .into_iter()
                .map(|(f, id)| {
                    Ok((f, transaction.repo().find_commit(id)?.tree()?))
                })
                .collect::<JoshResult<Vec<_>>>()?;

            treeops::compose(&transaction.repo(), filtered)?
        }
        Op::Workspace(ws_path) => {
            let normal_parents = commit
                .parent_ids()
                .map(|parent| history::walk2(filter, parent, transaction))
                .collect::<JoshResult<Vec<git2::Oid>>>()?;

            let cw = compose_filter_from_ws_no_fail(
                &transaction.repo(),
                &commit.tree()?,
                &ws_path,
            )?;

            let extra_parents = commit
                .parents()
                .map(|parent| {
                    rs_tracing::trace_scoped!("parent", "id": parent.id().to_string());
                    let pcw = compose_filter_from_ws_no_fail(
                        &transaction.repo(),
                        &parent.tree()?,
                        &ws_path,
                    )?;

                    apply_to_commit2(
                        &Op::Subtract(
                            to_filter(Op::Compose(cw.clone())),
                            to_filter(Op::Compose(pcw)),
                            ),
                        &parent,
                        transaction,
                    )
                })
                .collect::<JoshResult<Vec<git2::Oid>>>()?;

            let filtered_parent_ids = normal_parents
                .into_iter()
                .chain(extra_parents.into_iter())
                .collect();

            let filtered_tree =
                apply(&transaction.repo(), filter, commit.tree()?)?;

            return history::create_filtered_commit(
                commit,
                filtered_parent_ids,
                filtered_tree,
                transaction,
                filter,
            );
        }
        Op::Fold => {
            let filtered_parent_ids: Vec<git2::Oid> = commit
                .parents()
                .map(|x| history::walk2(filter, x.id(), transaction))
                .collect::<JoshResult<_>>()?;

            let trees: Vec<git2::Oid> = filtered_parent_ids
                .iter()
                .map(|x| Ok(transaction.repo().find_commit(*x)?.tree_id()))
                .collect::<JoshResult<_>>()?;

            let mut filtered_tree = commit.tree_id();

            for t in trees {
                filtered_tree =
                    treeops::overlay(&transaction.repo(), filtered_tree, t)?;
            }

            transaction.repo().find_tree(filtered_tree)?
        }
        Op::Subtract(a, b) => {
            let af = {
                transaction
                    .repo()
                    .find_commit(apply_to_commit(*a, &commit, transaction)?)
                    .map(|x| x.tree_id())
                    .unwrap_or(empty_tree_id())
            };
            let bf = {
                transaction
                    .repo()
                    .find_commit(apply_to_commit(*b, &commit, transaction)?)
                    .map(|x| x.tree_id())
                    .unwrap_or(empty_tree_id())
            };
            let bf = transaction.repo().find_tree(bf)?;
            let bu = unapply(
                &transaction.repo(),
                *b,
                bf,
                empty_tree(&transaction.repo()),
            )?;
            let ba = apply(&transaction.repo(), *a, bu)?;

            transaction.repo().find_tree(treeops::subtract_fast(
                &transaction.repo(),
                af,
                ba.id(),
            )?)?
        }
        _ => apply(&transaction.repo(), filter, commit.tree()?)?,
    };

    let filtered_parent_ids = {
        rs_tracing::trace_scoped!("filtered_parent_ids", "n": commit.parent_ids().len());
        commit
            .parents()
            .map(|x| history::walk2(filter, x.id(), transaction))
            .collect::<JoshResult<_>>()?
    };

    return history::create_filtered_commit(
        commit,
        filtered_parent_ids,
        filtered_tree,
        transaction,
        filter,
    );
}

pub fn apply<'a>(
    repo: &'a git2::Repository,
    filter: Filter,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    apply2(repo, &to_op(filter), tree)
}

fn apply2<'a>(
    repo: &'a git2::Repository,
    op: &Op,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    match op {
        Op::Nop => return Ok(tree),
        Op::Empty => return Ok(empty_tree(&repo)),
        Op::Fold => return Ok(tree),
        Op::Squash => return Ok(tree),

        Op::Glob(pattern) => {
            let pattern = glob::Pattern::new(pattern)?;
            let options = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            };
            treeops::subtract_tree(
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
            let file = tree
                .get_path(&path)
                .map(|x| x.id())
                .unwrap_or(git2::Oid::zero());
            if let Ok(_) = repo.find_blob(file) {
                treeops::replace_subtree(&repo, &path, file, &empty_tree(&repo))
            } else {
                Ok(empty_tree(&repo))
            }
        }

        Op::Subdir(path) => {
            return Ok(tree
                .get_path(&path)
                .and_then(|x| repo.find_tree(x.id()))
                .unwrap_or(empty_tree(&repo)));
        }
        Op::Prefix(path) => treeops::replace_subtree(
            &repo,
            &path,
            tree.id(),
            &empty_tree(&repo),
        ),

        Op::Subtract(a, b) => {
            let af = apply(&repo, *a, tree.clone())?;
            let bf = apply(&repo, *b, tree.clone())?;
            let bu = unapply(&repo, *b, bf, empty_tree(&repo))?;
            let ba = apply(&repo, *a, bu)?;
            Ok(repo.find_tree(treeops::subtract_fast(
                &repo,
                af.id(),
                ba.id(),
            )?)?)
        }

        Op::Dirs => treeops::dirtree(
            &repo,
            "",
            tree.id(),
            &mut std::collections::HashMap::new(),
        ),

        Op::Workspace(path) => apply2(
            repo,
            &Op::Compose(compose_filter_from_ws_no_fail(repo, &tree, &path)?),
            tree,
        ),

        Op::Compose(filters) => {
            let filtered: Vec<_> = filters
                .iter()
                .map(|f| Ok(apply(&repo, *f, tree.clone())?))
                .collect::<JoshResult<_>>()?;
            let filtered: Vec<_> =
                filters.iter().zip(filtered.into_iter()).collect();
            return treeops::compose(&repo, filtered);
        }

        Op::Chain(a, b) => {
            return apply(&repo, *b, apply(&repo, *a, tree)?);
        }
    }
}

pub fn unapply<'a>(
    repo: &'a git2::Repository,
    filter: Filter,
    tree: git2::Tree<'a>,
    parent_tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    unapply2(repo, &to_op(filter), tree, parent_tree)
}

fn unapply2<'a>(
    repo: &'a git2::Repository,
    op: &Op,
    tree: git2::Tree<'a>,
    parent_tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    return match op {
        Op::Nop => Ok(tree),

        Op::Chain(a, b) => {
            let p = apply(&repo, *a, parent_tree.clone())?;
            let x = unapply(&repo, *b, tree, p)?;
            unapply(&repo, *a, x, parent_tree)
        }
        Op::Workspace(path) => {
            let mut cw = build_compose_filter(&string_blob(
                &repo,
                &tree,
                &Path::new("workspace.josh"),
            ))?;
            cw.insert(0, to_filter(Op::Subdir(path.to_owned())));
            return unapply2(repo, &Op::Compose(cw), tree, parent_tree);
        }
        Op::Compose(filters) => {
            let mut remaining = tree.clone();
            let mut result = parent_tree.clone();

            for other in filters.iter().rev() {
                let from_empty = unapply(
                    &repo,
                    *other,
                    remaining.clone(),
                    empty_tree(&repo),
                )?;
                if empty_tree_id() == from_empty.id() {
                    continue;
                }
                result = unapply(&repo, *other, remaining.clone(), result)?;
                let reapply = apply(&repo, *other, from_empty.clone())?;

                remaining = repo.find_tree(treeops::subtract_fast(
                    &repo,
                    remaining.id(),
                    reapply.id(),
                )?)?;
            }

            return Ok(result);
        }

        Op::File(path) => {
            let file = tree
                .get_path(&path)
                .map(|x| x.id())
                .unwrap_or(git2::Oid::zero());
            if let Ok(_) = repo.find_blob(file) {
                treeops::replace_subtree(&repo, &path, file, &parent_tree)
            } else {
                Ok(empty_tree(&repo))
            }
        }

        Op::Subtract(a, b) => match (to_op(*a), to_op(*b)) {
            (Op::Nop, b) => {
                let subtracted = treeops::subtract_fast(
                    &repo,
                    tree.id(),
                    unapply2(repo, &b, tree, empty_tree(&repo))?.id(),
                )?;
                Ok(repo.find_tree(treeops::overlay(
                    &repo,
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
            let subtracted = treeops::subtract_tree(
                &repo,
                "",
                tree.id(),
                &|path, isblob| {
                    isblob && (pattern.matches_path_with(&path, options))
                },
                git2::Oid::zero(),
                &mut std::collections::HashMap::new(),
            )?;
            Ok(repo.find_tree(treeops::overlay(
                &repo,
                parent_tree.id(),
                subtracted.id(),
            )?)?)
        }
        Op::Prefix(path) => Ok(tree
            .get_path(&path)
            .and_then(|x| repo.find_tree(x.id()))
            .unwrap_or(empty_tree(&repo))),
        Op::Subdir(path) => {
            treeops::replace_subtree(&repo, &path, tree.id(), &parent_tree)
        }
        _ => return Err(josh_error("filter not reversible")),
    };
}

fn group(filters: &Vec<Filter>) -> Vec<Vec<Filter>> {
    let mut res: Vec<Vec<Filter>> = vec![];
    for f in filters {
        if res.len() == 0 {
            res.push(vec![*f]);
            continue;
        }

        if let Op::Chain(a, _) = to_op(*f) {
            if let Op::Chain(x, _) = to_op(res[res.len() - 1][0]) {
                if a == x {
                    let n = res.len();
                    res[n - 1].push(*f);
                    continue;
                }
            }
        }
        res.push(vec![f.clone()]);
    }
    if res.len() != filters.len() {
        return res;
    }

    let mut res: Vec<Vec<Filter>> = vec![];
    for f in filters {
        if res.len() == 0 {
            res.push(vec![f.clone()]);
            continue;
        }

        let (_, a) = last_chain(to_filter(Op::Nop), *f);
        {
            let (_, x) = last_chain(to_filter(Op::Nop), res[res.len() - 1][0]);
            {
                if a == x {
                    let n = res.len();
                    res[n - 1].push(*f);
                    continue;
                }
            }
        }
        res.push(vec![*f]);
    }
    return res;
}

fn prefix_sort(filters: &Vec<Filter>) -> Vec<Filter> {
    let mut sorted = filters.clone();
    let mut ok = true;
    sorted.sort_by(|a, b| {
        if let (Op::Chain(a, _), Op::Chain(b, _)) = (to_op(*a), to_op(*b)) {
            if let (Op::Subdir(a), Op::Subdir(b)) = (to_op(a), to_op(b)) {
                return a.partial_cmp(&b).unwrap();
            }
        }
        ok = false;
        std::cmp::Ordering::Equal
    });
    return if ok { sorted } else { filters.clone() };
}

fn common_pre(filters: &Vec<Filter>) -> Option<(Filter, Vec<Filter>)> {
    let mut rest = vec![];
    let mut c: Option<Filter> = None;
    for f in filters {
        if let Op::Chain(a, b) = to_op(*f) {
            rest.push(b);
            if c == None {
                c = Some(a);
            }
            if c != Some(a) {
                return None;
            }
        } else {
            return None;
        }
    }
    if let Some(c) = c {
        return Some((c, rest));
    } else {
        return None;
    }
}

fn common_post(filters: &Vec<Filter>) -> Option<(Filter, Vec<Filter>)> {
    let mut rest = vec![];
    let mut c: Option<Filter> = None;
    for f in filters {
        let (a, b) = last_chain(to_filter(Op::Nop), *f);
        {
            rest.push(a);
            if c == None {
                c = Some(b);
            }
            if c != Some(b) {
                return None;
            }
        }
    }
    if Some(to_filter(Op::Nop)) == c {
        return None;
    } else if let Some(c) = c {
        return Some((c, rest));
    } else {
        return None;
    }
}

fn last_chain(rest: Filter, filter: Filter) -> (Filter, Filter) {
    match to_op(filter) {
        Op::Chain(a, b) => last_chain(to_filter(Op::Chain(rest, a)), b),
        _ => (rest, filter),
    }
}

/*
 * Atempt to create an alternative representaion of a filter AST that is most
 * suitable for fast evaluation and cache reuse.
 */
fn optimize(filter: Filter) -> Filter {
    let mut filter = filter;
    loop {
        let pretty = simplify(filter);
        let optimized = optimization_iterate(filter);
        filter = simplify(optimized);

        if filter == pretty {
            return optimization_iterate(filter);
        }
    }
}

/*
 * Apply optimization steps to a filter until it converges (no rules apply anymore)
 */
fn optimization_iterate(filter: Filter) -> Filter {
    let mut filter = filter;
    loop {
        let optimized = optimization_step(filter);
        if filter == optimized {
            return optimized;
        }
        filter = optimized;
    }
}

/*
 * Atempt to apply one optimization rule to a filter. If no rule applies the input
 * is returned.
 */
fn optimization_step(filter: Filter) -> Filter {
    if let Some(f) = OPTIMIZED.lock().unwrap().get(&filter) {
        return *f;
    }
    rs_tracing::trace_scoped!("optimization_step", "spec": spec(filter));
    let original = filter;
    let result = to_filter(match to_op(filter) {
        Op::Subdir(path) => {
            if path.components().count() > 1 {
                let mut components = path.components();
                let a = components.next().unwrap();
                Op::Chain(
                    to_filter(Op::Subdir(std::path::PathBuf::from(&a))),
                    to_filter(Op::Subdir(components.as_path().to_owned())),
                )
            } else {
                Op::Subdir(path)
            }
        }
        Op::Prefix(path) => {
            if path.components().count() > 1 {
                let mut components = path.components();
                let a = components.next().unwrap();
                Op::Chain(
                    to_filter(Op::Prefix(components.as_path().to_owned())),
                    to_filter(Op::Prefix(std::path::PathBuf::from(&a))),
                )
            } else {
                Op::Prefix(path)
            }
        }
        Op::Compose(filters) if filters.len() == 0 => Op::Empty,
        Op::Compose(filters) if filters.len() == 1 => to_op(filters[0]),
        Op::Compose(mut filters) => {
            filters.dedup();
            let mut grouped = group(&filters);
            if let Some((common, rest)) = common_pre(&filters) {
                Op::Chain(common, to_filter(Op::Compose(rest)))
            } else if let Some((common, rest)) = common_post(&filters) {
                Op::Chain(to_filter(Op::Compose(rest)), common)
            } else if grouped.len() != filters.len() {
                Op::Compose(
                    grouped
                        .drain(..)
                        .map(|x| to_filter(Op::Compose(x)))
                        .collect(),
                )
            } else {
                let mut filters = prefix_sort(&filters);
                Op::Compose(filters.drain(..).map(optimization_step).collect())
            }
        }
        Op::Chain(a, b) => match (to_op(a), to_op(b)) {
            (Op::Chain(x, y), b) => {
                Op::Chain(x, to_filter(Op::Chain(y, to_filter(b))))
            }
            (Op::Nop, b) => b,
            (a, Op::Nop) => a,
            (a, b) => Op::Chain(
                optimization_step(to_filter(a)),
                optimization_step(to_filter(b)),
            ),
        },
        Op::Subtract(a, b) if a == b => Op::Empty,
        Op::Subtract(a, b) => match (to_op(a), to_op(b)) {
            (Op::Empty, _) => Op::Empty,
            (a, Op::Empty) => a,
            (Op::Chain(a, b), Op::Chain(c, d)) if a == c => {
                Op::Chain(a, to_filter(Op::Subtract(b, d)))
            }
            (Op::Compose(mut av), Op::Compose(mut bv)) => {
                let v = av.clone();
                av.retain(|x| !bv.contains(x));
                bv.retain(|x| !v.contains(x));
                Op::Subtract(
                    optimization_step(to_filter(Op::Compose(av))),
                    optimization_step(to_filter(Op::Compose(bv))),
                )
            }
            (a, b) => Op::Subtract(
                optimization_step(to_filter(a)),
                optimization_step(to_filter(b)),
            ),
        },
        _ => to_op(filter),
    });

    OPTIMIZED.lock().unwrap().insert(original, result);
    return result;
}

/*
 * Attempt to create an equivalent representaion of a filter AST, that has fewer nodes than the
 * input, but still has a similar structure.
 * Usefull as a pre-processing step for pretty printing and also during filter optimization.
 */
fn simplify(filter: Filter) -> Filter {
    if let Some(f) = PRETTIFYED.lock().unwrap().get(&filter) {
        return *f;
    }
    rs_tracing::trace_scoped!("simplify", "spec": spec(filter));
    let original = filter;
    let result = to_filter(match to_op(filter) {
        Op::Compose(filters) => {
            let mut out = vec![];
            for f in filters {
                if let Op::Compose(mut v) = to_op(f) {
                    out.append(&mut v);
                } else {
                    out.push(f);
                }
            }
            Op::Compose(out.drain(..).map(simplify).collect())
        }
        Op::Chain(a, b) => match (to_op(a), to_op(b)) {
            (a, Op::Chain(x, y)) => {
                Op::Chain(to_filter(Op::Chain(to_filter(a), x)), y)
            }
            (Op::Prefix(x), Op::Prefix(y)) => Op::Prefix(y.join(x)),
            (Op::Subdir(x), Op::Subdir(y)) => Op::Subdir(x.join(y)),
            (Op::Chain(x, y), b) => match (to_op(x), to_op(y), b.clone()) {
                (x, Op::Prefix(p1), Op::Prefix(p2)) => Op::Chain(
                    simplify(to_filter(x)),
                    to_filter(Op::Prefix(p2.join(p1))),
                ),
                _ => Op::Chain(simplify(a), simplify(to_filter(b))),
            },
            (a, b) => Op::Chain(simplify(to_filter(a)), simplify(to_filter(b))),
        },
        Op::Subtract(a, b) => match (to_op(a), to_op(b)) {
            (a, b) => {
                Op::Subtract(simplify(to_filter(a)), simplify(to_filter(b)))
            }
        },
        _ => to_op(filter),
    });

    let r = if result == original {
        result
    } else {
        simplify(result)
    };

    PRETTIFYED.lock().unwrap().insert(original, r);
    return r;
}

fn compose_filter_from_ws_no_fail(
    repo: &git2::Repository,
    tree: &git2::Tree,
    ws_path: &Path,
) -> JoshResult<Vec<Filter>> {
    let mut cw = build_compose_filter(&string_blob(
        &repo,
        &tree,
        &ws_path.join("workspace.josh"),
    ))
    .unwrap_or(vec![]);
    cw.insert(0, to_filter(Op::Subdir(ws_path.to_owned())));

    return Ok(cw);
}

fn string_blob(
    repo: &git2::Repository,
    tree: &git2::Tree,
    path: &Path,
) -> String {
    let entry_oid = ok_or!(tree.get_path(&path).map(|x| x.id()), {
        return "".to_owned();
    });

    let blob = ok_or!(repo.find_blob(entry_oid), {
        return "".to_owned();
    });

    let content = ok_or!(std::str::from_utf8(blob.content()), {
        return "".to_owned();
    });

    return content.to_owned();
}

#[derive(Parser)]
#[grammar = "filter_parser.pest"]
struct MyParser;

fn make_op(args: &[&str]) -> JoshResult<Op> {
    match args {
        ["nop"] => Ok(Op::Nop),
        ["empty"] => Ok(Op::Empty),
        ["prefix", arg] => Ok(Op::Prefix(Path::new(arg).to_owned())),
        ["workspace", arg] => Ok(Op::Workspace(Path::new(arg).to_owned())),
        ["SQUASH"] => Ok(Op::Squash),
        ["DIRS"] => Ok(Op::Dirs),
        ["FOLD"] => Ok(Op::Fold),
        _ => Err(josh_error("invalid filter")),
    }
}

fn parse_item(pair: pest::iterators::Pair<Rule>) -> JoshResult<Op> {
    match pair.as_rule() {
        Rule::filter => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();
            make_op(v.as_slice())
        }
        Rule::filter_subdir => Ok(Op::Subdir(
            Path::new(pair.into_inner().next().unwrap().as_str()).to_owned(),
        )),
        Rule::filter_presub => {
            let mut inner = pair.into_inner();
            let arg = inner.next().unwrap().as_str();
            if arg.ends_with("/") {
                let arg = arg.trim_end_matches("/");
                Ok(Op::Chain(
                    to_filter(Op::Subdir(std::path::PathBuf::from(arg))),
                    to_filter(make_op(&["prefix", arg])?),
                ))
            } else if arg.contains("*") {
                Ok(Op::Glob(arg.to_string()))
            } else {
                Ok(Op::File(Path::new(arg).to_owned()))
            }
        }
        Rule::filter_noarg => {
            let mut inner = pair.into_inner();
            make_op(&[inner.next().unwrap().as_str()])
        }
        Rule::filter_compose => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();
            Ok(Op::Compose(build_compose_filter(v[0])?))
        }
        Rule::filter_exclude => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();
            Ok(Op::Subtract(
                to_filter(Op::Nop),
                to_filter(Op::Compose(build_compose_filter(v[0])?)),
            ))
        }
        _ => Err(josh_error("parse_item: no match")),
    }
}

fn parse_file_entry(
    pair: pest::iterators::Pair<Rule>,
    filters: &mut Vec<Filter>,
) -> JoshResult<()> {
    match pair.as_rule() {
        Rule::file_entry => {
            let mut inner = pair.into_inner();
            let path = inner.next().unwrap().as_str();
            let filter = inner
                .next()
                .map(|x| x.as_str().to_owned())
                .unwrap_or(format!(":/{}", path));
            let filter = parse(&filter)?;
            let filter = chain(
                filter,
                to_filter(Op::Prefix(Path::new(path).to_owned())),
            );
            filters.push(filter);
            Ok(())
        }
        Rule::filter_spec => {
            let filter = pair.as_str();
            filters.push(parse(&filter)?);
            Ok(())
        }
        Rule::EOI => Ok(()),
        _ => Err(josh_error(&format!("invalid workspace file {:?}", pair))),
    }
}

fn build_compose_filter(filter_spec: &str) -> JoshResult<Vec<Filter>> {
    rs_tracing::trace_scoped!("build_compose_filter");
    let mut filters = vec![];

    if let Ok(mut r) = MyParser::parse(Rule::workspace_file, filter_spec) {
        let r = r.next().unwrap();
        for pair in r.into_inner() {
            parse_file_entry(pair, &mut filters)?;
        }

        return Ok(filters);
    }
    return Err(josh_error(&format!(
        "Invalid workspace:\n----\n{}\n----",
        filter_spec
    )));
}

pub fn chain(first: Filter, second: Filter) -> Filter {
    to_filter(Op::Chain(first, second))
}

pub fn parse(filter_spec: &str) -> JoshResult<Filter> {
    if filter_spec == "" {
        return parse(":nop");
    }
    let mut chain: Option<Op> = None;
    if let Ok(r) = MyParser::parse(Rule::filter_chain, filter_spec) {
        let mut r = r;
        let r = r.next().unwrap();
        for pair in r.into_inner() {
            let v = parse_item(pair)?;
            chain = Some(if let Some(c) = chain {
                Op::Chain(to_filter(c), to_filter(v))
            } else {
                v
            });
        }
        return Ok(optimize(to_filter(chain.unwrap_or(Op::Nop))));
    };

    return Ok(optimize(to_filter(Op::Compose(build_compose_filter(
        filter_spec,
    )?))));
}
