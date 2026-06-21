mod common;
use common::*;
use josh_filter::eggopt::{egg_optimize, equivalent};
use josh_filter::op::Op;
use josh_filter::persist::to_filter;

#[test]
fn factored_form_stays_equivalent() {
    let f = factored();
    assert!(
        equivalent(f, egg_optimize(f)),
        "egg_optimize must preserve equivalence on the factored form"
    );
}

#[test]
fn distributed_form_factors_to_cheaper() {
    let dist = distributed();
    let out = egg_optimize(dist);
    // The extractor did real work: it picked the cheaper factored form
    // rather than returning the input unchanged. With structural Prefix/Subdir
    // each path-carrying op costs 2 (op + path symbol), so AstSize(factored)
    // = 8 < AstSize(distributed) = 11.
    assert_eq!(out, factored(), "expected the cheaper factored form");
    assert_ne!(out, dist, "egg must not have returned the input verbatim");
}

#[test]
fn distribute_compose_4_factors() {
    // The arity-4 factored form Chain[p, Compose(z1..z4)] is cheaper than the
    // distributed form, so either input extracts to the factored form.
    let fact = to_filter(Op::Chain(vec![
        subdir("p"),
        compose(&[subdir("z1"), subdir("z2"), subdir("z3"), subdir("z4")]),
    ]));
    let dist = to_filter(Op::Compose(vec![
        to_filter(Op::Chain(vec![subdir("p"), subdir("z1")])),
        to_filter(Op::Chain(vec![subdir("p"), subdir("z2")])),
        to_filter(Op::Chain(vec![subdir("p"), subdir("z3")])),
        to_filter(Op::Chain(vec![subdir("p"), subdir("z4")])),
    ]));
    assert_eq!(
        egg_optimize(dist),
        fact,
        "arity-4 distributed must factor back"
    );
    assert_eq!(
        egg_optimize(fact),
        fact,
        "arity-4 factored must stay factored"
    );
    assert_ne!(
        egg_optimize(dist),
        dist,
        "egg must not have returned the input"
    );
}
