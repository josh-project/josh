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
