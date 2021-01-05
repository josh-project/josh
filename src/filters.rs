use super::*;
use pest::Parser;
use std::path::Path;

lazy_static! {
    static ref FILTERS: std::sync::Mutex<std::collections::HashMap<Filter, Filter>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
}

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Filter(Op);

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
enum Op {
    Nop,
    Empty,
    Fold,
    Squash,
    Dirs,

    Hide(std::path::PathBuf),
    Prefix(std::path::PathBuf),
    Subdir(std::path::PathBuf),
    Workspace(std::path::PathBuf),

    Glob(String),

    Compose(Vec<Filter>),
    Chain(Box<Filter>, Box<Filter>),
    Subtract(Box<Filter>, Box<Filter>),
}

pub fn pretty(filter: &Filter, indent: usize) -> String {
    let i = format!("\n{}", " ".repeat(indent));
    match &filter.0 {
        Op::Compose(filters) => {
            let joined = filters
                .iter()
                .map(|x| pretty(x, indent + 4))
                .collect::<Vec<_>>()
                .join(&i);
            if indent == 0 {
                joined
            } else {
                format!(
                    ":({}{}{})",
                    &i,
                    joined,
                    &format!("\n{}", " ".repeat(indent - 4))
                )
            }
        }
        Op::Subtract(a, b) => {
            format!(":SUBTRACT(\n{}\n -{}\n)", spec(&a), spec(&b))
        }
        Op::Chain(a, b) => match (a.as_ref(), b.as_ref()) {
            (Filter(Op::Subdir(p1)), Filter(Op::Prefix(p2))) if p1 == p2 => {
                format!("::{}/", p1.to_string_lossy().to_string())
            }
            (a, b) => format!("{}{}", pretty(&a, indent), pretty(&b, indent)),
        },
        _ => spec(filter),
    }
}

pub fn spec(filter: &Filter) -> String {
    match &filter.0 {
        Op::Compose(filters) => {
            format!(
                ":({})",
                filters.iter().map(spec).collect::<Vec<_>>().join("&")
            )
        }
        Op::Subtract(a, b) => {
            format!(":SUBTRACT({} - {})", spec(&a), spec(&b))
        }
        Op::Workspace(path) => {
            format!(":workspace={}", path.to_string_lossy())
        }

        Op::Nop => ":nop".to_string(),
        Op::Empty => ":empty".to_string(),
        Op::Dirs => ":DIRS".to_string(),
        Op::Fold => ":FOLD".to_string(),
        Op::Squash => ":SQUASH".to_string(),
        Op::Chain(a, b) => format!("{}{}", spec(&a), spec(&b)),
        Op::Subdir(path) => format!(":/{}", path.to_string_lossy()),
        Op::Prefix(path) => format!(":prefix={}", path.to_string_lossy()),
        Op::Hide(path) => format!(":hide={}", path.to_string_lossy()),
        Op::Glob(pattern) => format!(":glob={}", pattern),
    }
}

pub fn apply_to_commit(
    repo: &git2::Repository,
    filter: &Filter,
    commit: &git2::Commit,
) -> JoshResult<git2::Oid> {
    apply_to_commit2(
        repo,
        filter,
        commit,
        &mut filter_cache::Transaction::new(&repo),
    )
}

pub fn apply_to_commit2(
    repo: &git2::Repository,
    filter: &Filter,
    commit: &git2::Commit,
    transaction: &mut filter_cache::Transaction,
) -> JoshResult<git2::Oid> {
    let filter = optimize(filter.clone());

    match &filter.0 {
        Op::Nop => return Ok(commit.id()),
        Op::Empty => return Ok(git2::Oid::zero()),

        Op::Chain(a, b) => {
            let r = apply_to_commit2(repo, &a, &commit, transaction)?;
            if let Ok(r) = repo.find_commit(r) {
                return apply_to_commit2(repo, &b, &r, transaction);
            } else {
                return Ok(git2::Oid::zero());
            }
        }
        Op::Squash => {
            return scratch::rewrite_commit(
                &repo,
                &commit,
                &vec![],
                &commit.tree()?,
            )
        }
        _ => {
            if let Some(oid) =
                transaction.get(&filters::spec(&filter), commit.id())
            {
                return Ok(oid);
            }
        }
    };

    rs_tracing::trace_scoped!("apply_to_commit", "spec": spec(&filter), "commit": commit.id().to_string());

    let filtered_tree = match &filter.0 {
        Op::Compose(filters) => {
            let filtered = filters
                .iter()
                .map(|f| apply_to_commit2(&repo, &f, &commit, transaction))
                .collect::<JoshResult<Vec<_>>>()?;

            let filtered: Vec<_> =
                filters.iter().zip(filtered.into_iter()).collect();

            let filtered = filtered
                .into_iter()
                .filter(|(_, id)| *id != git2::Oid::zero());

            let filtered = filtered
                .into_iter()
                .map(|(f, id)| Ok((f, repo.find_commit(id)?.tree()?)))
                .collect::<JoshResult<Vec<_>>>()?;

            treeops::compose(&repo, filtered)?
        }
        Op::Workspace(ws_path) => {
            let normal_parents = commit
                .parent_ids()
                .map(|parent| {
                    history::walk2(repo, &filter, parent, transaction)
                })
                .collect::<JoshResult<Vec<git2::Oid>>>()?;

            let cw = compose_filter_from_ws_no_fail(
                repo,
                &commit.tree()?,
                &ws_path,
            )?;

            let extra_parents = commit
                .parents()
                .map(|parent| {
                    rs_tracing::trace_scoped!("parent", "id": parent.id().to_string());
                    let pcw = compose_filter_from_ws_no_fail(
                        repo,
                        &parent.tree()?,
                        &ws_path,
                    )?;

                    apply_to_commit2(
                        repo,
                        &Filter(Op::Subtract(
                            Box::new(Filter(Op::Compose(cw.clone()))),
                            Box::new(Filter(Op::Compose(pcw))),
                        )),
                        &parent,
                        transaction,
                    )
                })
                .collect::<JoshResult<Vec<git2::Oid>>>()?;

            let filtered_parent_ids = normal_parents
                .into_iter()
                .chain(extra_parents.into_iter())
                .collect();

            let filtered_tree = apply(repo, &filter, commit.tree()?)?;

            return scratch::create_filtered_commit(
                repo,
                commit,
                filtered_parent_ids,
                filtered_tree,
                transaction,
                &spec(&filter),
            );
        }
        Op::Fold => {
            let filtered_parent_ids: Vec<git2::Oid> = commit
                .parents()
                .map(|x| history::walk2(repo, &filter, x.id(), transaction))
                .collect::<JoshResult<_>>()?;

            let trees: Vec<git2::Oid> = filtered_parent_ids
                .iter()
                .map(|x| Ok(repo.find_commit(*x)?.tree_id()))
                .collect::<JoshResult<_>>()?;

            let mut filtered_tree = commit.tree_id();

            for t in trees {
                filtered_tree = treeops::overlay(repo, filtered_tree, t)?;
            }

            repo.find_tree(filtered_tree)?
        }
        Op::Subtract(a, b) => {
            let af = {
                repo.find_commit(apply_to_commit2(
                    &repo,
                    &a,
                    &commit,
                    transaction,
                )?)
                .map(|x| x.tree_id())
                .unwrap_or(empty_tree_id())
            };
            let bf = {
                repo.find_commit(apply_to_commit2(
                    &repo,
                    &b,
                    &commit,
                    transaction,
                )?)
                .map(|x| x.tree_id())
                .unwrap_or(empty_tree_id())
            };
            let bf = repo.find_tree(bf)?;
            let bu = unapply(&repo, &b, bf, empty_tree(&repo))?;
            let ba = apply(&repo, &a, bu)?;

            repo.find_tree(treeops::subtract_fast(&repo, af, ba.id())?)?
        }
        _ => apply(&repo, &filter, commit.tree()?)?,
    };

    let filtered_parent_ids = {
        rs_tracing::trace_scoped!("filtered_parent_ids", "n": commit.parent_ids().len());
        commit
            .parents()
            .map(|x| history::walk2(repo, &filter, x.id(), transaction))
            .collect::<JoshResult<_>>()?
    };

    return scratch::create_filtered_commit(
        repo,
        commit,
        filtered_parent_ids,
        filtered_tree,
        transaction,
        &spec(&filter),
    );
}

pub fn apply<'a>(
    repo: &'a git2::Repository,
    filter: &Filter,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    match &filter.0 {
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

        Op::Hide(path) => {
            treeops::replace_subtree(&repo, &path, git2::Oid::zero(), &tree)
        }

        Op::Subtract(a, b) => {
            let af = apply(&repo, &a, tree.clone())?;
            let bf = apply(&repo, &b, tree.clone())?;
            let bu = unapply(&repo, &b, bf, empty_tree(&repo))?;
            let ba = apply(&repo, &a, bu)?;
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

        Op::Workspace(path) => apply(
            repo,
            &Filter(Op::Compose(compose_filter_from_ws_no_fail(
                repo, &tree, &path,
            )?)),
            tree,
        ),

        Op::Compose(filters) => {
            let filtered: Vec<_> = filters
                .iter()
                .map(|f| Ok(apply(&repo, &f, tree.clone())?))
                .collect::<JoshResult<_>>()?;
            let filtered: Vec<_> =
                filters.iter().zip(filtered.into_iter()).collect();
            return treeops::compose(&repo, filtered);
        }

        Op::Chain(a, b) => {
            return apply(&repo, &b, apply(&repo, &a, tree)?);
        }
    }
}

pub fn unapply<'a>(
    repo: &'a git2::Repository,
    filter: &Filter,
    tree: git2::Tree<'a>,
    parent_tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    return match &filter.0 {
        Op::Nop => Ok(tree),

        Op::Chain(a, b) => {
            let p = apply(&repo, &a, parent_tree.clone())?;
            let x = unapply(&repo, &b, tree, p)?;
            unapply(&repo, &a, x, parent_tree)
        }
        Op::Workspace(path) => {
            let cw = build_compose_filter(
                &string_blob(&repo, &tree, &Path::new("workspace.josh")),
                vec![Filter(Op::Subdir(path.to_owned()))],
            )?;
            return unapply(repo, &Filter(Op::Compose(cw)), tree, parent_tree);
        }
        Op::Compose(filters) => {
            let mut remaining = tree.clone();
            let mut result = parent_tree.clone();

            for other in filters.iter().rev() {
                let from_empty = unapply(
                    &repo,
                    &other,
                    remaining.clone(),
                    empty_tree(&repo),
                )?;
                if empty_tree_id() == from_empty.id() {
                    continue;
                }
                result = unapply(&repo, &other, remaining.clone(), result)?;
                let reapply = apply(&repo, &other, from_empty.clone())?;

                remaining = repo.find_tree(treeops::subtract_fast(
                    &repo,
                    remaining.id(),
                    reapply.id(),
                )?)?;
            }

            return Ok(result);
        }
        Op::Hide(path) => {
            let hidden = parent_tree
                .get_path(&path)
                .map(|x| x.id())
                .unwrap_or(git2::Oid::zero());
            treeops::replace_subtree(&repo, &path, hidden, &tree)
        }
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
            res.push(vec![f.clone()]);
            continue;
        }

        if let Filter(Op::Chain(a, _)) = f {
            if let Filter(Op::Chain(x, _)) = &res[res.len() - 1][0] {
                if a == x {
                    let n = res.len();
                    res[n - 1].push(f.clone());
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

        let (_, a) = last_chain(Filter(Op::Nop), f.clone());
        {
            let (_, x) =
                last_chain(Filter(Op::Nop), res[res.len() - 1][0].clone());
            {
                if a == x {
                    let n = res.len();
                    res[n - 1].push(f.clone());
                    continue;
                }
            }
        }
        res.push(vec![f.clone()]);
    }
    return res;
}

fn common_pre(filters: &Vec<Filter>) -> Option<(Filter, Vec<Filter>)> {
    let mut rest = vec![];
    let mut c: Option<Filter> = None;
    for f in filters {
        if let Filter(Op::Chain(a, b)) = f {
            rest.push(b.as_ref().clone());
            if c == None {
                c = Some(a.as_ref().clone());
            }
            if c != Some(a.as_ref().clone()) {
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
        let (a, b) = last_chain(Filter(Op::Nop), f.clone());
        {
            rest.push(a.clone());
            if c == None {
                c = Some(b.clone());
            }
            if c != Some(b.clone()) {
                return None;
            }
        }
    }
    if let Some(Filter(Op::Nop)) = c {
        return None;
    } else if let Some(c) = c {
        return Some((c, rest));
    } else {
        return None;
    }
}

fn last_chain(rest: Filter, filter: Filter) -> (Filter, Filter) {
    match filter.0 {
        Op::Chain(a, b) => last_chain(Filter(Op::Chain(Box::new(rest), a)), *b),
        _ => (rest, filter),
    }
}

fn optimize(filter: Filter) -> Filter {
    if let Some(f) = FILTERS.lock().unwrap().get(&filter) {
        return f.clone();
    }
    rs_tracing::trace_scoped!("optimize", "spec": spec(&filter));
    let original = filter.clone();
    let result = match filter.0 {
        Op::Subdir(path) => {
            if path.components().count() > 1 {
                let mut components = path.components();
                let a = components.next().unwrap();
                Filter(Op::Chain(
                    Box::new(Filter(Op::Subdir(std::path::PathBuf::from(&a)))),
                    Box::new(Filter(Op::Subdir(
                        components.as_path().to_owned(),
                    ))),
                ))
            } else {
                Filter(Op::Subdir(path))
            }
        }
        Op::Prefix(path) => {
            if path.components().count() > 1 {
                let mut components = path.components();
                let a = components.next().unwrap();
                Filter(Op::Chain(
                    Box::new(Filter(Op::Prefix(
                        components.as_path().to_owned(),
                    ))),
                    Box::new(Filter(Op::Prefix(std::path::PathBuf::from(&a)))),
                ))
            } else {
                Filter(Op::Prefix(path))
            }
        }
        Op::Compose(filters) if filters.len() == 0 => Filter(Op::Empty),
        Op::Compose(filters) if filters.len() == 1 => filters[0].clone(),
        Op::Compose(mut filters) => {
            let mut grouped = group(&filters);
            if let Some((common, rest)) = common_pre(&filters) {
                Filter(Op::Chain(
                    Box::new(common),
                    Box::new(Filter(Op::Compose(rest))),
                ))
            } else if let Some((common, rest)) = common_post(&filters) {
                Filter(Op::Chain(
                    Box::new(Filter(Op::Compose(rest))),
                    Box::new(common),
                ))
            } else if grouped.len() != filters.len() {
                Filter(Op::Compose(
                    grouped.drain(..).map(|x| Filter(Op::Compose(x))).collect(),
                ))
            } else {
                Filter(Op::Compose(filters.drain(..).map(optimize).collect()))
            }
        }
        Op::Chain(a, b) => match (*a, *b) {
            (Filter(Op::Chain(x, y)), b) => Filter(Op::Chain(
                x,
                Box::new(Filter(Op::Chain(y, Box::new(b)))),
            )),
            (Filter(Op::Nop), b) => b,
            (a, Filter(Op::Nop)) => a,
            (a, b) => {
                Filter(Op::Chain(Box::new(optimize(a)), Box::new(optimize(b))))
            }
        },
        Op::Subtract(a, b) => match (*a, *b) {
            (Filter(Op::Empty), _) => Filter(Op::Empty),
            (a, b) if a == b => Filter(Op::Empty),
            (a, Filter(Op::Empty)) => a,
            (Filter(Op::Chain(a, b)), Filter(Op::Chain(c, d))) if a == c => {
                Filter(Op::Chain(a, Box::new(Filter(Op::Subtract(b, d)))))
            }
            (a, b) => {
                let (a, b) = if let (
                    Filter(Op::Compose(mut av)),
                    Filter(Op::Compose(mut bv)),
                ) = (a.clone(), b.clone())
                {
                    let v = av.clone();
                    av.retain(|x| !bv.contains(x));
                    bv.retain(|x| !v.contains(x));
                    (Filter(Op::Compose(av)), Filter(Op::Compose(bv)))
                } else {
                    (a, b)
                };
                Filter(Op::Subtract(
                    Box::new(optimize(a)),
                    Box::new(optimize(b)),
                ))
            }
        },
        _ => filter,
    }
    .clone();

    let r = if result == original {
        result
    } else {
        log::debug!(
            "optimized: \n    {}\n    ->\n{}",
            pretty(&original, 4),
            pretty(&result, 4)
        );
        optimize(result)
    };

    FILTERS.lock().unwrap().insert(original, r.clone());
    return r;
}

fn compose_filter_from_ws_no_fail(
    repo: &git2::Repository,
    tree: &git2::Tree,
    ws_path: &Path,
) -> JoshResult<Vec<Filter>> {
    let base = vec![Filter(Op::Subdir(ws_path.to_owned()))];
    let cw = build_compose_filter(
        &string_blob(&repo, &tree, &ws_path.join("workspace.josh")),
        base.clone(),
    );

    return Ok(cw.unwrap_or(base));
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
        ["/", arg] => Ok(Op::Subdir(Path::new(arg).to_owned())),
        ["nop"] => Ok(Op::Nop),
        ["empty"] => Ok(Op::Empty),
        ["prefix", arg] => Ok(Op::Prefix(Path::new(arg).to_owned())),
        ["hide", arg] => Ok(Op::Hide(Path::new(arg).to_owned())),
        ["glob", arg] => Ok(Op::Glob(arg.to_string())),
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
        Rule::filter_subdir => {
            let mut inner = pair.into_inner();
            make_op(&["/", inner.next().unwrap().as_str()])
        }
        Rule::filter_presub => {
            let mut inner = pair.into_inner();
            let arg = inner.next().unwrap().as_str();
            if arg.ends_with("/") {
                let arg = arg.trim_end_matches("/");
                Ok(Op::Chain(
                    Box::new(Filter(make_op(&["/", arg])?)),
                    Box::new(Filter(make_op(&["prefix", arg])?)),
                ))
            } else {
                make_op(&["glob", arg])
            }
        }
        Rule::filter_noarg => {
            let mut inner = pair.into_inner();
            make_op(&[inner.next().unwrap().as_str()])
        }
        Rule::filter_compose => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();
            Ok(Op::Compose(build_compose_filter(v[0], vec![])?))
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
            let filter = build_chain(
                filter,
                Filter(Op::Prefix(Path::new(path).to_owned())),
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

fn build_compose_filter(
    filter_spec: &str,
    base: Vec<Filter>,
) -> JoshResult<Vec<Filter>> {
    rs_tracing::trace_scoped!("build_compose_filter");
    let mut filters = base;

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

pub fn build_chain(first: Filter, second: Filter) -> Filter {
    Filter(Op::Chain(Box::new(first), Box::new(second)))
}

pub fn parse(filter_spec: &str) -> JoshResult<Filter> {
    if filter_spec.contains("SUBTRACT") {
        assert!(false);
    }
    if filter_spec == "" {
        return parse(":nop");
    }
    if filter_spec.starts_with(":") {
        let mut chain: Option<Op> = None;
        if let Ok(r) = MyParser::parse(Rule::filter_spec, filter_spec) {
            let mut r = r;
            let r = r.next().unwrap();
            for pair in r.into_inner() {
                let v = parse_item(pair)?;
                chain = Some(if let Some(c) = chain {
                    Op::Chain(Box::new(Filter(c)), Box::new(Filter(v)))
                } else {
                    v
                });
            }
            return Ok(optimize(Filter(chain.unwrap_or(Op::Nop))));
        };
    }

    return Ok(optimize(Filter(Op::Compose(build_compose_filter(
        filter_spec,
        vec![],
    )?))));
}
