pub mod parse;

use crate::filter::MESSAGE_MATCH_ALL_REGEX;
use crate::filter::sequence_number;
use crate::opt;
use crate::persist::{to_filter, to_op, to_ops};
use crate::{Filter, Op, RevMatch};

/// Pretty print the filter on multiple lines with initial indentation level.
/// Nested filters will be indented with additional 4 spaces per nesting level.
pub fn pretty(filter: Filter, indent: usize) -> String {
    let filter = opt::simplify(filter);

    if let Op::Compose(filters) = to_op(filter)
        && indent == 0
    {
        let i = format!("\n{}", " ".repeat(indent));
        return filters
            .iter()
            .map(|x| pretty2(&to_op(*x), indent + 4, true))
            .collect::<Vec<_>>()
            .join(&i);
    }
    pretty2(&to_op(filter), indent, true)
}

/// Pretty print the filter for writing to a file or blob.
/// This ensures the output always ends with a newline, which is required for files.
pub fn as_file(filter: Filter, indent: usize) -> String {
    let mut content = pretty(filter, indent);
    // Ensure the content ends with a newline
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content
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
        Op::Pin(filter) => match to_op(*filter) {
            Op::Compose(filters) => ff(&filters, "pin", indent),
            b => format!(":pin[{}]", pretty2(&b, indent, false)),
        },
        Op::Chain(filters) => {
            if filters.is_empty() {
                return ":/".to_string();
            }
            if filters.len() == 1 {
                return pretty2(&to_op(filters[0]), indent, compose);
            }
            // Check for special case: Subdir + Prefix that cancel
            match &to_ops(filters)[..] {
                [Op::Subdir(p1), Op::Prefix(p2)] if p1 == p2 => {
                    return format!("::{}/", parse::quote_if(&p1.to_string_lossy()));
                }
                [a @ .., Op::Prefix(p)] if compose => {
                    return format!(
                        "{} = {}",
                        parse::quote_if(&p.to_string_lossy()),
                        pretty2(
                            &Op::Chain(a.iter().map(|x| to_filter(x.clone())).collect()),
                            indent,
                            false
                        )
                    );
                }
                _ => {}
            }
            // General case: concatenate all filters
            filters
                .iter()
                .map(|f| pretty2(&to_op(*f), indent, false))
                .collect::<Vec<_>>()
                .join("")
        }
        Op::RegexReplace(replacements) => {
            let v = replacements
                .iter()
                .map(|(regex, r)| {
                    format!(
                        "{}{}:{}",
                        " ".repeat(indent),
                        parse::quote(&regex.to_string()),
                        parse::quote(r)
                    )
                })
                .collect::<Vec<_>>();
            format!(":replace(\n{}\n)", v.join("\n"))
        }
        Op::Squash(Some(ids)) => {
            let mut v = ids
                .iter()
                .map(|(oid, f)| format!("{}{}{}", " ".repeat(indent), &oid.to_string(), spec(*f)))
                .collect::<Vec<_>>();
            v.sort();
            format!(":squash(\n{}\n)", v.join("\n"))
        }
        Op::Meta(meta, filter) => {
            let ind2 = std::cmp::max(indent, 4);
            let mut meta_parts: Vec<_> = meta
                .iter()
                .map(|(k, v)| format!("{}{}={}", " ".repeat(ind2), k, parse::quote(v)))
                .collect();
            meta_parts.sort();
            let meta_str = meta_parts.join("\n");

            if let Op::Compose(filters) = to_op(*filter) {
                ff(&filters, &format!("~(\n{}\n)", meta_str), indent)
            } else {
                ff(&vec![*filter], &format!("~(\n{}\n)", meta_str), indent)
            }
        }
        _ => spec2(op),
    }
}

/// Compact, single line string representation of a filter so that `parse(spec(F)) == F`
/// Note that this is will not be the best human readable representation. For that see `pretty(...)`
pub fn spec(filter: Filter) -> String {
    if filter == sequence_number() {
        return "sequence_number".to_string();
    }
    let filter = opt::simplify(filter);
    spec2(&to_op(filter))
}

pub(crate) fn spec2(op: &Op) -> String {
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
        Op::Pin(filter) => {
            format!(":pin[{}]", spec(*filter))
        }
        Op::Rev(filters) => {
            // No sorting - preserve order for first-match semantics
            let v = filters
                .iter()
                .map(|(match_op, k, v)| {
                    let match_str = match match_op {
                        RevMatch::AncestorStrict => format!("<{}", k),
                        RevMatch::AncestorInclusive => format!("<={}", k),
                        RevMatch::Equal => format!("=={}", k),
                        RevMatch::Default => "_".to_string(),
                    };
                    format!("{}{}", match_str, spec(*v))
                })
                .collect::<Vec<_>>();
            format!(":rev({})", v.join(","))
        }
        Op::Workspace(path) => {
            format!(":workspace={}", parse::quote_if(&path.to_string_lossy()))
        }
        Op::Stored(path) => {
            format!(":+{}", parse::quote_if(&path.to_string_lossy()))
        }
        #[cfg(feature = "incubating")]
        Op::Starlark(path, sub) => {
            if *sub == to_filter(Op::Empty) {
                format!(":*{}", parse::quote_if(&path.to_string_lossy()))
            } else {
                format!(
                    ":*{}[{}]",
                    parse::quote_if(&path.to_string_lossy()),
                    spec(*sub)
                )
            }
        }
        Op::RegexReplace(replacements) => {
            let v = replacements
                .iter()
                .map(|(regex, r)| {
                    format!("{}:{}", parse::quote(&regex.to_string()), parse::quote(r))
                })
                .collect::<Vec<_>>();
            format!(":replace({})", v.join(","))
        }

        Op::Chain(filters) => {
            if filters.is_empty() {
                return ":/".to_string();
            }
            if filters.len() == 2 {
                match (to_op(filters[0]), to_op(filters[1])) {
                    (Op::Subdir(p1), Op::Prefix(p2)) if p1 == p2 => {
                        return format!("::{}/", parse::quote_if(&p1.to_string_lossy()));
                    }
                    _ => {}
                }
            }
            filters
                .iter()
                .map(|f| spec2(&to_op(*f)))
                .collect::<Vec<_>>()
                .join("")
        }

        Op::Nop => ":/".to_string(),
        Op::Empty => ":empty".to_string(),
        Op::Paths => ":PATHS".to_string(),
        Op::Invert => ":INVERT".to_string(),
        Op::Index => ":INDEX".to_string(),
        Op::Fold => ":FOLD".to_string(),
        Op::Squash(None) => ":SQUASH".to_string(),
        Op::Squash(Some(ids)) => {
            let mut v = ids
                .iter()
                .map(|(oid, f)| format!("{}{}", oid, spec(*f)))
                .collect::<Vec<_>>();
            v.sort();
            format!(":squash({})", v.join(","))
        }
        #[cfg(feature = "incubating")]
        Op::Adapt(adapter) => format!(":adapt={}", adapter),
        #[cfg(feature = "incubating")]
        Op::Link(mode) => format!(":link={}", mode),
        #[cfg(feature = "incubating")]
        Op::Export => ":export".to_string(),
        #[cfg(feature = "incubating")]
        Op::Unlink => ":unlink".to_string(),
        Op::Subdir(path) => format!(":/{}", parse::quote_if(&path.to_string_lossy())),
        Op::File(dest_path, source_path) => {
            if source_path == dest_path {
                format!("::{}", parse::quote_if(&dest_path.to_string_lossy()))
            } else {
                format!(
                    "::{}={}",
                    parse::quote_if(&dest_path.to_string_lossy()),
                    parse::quote_if(&source_path.to_string_lossy())
                )
            }
        }
        Op::Prune => ":prune=trivial-merge".to_string(),
        Op::Prefix(path) => format!(":prefix={}", parse::quote_if(&path.to_string_lossy())),
        Op::Pattern(pattern) => format!("::{}", parse::quote_if(pattern)),
        #[cfg(feature = "incubating")]
        Op::Embed(path) => {
            format!(":embed={}", parse::quote_if(&path.to_string_lossy()),)
        }
        Op::Author(author, email) => {
            format!(":author={};{}", parse::quote(author), parse::quote(email))
        }
        Op::Committer(author, email) => {
            format!(
                ":committer={};{}",
                parse::quote(author),
                parse::quote(email)
            )
        }
        Op::Message(m, r) if r.as_str() == MESSAGE_MATCH_ALL_REGEX.as_str() => {
            format!(":{}", parse::quote(m))
        }
        Op::Message(m, r) => {
            format!(":{};{}", parse::quote(m), parse::quote(r.as_str()))
        }
        #[cfg(feature = "incubating")]
        Op::Unapply(r, filter) => {
            format!(":unapply({}{})", r, spec(*filter))
        }
        Op::Hook(hook) => {
            format!(":hook={}", parse::quote(hook))
        }
        Op::Meta(meta, filter) => {
            let mut meta_parts = meta
                .iter()
                .map(|(k, v)| format!("{}={}", k, parse::quote(v)))
                .collect::<Vec<_>>();
            meta_parts.sort();
            format!(":~({})[{}]", meta_parts.join(","), spec(*filter))
        }
    }
}
