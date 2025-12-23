use crate::filter::Filter;
use crate::filter::op::Op;
use crate::filter::opt;
use crate::filter::persist::to_filter;

/// Create a filter that is the result of overlaying the output of filters in a vector
/// sequentially; so f(0) -> f(1) -> ... -> f(N)
pub fn compose(filters: &[Filter]) -> Filter {
    opt::optimize(to_filter(Op::Compose(filters.to_vec())))
}
