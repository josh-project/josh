mod common;
use common::*;
use josh_filter::eggopt::egg_optimize;
use josh_filter::op::Op;
use josh_filter::persist::to_filter;

#[test]
fn compose_dedup_and_empty_removal() {
    let a = subdir("a");
    let b = subdir("b");

    // Adjacent equal elements collapse (Vec::dedup in opt).
    let out = egg_optimize(compose(&[a, a]));
    assert_eq!(out, a, "compose(a, a) must dedup to a");

    // Empty is the identity of compose, so it is dropped.
    let out = egg_optimize(compose(&[a, to_filter(Op::Empty)]));
    assert_eq!(out, a, "compose(a, empty) must drop empty");

    // Empty removed from the middle of a 3-compose, leaving the pair.
    let out = egg_optimize(compose(&[a, to_filter(Op::Empty), b]));
    assert_eq!(
        out,
        compose(&[a, b]),
        "compose(a, empty, b) must reduce to compose(a, b)"
    );
}
