use super::*;
use indoc::{formatdoc, indoc};
use itertools::Itertools;

fn make_op(args: &[&str]) -> JoshResult<Op> {
    match args {
        ["nop"] => Ok(Op::Nop),
        ["empty"] => Ok(Op::Empty),
        ["prefix", arg] => Ok(Op::Prefix(Path::new(arg).to_owned())),
        ["author", author, email] => Ok(Op::Author(author.to_string(), email.to_string())),
        ["committer", author, email] => Ok(Op::Committer(author.to_string(), email.to_string())),
        ["workspace", arg] => Ok(Op::Workspace(Path::new(arg).to_owned())),
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
        ["SQUASH"] => Ok(Op::Squash(None)),
        ["SQUASH", _ids @ ..] => Err(josh_error("SQUASH with ids can't be parsed")),
        ["linear"] => Ok(Op::Linear),
        ["unsign"] => Ok(Op::Unsign),
        ["PATHS"] => Ok(Op::Paths),
        ["INDEX"] => Ok(Op::Index),
        ["INVERT"] => Ok(Op::Invert),
        ["FOLD"] => Ok(Op::Fold),
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

fn parse_item(pair: pest::iterators::Pair<Rule>) -> JoshResult<Op> {
    match pair.as_rule() {
        Rule::filter => {
            let v: Vec<_> = pair.into_inner().map(|x| unquote(x.as_str())).collect();
            make_op(v.iter().map(String::as_str).collect::<Vec<_>>().as_slice())
        }
        Rule::filter_nop => Ok(Op::Nop),
        Rule::filter_subdir => Ok(Op::Subdir(
            Path::new(&unquote(pair.into_inner().next().unwrap().as_str())).to_owned(),
        )),
        Rule::filter_presub => {
            let mut inner = pair.into_inner();
            let arg = &unquote(inner.next().unwrap().as_str());
            if arg.ends_with('/') {
                let arg = arg.trim_end_matches('/');
                Ok(Op::Chain(
                    to_filter(Op::Subdir(std::path::PathBuf::from(arg))),
                    to_filter(make_op(&["prefix", arg])?),
                ))
            } else if arg.contains('*') {
                Ok(Op::Glob(arg.to_string()))
            } else {
                Ok(Op::File(Path::new(arg).to_owned()))
            }
        }
        Rule::filter_noarg => {
            let mut inner = pair.into_inner();
            make_op(&[inner.next().unwrap().as_str()])
        }
        Rule::filter_message => {
            let mut inner = pair.into_inner();
            Ok(Op::Message(unquote(inner.next().unwrap().as_str())))
        }
        Rule::filter_group => {
            let v: Vec<_> = pair.into_inner().map(|x| unquote(x.as_str())).collect();

            match v.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                [args] => Ok(Op::Compose(parse_group(args)?)),
                [cmd, args] => {
                    let g = parse_group(args)?;
                    match *cmd {
                        "exclude" => Ok(Op::Exclude(to_filter(Op::Compose(g)))),
                        "subtract" if g.len() == 2 => Ok(Op::Subtract(g[0], g[1])),
                        _ => Err(josh_error(&format!("parse_item: no match {:?}", cmd))),
                    }
                }
                _ => Err(josh_error("parse_item: no match {:?}")),
            }
        }
        Rule::filter_join => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();

            let hm = v
                .iter()
                .tuples()
                .map(|(oid, filter)| Ok((LazyRef::parse(oid)?, parse(filter)?)))
                .collect::<JoshResult<_>>()?;

            Ok(Op::Join(hm))
        }
        Rule::filter_rev => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();

            let hm = v
                .iter()
                .tuples()
                .map(|(oid, filter)| Ok((LazyRef::parse(oid)?, parse(filter)?)))
                .collect::<JoshResult<_>>()?;

            Ok(Op::Rev(hm))
        }
        Rule::filter_replace => {
            let replacements = pair
                .into_inner()
                .map(|x| unquote(x.as_str()))
                .tuples()
                .map(|(regex, replacement)| Ok((regex::Regex::new(&regex)?, replacement)))
                .collect::<JoshResult<_>>()?;

            Ok(Op::RegexReplace(replacements))
        }
        Rule::filter_squash => {
            let ids = pair
                .into_inner()
                .tuples()
                .map(|(oid, filter)| Ok((LazyRef::parse(oid.as_str())?, parse(filter.as_str())?)))
                .collect::<JoshResult<_>>()?;

            Ok(Op::Squash(Some(ids)))
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
            let filter = chain(filter, to_filter(Op::Prefix(Path::new(path).to_owned())));
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
        Err(r) => {
            return Err(josh_error(&format!(
                "Invalid workspace:\n----\n{}\n\n{}\n----",
                r.to_string().replace('␊', ""),
                filter_spec
            )));
        }
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
        Err(r) => {
            return Err(josh_error(&format!(
                "Invalid workspace:\n----\n{}\n\n{}\n----",
                r.to_string().replace('␊', ""),
                filter_spec
            )));
        }
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
    let mut chain: Option<Op> = None;
    if let Ok(r) = Grammar::parse(Rule::filter_chain, filter_spec) {
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
        return Ok(opt::optimize(to_filter(chain.unwrap_or(Op::Nop))));
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

    return Err(josh_error(&format!(
        "Invalid workspace:\n----\n{}\n----",
        filter_spec
    )));
}

#[derive(pest_derive::Parser)]
#[grammar = "filter/grammar.pest"]
struct Grammar;
