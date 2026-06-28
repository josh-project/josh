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
    c.map(|c| (c, rest))
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
