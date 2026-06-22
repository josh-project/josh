//! eggopt corpus: the Subtract algebra (identity, pluck, absorb, set-difference).
//!
//! Each snapshot is a raw/opt/egg report (see `common::report`). Where `egg`
//! differs from `opt`, egg is leaving reduction on the table.
//!
//! Why most of these show `egg == raw`: the spec `::a/` parses to
//! `Chain[Subdir("a"), Prefix("a")]` (re-root to `a`, then place under `a`) — not
//! a bare `Subdir`. `opt` reduces that chain to a canonical `:/a`; egg has no
//! path-op rule (E3–E7), so its elements stay unreduced, the equivalence gate
//! rejects egg's otherwise-valid result (opt re-canonicalizes the unreduced
//! elements differently), and `egg_optimize` falls back to the raw input. This is
//! not for want of pluck/set-diff — those rules fire on bare `Subdir`s (see the
//! `eggopt_subtract` unit tests); it is the path-op gap. `subtract_self` works
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
/// egg has no chain-aware subtract factor, so it leaves both operands intact. The
/// constructed `Chain[Subdir("a"), …]` form is used because the spec syntax for a
/// chain of single-component ops round-trips ambiguously through `spec_egg`.
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
    egg:  :subtract[:/a/b,:/a/c] (cost 7)
    "#
    );
    Ok(())
}

/// `opt` factors a shared *trailing* element (here `Prefix("a")`) out of a
/// `Subtract` of two chains — the subtract analogue of `common_post`, plus a
/// prefix-hoist (opt.rs:750-764). egg has neither rule, so both operands keep
/// their trailing prefix.
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
    egg:  :subtract[:/b:prefix=a,:/c:prefix=a] (cost 7)
    "#
    );
    Ok(())
}

/// A realistic compound `Subtract` of two namespaces — the shape that exposes all
/// the subtract gaps compounding: the shared `::x/,::y/` elements should
/// difference away, the operands' `::z/`/`::w/` should canonicalize, and the
/// trailing `prefix=a` should hoist. `opt` collapses 25 nodes to 6; egg leaves it
/// untouched. (The equivalence still holds — egg falls back to the input — so this
/// is purely a reduction-quality gap, the kind that bloats the filter applied at
/// runtime.)
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
