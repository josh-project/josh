use crate::filter::op::{LazyRef, Op};
use crate::filter::opt;
use crate::filter::persist::to_filter;
use crate::filter::{Filter, MESSAGE_MATCH_ALL_REGEX};

/// Create a no-op filter that passes everything through unchanged
pub fn nop() -> Filter {
    to_filter(Op::Nop)
}

/// Create an empty filter that matches nothing
pub fn empty() -> Filter {
    to_filter(Op::Empty)
}

/// Create a message filter that transforms commit messages
pub fn message(m: &str) -> Filter {
    to_filter(Op::Message(m.to_string(), MESSAGE_MATCH_ALL_REGEX.clone()))
}

/// Create a file filter that selects a single file
pub fn file(path: impl Into<std::path::PathBuf>) -> Filter {
    let p = path.into();
    to_filter(Op::File(p.clone(), p))
}

/// Create a hook filter
pub fn hook(h: &str) -> Filter {
    to_filter(Op::Hook(h.to_string()))
}

/// Create a squash filter
pub fn squash(ids: Option<&[(git2::Oid, Filter)]>) -> Filter {
    if let Some(ids) = ids {
        to_filter(Op::Squash(Some(
            ids.iter()
                .map(|(x, y)| (LazyRef::Resolved(*x), *y))
                .collect(),
        )))
    } else {
        to_filter(Op::Squash(None))
    }
}

/// Create a filter that is the result of feeding the output of `first` into `second`
pub fn chain(first: Filter, second: Filter) -> Filter {
    opt::optimize(to_filter(Op::Chain(vec![first, second])))
}

/// Create a filter that is the result of overlaying the output of `first` onto `second`
pub fn compose(first: Filter, second: Filter) -> Filter {
    opt::optimize(to_filter(Op::Compose(vec![first, second])))
}

/// Create a sequence_number filter used for tracking commit sequence numbers
pub fn sequence_number() -> Filter {
    Filter::from_oid(git2::Oid::zero())
}
