use super::INVERTED;
use crate::filter::Filter;
use crate::op::Op;
use crate::persist::{to_filter, to_op_ref};
use anyhow::anyhow;

pub fn invert(filter: Filter) -> anyhow::Result<Filter> {
    if let Some(cached) = INVERTED.lock().unwrap().get(&filter).copied() {
        return cached.ok_or_else(|| anyhow!("no invert {:?}", filter));
    }

    let computed = (|| -> anyhow::Result<Filter> {
        let result = match to_op_ref(filter) {
            Op::Nop => Some(Op::Nop),
            Op::Message(..) => Some(Op::Nop),
            Op::Prune => Some(Op::Prune),
            Op::Export => Some(Op::Export),
            Op::Empty => Some(Op::Empty),
            Op::Link(..) => Some(Op::Unlink),
            Op::Subdir(path) => Some(Op::Prefix(path.clone())),
            Op::File(dest_path, source_path) => {
                Some(Op::File(source_path.clone(), dest_path.clone()))
            }
            Op::Prefix(path) => Some(Op::Subdir(path.clone())),
            Op::Pattern(pattern) => Some(Op::Pattern(pattern.clone())),
            Op::Rev(_) => Some(Op::Nop),
            Op::RegexReplace(_) => Some(Op::Nop),
            Op::Pin(_) => Some(Op::Nop),
            Op::Insert(path, _) => {
                Some(Op::Exclude(to_filter(Op::File(path.clone(), path.clone()))))
            }
            Op::TreeId(path, _) => {
                Some(Op::Exclude(to_filter(Op::File(path.clone(), path.clone()))))
            }
            Op::ObjectDeref(path) => Some(Op::ObjectRef(path.clone())),
            Op::ObjectRef(path) => Some(Op::ObjectDeref(path.clone())),
            _ => None,
        };

        if let Some(result) = result {
            return Ok(to_filter(result));
        }

        let result = to_filter(match to_op_ref(filter) {
            Op::Meta(m, f) => Op::Meta(m.clone(), invert(*f)?),
            Op::Chain(filters) => {
                let inverted: Vec<_> = filters
                    .iter()
                    .rev()
                    .map(|f| invert(*f))
                    .collect::<Result<_, _>>()?;
                Op::Chain(inverted)
            }
            Op::Compose(filters) => Op::Compose(
                filters
                    .iter()
                    .map(|f| invert(*f))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            Op::Exclude(filter) => Op::Exclude(invert(*filter)?),
            _ => return Err(anyhow!("no invert {:?}", filter)),
        });

        Ok(result)
    })();

    INVERTED
        .lock()
        .unwrap()
        .insert(filter, computed.as_ref().ok().copied());
    computed
}
