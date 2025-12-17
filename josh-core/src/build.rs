use crate::filter::Filter;
use crate::filter::op::Op;
use crate::filter::opt;
use crate::filter::persist::to_filter;

/// Create a filter that is the result of overlaying the output of `first` onto `second`
pub fn compose(first: Filter, second: Filter) -> Filter {
    opt::optimize(to_filter(Op::Compose(vec![first, second])))
}
