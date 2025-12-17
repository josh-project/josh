use crate::filter::{self, Filter, invert, to_filter};
use crate::{JoshResult, josh_error};
use indoc::{formatdoc, indoc};
use itertools::Itertools;
use pest::Parser;
use std::path::Path;

use crate::filter::op::{LazyRef, Op};
use crate::filter::opt;

fn make_filter(args: &[&str]) -> JoshResult<Filter> {
    let f = Filter::new();
    match args {
        ["nop"] => Ok(f),
        ["empty"] => Ok(f.empty()),
        ["prefix", arg] => Ok(f.prefix(arg)),
        ["author", name, email] => Ok(f.author(*name, *email)),
        ["committer", name, email] => Ok(f.committer(*name, *email)),
        ["workspace", arg] => Ok(f.workspace(arg)),
        #[cfg(feature = "incubating")]
        ["lookup", arg] => Ok(to_filter(Op::Lookup(Path::new(arg).to_owned()))),
        ["prefix"] => Err(josh_error(indoc!(
            r#"
            Filter ":prefix" requires an argument.

            Note: use "=" to provide the argument value:

              :prefix=path

            Where `path` is path to be used as a prefix
            "#
        ))),
        ["workspace"] => Err(josh_error(indoc!(
            r#"
            Filter ":workspace" requires an argument.

            Note: use "=" to provide the argument value:

              :workspace=path

            Where `path` is path to the directory where workspace.josh file is located
            "#
        ))),
        ["SQUASH"] => Ok(f.squash(None)),
        ["SQUASH", _ids @ ..] => Err(josh_error("SQUASH with ids can't be parsed")),
        ["linear"] => Ok(f.linear()),
        ["prune", "trivial-merge"] => Ok(f.prune_trivial_merge()),
        ["prune"] => Err(josh_error(indoc!(
            r#"
            Filter ":prune" requires an argument.

            Note: use "=" to provide the argument value:

              :prune=trivial-merge
            "#
        ))),
        ["prune", _] => Err(josh_error(indoc!(
            r#"
            Filter ":prune" only supports "trivial-merge"
            as argument value.
            "#
        ))),
        ["unsign"] => Ok(f.unsign()),

        #[cfg(feature = "incubating")]
        ["unlink"] => Ok(to_filter(Op::Unlink)),
        #[cfg(feature = "incubating")]
        ["adapt", adapter] => Ok(to_filter(Op::Adapt(adapter.to_string()))),
        #[cfg(feature = "incubating")]
        ["link"] => Ok(to_filter(Op::Link("embedded".to_string()))),
        #[cfg(feature = "incubating")]
        ["link", mode] => Ok(to_filter(Op::Link(mode.to_string()))),
        #[cfg(feature = "incubating")]
        ["embed", path] => Ok(to_filter(Op::Embed(Path::new(path).to_owned()))),
        #[cfg(feature = "incubating")]
        ["export"] => Ok(to_filter(Op::Export)),

        ["PATHS"] => Ok(to_filter(Op::Paths)),
        ["INDEX"] => Ok(to_filter(Op::Index)),
        ["INVERT"] => Ok(to_filter(Op::Invert)),
        ["FOLD"] => Ok(to_filter(Op::Fold)),
        ["hook", arg] => Ok(f.hook(arg)),
        _ => Err(josh_error(
            formatdoc!(
                r#"
                Invalid filter: ":{0}"

                Note: use forward slash at the start of the filter if you're
                trying to select a subdirectory:

                  :/{0}
                "#,
                args[0]
            )
            .as_str(),
        )),
    }
}

fn parse_item(pair: pest::iterators::Pair<Rule>) -> JoshResult<Filter> {
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
                    return Err(josh_error(&format!(
                        "Pattern filters cannot use destination=source syntax: {}",
                        arg
                    )));
                }
                Ok(f.pattern(arg))
            } else {
                // File case - error if source contains * (patterns not supported in source)
                if let Some(ref source_arg) = second_arg
                    && source_arg.contains('*')
                {
                    return Err(josh_error(&format!(
                        "Pattern filters not supported in source path: {}",
                        source_arg
                    )));
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
                regex::Regex::new(&unquote(r.as_str()))
                    .map_err(|e| josh_error(&format!("invalid regex: {}", e)))?
            } else {
                crate::filter::MESSAGE_MATCH_ALL_REGEX.clone()
            };
            Ok(f.message_regex(fmt, regex))
        }
        Rule::filter_group => {
            let v: Vec<_> = pair.into_inner().map(|x| unquote(x.as_str())).collect();

            match v.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                [args] => Ok(to_filter(Op::Compose(parse_group(args)?))),
                [cmd, args] => {
                    let g = parse_group(args)?;
                    match *cmd {
                        "pin" => Ok(to_filter(Op::Pin(to_filter(Op::Compose(g))))),
                        "exclude" => Ok(to_filter(Op::Exclude(to_filter(Op::Compose(g))))),
                        "invert" => {
                            let filter = to_filter(Op::Compose(g));
                            invert(filter)
                        }
                        "subtract" if g.len() == 2 => Ok(to_filter(Op::Subtract(g[0], g[1]))),
                        _ => Err(josh_error(&format!("parse_item: no match {:?}", cmd))),
                    }
                }
                _ => Err(josh_error("parse_item: no match {:?}")),
            }
        }
        Rule::filter_rev => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();

            let hm = v
                .iter()
                .tuples()
                .map(|(oid, filter)| Ok((LazyRef::parse(oid)?, parse(filter)?)))
                .collect::<JoshResult<_>>()?;

            Ok(to_filter(Op::Rev(hm)))
        }
        Rule::filter_from => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();

            if v.len() == 2 {
                let oid = LazyRef::parse(v[0])?;
                let filter = parse(v[1])?;
                Ok(filter.chain(filter::to_filter(Op::HistoryConcat(oid, filter))))
            } else {
                Err(josh_error("wrong argument count for :from"))
            }
        }
        Rule::filter_concat => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();

            if v.len() == 2 {
                let oid = LazyRef::parse(v[0])?;
                let filter = parse(v[1])?;
                Ok(to_filter(Op::HistoryConcat(oid, filter)))
            } else {
                Err(josh_error("wrong argument count for :concat"))
            }
        }
        #[cfg(feature = "incubating")]
        Rule::filter_unapply => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();

            if v.len() == 2 {
                let oid = LazyRef::parse(v[0])?;
                let filter = parse(v[1])?;
                Ok(to_filter(Op::Unapply(oid, filter)))
            } else {
                Err(josh_error("wrong argument count for :unapply"))
            }
        }
        Rule::filter_replace => {
            let replacements = pair
                .into_inner()
                .map(|x| unquote(x.as_str()))
                .tuples()
                .map(|(regex, replacement)| Ok((regex::Regex::new(&regex)?, replacement)))
                .collect::<JoshResult<_>>()?;

            Ok(to_filter(Op::RegexReplace(replacements)))
        }
        Rule::filter_squash => {
            let ids = pair
                .into_inner()
                .tuples()
                .map(|(oid, filter)| Ok((LazyRef::parse(oid.as_str())?, parse(filter.as_str())?)))
                .collect::<JoshResult<_>>()?;

            Ok(to_filter(Op::Squash(Some(ids))))
        }
        Rule::filter_scope => {
            let mut inner = pair.into_inner();
            let x_filter_spec = inner
                .next()
                .ok_or_else(|| josh_error("filter_scope: missing filter_spec"))?;
            let y_compose = inner
                .next()
                .ok_or_else(|| josh_error("filter_scope: missing compose"))?;

            let x = parse(x_filter_spec.as_str())?;
            let y_filters = parse_group(y_compose.as_str())?;
            let y = to_filter(Op::Compose(y_filters));

            Ok(f.chain(x).chain(y).chain(invert(x)?))
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
            let filter = filter.chain(to_filter(Op::Prefix(Path::new(path).to_owned())));
            filters.push(filter);
            Ok(())
        }
        Rule::filter_spec => {
            let filter = pair.as_str();
            filters.push(parse(filter)?);
            Ok(())
        }
        Rule::EOI => Ok(()),
        _ => Err(josh_error(&format!("invalid workspace file {:?}", pair))),
    }
}

fn parse_group(filter_spec: &str) -> JoshResult<Vec<Filter>> {
    rs_tracing::trace_scoped!("parse_group");
    let mut filters = vec![];

    match Grammar::parse(Rule::compose, filter_spec) {
        Ok(mut r) => {
            let r = r.next().unwrap();
            for pair in r.into_inner() {
                parse_file_entry(pair, &mut filters)?;
            }

            Ok(filters)
        }
        Err(r) => Err(josh_error(&format!(
            "Invalid workspace:\n----\n{}\n\n{}\n----",
            r.to_string().replace('␊', ""),
            filter_spec
        ))),
    }
}

fn parse_workspace(filter_spec: &str) -> JoshResult<Vec<Filter>> {
    rs_tracing::trace_scoped!("parse_workspace");

    match Grammar::parse(Rule::workspace_file, filter_spec) {
        Ok(mut r) => {
            let r = r.next().unwrap();
            for pair in r.into_inner() {
                match pair.as_rule() {
                    Rule::compose => {
                        let filters = parse_group(pair.as_str())?;
                        return Ok(filters);
                    }
                    Rule::workspace_comments => {
                        continue;
                    }
                    _ => return Err(josh_error(&format!("invalid workspace file {:?}", pair))),
                };
            }
            Err(josh_error("invalid workspace file"))
        }
        Err(r) => Err(josh_error(&format!(
            "Invalid workspace:\n----\n{}\n\n{}\n----",
            r.to_string().replace('␊', ""),
            filter_spec
        ))),
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
    if let Ok(r) = Grammar::parse(Rule::filter_path, s) {
        if r.as_str() == s {
            return s.to_string();
        }
    }
    quote(s)
}

pub fn quote(s: &str) -> String {
    serde_json::to_string(&serde_json::Value::String(s.to_string()))
        .unwrap_or("<invalid string>".to_string())
}

/// Create a `Filter` from a string representation
pub fn parse(filter_spec: &str) -> JoshResult<Filter> {
    if filter_spec.is_empty() {
        return Ok(to_filter(Op::Empty));
    }
    let mut chain = Filter::new();
    if let Ok(r) = Grammar::parse(Rule::filter_chain, filter_spec) {
        let mut r = r;
        let r = r.next().unwrap();
        for pair in r.into_inner() {
            let v = parse_item(pair)?;
            chain = chain.chain(v);
        }
        return Ok(chain);
    };

    Ok(opt::optimize(to_filter(Op::Compose(parse_workspace(
        filter_spec,
    )?))))
}

/// Get the potential leading comments from a workspace.josh as a string
pub fn get_comments(filter_spec: &str) -> JoshResult<String> {
    if let Ok(r) = Grammar::parse(Rule::workspace_file, filter_spec) {
        let mut r = r;
        let r = r.next().unwrap();
        if let Some(pair) = r.into_inner().next() {
            return match pair.as_rule() {
                Rule::workspace_comments => Ok(pair.as_str().to_string()),
                Rule::compose => Ok("".to_string()),
                _ => Err(josh_error(&format!(
                    "Invalid workspace:\n----\n{}\n----",
                    filter_spec
                ))),
            };
        }
    }

    Err(josh_error(&format!(
        "Invalid workspace:\n----\n{}\n----",
        filter_spec
    )))
}

#[derive(pest_derive::Parser)]
#[grammar = "flang/grammar.pest"]
struct Grammar;
