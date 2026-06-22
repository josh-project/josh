//! eggopt corpus: the Subtract algebra (identity, pluck, absorb, set-difference).
//!
//! Each snapshot is a raw/opt/egg report (see `common::report`). Where `egg`
//! differs from `opt`, egg is leaving reduction on the table — or, for the
//! `::a/`-heavy cases below, deliberately declining to match an unsound opt rule.
//!
//! The shared-element cases (`subtract_chain_shared_head` / `_tail`) are CLOSED:
//! egg's `factor_all_subtract` pass factors a shared head or a shared `Prefix`
//! tail out of a `Subtract`, matching opt (opt.rs:733-749, 761-764), and both are
//! sound (sequence distributes over subtract; a `Prefix` is a path bijection).
//!
//! The remaining `egg == raw` cases all hinge on opt's *one-sided* prefix-hoist
//! (opt.rs:750-759), which strips a trailing `Prefix` from a *single* subtract
//! operand. That rule is not sound by tree set-difference semantics — it reduces
//! `Subtract(Prefix(p), Prefix(q))` with `p != q` to `empty`, but two disjoint
//! relocations' difference is the first relocation, not empty — so egg does NOT
//! replicate it (the `::a/` operand is `Chain[Subdir(a), Prefix(a)]`; opt peels
//! its trailing `Prefix` one-sidedly). egg falls back to the raw input there,
//! which is the correct (if un-reduced) filter. `subtract_self` works regardless
//! because `a - a = empty` needs no element reduction.

mod common;
use common::{chain, prefix, report, report_f, subdir, subtract};
use insta::assert_snapshot;

#[test]
fn subtract_self() -> anyhow::Result<()> {
    assert_snapshot!(report(":subtract[::a/,::a/]")?, @r#"
    raw:  :subtract[::a/,::a/] (cost 7)
    opt:  :empty (cost 1)
    egg:  :empty (cost 1)
    "#);
    Ok(())
}

#[test]
fn subtract_pluck_from_compose() -> anyhow::Result<()> {
    assert_snapshot!(report(":subtract[:[::a/,::b/],::a/]")?, @r#"
    raw:  :subtract[:[::a/,::b/],::a/] (cost 11)
    opt:  :subtract[:[::a/,::b/],:/a] (cost 9)
    egg:  :subtract[:[::a/,::b/],::a/] (cost 11)
    "#);
    Ok(())
}

#[test]
fn subtract_absorb_into_compose() -> anyhow::Result<()> {
    assert_snapshot!(report(":subtract[::a/,:[::a/,::b/]]")?, @r#"
    raw:  :subtract[::a/,:[::a/,::b/]] (cost 11)
    opt:  :subtract[:/a,:[::a/,::b/]]:prefix=a (cost 11)
    egg:  :subtract[::a/,:[::a/,::b/]] (cost 11)
    "#);
    Ok(())
}

#[test]
fn subtract_compose_set_difference() -> anyhow::Result<()> {
    assert_snapshot!(report(":subtract[:[::a/,::b/,::c/],:[::b/,::c/,::d/]]")?, @r#"
    raw:  :subtract[:[::a/,::b/,::c/],:[::b/,::c/,::d/]] (cost 21)
    opt:  :subtract[:/a,:/d]:prefix=a (cost 5)
    egg:  :subtract[:[::a/,::b/,::c/],:[::b/,::c/,::d/]] (cost 21)
    "#);
    Ok(())
}

#[test]
fn subtract_disjoint_stays() -> anyhow::Result<()> {
    assert_snapshot!(report(":subtract[::a/,::b/]")?, @r#"
    raw:  :subtract[::a/,::b/] (cost 7)
    opt:  :subtract[:/a,:/b]:prefix=a (cost 5)
    egg:  :subtract[::a/,::b/] (cost 7)
    "#);
    Ok(())
}

/// `opt` factors a shared leading element out of a `Subtract` of two chains
/// (`Subtract(Chain[a,…], Chain[a,…]) → Chain[a, Subtract(…)]`, opt.rs:733-749).
/// egg's `factor_all_subtract` pass does the same (shared-head factoring, sound by
/// sequence-distributing-over-subtract), so egg matches opt here. The constructed
/// `Chain[Subdir("a"), …]` form is used because the spec syntax for a chain of
/// single-component ops round-trips ambiguously through `spec_egg`.
#[test]
fn subtract_chain_shared_head() -> anyhow::Result<()> {
    assert_snapshot!(
        report_f(subtract(
            chain(&[subdir("a"), subdir("b")]),
            chain(&[subdir("a"), subdir("c")]),
        ))?,
        @r#"
    raw:  :subtract[:/a/b,:/a/c] (cost 7)
    opt:  :/a:subtract[:/b,:/c] (cost 5)
    egg:  :/a:subtract[:/b,:/c] (cost 5)
    "#
    );
    Ok(())
}

/// `opt` factors a shared *trailing* `Prefix` out of a `Subtract` of two chains —
/// the subtract analogue of `common_post` (opt.rs:761-764). egg's
/// `factor_all_subtract` pass does the same when the shared tail is a `Prefix` (a
/// path bijection, the soundness guard), so egg matches opt here. (opt's *one-sided*
/// prefix-hoist, opt.rs:750-759, is a different, unsound rule that egg does not
/// replicate — see `subtract_namespaces_compounded`.)
#[test]
fn subtract_chain_shared_tail() -> anyhow::Result<()> {
    assert_snapshot!(
        report_f(subtract(
            chain(&[subdir("b"), prefix("a")]),
            chain(&[subdir("c"), prefix("a")]),
        ))?,
        @r#"
    raw:  :subtract[:/b:prefix=a,:/c:prefix=a] (cost 7)
    opt:  :subtract[:/b,:/c]:prefix=a (cost 5)
    egg:  :subtract[:/b,:/c]:prefix=a (cost 5)
    "#
    );
    Ok(())
}

/// A realistic compound `Subtract` of two namespaces — the shape that exposes all
/// the subtract gaps compounding: the shared `::x/,::y/` elements difference away,
/// the operands' `::z/`/`::w/` canonicalize, and the trailing `prefix=a` hoists.
/// `opt` collapses 25 nodes to 6; egg leaves it untouched.
///
/// egg's reduction stops short here ON PURPOSE. opt's 25→6 leans on its one-sided
/// prefix-hoist (opt.rs:750-759): after the set-difference removes the shared
/// `::x/,::y/`, the residual `Subtract(::z/, ::w/)` has operands
/// `Chain[Subdir(z),Prefix(z)]` / `Chain[Subdir(w),Prefix(w)]` whose trailing
/// `Prefix`es DIFFER, so no shared-element factoring applies. opt peels them
/// one-sidedly anyway — the same rule that mis-reduces `Subtract(Prefix(p),
/// Prefix(q))` (p≠q) to `empty` — so egg declines to follow. The equivalence gate
/// then falls egg back to the (correct, un-reduced) input. This is the one
/// remaining headlining "gap", and it is a soundness guard, not missed reduction.
#[test]
fn subtract_namespaces_compounded() -> anyhow::Result<()> {
    assert_snapshot!(
        report(":subtract[a=:[::x/,::y/,::z/],b=:[::x/,::y/,::w/]]")?,
        @r#"
    raw:  :subtract[:[::x/,::y/,::z/]:prefix=a,:[::w/,::x/,::y/]:prefix=b] (cost 25)
    opt:  :subtract[:/z,:/w]:prefix=z:prefix=a (cost 6)
    egg:  :subtract[:[::x/,::y/,::z/]:prefix=a,:[::w/,::x/,::y/]:prefix=b] (cost 25)
    "#
    );
    Ok(())
}
