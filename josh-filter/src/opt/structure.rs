use super::invert::invert;
use crate::filter::Filter;
use crate::op::Op;
use crate::persist::{to_filter, to_op_ref};

// FIXME: This code is somewhat complex and can probably be simplified
// after the "chain as vec" refactor.
pub(super) fn group(filters: &Vec<Filter>) -> Vec<Vec<Filter>> {
    let mut res: Vec<Vec<Filter>> = vec![];
    for f in filters {
        if res.is_empty() {
            res.push(vec![*f]);
            continue;
        }

        if let Op::Chain(filters) = to_op_ref(*f)
            && !filters.is_empty()
            && let Op::Chain(other_filters) = to_op_ref(res[res.len() - 1][0])
            && !other_filters.is_empty()
            && filters[0] == other_filters[0]
        {
            let n = res.len();
            res[n - 1].push(*f);
            continue;
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

pub(super) fn last_chain(rest: Filter, filter: Filter) -> (Filter, Filter) {
    match to_op_ref(filter) {
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

pub(super) fn common_pre(filters: &Vec<Filter>) -> Option<(Filter, Vec<Filter>)> {
    let mut rest = vec![];
    let mut c: Option<Filter> = None;
    for f in filters {
        if let Op::Chain(chain_filters) = to_op_ref(*f) {
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
    let c = c?;
    // Do not hoist a generative leaf (Insert/TreeId). Such a filter fabricates its
    // output independently in each branch; hoisting it would turn the branches into a
    // single shared source fed through a prefix-compose, which the compose uniqueness
    // handling then collapses down to one branch (dropping the siblings).
    if is_generative(c) {
        return None;
    }
    Some((c, rest))
}

/// A generative filter produces tree entries out of nothing (it ignores its input), so its
/// output is meant to appear independently in every branch of a compose rather than being
/// treated as a single shared source that the uniqueness handling may deduplicate.
fn is_generative(filter: Filter) -> bool {
    matches!(to_op_ref(filter), Op::Insert(..) | Op::TreeId(..))
}

/// Whether applying `filter` to an *empty* tree can produce a *non-empty* tree.
///
/// Only the generative ops (`Insert`, `TreeId`) fabricate content from nothing; every other
/// tree op maps empty to empty (`File` uses a zero oid as a delete sentinel, and path/content
/// ops have nothing to act on). Generative entries also survive `tree::compose`, so a generative
/// op nested inside `Chain`/`Compose`/the kept side of `Subtract` still resurrects. `Exclude`,
/// `Select` and `Pin` always yield empty on an empty input regardless of their operand, so they
/// (and every other op) are treated as non-resurrecting. Unknown ops default to `false`, which is
/// sound for the verified op set and only ever makes callers less aggressive.
pub(super) fn resurrects_from_empty(filter: Filter) -> bool {
    match to_op_ref(filter) {
        Op::Insert(..) | Op::TreeId(..) => true,
        Op::Chain(filters) | Op::Compose(filters) => {
            filters.iter().any(|f| resurrects_from_empty(*f))
        }
        Op::Subtract(a, _) => resurrects_from_empty(*a),
        _ => false,
    }
}

/// Whether `filter` commutes with `tree::compose`, i.e. `filter(compose(t0, t1, ..)) ==
/// compose(filter(t0), filter(t1), ..)`. This is the condition for pulling a chain element that
/// sits *after* a `Compose` into each of the compose's branches (see `flatten`). Only pure path
/// relocation/selection ops distribute: `Prefix`/`Subdir` (and `Nop`/`Empty`), plus `Chain`/
/// `Compose` built from them. Ops whose result at one path depends on another path -- `Exclude`,
/// `Select`, `Subtract`, `File` -- do not, since the paths they read may be split across branches
/// and only meet in the composed tree. The whitelist is deliberately conservative: anything not
/// listed is assumed not to distribute, which only ever suppresses the optimization.
pub(super) fn distributes_over_compose(filter: Filter) -> bool {
    match to_op_ref(filter) {
        Op::Nop | Op::Empty | Op::Prefix(_) | Op::Subdir(_) => true,
        Op::Chain(filters) | Op::Compose(filters) => {
            filters.iter().all(|f| distributes_over_compose(*f))
        }
        _ => false,
    }
}

pub(super) fn common_post(filters: &Vec<Filter>) -> Option<(Filter, Vec<Filter>)> {
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
        if invert(c).is_ok() && invert(c).unwrap() == c {
            common_post.map(|c| (c, rest))
        } else if let Op::Prefix(_) = to_op_ref(c) {
            common_post.map(|c| (c, rest))
        } else if let Op::Message(..) = to_op_ref(c) {
            common_post.map(|c| (c, rest))
        } else {
            None
        }
    } else {
        None
    }
}
