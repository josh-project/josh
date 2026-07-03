use super::FLATTENED;
use super::invert::invert;
use crate::filter::Filter;
use crate::op::Op;
use crate::persist::{to_filter, to_op, to_op_ref};

/*
 * Remove nesting from a filter.
 * This "flat" representation of the filter is more suitable calculate
 * the difference between two complex filters.
 */
pub fn flatten(filter: Filter) -> Filter {
    if let Some(f) = FLATTENED.lock().unwrap().get(&filter) {
        return *f;
    }
    let original = filter;
    let result = to_filter(match to_op_ref(filter) {
        Op::Compose(filters) => {
            let mut out = vec![];
            for f in filters {
                if let Op::Compose(v) = to_op_ref(*f) {
                    out.extend(v.iter().copied());
                } else {
                    out.push(*f);
                }
            }
            Op::Compose(out.drain(..).map(flatten).collect())
        }
        Op::Chain(filters) => {
            // Flatten nested chains first
            let mut flattened = vec![];
            for filter in filters {
                if let Op::Chain(nested) = to_op_ref(*filter) {
                    flattened.extend(nested.iter().copied());
                } else {
                    flattened.push(*filter);
                }
            }
            // Check if any filter is a Compose and distribute
            for (i, filter) in flattened.iter().enumerate() {
                if let Op::Compose(compose_filters) = to_op_ref(*filter) {
                    // Distribution duplicates the other chain elements into
                    // each branch of a new Compose. Compose children must be
                    // invertible (downstream `step()`/`common_post` calls
                    // `invert()` on them), so only distribute when every other
                    // chain element is invertible. Otherwise leave the Chain
                    // intact — distribution is an optimization, not required
                    // for correctness.
                    let others_invertible = flattened
                        .iter()
                        .enumerate()
                        .all(|(j, f)| j == i || invert(*f).is_ok());
                    if !others_invertible {
                        break;
                    }
                    // EXPERIMENTAL: distributing a trailing Compose that has a non-empty prefix is
                    // often futile: it produces Compose([Chain[prefix, z_j]]) which step()'s
                    // common_pre re-merges back to Chain[prefix, Compose(Z)]. For the
                    // Chain[file_chain, compose(pinned)] shape (reached via pin-legalization in
                    // ultrawide_pin) this is an O(N*|pinned|) build+collapse round-trip that
                    // converges to the input, so skip it to keep that path O(N). Leading/middle
                    // Composes yield branches that are not re-merged and are still distributed.
                    //
                    // Known trade-off: for some subtract/exclude-of-compose filters this leaves a
                    // valid but less-collapsed representation (see the updated pretty_print /
                    // filter_id / workspace_errors snapshots); the distribution there had enabled a
                    // downstream collapse. Refining the guard to skip only the truly-futile case is
                    // future work.
                    if flattened.len() > 1 && i == flattened.len() - 1 {
                        continue;
                    }
                    // Distribute: create a Compose where each element is the chain with one compose element
                    let mut result = vec![];
                    for compose_filter in compose_filters {
                        let mut new_chain = flattened.clone();
                        new_chain[i] = *compose_filter;
                        result.push(to_filter(Op::Chain(new_chain)));
                    }
                    let distributed = to_filter(Op::Compose(result));
                    FLATTENED.lock().unwrap().insert(original, distributed);
                    return distributed;
                }
            }
            Op::Chain(flattened.iter().map(|f| flatten(*f)).collect())
        }
        Op::Subtract(a, b) => Op::Subtract(flatten(*a), flatten(*b)),
        Op::Exclude(b) => Op::Exclude(flatten(*b)),
        Op::Pin(b) => Op::Pin(flatten(*b)),
        Op::Starlark(path, sub) => Op::Starlark(path.clone(), flatten(*sub)),
        Op::TreeId(path, sub) => Op::TreeId(path.clone(), flatten(*sub)),
        Op::ObjectDeref(_) => to_op(filter),
        Op::ObjectRef(_) => to_op(filter),
        _ => to_op(filter),
    });

    let r = if result == original {
        result
    } else {
        flatten(result)
    };

    FLATTENED.lock().unwrap().insert(original, r);
    r
}
