mod common;
use common::*;
use josh_filter::eggopt::egg_optimize;
use josh_filter::op::Op;
use josh_filter::persist::to_filter;

#[test]
fn subtract_pluck_element_from_compose() {
    // Subtract(Compose(a,b), a): a is an element of the left compose, so it
    // is removed — opt.rs case 11. The variadic applier can't handle this
    // (the right operand is a single element, not a compose); the pure
    // pluck pattern fills that gap.
    let input = to_filter(Op::Subtract(
        compose(&[subdir("a"), subdir("b")]),
        subdir("a"),
    ));
    let out = egg_optimize(input);
    assert_eq!(out, subdir("b"), "plucked element must be removed");
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}

#[test]
fn subtract_pluck_middle_of_three() {
    // Subtract(Compose(a,b,c), b) -> Compose(a,c): the 3-ary pluck rules are
    // position-independent.
    let input = to_filter(Op::Subtract(
        compose(&[subdir("a"), subdir("b"), subdir("c")]),
        subdir("b"),
    ));
    let out = egg_optimize(input);
    assert_eq!(
        out,
        compose(&[subdir("a"), subdir("c")]),
        "middle element must be plucked"
    );
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}

#[test]
fn subtract_pluck_element_from_four_compose() {
    // 4-ary pluck, middle position: Subtract(Compose(a,b,c,d), b) ->
    // Compose(a,c,d). Exercises the arity-4 pluck family.
    let input = to_filter(Op::Subtract(
        compose(&[subdir("a"), subdir("b"), subdir("c"), subdir("d")]),
        subdir("b"),
    ));
    let out = egg_optimize(input);
    assert_eq!(
        out,
        compose(&[subdir("a"), subdir("c"), subdir("d")]),
        "must pluck b from the 4-compose"
    );
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}

#[test]
fn subtract_absorb_into_compose() {
    // Subtract(a, Compose(a,b)): a is contained in the right compose, so the
    // difference is empty — opt.rs case 12.
    let input = to_filter(Op::Subtract(
        subdir("a"),
        compose(&[subdir("a"), subdir("b")]),
    ));
    let out = egg_optimize(input);
    assert_eq!(
        out,
        to_filter(Op::Empty),
        "contained element must absorb to empty"
    );
    assert_ne!(out, input, "egg must not have returned the input verbatim");
}
