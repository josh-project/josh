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
use common::report;
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
