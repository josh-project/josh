use super::*;

fn make_op(args: &[&str]) -> JoshResult<Op> {
    match args {
        ["nop"] => Ok(Op::Nop),
        ["empty"] => Ok(Op::Empty),
        ["prefix", arg] => Ok(Op::Prefix(Path::new(arg).to_owned())),
        ["workspace", arg] => Ok(Op::Workspace(Path::new(arg).to_owned())),
        ["SQUASH"] => Ok(Op::Squash),
        ["PATHS"] => Ok(Op::Paths),
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
        Rule::filter_group => {
            let v: Vec<_> = pair.into_inner().map(|x| x.as_str()).collect();

            match v.as_slice() {
                [args] => Ok(Op::Compose(parse_group(args)?)),
                [cmd, args] => {
                    let g = parse_group(args)?;
                    match *cmd {
                        "exclude" => Ok(Op::Subtract(
                            to_filter(Op::Nop),
                            to_filter(Op::Compose(g)),
                        )),
                        "subtract" if g.len() == 2 => {
                            Ok(Op::Subtract(g[0], g[1]))
                        }
                        _ => Err(josh_error("parse_item: no match")),
                    }
                }
                _ => Err(josh_error("parse_item: no match {:?}")),
            }
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

fn parse_group(filter_spec: &str) -> JoshResult<Vec<Filter>> {
    rs_tracing::trace_scoped!("parse_group");
    let mut filters = vec![];

    if let Ok(mut r) = Grammar::parse(Rule::workspace_file, filter_spec) {
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

/// Create a `Filter` from a string representation
pub fn parse(filter_spec: &str) -> JoshResult<Filter> {
    if filter_spec == "" {
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

    return Ok(opt::optimize(to_filter(Op::Compose(parse_group(
        filter_spec,
    )?))));
}

#[derive(Parser)]
#[grammar = "filter/grammar.pest"]
struct Grammar;
