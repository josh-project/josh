mod common;
use common::*;
use josh_filter::eggopt::egg_optimize;
use josh_filter::op::Op;
use josh_filter::persist::to_filter;

#[test]
fn prefix_subdir_chain_cancels_to_nop() {
    // Chain[Prefix(p), Subdir(p)] mirrors the adjacent-pair cancellation in
    // opt.rs flatten; the trusted optimizer reduces it to Nop, so egg's
    // extracted Nop passes the equivalence gate and is returned.
    let input = to_filter(Op::Chain(vec![prefix("p"), subdir("p")]));
    let out = egg_optimize(input);
    assert_eq!(out, to_filter(Op::Nop), "expected cancellation to Nop");
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}

#[test]
fn prefix_subdir_conflict_yields_empty() {
    // Chain[Prefix(a), Subdir(b)] with a != b but the same component count is a
    // conflict: after re-rooting at `a`, a same-depth Subdir(b) selects a subtree
    // that cannot exist, so opt reduces the whole chain to Empty.
    let input = to_filter(Op::Chain(vec![prefix("a"), subdir("b")]));
    let out = egg_optimize(input);
    assert_eq!(out, to_filter(Op::Empty), "conflicting pair must be Empty");
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}

#[test]
fn prefix_subdir_conflict_multicomponent_yields_empty() {
    // Same-depth different multi-component paths conflict too (both depth 2).
    let input = to_filter(Op::Chain(vec![prefix("a/b"), subdir("c/d")]));
    let out = egg_optimize(input);
    assert_eq!(
        out,
        to_filter(Op::Empty),
        "same-depth different multi-component paths must conflict to Empty",
    );
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}

#[test]
fn prefix_subdir_different_depth_is_empty() {
    // After structural-path decompose, Prefix(a/b) splits to [Prefix(b), Prefix(a)]
    // and flattens next to Subdir(c): the adjacent Prefix(a).Subdir(c) pair (depth
    // 1==1, different path) is a conflict -> Empty. This MATCHES opt, whose step
    // decomposes and conflicts the same way (canon of the input is Empty). Before
    // structural paths egg left this unchanged -- a divergence the equivalence
    // gate masked (canon of the input was already Empty); promotion closes it.
    let input = to_filter(Op::Chain(vec![prefix("a/b"), subdir("c")]));
    let out = egg_optimize(input);
    assert_eq!(
        out,
        to_filter(Op::Empty),
        "different-depth pair conflicts to Empty after decompose (matching opt)",
    );
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}
