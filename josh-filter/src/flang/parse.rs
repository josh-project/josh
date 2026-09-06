use crate::check_experimental_features_enabled;
use crate::filter::Filter;
use crate::opt;
use crate::opt::invert;
use crate::persist::to_filter;
use crate::{InsertContent, Op, Regex, RevMatch};

use anyhow::{Context, anyhow};
use indoc::{formatdoc, indoc};
use itertools::Itertools;
use pest::Parser;

use std::path::Path;

/// Object type requested by a contextual revision expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    /// The expression selects a commit object.
    Commit,
    /// The expression selects the selected commit's tree.
    Tree,
}

/// Resolves the revision portion of contextual object expressions.
///
/// `revision` is canonical grammar text without its `#` or `@` selector, such
/// as `""`, `"^"`, `"baseline"`, or `"target^2"`.
pub trait ObjectResolver {
    /// Resolve a commit- or tree-valued revision expression.
    ///
    /// `Ok(None)` means that a named revision variable is unset. Invalid
    /// values and failed ancestry or type resolution are errors.
    fn resolve(
        &self,
        kind: ObjectKind,
        revision: &str,
    ) -> anyhow::Result<Option<gix_hash::ObjectId>>;
}

#[derive(Clone, Copy)]
struct ParserContext<'a> {
    resolver: Option<&'a dyn ObjectResolver>,
}

fn object_kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Commit => "commit",
        ObjectKind::Tree => "tree",
    }
}

fn parse_object_arm(pair: pest::iterators::Pair<'_, Rule>) -> (ObjectKind, &str, Option<&str>) {
    let arm = pair.as_str();
    let (selector, revision) = arm.split_at(1);
    let kind = match selector {
        "@" => ObjectKind::Commit,
        "#" => ObjectKind::Tree,
        _ => unreachable!(),
    };
    let name_end = revision
        .find(|c| c == '^' || c == '~')
        .unwrap_or(revision.len());
    let name = &revision[..name_end];
    let name = (!name.is_empty()).then_some(name);
    (kind, revision, name)
}

fn parse_object_value(
    pair: pest::iterators::Pair<Rule>,
    expected: ObjectKind,
    context: ParserContext<'_>,
) -> anyhow::Result<gix_hash::ObjectId> {
    match pair.as_rule() {
        Rule::rev | Rule::object_value => parse_object_value(
            pair.into_inner().next().context("object value is empty")?,
            expected,
            context,
        ),
        Rule::object_oid => Ok(pair.as_str().parse()?),
        Rule::object_expr => {
            check_experimental_features_enabled("revision object expression")?;
            let expression = pair.as_str().to_owned();
            let arms = pair.into_inner().map(parse_object_arm).collect::<Vec<_>>();
            let resolver = context.resolver.ok_or_else(|| {
                anyhow!("object expression `{expression}` requires a revision resolver")
            })?;
            let (primary_kind, primary_revision, primary_name) = arms[0];

            if primary_kind != expected {
                return Err(anyhow!(
                    "object expression `{expression}` selects a {}, but this position requires a {}",
                    object_kind_name(primary_kind),
                    object_kind_name(expected)
                ));
            }

            if let Some((fallback_kind, _, _)) = arms.get(1)
                && *fallback_kind != primary_kind
            {
                return Err(anyhow!(
                    "object expression `{expression}` has mismatched selectors"
                ));
            }
            if arms.len() == 2 && primary_name.is_none() {
                return Err(anyhow!(
                    "object expression `{expression}` has an unreachable fallback because its primary arm is input-relative"
                ));
            }

            let resolve = |kind: ObjectKind, revision: &str, name: Option<&str>| {
                resolver
                    .resolve(kind, revision)
                    .with_context(|| {
                        let selected = name.unwrap_or("input");
                        format!(
                            "failed to resolve object expression `{expression}` using `{selected}`"
                        )
                    })?
                    .ok_or_else(|| {
                        anyhow!(
                            "object expression `{expression}` references undefined revision variable `{}`",
                            name.unwrap_or("input")
                        )
                    })
            };

            match resolver
                .resolve(primary_kind, primary_revision)
                .with_context(|| {
                    let selected = primary_name.unwrap_or("input");
                    format!("failed to resolve object expression `{expression}` using `{selected}`")
                })? {
                Some(oid) => Ok(oid),
                None => {
                    if let Some((kind, revision, name)) = arms.get(1).copied() {
                        resolve(kind, revision, name)
                    } else {
                        Err(anyhow!(
                            "object expression `{expression}` references undefined revision variable `{}`",
                            primary_name.unwrap_or("input")
                        ))
                    }
                }
            }
        }
        rule => Err(anyhow!("expected object value, found {rule:?}")),
    }
}

fn make_filter(args: &[&str]) -> anyhow::Result<Filter> {
    let f = Filter::new();
    match args {
        ["nop"] => Ok(f),
        ["empty"] => Ok(f.empty()),
        ["prefix", arg] => Ok(f.prefix(arg)),
        ["author", name, email] => Ok(f.author(*name, *email)),
        ["committer", name, email] => Ok(f.committer(*name, *email)),
        ["workspace", arg] => Ok(f.workspace(arg)),
        ["prefix"] => Err(anyhow!(indoc!(
            r#"
            Filter ":prefix" requires an argument.

            Note: use "=" to provide the argument value:

              :prefix=path

            Where `path` is path to be used as a prefix
            "#
        ))),
        ["workspace"] => Err(anyhow!(indoc!(
            r#"
            Filter ":workspace" requires an argument.

            Note: use "=" to provide the argument value:

              :workspace=path

            Where `path` is path to the directory where workspace.josh file is located
            "#
        ))),
        ["SQUASH"] => Ok(f.squash()),
        ["SQUASH", _ids @ ..] => Err(anyhow!("SQUASH with ids can't be parsed")),
        ["linear"] => Ok(f.linear()),
        ["prune", "trivial-merge"] => Ok(f.prune_trivial_merge()),
        ["prune"] => Err(anyhow!(indoc!(
            r#"
            Filter ":prune" requires an argument.

            Note: use "=" to provide the argument value:

              :prune=trivial-merge
            "#
        ))),
        ["prune", _] => Err(anyhow!(indoc!(
            r#"
            Filter ":prune" only supports "trivial-merge"
            as argument value.
            "#
        ))),
        ["unsign"] => Ok(f.unsign()),

        ["export"] => {
            check_experimental_features_enabled("export filter")?;
            Ok(to_filter(Op::Export))
        }

        ["PATHS"] => Ok(to_filter(Op::Paths)),
        ["INDEX"] => {
            check_experimental_features_enabled(":INDEX filter")?;
            Ok(to_filter(Op::Index))
        }
        ["INVERT"] => Ok(to_filter(Op::Invert)),
        ["FOLD"] => Ok(to_filter(Op::Fold)),
        ["hook", arg] => Ok(f.hook(arg)),
        ["_"] => Err(anyhow!(indoc!(
            r#"
            Filter ":_" requires a base SHA argument.

            Note: use "=" to provide the argument value:

              :_=<sha>

            Where `<sha>` is the base commit to rebase against.
            "#
        ))),
        _ => Err(anyhow!(formatdoc!(
            r#"
            Invalid filter: ":{0}"

            Note: use forward slash at the start of the filter if you're
            trying to select a subdirectory:

              :/{0}
            "#,
            args[0]
        ))),
    }
}

fn parse_treederef(arg: &str) -> Filter {
    let f = Filter::new();
    if let Some(path) = arg.strip_prefix('/') {
        let path = Path::new(path).to_owned();
        f.chain(to_filter(Op::ObjectDeref(path.clone())))
            .subdir(path)
    } else {
        let path = Path::new(arg).to_owned();
        f.chain(to_filter(Op::ObjectDeref(path)))
    }
}

fn parse_item(
    pair: pest::iterators::Pair<Rule>,
    context: ParserContext<'_>,
) -> anyhow::Result<Filter> {
    let f = Filter::new();
    match pair.as_rule() {
        Rule::filter => {
            let v: Vec<_> = pair.into_inner().map(|x| unquote(x.as_str())).collect();
            make_filter(v.iter().map(String::as_str).collect::<Vec<_>>().as_slice())
        }
        Rule::filter_nop => Ok(f),
        Rule::filter_subdir => Ok(
            f.subdir(Path::new(&unquote(pair.into_inner().next().unwrap().as_str())).to_owned())
        ),
        Rule::filter_stored => Ok(
            f.stored(Path::new(&unquote(pair.into_inner().next().unwrap().as_str())).to_owned())
        ),
        Rule::filter_starlark => {
            check_experimental_features_enabled("Starlark filter")?;
            let mut inner = pair.into_inner();
            let path = Path::new(&unquote(inner.next().unwrap().as_str())).to_owned();
            let subfilter = to_filter(Op::Compose(parse_group(
                inner.next().unwrap().as_str(),
                context,
            )?));
            Ok(f.starlark(path, subfilter)?)
        }
        Rule::filter_treeid => {
            check_experimental_features_enabled("TreeId filter")?;
            let mut inner = pair.into_inner();
            let path = Path::new(&unquote(inner.next().unwrap().as_str())).to_owned();
            let sf = to_filter(Op::Compose(parse_group(
                inner.next().unwrap().as_str(),
                context,
            )?));
            Ok(f.treeid(path, sf)?)
        }
        Rule::filter_treeref => {
            check_experimental_features_enabled("ObjectRef filter")?;
            let path = Path::new(&unquote(pair.into_inner().next().unwrap().as_str())).to_owned();
            Ok(f.chain(to_filter(Op::ObjectRef(path))))
        }
        Rule::filter_treederef => {
            check_experimental_features_enabled("ObjectDeref filter")?;
            let path = unquote(pair.into_inner().next().unwrap().as_str());
            Ok(parse_treederef(&path))
        }
        Rule::filter_insert => {
            let mut inner = pair.into_inner();
            let path = Path::new(&unquote(inner.next().unwrap().as_str())).to_owned();
            let content_pair = inner.next().unwrap();
            let content = match content_pair.as_rule() {
                Rule::string => InsertContent::Inline(unquote(content_pair.as_str())),
                Rule::object_value => {
                    InsertContent::Oid(parse_object_value(content_pair, ObjectKind::Tree, context)?)
                }
                _ => unreachable!(),
            };
            Ok(to_filter(Op::Insert(path, content)))
        }
        Rule::filter_downstack => {
            check_experimental_features_enabled("downstack filter")?;
            let value = pair
                .into_inner()
                .next()
                .context("downstack filter is missing its base commit")?;
            Ok(to_filter(Op::Downstack(parse_object_value(
                value,
                ObjectKind::Commit,
                context,
            )?)))
        }
        Rule::filter_presub => {
            let mut inner = pair.into_inner();
            let arg = &unquote(inner.next().unwrap().as_str());
            let second_arg = inner.next().map(|x| unquote(x.as_str()));

            if arg.ends_with('/') {
                let arg = arg.trim_end_matches('/');
                Ok(f.subdir(arg).prefix(arg))
            } else if arg.contains('*') {
                // Pattern case - error if combined with = (destination=source syntax)
                if second_arg.is_some() {
                    return Err(anyhow!(
                        "Pattern filters cannot use destination=source syntax: {}",
                        arg
                    ));
                }
                f.pattern(arg)
            } else {
                // File case - error if source contains * (patterns not supported in source)
                if let Some(ref source_arg) = second_arg
                    && source_arg.contains('*')
                {
                    return Err(anyhow!(
                        "Pattern filters not supported in source path: {}",
                        source_arg
                    ));
                }
                let dest_path = Path::new(arg).to_owned();
                let source_path = second_arg
                    .map(|s| Path::new(&s).to_owned())
                    .unwrap_or_else(|| dest_path.clone());
                Ok(f.rename(dest_path, source_path))
            }
        }
        Rule::filter_noarg => {
            let mut inner = pair.into_inner();
            make_filter(&[inner.next().unwrap().as_str()])
        }
        Rule::filter_message => {
            let mut inner = pair.into_inner();
            let fmt = unquote(inner.next().unwrap().as_str());
            let regex = if let Some(r) = inner.next() {
                regex::Regex::new(&unquote(r.as_str())).context("invalid regex")?
            } else {
                crate::filter::MESSAGE_MATCH_ALL_REGEX.clone()
            };
            Ok(f.message_regex(fmt, regex))
        }
        Rule::filter_group => {
            let v: Vec<_> = pair.into_inner().map(|x| unquote(x.as_str())).collect();

            match v.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                [args] => Ok(to_filter(Op::Compose(parse_group(args, context)?))),
                [cmd, args] => {
                    let g = parse_group(args, context)?;
                    match *cmd {
                        "pin" => Ok(to_filter(Op::Pin(to_filter(Op::Compose(g))))),
                        "exclude" => Ok(to_filter(Op::Exclude(to_filter(Op::Compose(g))))),
                        "select" => Ok(to_filter(Op::Select(to_filter(Op::Compose(g))))),
                        "linear" => Ok(to_filter(Op::Compose(g)).linear()),
                        "invert" => {
                            let filter = to_filter(Op::Compose(g));
                            invert(filter)
                        }
                        "subtract" if g.len() == 2 => Ok(to_filter(Op::Subtract(g[0], g[1]))),
                        _ => Err(anyhow!("parse_item: no match {:?}", cmd)),
                    }
                }
                _ => Err(anyhow!("parse_item: no match")),
            }
        }
        Rule::filter_rev => {
            let mut entries = Vec::new();
            for entry_pair in pair.into_inner() {
                match entry_pair.as_rule() {
                    Rule::rev_entry => {
                        let mut inner = entry_pair.into_inner();
                        let first = inner.next().context("rev_entry: empty")?;

                        match first.as_rule() {
                            Rule::rev_default => {
                                // `_` - default filter, no SHA needed
                                // The rev_default rule contains just filter_spec (the `_` is a literal)
                                let filter_pair = first
                                    .into_inner()
                                    .next()
                                    .context("rev_default: missing filter")?;
                                let filter = parse_internal(filter_pair.as_str(), context)?;
                                entries.push((RevMatch::Default, filter));
                            }
                            Rule::rev_match => {
                                // Regular match with operator, SHA, and filter
                                let oid_pair = inner.next().context("rev_entry: missing rev")?;
                                let filter_pair =
                                    inner.next().context("rev_entry: missing filter")?;
                                let oid =
                                    parse_object_value(oid_pair, ObjectKind::Commit, context)?;
                                let filter = parse_internal(filter_pair.as_str(), context)?;
                                let match_op = match first.as_str() {
                                    "<" => RevMatch::AncestorStrict(oid),
                                    "<=" => RevMatch::AncestorInclusive(oid),
                                    "==" => RevMatch::Equal(oid),
                                    _ => {
                                        return Err(anyhow!(
                                            "invalid rev match operator: {:?}",
                                            first.as_str()
                                        ));
                                    }
                                };

                                entries.push((match_op, filter));
                            }
                            _ => {
                                return Err(anyhow!(
                                    "rev_entry: unexpected rule: {:?}",
                                    first.as_rule()
                                ));
                            }
                        }
                    }
                    _ => {
                        return Err(anyhow!(
                            "filter_rev: unexpected rule: {:?}",
                            entry_pair.as_rule()
                        ));
                    }
                }
            }

            Ok(to_filter(Op::Rev(entries)))
        }
        Rule::filter_unapply => {
            check_experimental_features_enabled("unapply filter")?;
            let mut inner = pair.into_inner();
            let oid = parse_object_value(
                inner
                    .next()
                    .context("unapply filter is missing its commit")?,
                ObjectKind::Commit,
                context,
            )?;
            let filter = parse_internal(
                inner
                    .next()
                    .context("unapply filter is missing its body")?
                    .as_str(),
                context,
            )?;
            if inner.next().is_some() {
                return Err(anyhow!("wrong argument count for :unapply"));
            }
            Ok(to_filter(Op::Unapply(oid, filter)))
        }
        Rule::filter_replace => {
            let replacements = pair
                .into_inner()
                .map(|x| unquote(x.as_str()))
                .tuples()
                .map(|(regex, replacement)| {
                    regex::Regex::new(&regex)
                        .map(|r| (Regex(r), replacement))
                        .context("invalid regex")
                })
                .collect::<Result<Vec<_>, _>>()?;

            Ok(to_filter(Op::RegexReplace(replacements)))
        }
        Rule::filter_squash => {
            // BTreeMap deduplicates IDs and makes the expansion deterministic.
            let ids: std::collections::BTreeMap<gix_hash::ObjectId, Filter> = pair
                .into_inner()
                .tuples()
                .map(
                    |(oid, filter)| -> anyhow::Result<(gix_hash::ObjectId, Filter)> {
                        Ok((
                            parse_object_value(oid, ObjectKind::Commit, context)?,
                            parse_internal(filter.as_str(), context)?,
                        ))
                    },
                )
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .collect();

            Ok(to_filter(crate::filter::squash_to_rev(ids)))
        }
        Rule::filter_meta => {
            let inner = pair.into_inner();
            let mut meta = std::collections::BTreeMap::new();
            let mut compose_item = None;

            // Collect all items - filter_path (keys) and string (values) come in pairs, then compose
            let mut items = Vec::new();
            for item in inner {
                match item.as_rule() {
                    Rule::filter_path => items.push(item),
                    Rule::string => items.push(item),
                    Rule::compose => {
                        compose_item = Some(item);
                        break;
                    }
                    _ => {}
                }
            }

            // Parse key=value pairs - keys are filter_path (unquoted), values are string (quoted, need unquote)
            for chunk in items.chunks(2) {
                if chunk.len() == 2 {
                    let key = chunk[0].as_str().to_string();
                    let value = unquote(chunk[1].as_str());
                    meta.insert(key, value);
                }
            }

            let filter = if let Some(compose_pair) = compose_item {
                let filters = parse_group(compose_pair.as_str(), context)?;
                if filters.len() == 1 {
                    filters[0]
                } else {
                    to_filter(Op::Compose(filters))
                }
            } else {
                return Err(anyhow!("filter_meta: missing filter"));
            };

            Ok(to_filter(Op::Meta(meta, filter)))
        }
        Rule::filter_scope => {
            let mut inner = pair.into_inner();
            let x_filter_spec = inner.next().context("filter_scope: missing filter_spec")?;
            let y_compose = inner.next().context("filter_scope: missing compose")?;

            let x = parse_internal(x_filter_spec.as_str(), context)?;
            let y_filters = parse_group(y_compose.as_str(), context)?;
            let y = to_filter(Op::Compose(y_filters));

            Ok(f.chain(x).chain(y).chain(invert(x)?))
        }
        _ => Err(anyhow!("parse_item: no match")),
    }
}

fn parse_file_entry(
    pair: pest::iterators::Pair<Rule>,
    filters: &mut Vec<Filter>,
    context: ParserContext<'_>,
) -> anyhow::Result<()> {
    match pair.as_rule() {
        Rule::file_entry => {
            let mut inner = pair.into_inner();
            let path = inner.next().unwrap().as_str();
            let filter = inner
                .next()
                .map(|x| x.as_str().to_owned())
                .unwrap_or(format!(":/{}", path));
            let filter = parse_internal(&filter, context)?;
            let filter = filter.chain(to_filter(Op::Prefix(Path::new(path).to_owned())));
            filters.push(filter);
            Ok(())
        }
        Rule::filter_spec => {
            let filter = pair.as_str();
            filters.push(parse_internal(filter, context)?);
            Ok(())
        }
        Rule::EOI => Ok(()),
        _ => Err(anyhow!("invalid workspace file {:?}", pair)),
    }
}

fn parse_group(filter_spec: &str, context: ParserContext<'_>) -> anyhow::Result<Vec<Filter>> {
    let mut filters = vec![];

    match Grammar::parse(Rule::compose, filter_spec) {
        Ok(mut r) => {
            let r = r.next().unwrap();
            for pair in r.into_inner() {
                parse_file_entry(pair, &mut filters, context)?;
            }

            Ok(filters)
        }
        Err(r) => Err(anyhow!(
            "Invalid workspace:\n----\n{}\n\n{}\n----",
            r.to_string().replace('␊', ""),
            filter_spec
        )),
    }
}

fn parse_workspace(filter_spec: &str, context: ParserContext<'_>) -> anyhow::Result<Vec<Filter>> {
    match Grammar::parse(Rule::workspace_file, filter_spec) {
        Ok(mut r) => {
            let r = r.next().unwrap();
            for pair in r.into_inner() {
                match pair.as_rule() {
                    Rule::compose => {
                        let filters = parse_group(pair.as_str(), context)?;
                        return Ok(filters);
                    }
                    Rule::workspace_comments => {
                        continue;
                    }
                    _ => return Err(anyhow!("invalid workspace file {:?}", pair)),
                };
            }
            Err(anyhow!("invalid workspace file"))
        }
        Err(r) => Err(anyhow!(
            "Invalid workspace:\n----\n{}\n\n{}\n----",
            r.to_string().replace('␊', ""),
            filter_spec
        )),
    }
}

// Parse json string if necessary
fn unquote(s: &str) -> String {
    let s = s.replace("'", "\"");
    if let Ok(serde_json::Value::String(s)) = serde_json::from_str(&s) {
        return s;
    }
    s.to_string()
}

// Encode string as json if it contains any chars reserved
// by the filter language
pub fn quote_if(s: &str) -> String {
    if !s.contains(char::is_whitespace)
        && let Ok(r) = Grammar::parse(Rule::filter_path, s)
        && r.as_str() == s
    {
        return s.to_string();
    }
    quote(s)
}

pub fn quote(s: &str) -> String {
    serde_json::to_string(&serde_json::Value::String(s.to_string()))
        .unwrap_or("<invalid string>".to_string())
}

fn parse_internal(filter_spec: &str, context: ParserContext<'_>) -> anyhow::Result<Filter> {
    if filter_spec.is_empty() {
        return Ok(to_filter(Op::Empty));
    }
    let mut chain = Filter::new();
    if let Ok(r) = Grammar::parse(Rule::filter_chain, filter_spec) {
        let mut r = r;
        let r = r.next().unwrap();
        for pair in r.into_inner() {
            let v = parse_item(pair, context)?;
            chain = chain.chain(v);
        }
        return Ok(chain);
    };

    Ok(opt::optimize(to_filter(Op::Compose(parse_workspace(
        filter_spec,
        context,
    )?))))
}

/// Create a `Filter` from a string representation.
pub fn parse(filter_spec: &str) -> anyhow::Result<Filter> {
    parse_internal(filter_spec, ParserContext { resolver: None })
}

/// Parse and immediately lower object expressions through `resolver`.
///
/// The resulting filter is canonical and context-free: every accepted object
/// expression has been replaced by its resolved object ID.
pub fn parse_with_resolver(
    filter_spec: &str,
    resolver: &dyn ObjectResolver,
) -> anyhow::Result<Filter> {
    parse_internal(
        filter_spec,
        ParserContext {
            resolver: Some(resolver),
        },
    )
}

/// Get the potential leading comments from a workspace.josh as a string
pub fn get_comments(filter_spec: &str) -> Result<String, String> {
    if let Ok(r) = Grammar::parse(Rule::workspace_file, filter_spec) {
        let mut r = r;
        let r = r.next().unwrap();
        if let Some(pair) = r.into_inner().next() {
            return match pair.as_rule() {
                Rule::workspace_comments => Ok(pair.as_str().to_string()),
                Rule::compose => Ok("".to_string()),
                _ => Err(format!("Invalid workspace:\n----\n{}\n----", filter_spec)),
            };
        }
    }

    Err(format!("Invalid workspace:\n----\n{}\n----", filter_spec))
}

#[derive(pest_derive::Parser)]
#[grammar = "flang/grammar.pest"]
struct Grammar;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const BASELINE_TREE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[derive(Clone, Copy)]
    enum Baseline {
        Present,
        Unset,
        Invalid,
    }

    struct FakeResolver {
        baseline: Baseline,
        calls: RefCell<Vec<(ObjectKind, String)>>,
    }

    impl FakeResolver {
        fn new(baseline: Baseline) -> Self {
            Self {
                baseline,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    fn oid(value: &str) -> gix_hash::ObjectId {
        value.parse().unwrap()
    }

    impl ObjectResolver for FakeResolver {
        fn resolve(
            &self,
            kind: ObjectKind,
            revision: &str,
        ) -> anyhow::Result<Option<gix_hash::ObjectId>> {
            self.calls.borrow_mut().push((kind, revision.to_owned()));
            let value = match revision {
                "baseline" => match self.baseline {
                    Baseline::Present => Some(BASELINE_TREE),
                    Baseline::Unset => None,
                    Baseline::Invalid => return Err(anyhow!("invalid baseline")),
                },
                "^9" => return Err(anyhow!("missing parent")),
                "missing" => None,
                "" => Some("1111111111111111111111111111111111111111"),
                "^" => Some("2222222222222222222222222222222222222222"),
                "^2" => Some("3333333333333333333333333333333333333333"),
                "~" => Some("4444444444444444444444444444444444444444"),
                "~3" => Some("5555555555555555555555555555555555555555"),
                "^2~3" => Some("6666666666666666666666666666666666666666"),
                "target" => Some("7777777777777777777777777777777777777777"),
                "target^2" => Some("8888888888888888888888888888888888888888"),
                other => panic!("unexpected revision expression {other}"),
            };
            Ok(value.map(oid))
        }
    }

    #[test]
    fn input_and_ancestry_expressions_reach_the_resolver() {
        if !crate::experimental_features_enabled() {
            return;
        }
        let resolver = FakeResolver::new(Baseline::Present);
        for expression in ["{#}", "{#^}", "{#^2}", "{#~}", "{#~3}", "{#^2~3}"] {
            parse_with_resolver(&format!(":$.={expression}"), &resolver).unwrap();
        }
        parse_with_resolver(":rev(=={@}:/)", &resolver).unwrap();
        assert_eq!(
            resolver.calls.into_inner(),
            ["", "^", "^2", "~", "~3", "^2~3"]
                .into_iter()
                .map(|revision| (ObjectKind::Tree, revision.to_owned()))
                .chain(std::iter::once((ObjectKind::Commit, String::new())))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn named_tree_and_commit_expressions_are_lowered() {
        if !crate::experimental_features_enabled() {
            return;
        }
        let resolver = FakeResolver::new(Baseline::Present);
        let filter = parse_with_resolver(
            ":[tree=:$.={#baseline},commit=:rev(=={@target^2}:/)]",
            &resolver,
        )
        .unwrap();
        let rendered = crate::pretty(filter, 0);
        assert!(rendered.contains(BASELINE_TREE));
        assert!(rendered.contains("8888888888888888888888888888888888888888"));
        assert!(!rendered.contains("{#"));
        assert!(!rendered.contains("{@"));
    }

    #[test]
    fn present_primary_does_not_resolve_fallback() {
        if !crate::experimental_features_enabled() {
            return;
        }
        let resolver = FakeResolver::new(Baseline::Present);
        parse_with_resolver(":$.={#baseline|#^9}", &resolver).unwrap();
        assert_eq!(
            resolver.calls.into_inner(),
            vec![(ObjectKind::Tree, "baseline".to_owned())]
        );
    }

    #[test]
    fn absent_primary_selects_fallback() {
        if !crate::experimental_features_enabled() {
            return;
        }
        let resolver = FakeResolver::new(Baseline::Unset);
        parse_with_resolver(":$.={#baseline|#^}", &resolver).unwrap();
        assert_eq!(
            resolver.calls.into_inner(),
            vec![
                (ObjectKind::Tree, "baseline".to_owned()),
                (ObjectKind::Tree, "^".to_owned())
            ]
        );
    }

    #[test]
    fn invalid_primary_does_not_select_fallback() {
        if !crate::experimental_features_enabled() {
            return;
        }
        let resolver = FakeResolver::new(Baseline::Invalid);
        let error = parse_with_resolver(":$.={#baseline|#^}", &resolver)
            .unwrap_err()
            .to_string();
        assert!(error.contains("failed to resolve object expression"));
        assert_eq!(
            resolver.calls.into_inner(),
            vec![(ObjectKind::Tree, "baseline".to_owned())]
        );
    }

    #[test]
    fn undefined_variables_and_fallback_failures_are_reported() {
        if !crate::experimental_features_enabled() {
            return;
        }
        let resolver = FakeResolver::new(Baseline::Unset);
        let undefined = parse_with_resolver(":$.={#baseline}", &resolver)
            .unwrap_err()
            .to_string();
        assert!(undefined.contains("undefined revision variable `baseline`"));

        let fallback = parse_with_resolver(":$.={#baseline|#^9}", &resolver).unwrap_err();
        let fallback = format!("{fallback:#}");
        assert!(fallback.contains("missing parent"));
        assert!(fallback.contains("`input`"));
    }

    #[test]
    fn invalid_fallback_forms_are_rejected() {
        if !crate::experimental_features_enabled() {
            return;
        }
        let resolver = FakeResolver::new(Baseline::Unset);
        assert!(
            parse_with_resolver(":$.={#baseline|@^}", &resolver)
                .unwrap_err()
                .to_string()
                .contains("mismatched selectors")
        );
        assert!(
            parse_with_resolver(":$.={#^|#baseline}", &resolver)
                .unwrap_err()
                .to_string()
                .contains("unreachable fallback")
        );
        assert!(parse_with_resolver(":$.={#baseline|#^|#~2}", &resolver).is_err());
    }

    #[test]
    fn object_positions_enforce_selector_kind() {
        if !crate::experimental_features_enabled() {
            return;
        }
        let resolver = FakeResolver::new(Baseline::Present);
        assert!(parse_with_resolver(":$.={#baseline}", &resolver).is_ok());
        assert!(parse_with_resolver(":$.={@baseline}", &resolver).is_err());
        assert!(parse_with_resolver(":rev(=={@baseline}:/)", &resolver).is_ok());
        assert!(parse_with_resolver(":rev(=={#baseline}:/)", &resolver).is_err());
        assert!(parse_with_resolver(":squash({@baseline}:/)", &resolver).is_ok());
        assert!(parse_with_resolver(":squash({#baseline}:/)", &resolver).is_err());

        for spec in [
            ":unapply({@baseline}:/)",
            ":squash({@baseline}:/)",
            ":_={@baseline}",
        ] {
            assert!(Grammar::parse(Rule::filter_chain, spec).is_ok(), "{spec}");
        }
    }

    #[test]
    fn nested_filters_keep_the_resolver_context() {
        if !crate::experimental_features_enabled() {
            return;
        }
        let resolver = FakeResolver::new(Baseline::Present);
        parse_with_resolver(
            ":[outer=:[inner=:$.={#baseline}],rev=:rev(=={@target}:$.={#^})]",
            &resolver,
        )
        .unwrap();
        assert!(
            resolver
                .calls
                .borrow()
                .contains(&(ObjectKind::Tree, "baseline".to_owned()))
        );
        assert!(
            resolver
                .calls
                .borrow()
                .contains(&(ObjectKind::Commit, "target".to_owned()))
        );
        assert!(
            resolver
                .calls
                .borrow()
                .contains(&(ObjectKind::Tree, "^".to_owned()))
        );
    }

    #[test]
    fn context_free_parser_requires_a_resolver() {
        let error = parse(":$.={#baseline|#^}").unwrap_err().to_string();
        if crate::experimental_features_enabled() {
            assert_eq!(
                error,
                "object expression `{#baseline|#^}` requires a revision resolver"
            );
        } else {
            assert_eq!(
                error,
                "revision object expression requires JOSH_EXPERIMENTAL_FEATURES=1"
            );
        }
    }

    #[test]
    fn existing_object_ids_strings_and_message_templates_are_unchanged() {
        let tree = "0123456789012345678901234567890123456789";
        assert!(parse(&format!(":$.={tree}")).is_ok());
        assert!(parse(r#":$.="inline content""#).is_ok());
        assert!(parse(r#":"commit {@}, tree {#}""#).is_ok());
        assert!(parse(r#":rev(=="/repo.git@refs/heads/main":/)"#).is_err());
    }
}
