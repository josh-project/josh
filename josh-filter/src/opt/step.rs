use super::prefix_sort::prefix_sort;
use super::structure::{common_post, common_pre, group, last_chain};
use super::{FilterSet, OPTIMIZED};
use crate::filter::Filter;
use crate::op::Op;
use crate::persist::{to_filter, to_op, to_op_ref};

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
pub(super) fn step(filter: Filter) -> Filter {
    if let Some(f) = OPTIMIZED.lock().unwrap().get(&filter) {
        return *f;
    }
    let original = filter;
    let result = to_filter(match to_op_ref(filter) {
        Op::Subdir(path) if path.as_os_str().is_empty() => Op::Nop,
        Op::Subdir(path) => {
            if path.components().count() > 1 {
                Op::Chain(
                    path.components()
                        .map(|x| to_filter(Op::Subdir(std::path::PathBuf::from(&x))))
                        .collect(),
                )
            } else {
                Op::Subdir(path.clone())
            }
        }
        Op::Prefix(path) if path.as_os_str().is_empty() => Op::Nop,
        Op::Prefix(path) => {
            if path.components().count() > 1 {
                Op::Chain(
                    path.components()
                        .rev()
                        .map(|x| to_filter(Op::Prefix(std::path::PathBuf::from(&x))))
                        .collect(),
                )
            } else {
                Op::Prefix(path.clone())
            }
        }
        Op::Insert(dest_path, content) if dest_path.components().count() > 1 => {
            if let (Some(dst_parent), Some(dst_name)) = (dest_path.parent(), dest_path.file_name())
            {
                Op::Chain(vec![
                    to_filter(Op::Insert(
                        std::path::PathBuf::from(dst_name),
                        content.clone(),
                    )),
                    to_filter(Op::Prefix(dst_parent.to_path_buf())),
                ])
            } else {
                Op::Insert(dest_path.clone(), content.clone())
            }
        }
        Op::File(dest_path, source_path)
            if source_path.components().count() > 1 || dest_path.components().count() > 1 =>
        {
            if let (Some(src_parent), Some(src_name), Some(dst_parent), Some(dst_name)) = (
                source_path.parent(),
                source_path.file_name(),
                dest_path.parent(),
                dest_path.file_name(),
            ) {
                Op::Chain(vec![
                    to_filter(Op::Subdir(src_parent.to_path_buf())),
                    to_filter(Op::File(
                        std::path::PathBuf::from(dst_name),
                        std::path::PathBuf::from(src_name),
                    )),
                    to_filter(Op::Prefix(dst_parent.to_path_buf())),
                ])
            } else {
                Op::File(dest_path.clone(), source_path.clone())
            }
        }
        Op::Rev(filters) => Op::Rev(
            filters
                .iter()
                .map(|(m, i, f)| (*m, i.clone(), step(*f)))
                .collect(),
        ),
        Op::Compose(filters) if filters.is_empty() => Op::Empty,
        Op::Compose(filters) if filters.len() == 1 => to_op(filters[0]),
        Op::Compose(filters) => {
            let mut filters = filters.clone();
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
            for filter in filters {
                let filter = step(*filter);
                if let Op::Chain(nested) = to_op_ref(filter) {
                    flattened.extend(nested.iter().copied());
                } else {
                    flattened.push(filter);
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
                    match (to_op_ref(flattened[i]), to_op_ref(flattened[i + 1])) {
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
        Op::Exclude(b) if *b == to_filter(Op::Nop) => Op::Empty,
        Op::Exclude(b) | Op::Pin(b) if *b == to_filter(Op::Empty) => Op::Nop,
        Op::Exclude(b) => Op::Exclude(step(*b)),
        Op::Select(b) if *b == to_filter(Op::Empty) => Op::Empty,
        Op::Select(b) if *b == to_filter(Op::Nop) => Op::Nop,
        Op::Select(b) => Op::Select(step(*b)),
        Op::Pin(b) => Op::Pin(step(*b)),
        Op::Starlark(path, sub) => Op::Starlark(path.clone(), step(*sub)),
        Op::TreeId(path, sub) => Op::TreeId(path.clone(), step(*sub)),
        Op::ObjectDeref(_) => to_op(filter),
        Op::ObjectRef(_) => to_op(filter),
        Op::Subtract(a, b) if a == b => Op::Empty,
        Op::Subtract(af, bf) => {
            let (af, bf) = (*af, *bf);
            match (to_op(af), to_op(bf)) {
                (Op::Empty, _) => Op::Empty,
                (Op::Message(..), Op::Message(..)) => Op::Empty,
                (_, Op::Nop) => Op::Empty,
                (a, Op::Empty) => a,
                // `Select(F)` and `Exclude(F)` partition the input tree by path: one keeps exactly
                // the paths `F` selects, the other keeps exactly the rest. Subtracting the excluded
                // (complement) side from the selected side therefore removes nothing, leaving just
                // `Select(F)` -- and collapsing the full `Op::Subtract` machinery (four sub-applies)
                // down to a single `Select`.
                (Op::Select(sa), Op::Exclude(sb)) if sa == sb => Op::Select(sa),
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
                _ if common_post(&vec![af, bf]).is_some_and(|cp| cp.0 != to_filter(Op::Nop)) => {
                    let (cp, rest) = common_post(&vec![af, bf]).unwrap();
                    Op::Chain(vec![to_filter(Op::Subtract(rest[0], rest[1])), cp])
                }
                (Op::Compose(mut av), _) if av.contains(&bf) => {
                    av.retain(|x| *x != bf);
                    to_op(step(to_filter(Op::Compose(av))))
                }
                (_, Op::Compose(bv)) if bv.contains(&af) => to_op(step(to_filter(Op::Empty))),
                (Op::Compose(mut av), Op::Compose(mut bv)) => {
                    // Set difference via hash lookup instead of linear `contains`,
                    // turning the O(N*M) retains into O(N+M). `Filter` is just a
                    // 20-byte git OID, so `PassthroughHasher` (identity) avoids
                    // rehashing bytes that are already a hash.
                    let bv_set: FilterSet = bv.iter().copied().collect();
                    let av_set: FilterSet = av.iter().copied().collect();
                    av.retain(|x| !bv_set.contains(x));
                    bv.retain(|x| !av_set.contains(x));

                    Op::Subtract(
                        step(to_filter(Op::Compose(av))),
                        step(to_filter(Op::Compose(bv))),
                    )
                }
                (a, b) => Op::Subtract(step(to_filter(a)), step(to_filter(b))),
            }
        }
        _ => to_op(filter),
    });

    OPTIMIZED.lock().unwrap().insert(original, result);
    result
}
