use josh_filter::eggopt::egg_optimize;
use josh_filter::op::Op;
use josh_filter::persist::to_filter;

#[test]
fn exclude_pin_identity_rules() {
    let nop = to_filter(Op::Nop);
    let empty = to_filter(Op::Empty);

    // (exclude nop) => empty: excluding everything keeps nothing.
    let out = egg_optimize(to_filter(Op::Exclude(nop)));
    assert_eq!(out, empty, "exclude(nop) must be empty");
    assert_ne!(out, to_filter(Op::Exclude(nop)));

    // (exclude empty) => nop: an empty tree has nothing to exclude.
    let out = egg_optimize(to_filter(Op::Exclude(empty)));
    assert_eq!(out, nop, "exclude(empty) must be nop");

    // (pin empty) => nop: pinning an empty tree is a no-op.
    let out = egg_optimize(to_filter(Op::Pin(empty)));
    assert_eq!(out, nop, "pin(empty) must be nop");
}
