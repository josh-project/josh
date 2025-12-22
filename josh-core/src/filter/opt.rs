/*
 * Filter optimization and transformation functions.
 * All those functions convert filters from one equivalent representation into another.
 */

use super::*;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::LazyLock;

use crate::filter::hash::PassthroughHasher;

type FilterHashMap = HashMap<Filter, Filter, BuildHasherDefault<PassthroughHasher>>;

static OPTIMIZED: LazyLock<std::sync::Mutex<FilterHashMap>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::default()));
static INVERTED: LazyLock<std::sync::Mutex<FilterHashMap>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::default()));
static SIMPLIFIED: LazyLock<std::sync::Mutex<FilterHashMap>> =
    LazyLock::new(|| std::sync::Mutex::new(HashMap::default()));

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
    rs_tracing::trace_scoped!(
        "simplify",
        "spec": crate::flang::spec2(&to_op(filter))
    );
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
        Op::Chain(filters) => {
            // Flatten nested chains
            let mut flattened = Vec::with_capacity(filters.len());
            for filter in filters {
                if let Op::Chain(nested) = to_op(filter) {
                    flattened.extend(nested);
                } else {
                    flattened.push(filter);
                }
            }
            // Simplify each filter
            let simplified: Vec<_> = flattened.iter().map(|f| simplify(*f)).collect();
            // Try to combine adjacent Prefix/Subdir operations
            let mut result = vec![];
            let mut i = 0;
            while i < simplified.len() {
                if i + 1 < simplified.len() {
                    match (to_op(simplified[i]), to_op(simplified[i + 1])) {
                        (Op::Prefix(x), Op::Prefix(y)) => {
                            result.push(to_filter(Op::Prefix(y.join(x))));
                            i += 2;
                            continue;
                        }
                        (Op::Subdir(x), Op::Subdir(y)) => {
                            result.push(to_filter(Op::Subdir(x.join(y))));
                            i += 2;
                            continue;
                        }
                        _ => {}
                    }
                }
                result.push(simplified[i]);
                i += 1;
            }
            if result.len() == 1 {
                to_op(result[0])
            } else {
                Op::Chain(result)
            }
        }
        Op::Subtract(a, b) => {
            let (a, b) = (to_op(a), to_op(b));
            Op::Subtract(simplify(to_filter(a)), simplify(to_filter(b)))
        }
        Op::Exclude(b) => Op::Exclude(simplify(b)),
        Op::Pin(b) => Op::Pin(simplify(b)),
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
        Op::Chain(filters) => {
            // Flatten nested chains first
            let mut flattened = vec![];
            for filter in filters {
                if let Op::Chain(nested) = to_op(filter) {
                    flattened.extend(nested);
                } else {
                    flattened.push(filter);
                }
            }
            // Check if any filter is a Compose and distribute
            for (i, filter) in flattened.iter().enumerate() {
                if let Op::Compose(compose_filters) = to_op(*filter) {
                    // Distribute: create a Compose where each element is the chain with one compose element
                    let mut result = vec![];
                    for compose_filter in compose_filters {
                        let mut new_chain = flattened.clone();
                        new_chain[i] = compose_filter;
                        result.push(to_filter(Op::Chain(new_chain)));
                    }
                    return to_filter(Op::Compose(result));
                }
            }
            Op::Chain(flattened.iter().map(|f| flatten(*f)).collect())
        }
        Op::Subtract(a, b) => {
            let (a, b) = (to_op(a), to_op(b));
            Op::Subtract(flatten(to_filter(a)), flatten(to_filter(b)))
        }
        Op::Exclude(b) => Op::Exclude(flatten(b)),
        Op::Pin(b) => Op::Pin(flatten(b)),
        _ => to_op(filter),
    });

    if result == original {
        result
    } else {
        flatten(result)
    }
}

// FIXME: This code is somewhat complex and can probably be simplified
// after the "chain as vec" refactor.
fn group(filters: &Vec<Filter>) -> Vec<Vec<Filter>> {
    let mut res: Vec<Vec<Filter>> = vec![];
    for f in filters {
        if res.is_empty() {
            res.push(vec![*f]);
            continue;
        }

        if let Op::Chain(filters) = to_op(*f)
            && !filters.is_empty()
        {
            if let Op::Chain(other_filters) = to_op(res[res.len() - 1][0])
                && !other_filters.is_empty()
                && filters[0] == other_filters[0]
            {
                let n = res.len();
                res[n - 1].push(*f);
                continue;
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
        let (_, x) = last_chain(to_filter(Op::Nop), res[res.len() - 1][0]);
        if a == x {
            let n = res.len();
            res[n - 1].push(*f);
            continue;
        }
        res.push(vec![*f]);
    }
    res
}

fn last_chain(rest: Filter, filter: Filter) -> (Filter, Filter) {
    match to_op(filter) {
        Op::Chain(filters) => {
            if filters.is_empty() {
                (rest, filter)
            } else {
                let mut new_rest = vec![rest];
                new_rest.extend(filters[..filters.len() - 1].iter().copied());
                last_chain(to_filter(Op::Chain(new_rest)), filters[filters.len() - 1])
            }
        }
        _ => (rest, filter),
    }
}

#[derive(Default)]
struct PathTrie {
    children: HashMap<std::ffi::OsString, PathTrie>,
    indices: Vec<usize>,
}

impl PathTrie {
    fn insert(&mut self, path: &Path, index: usize) {
        let mut node = self;
        for comp in path.components() {
            let key = comp.as_os_str().to_owned();
            node = node.children.entry(key).or_default();
        }
        node.indices.push(index);
    }

    fn find_overlapping(&self, path: &Path) -> Vec<usize> {
        let mut result = Vec::new();
        let mut node = self;

        result.extend(&node.indices);
        for comp in path.components() {
            match node.children.get(comp.as_os_str()) {
                Some(child) => {
                    node = child;
                    result.extend(&node.indices);
                }
                None => return result,
            }
        }

        node.collect_descendants(&mut result);
        result
    }

    fn collect_descendants(&self, result: &mut Vec<usize>) {
        for child in self.children.values() {
            result.extend(&child.indices);
            child.collect_descendants(result);
        }
    }
}

type PrefixSortEdges = Vec<smallvec::SmallVec<[usize; 32]>>;

pub fn prefix_sort(filters: &[Filter]) -> Vec<Filter> {
    if filters.len() <= 1 {
        return filters.to_vec();
    }

    let n = filters.len();
    let mut outgoing: PrefixSortEdges = vec![Default::default(); n];

    let mut src_trie = PathTrie::default();
    let mut dst_trie = PathTrie::default();

    let mut maybe_push_outgoing = |j: usize, i: usize| {
        // `i` only can increase in the loop below,
        // so it's enough to just check the last element
        if Some(i) != outgoing[j].last().cloned() {
            outgoing[j].push(i);
        }
    };

    for (i, filter) in filters.iter().enumerate() {
        let src = src_path(filter.clone());
        let dst = dst_path(filter.clone());

        for j in src_trie.find_overlapping(&src) {
            maybe_push_outgoing(j, i);
        }

        for j in dst_trie.find_overlapping(&dst) {
            maybe_push_outgoing(j, i);
        }

        src_trie.insert(&src, i);
        dst_trie.insert(&dst, i);
    }

    topo_sort_with_tiebreak(&outgoing, filters)
}

fn topo_sort_with_tiebreak(outgoing: &PrefixSortEdges, filters: &[Filter]) -> Vec<Filter> {
    use std::collections::BinaryHeap;

    let mut indegree: Vec<usize> = vec![0; filters.len()];
    for neighbors in outgoing {
        for &j in neighbors {
            indegree[j] += 1;
        }
    }

    // Use a BinaryHeap with a wrapper for custom ordering
    #[derive(Eq, PartialEq)]
    struct SortKey(usize, std::path::PathBuf, std::path::PathBuf); // (index, src, dst)

    impl Ord for SortKey {
        fn cmp(&self, other: &Self) -> Ordering {
            match other.1.cmp(&self.1) {
                Ordering::Equal => other.2.cmp(&self.2),
                ord => ord,
            }
        }
    }

    impl PartialOrd for SortKey {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    let make_key = |i: usize| -> SortKey {
        SortKey(
            i,
            src_path(filters[i].clone()),
            dst_path(filters[i].clone()),
        )
    };

    let mut heap: BinaryHeap<SortKey> = indegree
        .iter()
        .enumerate()
        .filter(|(_, deg)| **deg == 0)
        .map(|(i, _)| make_key(i))
        .collect();

    let mut result = Vec::with_capacity(filters.len());

    while let Some(SortKey(i, _, _)) = heap.pop() {
        result.push(filters[i].clone());

        for &j in outgoing[i].iter() {
            indegree[j] -= 1;
            if indegree[j] == 0 {
                heap.push(make_key(j));
            }
        }
    }

    result
}

fn common_pre(filters: &Vec<Filter>) -> Option<(Filter, Vec<Filter>)> {
    let mut rest = vec![];
    let mut c: Option<Filter> = None;
    for f in filters {
        if let Op::Chain(chain_filters) = to_op(*f) {
            if !chain_filters.is_empty() {
                let first = chain_filters[0];
                let rest_chain = if chain_filters.len() > 1 {
                    to_filter(Op::Chain(chain_filters[1..].to_vec()))
                } else {
                    to_filter(Op::Nop)
                };
                rest.push(rest_chain);
                if c.is_none() {
                    c = Some(first);
                } else if c != Some(first) {
                    return None;
                }
            } else {
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
        } else if let Op::Message(..) = to_op(c) {
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
                Op::Chain(
                    path.components()
                        .map(|x| to_filter(Op::Subdir(std::path::PathBuf::from(&x))))
                        .collect(),
                )
            } else {
                Op::Subdir(path)
            }
        }
        Op::Prefix(path) => {
            if path.components().count() > 1 {
                Op::Chain(
                    path.components()
                        .rev()
                        .map(|x| to_filter(Op::Prefix(std::path::PathBuf::from(&x))))
                        .collect(),
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
                Op::Chain(vec![common, to_filter(Op::Compose(rest))])
            } else if let Some((common, rest)) = common_post(&filters) {
                Op::Chain(vec![to_filter(Op::Compose(rest)), common])
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
        Op::Chain(filters) => {
            // Flatten nested chains
            let mut flattened = vec![];
            for filter in &filters {
                if let Op::Chain(nested) = to_op(*filter) {
                    flattened.extend(nested);
                } else {
                    flattened.push(*filter);
                }
            }

            // If any filter is Op::Empty the whole chain results in Op::Empty
            if filters.contains(&to_filter(Op::Empty)) {
                return to_filter(Op::Empty);
            }

            // Remove Nop filters
            let nop_filter = to_filter(Op::Nop);
            flattened.retain(|f| *f != nop_filter);
            if flattened.is_empty() {
                return to_filter(Op::Nop);
            }

            // Optimize adjacent Prefix/Subdir pairs
            let mut result = vec![];
            let mut i = 0;
            while i < flattened.len() {
                if i + 1 < flattened.len() {
                    match (to_op(flattened[i]), to_op(flattened[i + 1])) {
                        (Op::Prefix(a), Op::Subdir(b)) if a == b => {
                            // Skip both, they cancel out
                            i += 2;
                            continue;
                        }
                        (Op::Prefix(a), Op::Subdir(b))
                            if a != b && a.components().count() == b.components().count() =>
                        {
                            // :prefix=a:/b will always result in an empty tree since the
                            // output of :prefix=a does not have a subtree "b"
                            return to_filter(Op::Empty);
                        }
                        _ => {}
                    }
                }
                result.push(step(flattened[i]));
                i += 1;
            }
            if result.is_empty() {
                Op::Nop
            } else if result.len() == 1 {
                to_op(result[0])
            } else {
                Op::Chain(result)
            }
        }
        Op::Exclude(b) if b == to_filter(Op::Nop) => Op::Empty,
        Op::Exclude(b) | Op::Pin(b) if b == to_filter(Op::Empty) => Op::Nop,
        Op::Exclude(b) => Op::Exclude(step(b)),
        Op::Pin(b) => Op::Pin(step(b)),
        Op::Subtract(a, b) if a == b => Op::Empty,
        Op::Subtract(af, bf) => match (to_op(af), to_op(bf)) {
            (Op::Empty, _) => Op::Empty,
            (Op::Message(..), Op::Message(..)) => Op::Empty,
            (_, Op::Nop) => Op::Empty,
            (a, Op::Empty) => a,
            (Op::Chain(a_filters), Op::Chain(b_filters))
                if !a_filters.is_empty()
                    && !b_filters.is_empty()
                    && a_filters[0] == b_filters[0] =>
            {
                let mut new_a = a_filters.clone();
                let mut new_b = b_filters.clone();
                let common = new_a.remove(0);
                new_b.remove(0);
                Op::Chain(vec![
                    common,
                    to_filter(Op::Subtract(
                        to_filter(Op::Chain(new_a)),
                        to_filter(Op::Chain(new_b)),
                    )),
                ])
            }
            (_, b) if prefix_of(b.clone()) != to_filter(Op::Nop) => {
                Op::Subtract(af, last_chain(to_filter(Op::Nop), to_filter(b)).0)
            }
            (a, _) if prefix_of(a.clone()) != to_filter(Op::Nop) => Op::Chain(vec![
                to_filter(Op::Subtract(
                    last_chain(to_filter(Op::Nop), to_filter(a.clone())).0,
                    bf,
                )),
                prefix_of(a),
            ]),
            (_, b) if is_prefix(b.clone()) => Op::Subtract(af, to_filter(Op::Nop)),
            _ if common_post(&vec![af, bf]).is_some() => {
                let (cp, rest) = common_post(&vec![af, bf]).unwrap();
                Op::Chain(vec![to_filter(Op::Subtract(rest[0], rest[1])), cp])
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
        #[cfg(feature = "incubating")]
        Op::Export => Some(Op::Export),
        Op::Unsign => Some(Op::Unsign),
        Op::Empty => Some(Op::Empty),
        #[cfg(feature = "incubating")]
        Op::Link(..) => Some(Op::Unlink),
        Op::Subdir(path) => Some(Op::Prefix(path)),
        Op::File(dest_path, source_path) => Some(Op::File(source_path, dest_path)),
        Op::Prefix(path) => Some(Op::Subdir(path)),
        Op::Pattern(pattern) => Some(Op::Pattern(pattern)),
        Op::Rev(_) => Some(Op::Nop),
        Op::RegexReplace(_) => Some(Op::Nop),
        Op::Pin(_) => Some(Op::Nop),
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
        Op::Chain(filters) => {
            let inverted: Vec<_> = filters
                .iter()
                .rev()
                .map(|f| invert(*f))
                .collect::<JoshResult<_>>()?;
            Op::Chain(inverted)
        }
        Op::Compose(filters) => Op::Compose(
            filters
                .into_iter()
                .map(invert)
                .collect::<JoshResult<Vec<_>>>()?,
        ),
        Op::Exclude(filter) => Op::Exclude(invert(filter)?),
        _ => return Err(josh_error(&format!("no invert {:?}", filter))),
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
        let filter = to_filter(Op::Chain(vec![
            to_filter(Op::Prefix(std::path::PathBuf::from("a"))),
            to_filter(Op::Subdir(std::path::PathBuf::from("a/b"))),
        ]));
        let expected = to_filter(Op::Subdir(std::path::PathBuf::from("b")));
        assert_eq!(expected, optimize(filter));
    }
}
