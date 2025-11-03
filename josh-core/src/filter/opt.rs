/*
 * Filter optimization and transformation functions.
 * All those functions convert filters from one equivalent representation into another.
 */

use super::*;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::VecDeque;

lazy_static! {
    static ref OPTIMIZED: std::sync::Mutex<std::collections::HashMap<Filter, Filter>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
    static ref INVERTED: std::sync::Mutex<std::collections::HashMap<Filter, Filter>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
    static ref SIMPLIFIED: std::sync::Mutex<std::collections::HashMap<Filter, Filter>> =
        std::sync::Mutex::new(std::collections::HashMap::new());
}

/*
 * Attempt to create an alternative representation of a filter AST that is most
 * suitable for fast evaluation and cache reuse.
 */
pub fn optimize(filter: Filter) -> Filter {
    if let Some(f) = OPTIMIZED.lock().unwrap().get(&filter) {
        return *f;
    }
    let original = filter;

    let mut filter = flatten(filter);
    let result = loop {
        let pretty = simplify(filter);
        let optimized = iterate(filter);
        filter = simplify(optimized);

        if filter == pretty {
            break iterate(filter);
        }
    };

    OPTIMIZED.lock().unwrap().insert(original, result);
    result
}

/*
 * Attempt to create an equivalent representation of a filter AST, that has fewer nodes than the
 * input, but still has a similar structure.
 * Useful as a pre-processing step for pretty printing and also during filter optimization.
 */
pub fn simplify(filter: Filter) -> Filter {
    if let Some(f) = SIMPLIFIED.lock().unwrap().get(&filter) {
        return *f;
    }
    rs_tracing::trace_scoped!("simplify", "spec": spec2(&to_op(filter)));
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
            (a, Op::Chain(x, y)) => Op::Chain(to_filter(Op::Chain(to_filter(a), x)), y),
            (Op::Prefix(x), Op::Prefix(y)) => Op::Prefix(y.join(x)),
            (Op::Subdir(x), Op::Subdir(y)) => Op::Subdir(x.join(y)),
            (Op::Chain(x, y), b) => match (to_op(x), to_op(y), b.clone()) {
                (x, Op::Prefix(p1), Op::Prefix(p2)) => {
                    Op::Chain(simplify(to_filter(x)), to_filter(Op::Prefix(p2.join(p1))))
                }
                _ => Op::Chain(simplify(a), simplify(to_filter(b))),
            },
            (a, b) => Op::Chain(simplify(to_filter(a)), simplify(to_filter(b))),
        },
        Op::Subtract(a, b) => {
            let (a, b) = (to_op(a), to_op(b));
            Op::Subtract(simplify(to_filter(a)), simplify(to_filter(b)))
        }
        Op::Exclude(b) => Op::Exclude(simplify(b)),
        _ => to_op(filter),
    });

    let r = if result == original {
        result
    } else {
        simplify(result)
    };

    SIMPLIFIED.lock().unwrap().insert(original, r);
    r
}

/*
 * Remove nesting from a filter.
 * This "flat" representation of the filter is more suitable calculate
 * the difference between two complex filters.
 */
pub fn flatten(filter: Filter) -> Filter {
    rs_tracing::trace_scoped!("flatten", "spec": spec(filter));
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
            Op::Compose(out.drain(..).map(flatten).collect())
        }
        Op::Chain(af, bf) => match (to_op(af), to_op(bf)) {
            (_, Op::Compose(filters)) => {
                let mut out = vec![];
                for f in filters {
                    out.push(to_filter(Op::Chain(af, f)));
                }
                Op::Compose(out)
            }
            (Op::Compose(filters), _) => {
                let mut out = vec![];
                for f in filters {
                    out.push(to_filter(Op::Chain(f, bf)));
                }
                Op::Compose(out)
            }
            _ => Op::Chain(flatten(af), flatten(bf)),
        },
        Op::Subtract(a, b) => {
            let (a, b) = (to_op(a), to_op(b));
            Op::Subtract(flatten(to_filter(a)), flatten(to_filter(b)))
        }
        Op::Exclude(b) => Op::Exclude(flatten(b)),
        _ => to_op(filter),
    });

    if result == original {
        result
    } else {
        flatten(result)
    }
}

fn group(filters: &Vec<Filter>) -> Vec<Vec<Filter>> {
    let mut res: Vec<Vec<Filter>> = vec![];
    for f in filters {
        if res.is_empty() {
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
        res.push(vec![*f]);
    }
    if res.len() != filters.len() {
        return res;
    }

    let mut res: Vec<Vec<Filter>> = vec![];
    for f in filters {
        if res.is_empty() {
            res.push(vec![*f]);
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
    res
}

fn last_chain(rest: Filter, filter: Filter) -> (Filter, Filter) {
    match to_op(filter) {
        Op::Chain(a, b) => last_chain(to_filter(Op::Chain(rest, a)), b),
        _ => (rest, filter),
    }
}

pub fn prefix_sort(filters: &[Filter]) -> Vec<Filter> {
    let n = filters.len();

    // Step 1: Build graph of ordering constraints
    let mut graph: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut indegree = vec![0; n];

    for i in 0..n {
        for j in i + 1..n {
            let src_i = src_path(filters[i].clone());
            let dst_i = dst_path(filters[i].clone());
            let src_j = src_path(filters[j].clone());
            let dst_j = dst_path(filters[j].clone());

            let constraint = if src_j.starts_with(&src_i) || src_i.starts_with(&src_j) {
                Some((i, j))
            } else if dst_j.starts_with(&dst_i) || dst_i.starts_with(&dst_j) {
                Some((i, j))
            } else {
                None
            };

            if let Some((a, b)) = constraint {
                graph.entry(a).or_default().push(b);
                indegree[b] += 1;
            }
        }
    }

    // Step 2: Sort indices alphabetically by (src, dst)
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&i, &j| {
        let key_i = (src_path(filters[i].clone()), dst_path(filters[i].clone()));
        let key_j = (src_path(filters[j].clone()), dst_path(filters[j].clone()));

        match key_i.0.cmp(&key_j.0) {
            Ordering::Equal => key_i.1.cmp(&key_j.1),
            other => other,
        }
    });

    // Step 3: Topological sort with alphabetical tie-break
    let mut result = Vec::new();
    let mut available: VecDeque<usize> = indices
        .iter()
        .copied()
        .filter(|&i| indegree[i] == 0)
        .collect();

    while let Some(i) = available.pop_front() {
        result.push(i);
        if let Some(neighbors) = graph.get(&i) {
            for &j in neighbors {
                indegree[j] -= 1;
                if indegree[j] == 0 {
                    // Insert j into available, keeping alphabetical order
                    let pos = available.iter().position(|&x| {
                        let key_j = (src_path(filters[j].clone()), dst_path(filters[j].clone()));
                        let key_x = (src_path(filters[x].clone()), dst_path(filters[x].clone()));
                        key_j < key_x
                    });
                    if let Some(p) = pos {
                        available.insert(p, j);
                    } else {
                        available.push_back(j);
                    }
                }
            }
        }
    }

    result.into_iter().map(|i| filters[i].clone()).collect()
}

fn common_pre(filters: &Vec<Filter>) -> Option<(Filter, Vec<Filter>)> {
    let mut rest = vec![];
    let mut c: Option<Filter> = None;
    for f in filters {
        if let Op::Chain(a, b) = to_op(*f) {
            rest.push(b);
            if c.is_none() {
                c = Some(a);
            }
            if c != Some(a) {
                return None;
            }
        } else {
            return None;
        }
    }
    c.map(|c| (c, rest))
}

fn common_post(filters: &Vec<Filter>) -> Option<(Filter, Vec<Filter>)> {
    let mut rest = vec![];
    let mut common_post: Option<Filter> = None;
    for f in filters {
        let (a, b) = last_chain(to_filter(Op::Nop), *f);
        {
            rest.push(a);
            if common_post.is_none() {
                common_post = Some(b);
            }
            if common_post != Some(b) {
                return None;
            }
        }
    }

    if let Some(c) = common_post {
        if invert(c).ok() == common_post {
            common_post.map(|c| (c, rest))
        } else if let Op::Prefix(_) = to_op(c) {
            common_post.map(|c| (c, rest))
        } else {
            None
        }
    } else {
        None
    }
}

/*
 * Apply optimization steps to a filter until it converges (no rules apply anymore)
 */
fn iterate(filter: Filter) -> Filter {
    let mut filter = filter;
    log::debug!("opt::iterate:\n{}\n", pretty(filter, 0));
    for i in 0..1000 {
        let optimized = step(filter);
        if filter == optimized {
            break;
        }

        if log::log_enabled!(log::Level::Debug) {
            log::debug!(
                "stepop {}:\n{:?}\n->\n{:?}\n",
                i,
                to_op(filter),
                to_op(optimized)
            );
        }
        filter = optimized;
    }
    filter
}

fn is_prefix(op: Op) -> bool {
    matches!(op, Op::Prefix(_))
}

fn prefix_of(op: Op) -> Filter {
    let last = to_op(last_chain(to_filter(Op::Nop), to_filter(op)).1);
    to_filter(if is_prefix(last.clone()) {
        last
    } else {
        Op::Nop
    })
}

/*
 * Attempt to apply one optimization rule to a filter. If no rule applies the input
 * is returned.
 */
fn step(filter: Filter) -> Filter {
    if let Some(f) = OPTIMIZED.lock().unwrap().get(&filter) {
        return *f;
    }
    rs_tracing::trace_scoped!("step", "spec": spec(filter));
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
        Op::Rev(filters) => Op::Rev(filters.into_iter().map(|(i, f)| (i, step(f))).collect()),
        Op::Compose(filters) if filters.is_empty() => Op::Empty,
        Op::Compose(filters) if filters.len() == 1 => to_op(filters[0]),
        Op::Compose(mut filters) => {
            filters.dedup();
            filters.retain(|x| *x != to_filter(Op::Empty));
            let mut grouped = group(&filters);
            if let Some((common, rest)) = common_pre(&filters) {
                Op::Chain(common, to_filter(Op::Compose(rest)))
            } else if let Some((common, rest)) = common_post(&filters) {
                Op::Chain(to_filter(Op::Compose(rest)), common)
            } else if grouped.len() != 1 && grouped.len() != filters.len() {
                Op::Compose(
                    grouped
                        .drain(..)
                        .map(|x| to_filter(Op::Compose(x)))
                        .collect(),
                )
            } else {
                let mut filters = prefix_sort(&filters);
                Op::Compose(filters.drain(..).map(step).collect())
            }
        }
        Op::Chain(a, b) => match (to_op(a), to_op(b)) {
            (Op::Chain(x, y), b) => Op::Chain(x, to_filter(Op::Chain(y, to_filter(b)))),
            (Op::Prefix(a), Op::Subdir(b)) if a == b => Op::Nop,
            (Op::Prefix(a), Op::Subdir(b))
                if a != b && a.components().count() == b.components().count() =>
            {
                Op::Empty
            }
            (Op::Prefix(a), Op::Subdir(b)) if a != b => {
                if let Ok(stripped) = a.strip_prefix(&b) {
                    Op::Prefix(stripped.to_owned())
                } else {
                    to_op(filter)
                }
            }
            (Op::Nop, b) => b,
            (a, Op::Nop) => a,
            (Op::Empty, _) => Op::Empty,
            (_, Op::Empty) => Op::Empty,
            (a, b) => Op::Chain(step(to_filter(a)), step(to_filter(b))),
        },
        Op::Exclude(b) if b == to_filter(Op::Nop) => Op::Empty,
        Op::Exclude(b) if b == to_filter(Op::Empty) => Op::Nop,
        Op::Exclude(b) => Op::Exclude(step(b)),
        Op::Subtract(a, b) if a == b => Op::Empty,
        Op::Subtract(af, bf) => match (to_op(af), to_op(bf)) {
            (Op::Empty, _) => Op::Empty,
            (Op::Message(..), Op::Message(..)) => Op::Empty,
            (_, Op::Nop) => Op::Empty,
            (a, Op::Empty) => a,
            (Op::Chain(a, b), Op::Chain(c, d)) if a == c => {
                Op::Chain(a, to_filter(Op::Subtract(b, d)))
            }
            (_, b) if prefix_of(b.clone()) != to_filter(Op::Nop) => {
                Op::Subtract(af, last_chain(to_filter(Op::Nop), to_filter(b)).0)
            }
            (a, _) if prefix_of(a.clone()) != to_filter(Op::Nop) => Op::Chain(
                to_filter(Op::Subtract(
                    last_chain(to_filter(Op::Nop), to_filter(a.clone())).0,
                    bf,
                )),
                prefix_of(a),
            ),
            (_, b) if is_prefix(b.clone()) => Op::Subtract(af, to_filter(Op::Nop)),
            _ if common_post(&vec![af, bf]).is_some() => {
                let (cp, rest) = common_post(&vec![af, bf]).unwrap();
                Op::Chain(to_filter(Op::Subtract(rest[0], rest[1])), cp)
            }
            (Op::Compose(mut av), _) if av.contains(&bf) => {
                av.retain(|x| *x != bf);
                to_op(step(to_filter(Op::Compose(av))))
            }
            (_, Op::Compose(bv)) if bv.contains(&af) => to_op(step(to_filter(Op::Empty))),
            (Op::Compose(mut av), Op::Compose(mut bv)) => {
                let v = av.clone();
                av.retain(|x| !bv.contains(x));
                bv.retain(|x| !v.contains(x));

                Op::Subtract(
                    step(to_filter(Op::Compose(av))),
                    step(to_filter(Op::Compose(bv))),
                )
            }
            (a, b) => Op::Subtract(step(to_filter(a)), step(to_filter(b))),
        },
        _ => to_op(filter),
    });

    OPTIMIZED.lock().unwrap().insert(original, result);
    result
}

pub fn invert(filter: Filter) -> JoshResult<Filter> {
    let result = match to_op(filter) {
        Op::Nop => Some(Op::Nop),
        Op::Message(..) => Some(Op::Nop),
        Op::Linear => Some(Op::Nop),
        Op::Prune => Some(Op::Prune),
        Op::Unsign => Some(Op::Unsign),
        Op::Empty => Some(Op::Empty),
        Op::Subdir(path) => Some(Op::Prefix(path)),
        Op::File(path) => Some(Op::File(path)),
        Op::Prefix(path) => Some(Op::Subdir(path)),
        Op::Pattern(pattern) => Some(Op::Pattern(pattern)),
        Op::Rev(_) => Some(Op::Nop),
        Op::RegexReplace(_) => Some(Op::Nop),
        _ => None,
    };

    if let Some(result) = result {
        return Ok(to_filter(result));
    }

    let original = filter;
    if let Some(f) = INVERTED.lock().unwrap().get(&filter) {
        return Ok(*f);
    }
    rs_tracing::trace_scoped!("invert", "spec": spec(filter));

    let result = to_filter(match to_op(filter) {
        Op::Chain(a, b) => Op::Chain(invert(b)?, invert(a)?),
        Op::Compose(filters) => Op::Compose(
            filters
                .into_iter()
                .map(invert)
                .collect::<JoshResult<Vec<_>>>()?,
        ),
        Op::Exclude(filter) => Op::Exclude(invert(filter)?),
        _ => return Err(josh_error("no invert")),
    });

    let result = optimize(result);

    INVERTED.lock().unwrap().insert(original, result);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_chain_prefix_subdir() {
        // When prefix is chained with subdir, and the subdir is deeper than
        // the prefix, the errornous code optimized this to just Prefix("a")
        let filter = to_filter(Op::Chain(
            to_filter(Op::Prefix(std::path::PathBuf::from("a"))),
            to_filter(Op::Subdir(std::path::PathBuf::from("a/b"))),
        ));
        assert_eq!(filter, optimize(filter));
    }
}
