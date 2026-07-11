use super::invert::invert;
use super::{FLATTENED, FLATTENED_FULL};
use crate::filter::Filter;
use crate::op::Op;
use crate::persist::{to_filter, to_op, to_op_ref};

/*
 * Remove nesting from a filter.
 * This "flat" representation of the filter is more suitable calculate
 * the difference between two complex filters.
 */
pub fn flatten(filter: Filter) -> Filter {
    flatten_impl(filter, false)
}

/*
 * Like `flatten`, but distributes *every* Compose, including the trailing-Compose case that
 * `flatten` skips as futile (see below). Distributing a trailing Compose is normally an
 * O(N*|Z|) build+collapse round-trip that converges back to the input, so `flatten` avoids it.
 * Backs `minimize`, which is the maximally-collapsing counterpart of `optimize`.
 */
pub(super) fn flatten_full(filter: Filter) -> Filter {
    flatten_impl(filter, true)
}

fn flatten_impl(filter: Filter, full: bool) -> Filter {
    let cache = if full { &FLATTENED_FULL } else { &FLATTENED };
    if let Some(f) = cache.lock().unwrap().get(&filter) {
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
            Op::Compose(out.drain(..).map(|f| flatten_impl(f, full)).collect())
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
                    // Distributing a trailing Compose with a non-empty prefix is often futile: it
                    // produces Compose([Chain[prefix, z_j]]) which step()'s common_pre re-merges
                    // back to Chain[prefix, Compose(Z)]. For the Chain[file_chain, compose(pinned)]
                    // shape (reached via pin-legalization in ultrawide_pin) this is an
                    // O(N*|pinned|) build+collapse round-trip that converges to the input, so skip
                    // it to keep that path O(N). `flatten_full` distributes it anyway, for callers
                    // that need the maximally-collapsed form and whose operands cannot blow up.
                    if !full && flattened.len() > 1 && i == flattened.len() - 1 {
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
                    cache.lock().unwrap().insert(original, distributed);
                    return distributed;
                }
            }
            Op::Chain(flattened.iter().map(|f| flatten_impl(*f, full)).collect())
        }
        Op::Subtract(a, b) => Op::Subtract(flatten_impl(*a, full), flatten_impl(*b, full)),
        Op::Exclude(b) => Op::Exclude(flatten_impl(*b, full)),
        Op::Pin(b) => Op::Pin(flatten_impl(*b, full)),
        Op::Starlark(path, sub) => Op::Starlark(path.clone(), flatten_impl(*sub, full)),
        Op::TreeId(path, sub) => Op::TreeId(path.clone(), flatten_impl(*sub, full)),
        Op::ObjectDeref(_) => to_op(filter),
        Op::ObjectRef(_) => to_op(filter),
        _ => to_op(filter),
    });

    let r = if result == original {
        result
    } else {
        flatten_impl(result, full)
    };

    cache.lock().unwrap().insert(original, r);
    r
}
