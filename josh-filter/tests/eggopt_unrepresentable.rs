use josh_filter::eggopt::egg_optimize;
use josh_filter::op::Op;
use josh_filter::persist::to_filter;

#[test]
fn unrepresentable_filter_is_identity() {
    // Op::Author is not modelled by the egg language, so the whole filter is
    // returned unchanged (never a non-equivalent result).
    let f = to_filter(Op::Author("name".into(), "e@mail".into()));
    assert_eq!(egg_optimize(f), f);
}
