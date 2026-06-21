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
fn mismatched_paths_do_not_cancel() {
    // Different paths must not cancel: the pattern requires the prefix's and
    // subdir's path to be the same variable `?p`, which mismatched paths
    // fail to satisfy, so the chain is returned unchanged.
    let input = to_filter(Op::Chain(vec![prefix("p"), subdir("q")]));
    let out = egg_optimize(input);
    assert_eq!(out, input, "mismatched paths must not be rewritten");
    assert_ne!(out, to_filter(Op::Nop), "mismatched paths must not cancel");
}
