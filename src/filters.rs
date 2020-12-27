use super::*;
use pest::Parser;
use std::path::Path;

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
pub struct Filter(Op);

#[derive(Clone, Hash, PartialEq, Eq, Debug)]
enum Op {
    Nop,
    Fold,
    Squash,
    Dirs,

    Hide(std::path::PathBuf),
    Prefix(std::path::PathBuf),
    Subdir(std::path::PathBuf),
    Workspace(std::path::PathBuf),

    Glob(glob::Pattern, bool),

    Compose(Vec<Filter>),
    Chain(Box<Filter>, Box<Filter>),
    Substract(Box<Filter>, Box<Filter>),
}

pub fn spec(filter: &Filter) -> String {
    match &filter.0 {
        Op::Compose(filters) => {
            filters.iter().map(spec).collect::<Vec<_>>().join("\n")
        }
        Op::Substract(a, b) => {
            format!(":SUBSTRACT({} - {})", spec(&a), spec(&b))
        }
        Op::Workspace(path) => {
            format!(":workspace={}", path.to_string_lossy())
        }

        Op::Nop => ":nop".to_string(),
        Op::Dirs => ":DIRS".to_string(),
        Op::Fold => ":FOLD".to_string(),
        Op::Squash => ":SQUASH".to_string(),
        Op::Chain(a, b) => format!("{}{}", spec(&a), spec(&b)),
        Op::Subdir(path) => format!(":/{}", path.to_string_lossy()),
        Op::Prefix(path) => format!(":prefix={}", path.to_string_lossy()),
        Op::Hide(path) => format!(":hide={}", path.to_string_lossy()),
        Op::Glob(pattern, false) => format!(":glob={}", pattern.as_str()),
        Op::Glob(pattern, true) => format!(":~glob={}", pattern.as_str()),
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
    if let Some(oid) = transaction.get(&filters::spec(&filter), commit.id()) {
        return Ok(oid);
    }

    let filtered_tree = match &filter.0 {
        Op::Nop => return Ok(commit.id()),

        Op::Chain(a, b) => {
            let r = apply_to_commit2(repo, &a, &commit, transaction)?;
            if let Ok(r) = repo.find_commit(r) {
                return apply_to_commit2(repo, &b, &r, transaction);
            } else {
                return Ok(git2::Oid::zero());
            }
        }
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
                    let mut pcw = compose_filter_from_ws_no_fail(
                        repo,
                        &parent.tree()?,
                        &ws_path,
                    )?;

                    if pcw == cw {
                        return Ok(git2::Oid::zero());
                    }

                    let mut mcw = cw.clone();

                    mcw.retain(|x| !pcw.contains(x));
                    pcw.retain(|x| !cw.contains(x));

                    if mcw.len() == 0 {
                        return Ok(git2::Oid::zero());
                    }

                    apply_to_commit2(
                        repo,
                        &Filter(Op::Substract(
                            Box::new(Filter(Op::Compose(mcw))),
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
        Op::Substract(a, b) => {
            let a_c = {
                repo.find_commit(apply_to_commit2(
                    &repo,
                    &a,
                    &commit,
                    transaction,
                )?)
                .map(|x| x.tree_id())
                .unwrap_or(empty_tree_id())
            };
            let b_c = {
                repo.find_commit(apply_to_commit2(
                    &repo,
                    &b,
                    &commit,
                    transaction,
                )?)
                .map(|x| x.tree_id())
                .unwrap_or(empty_tree_id())
            };
            repo.find_tree(treeops::substract_fast(&repo, a_c, b_c)?)?
        }
        Op::Squash => {
            return scratch::rewrite(&repo, &commit, &vec![], &commit.tree()?)
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
    );
}

pub fn apply<'a>(
    repo: &'a git2::Repository,
    filter: &Filter,
    tree: git2::Tree<'a>,
) -> JoshResult<git2::Tree<'a>> {
    match &filter.0 {
        Op::Nop => return Ok(tree),
        Op::Fold => return Ok(tree),
        Op::Squash => return Ok(tree),

        Op::Glob(pattern, invert) => {
            let options = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            };
            treeops::substract_tree(
                &repo,
                "",
                tree.id(),
                &|path, isblob| {
                    isblob
                        && (*invert
                            != pattern.matches_path_with(&path, options))
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

        Op::Substract(a, b) => Ok(repo.find_tree(treeops::substract_fast(
            &repo,
            apply(&repo, &a, tree.clone())?.id(),
            apply(&repo, &b, tree.clone())?.id(),
        )?)?),

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
                vec![subdir_to_chain(Op::Subdir(path.to_owned()))],
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

                remaining = repo.find_tree(treeops::substract_fast(
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
        Op::Glob(pattern, invert) => {
            let options = glob::MatchOptions {
                case_sensitive: true,
                require_literal_separator: true,
                require_literal_leading_dot: true,
            };
            let substracted = treeops::substract_tree(
                &repo,
                "",
                tree.id(),
                &|path, isblob| {
                    isblob
                        && (*invert
                            != pattern.matches_path_with(&path, options))
                },
                git2::Oid::zero(),
                &mut std::collections::HashMap::new(),
            )?;
            Ok(repo.find_tree(treeops::overlay(
                &repo,
                parent_tree.id(),
                substracted.id(),
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

fn subdir_to_chain(f: Op) -> Filter {
    if let Op::Subdir(path) = f {
        let mut components = path.iter();
        let mut chain = if let Some(comp) = components.next() {
            Op::Subdir(Path::new(comp).to_owned())
        } else {
            Op::Nop
        };

        for comp in components {
            chain = Op::Chain(
                Box::new(Filter(chain)),
                Box::new(Filter(Op::Subdir(Path::new(comp).to_owned()))),
            )
        }
        return Filter(chain);
    }
    return Filter(f);
}

fn compose_filter_from_ws_no_fail(
    repo: &git2::Repository,
    tree: &git2::Tree,
    ws_path: &Path,
) -> JoshResult<Vec<Filter>> {
    let base = vec![subdir_to_chain(Op::Subdir(ws_path.to_owned()))];
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
        ["/", arg] => {
            Ok(subdir_to_chain(Op::Subdir(Path::new(arg).to_owned())).0)
        }
        ["nop"] => Ok(Op::Nop),
        ["prefix", arg] => Ok(Op::Prefix(Path::new(arg).to_owned())),
        ["hide", arg] => Ok(Op::Hide(Path::new(arg).to_owned())),
        ["~glob", arg] => Ok(Op::Glob(glob::Pattern::new(arg).unwrap(), true)),
        ["glob", arg] => Ok(Op::Glob(glob::Pattern::new(arg).unwrap(), false)),
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
    if filter_spec.contains("SUBSTRACT") {
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
            return Ok(Filter(chain.unwrap_or(Op::Nop)));
        };
    }

    return Ok(Filter(Op::Compose(build_compose_filter(
        filter_spec,
        vec![],
    )?)));
}
