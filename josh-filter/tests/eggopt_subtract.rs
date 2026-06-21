mod common;
use common::*;
use josh_filter::eggopt::{egg_optimize, equivalent};
use josh_filter::op::Op;
use josh_filter::persist::to_filter;

#[test]
fn subtract_self_cancels_to_empty() {
    // Subtract(x, x) == Empty. Both children are the same Filter, so they
    // share one e-class and the (subtract ?x ?x) pattern matches natively —
    // no Rust condition, exactly like cancel-prefix-subdir.
    let x = subdir("a");
    let input = to_filter(Op::Subtract(x, x));
    let out = egg_optimize(input);
    assert_eq!(out, to_filter(Op::Empty), "x - x must cancel to Empty");
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}

#[test]
fn subtract_identity_rules() {
    // x - nop = Empty (nop selects everything); x - empty = x.
    let x = subdir("a");
    let nop = to_filter(Op::Nop);
    let empty = to_filter(Op::Empty);

    let out = egg_optimize(to_filter(Op::Subtract(x, nop)));
    assert_eq!(out, empty, "x - nop must be Empty");

    let out = egg_optimize(to_filter(Op::Subtract(x, empty)));
    assert_eq!(out, x, "x - empty must be x");
}

#[test]
fn subtract_compose_set_difference() {
    // Subtract(Compose[a,b], Compose[a,c]): element `a` is shared, so it is
    // selected-then-subtracted on the left and present on the right — it
    // contributes nothing either way. The bidirectional difference leaves
    // Subtract(b, c), which opt also produces, so the gate accepts the
    // smaller extracted form.
    let input = to_filter(Op::Subtract(
        compose(&[subdir("a"), subdir("b")]),
        compose(&[subdir("a"), subdir("c")]),
    ));
    let out = egg_optimize(input);
    let expected = to_filter(Op::Subtract(subdir("b"), subdir("c")));
    assert_eq!(
        out, expected,
        "shared compose elements must be differenced away"
    );
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}

#[test]
fn subtract_subset_collapses_to_empty() {
    // A subset of B: A\B is empty, so the difference is Empty. Exercises the
    // compose-empty cleanup of the applier's empty left side.
    let input = to_filter(Op::Subtract(
        compose(&[subdir("a"), subdir("b")]),
        compose(&[subdir("a"), subdir("b"), subdir("c")]),
    ));
    let out = egg_optimize(input);
    assert_eq!(out, to_filter(Op::Empty), "subset subtract must be Empty");
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}

#[test]
fn disjoint_compose_subtract_stays_equivalent() {
    // No shared elements: the set-difference applier self-guards (no
    // overlap), so the subtract is left structurally as-is.
    let input = to_filter(Op::Subtract(
        compose(&[subdir("a"), subdir("b")]),
        compose(&[subdir("c"), subdir("d")]),
    ));
    let out = egg_optimize(input);
    assert!(
        equivalent(input, out),
        "disjoint subtract must stay equivalent"
    );
}

#[test]
fn subtract_message_message_collapses_to_empty() {
    // Any two Message filters produce the same tree (a Message only rewrites
    // commit metadata), so their difference is empty — opt.rs line 740.
    // Distinct format/regex payloads exercise that the rule matches "any
    // message", not just identical ones.
    let input = to_filter(Op::Subtract(message("{}", ".*"), message("v{}", "[0-9]+")));
    let out = egg_optimize(input);
    assert_eq!(out, to_filter(Op::Empty), "message - message must be empty");
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}
