use super::SIMPLIFIED;
use crate::filter::Filter;
use crate::op::Op;
use crate::persist::{to_filter, to_op, to_op_ref};

/*
 * Attempt to create an equivalent representation of a filter AST, that has fewer nodes than the
 * input, but still has a similar structure.
 * Useful as a pre-processing step for pretty printing and also during filter optimization.
 */
pub fn simplify(filter: Filter) -> Filter {
    if let Some(f) = SIMPLIFIED.lock().unwrap().get(&filter) {
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
            Op::Compose(out.drain(..).map(simplify).collect())
        }
        Op::Chain(filters) => {
            // Flatten nested chains
            let mut flattened = Vec::with_capacity(filters.len());
            for filter in filters {
                if let Op::Chain(nested) = to_op_ref(*filter) {
                    flattened.extend(nested.iter().copied());
                } else {
                    flattened.push(*filter);
                }
            }
            // Simplify each filter
            let simplified: Vec<_> = flattened.iter().map(|f| simplify(*f)).collect();
            // Try to combine adjacent Prefix/Subdir operations
            let mut result = vec![];
            let mut i = 0;
            while i < simplified.len() {
                if i + 1 < simplified.len() {
                    match (to_op_ref(simplified[i]), to_op_ref(simplified[i + 1])) {
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
        Op::Subtract(a, b) => Op::Subtract(simplify(*a), simplify(*b)),
        Op::Exclude(b) => Op::Exclude(simplify(*b)),
        Op::Pin(b) => Op::Pin(simplify(*b)),
        Op::Starlark(path, sub) => Op::Starlark(path.clone(), simplify(*sub)),
        Op::TreeId(path, sub) => Op::TreeId(path.clone(), simplify(*sub)),
        Op::ObjectDeref(_) => to_op(filter),
        Op::ObjectRef(_) => to_op(filter),
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
