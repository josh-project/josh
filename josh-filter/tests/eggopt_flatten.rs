mod common;
use common::*;
use josh_filter::eggopt::egg_optimize;
use josh_filter::op::Op;
use josh_filter::persist::to_filter;

/// `Compose[Compose[a, b], c]` — nesting at the head — flattens to
/// `Compose[a, b, c]`. Without the `compose-flatten` rules the nested form is
/// returned unchanged (no rule touches it), so this asserts real work.
#[test]
fn flatten_nested_head() {
    let a = subdir("a");
    let b = subdir("b");
    let c = subdir("c");

    let out = egg_optimize(compose(&[compose(&[a, b]), c]));
    assert_eq!(
        out,
        compose(&[a, b, c]),
        "Compose[Compose[a, b], c] must flatten to Compose[a, b, c]"
    );
}

/// `Compose[a, Compose[b, c]]` — nesting in the tail — flattens to
/// `Compose[a, b, c]`. The peel rule matches the inner list, not the outer
/// (whose head `a` is an atom), so this checks the spine is walked correctly.
#[test]
fn flatten_nested_tail() {
    let a = subdir("a");
    let b = subdir("b");
    let c = subdir("c");

    let out = egg_optimize(compose(&[a, compose(&[b, c])]));
    assert_eq!(
        out,
        compose(&[a, b, c]),
        "Compose[a, Compose[b, c]] must flatten to Compose[a, b, c]"
    );
}

/// `Compose[Compose[a, Compose[b, c]], d]` — nesting two levels deep — flattens
/// fully to `Compose[a, b, c, d]`. Run-to-fixpoint over the peel + base case
/// reaches any depth, mirroring `opt`'s recursive `flatten`.
#[test]
fn flatten_nested_deep() {
    let a = subdir("a");
    let b = subdir("b");
    let c = subdir("c");
    let d = subdir("d");

    let out = egg_optimize(compose(&[compose(&[a, compose(&[b, c])]), d]));
    assert_eq!(
        out,
        compose(&[a, b, c, d]),
        "Compose[Compose[a, Compose[b, c]], d] must flatten to Compose[a, b, c, d]"
    );
}

/// `Compose[Compose[a]]` — a singleton nested compose — collapses to `a`.
/// Peeling `Cons(x, Nil)` leaves a `Nil` head, which the base case drops; the
/// singleton then collapses at `rebuild`.
#[test]
fn flatten_nested_singleton() {
    let a = subdir("a");

    let out = egg_optimize(compose(&[compose(&[a])]));
    assert_eq!(out, a, "Compose[Compose[a]] must collapse to a");
}

/// `Compose[Compose[]]` — a nested empty compose (an empty list element) —
/// flattens to the `empty` atom. The base case `compose-flatten-nil` drops the
/// `Nil` head; the outer list becomes `Nil`, which `rebuild` renders as `Empty`.
/// `Compose[a, Compose[]]` likewise drops the empty element, leaving just `a`.
#[test]
fn flatten_nested_empty() {
    let a = subdir("a");
    let empty = to_filter(Op::Empty);

    assert_eq!(
        egg_optimize(compose(&[compose(&[])])),
        empty,
        "Compose[Compose[]] must flatten to Empty"
    );
    assert_eq!(
        egg_optimize(compose(&[a, compose(&[])])),
        a,
        "Compose[a, Compose[]] must drop the empty sub-compose to a"
    );
}

/// An already-flat compose is a no-op — flatten must not reorder or duplicate.
/// Guards against an over-firing rule that would change a canonical form.
#[test]
fn flatten_already_flat_is_noop() {
    let a = subdir("a");
    let b = subdir("b");

    let out = egg_optimize(compose(&[a, b]));
    assert_eq!(
        out,
        compose(&[a, b]),
        "Compose[a, b] must be returned unchanged"
    );
}
