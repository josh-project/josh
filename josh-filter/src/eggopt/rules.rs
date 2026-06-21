use crate::eggopt::appliers::SubtractComposeDiff;
use crate::eggopt::lang::Josh;
use egg::{Rewrite, rewrite};

/// The rewrites this POC runs.
///
/// All gated on the same semantic bar — the output must produce an equivalent
/// tree and history, checked by [`crate::eggopt::equivalent`] using the trusted
/// optimizer as a sufficient-but-not-necessary oracle. They capture the *spirit*
/// of `opt.rs`, not its exact mechanism: several are one declarative step where
/// `opt.rs` recurses.
///
/// * distribute/factor: for a single chain prefix element `p`,
///   ```text
///   distribute: Chain[p, Compose(z1..zn)] == Compose(Chain[p,z1], ..., Chain[p,zn])
///   factor:     the reverse.
///   ```
///   Unconditionally semantics-preserving (Chain is sequential composition,
///   Compose is parallel merge). Written for one prefix element and compose
///   arities 2, 3, and 4 — the subset covering the `:/a:[...]` shapes from
///   `tests/filter/pretty_print.t` plus one extra arity. Longer prefixes or
///   larger composes are left untouched (and thus stay equivalent); more
///   fixed-arity cases are a mechanical follow-up.
///
/// * cancel-prefix-subdir: `Chain[Prefix(p), Subdir(p)] == Nop`, mirroring the
///   adjacent-pair cancellation in the trusted optimizer (`opt.rs` flatten).
///   Because the path is a structural child, a single pattern variable `?p`
///   unifies the prefix's and subdir's path — egg's own matcher enforces path
///   equality, so this is a pure pattern rewrite with no Rust condition. Only
///   the exact two-element chain is handled; cancelling a pair inside a longer
///   chain is a follow-up (same arity limitation as distribute/factor).
///
/// * compose identity / dedup / empty-removal: `(compose)` is the empty tree and
///   `(compose ?x)` is `?x` (exact-arity match on `Box<[Id]>`); adjacent equal
///   elements collapse (`dedup`); and `empty` is dropped wherever it appears
///   (empty is the identity of parallel merge). Mirrors `opt.rs` Compose
///   normalization and cleans up singleton/empty results from
///   [`SubtractComposeDiff`]. Dedup/empty-removal are written for arities 2 and
///   3.
///
/// * exclude / pin identity: `Exclude(nop) == empty` (excluding everything keeps
///   nothing), `Exclude(empty) == Pin(empty) == nop` (an empty tree has nothing
///   to exclude/pin). Pure patterns; mirrors `opt.rs` step.
///
/// * subtract identity algebra (pure patterns): `x - x = empty`, `empty - x =
///   empty`, `x - nop = empty` (nop selects everything), `x - empty = x`, and
///   `Message - Message = empty` (two message filters produce the same tree, so
///   their difference is empty). Message is structural so a pattern matches "any
///   message".
///
/// * subtract pluck / absorb (pure patterns): when one operand is a single
///   element (not a compose), mirror `opt.rs` cases 11/12 — pluck it out of the
///   other side's compose, or collapse to empty if it is contained there. This
///   is the gap the variadic applier cannot cover (it needs both operands to be
///   composes). Fixed arities 2, 3, and 4.
///
/// * subtract-compose-diff: `Subtract(Compose(A), Compose(B))` bidirectional set
///   difference, via [`SubtractComposeDiff`] — the one rewrite that needs a
///   custom applier because set difference is variadic.
pub(crate) fn rules() -> Vec<Rewrite<Josh, ()>> {
    vec![
        rewrite!("distribute-compose-2";
            "(chain ?p (compose ?z1 ?z2))" => "(compose (chain ?p ?z1) (chain ?p ?z2))"),
        rewrite!("factor-compose-2";
            "(compose (chain ?p ?z1) (chain ?p ?z2))" => "(chain ?p (compose ?z1 ?z2))"),
        rewrite!("distribute-compose-3";
            "(chain ?p (compose ?z1 ?z2 ?z3))" =>
            "(compose (chain ?p ?z1) (chain ?p ?z2) (chain ?p ?z3))"),
        rewrite!("factor-compose-3";
            "(compose (chain ?p ?z1) (chain ?p ?z2) (chain ?p ?z3))" =>
            "(chain ?p (compose ?z1 ?z2 ?z3))"),
        rewrite!("distribute-compose-4";
            "(chain ?p (compose ?z1 ?z2 ?z3 ?z4))" =>
            "(compose (chain ?p ?z1) (chain ?p ?z2) (chain ?p ?z3) (chain ?p ?z4))"),
        rewrite!("factor-compose-4";
            "(compose (chain ?p ?z1) (chain ?p ?z2) (chain ?p ?z3) (chain ?p ?z4))" =>
            "(chain ?p (compose ?z1 ?z2 ?z3 ?z4))"),
        rewrite!("cancel-prefix-subdir";
            "(chain (prefix ?p) (subdir ?p))" => "nop"),
        // Compose identity: empty compose = empty tree; singleton compose = its
        // sole element. Exact-arity matching on Box<[Id]> makes these patterns.
        rewrite!("compose-empty"; "(compose)" => "empty"),
        rewrite!("compose-single"; "(compose ?x)" => "?x"),
        // Compose dedup + Empty-removal (mirrors opt's `dedup()` + `retain(!=
        // Empty)` on a Compose). Empty is the identity of Compose (parallel
        // merge), so it is dropped wherever it appears; adjacent equal elements
        // collapse. Fixed arity 2 and 3 (larger is a mechanical follow-up).
        rewrite!("compose-dedup-2"; "(compose ?x ?x)" => "?x"),
        rewrite!("compose-dedup-3-0"; "(compose ?x ?x ?y)" => "(compose ?x ?y)"),
        rewrite!("compose-dedup-3-1"; "(compose ?x ?y ?y)" => "(compose ?x ?y)"),
        rewrite!("compose-drop-empty-2-0"; "(compose empty ?x)" => "?x"),
        rewrite!("compose-drop-empty-2-1"; "(compose ?x empty)" => "?x"),
        rewrite!("compose-drop-empty-3-0"; "(compose empty ?x ?y)" => "(compose ?x ?y)"),
        rewrite!("compose-drop-empty-3-1"; "(compose ?x empty ?y)" => "(compose ?x ?y)"),
        rewrite!("compose-drop-empty-3-2"; "(compose ?x ?y empty)" => "(compose ?x ?y)"),
        // Exclude / Pin identity (mirrors opt step): Exclude(nop) selects nothing
        // -> Empty; Exclude/Pin of an empty tree is a no-op -> Nop. Pure patterns
        // because `nop`/`empty` are the opaque leaf atoms.
        rewrite!("exclude-nop"; "(exclude nop)" => "empty"),
        rewrite!("exclude-empty"; "(exclude empty)" => "nop"),
        rewrite!("pin-empty"; "(pin empty)" => "nop"),
        // Subtract identity / annihilator algebra — all pure patterns; `nop`
        // and `empty` are the opaque leaf atoms.
        rewrite!("subtract-self"; "(subtract ?x ?x)" => "empty"),
        rewrite!("subtract-empty-l"; "(subtract empty ?x)" => "empty"),
        rewrite!("subtract-nop-r"; "(subtract ?x nop)" => "empty"),
        rewrite!("subtract-empty-r"; "(subtract ?x empty)" => "?x"),
        // Any two Message filters have an empty tree difference: a Message only
        // rewrites commit metadata, never the tree (opt.rs line 740). A pure
        // pattern because Message is structural — `(message ?m)` matches any
        // message regardless of its format/regex payload. The two `?m` bindings
        // are distinct variables, so this also covers identical messages (which
        // `subtract-self` reaches first).
        rewrite!("subtract-message-message";
            "(subtract (message ?m1) (message ?m2))" => "empty"),
        // Subtract pluck / absorb (pure patterns), mirroring opt.rs cases 11/12
        // for the case one operand is a single element rather than a compose —
        // the gap the variadic applier below cannot cover (it needs both
        // operands to be composes). Pluck removes the element from the compose;
        // absorb collapses to empty when the element is contained in the right.
        // Fixed arities 2, 3, and 4 (same convention as distribute/factor);
        // larger arities are a mechanical follow-up. Duplicate-element
        // degenerate cases are caught by the equivalence gate (see `equivalent`).
        rewrite!("pluck-compose-2-0"; "(subtract (compose ?a ?b) ?a)" => "?b"),
        rewrite!("pluck-compose-2-1"; "(subtract (compose ?a ?b) ?b)" => "?a"),
        rewrite!("pluck-compose-3-0";
            "(subtract (compose ?a ?b ?c) ?a)" => "(compose ?b ?c)"),
        rewrite!("pluck-compose-3-1";
            "(subtract (compose ?a ?b ?c) ?b)" => "(compose ?a ?c)"),
        rewrite!("pluck-compose-3-2";
            "(subtract (compose ?a ?b ?c) ?c)" => "(compose ?a ?b)"),
        rewrite!("pluck-compose-4-0";
            "(subtract (compose ?a ?b ?c ?d) ?a)" => "(compose ?b ?c ?d)"),
        rewrite!("pluck-compose-4-1";
            "(subtract (compose ?a ?b ?c ?d) ?b)" => "(compose ?a ?c ?d)"),
        rewrite!("pluck-compose-4-2";
            "(subtract (compose ?a ?b ?c ?d) ?c)" => "(compose ?a ?b ?d)"),
        rewrite!("pluck-compose-4-3";
            "(subtract (compose ?a ?b ?c ?d) ?d)" => "(compose ?a ?b ?c)"),
        rewrite!("absorb-compose-2-0"; "(subtract ?a (compose ?a ?b))" => "empty"),
        rewrite!("absorb-compose-2-1"; "(subtract ?a (compose ?b ?a))" => "empty"),
        rewrite!("absorb-compose-3-0";
            "(subtract ?a (compose ?a ?b ?c))" => "empty"),
        rewrite!("absorb-compose-3-1";
            "(subtract ?a (compose ?b ?a ?c))" => "empty"),
        rewrite!("absorb-compose-3-2";
            "(subtract ?a (compose ?b ?c ?a))" => "empty"),
        rewrite!("absorb-compose-4-0";
            "(subtract ?a (compose ?a ?b ?c ?d))" => "empty"),
        rewrite!("absorb-compose-4-1";
            "(subtract ?a (compose ?b ?a ?c ?d))" => "empty"),
        rewrite!("absorb-compose-4-2";
            "(subtract ?a (compose ?b ?c ?a ?d))" => "empty"),
        rewrite!("absorb-compose-4-3";
            "(subtract ?a (compose ?b ?c ?d ?a))" => "empty"),
        // Subtract set-difference over two composes — variadic, so a custom
        // applier rather than a pattern. See [`SubtractComposeDiff`].
        rewrite!("subtract-compose-diff";
            "(subtract ?a ?b)" => { SubtractComposeDiff::new() }),
    ]
}
