//! eggopt corpus: the Compose family (flatten, dedup, identity).
//!
//! Each snapshot is a raw/opt/egg report (see `common::report`): the raw parse,
//! the trusted optimizer's output, and egg's, each with a structural cost. Where
//! `egg` differs from `opt`, egg is leaving reduction on the table.

mod common;
use common::report;
use insta::assert_snapshot;

#[test]
fn compose_dedup_adjacent() -> anyhow::Result<()> {
    assert_snapshot!(report(":[::a/,::a/]")?, @r#"
    raw:  :[::a/,::a/] (cost 7)
    opt:  ::a/ (cost 3)
    egg:  ::a/ (cost 3)
    "#);
    Ok(())
}

#[test]
fn compose_dedup_nonconsecutive() -> anyhow::Result<()> {
    assert_snapshot!(report(":[::a/,::b/,::c/,::a/]")?, @r#"
    raw:  :[::a/,::b/,::c/,::a/] (cost 13)
    opt:  :[::a/,::b/,::c/] (cost 10)
    egg:  :[::b/,::c/,::a/] (cost 10)
    "#);
    Ok(())
}

#[test]
fn compose_flatten_nested() -> anyhow::Result<()> {
    assert_snapshot!(report(":[::a/,:[::b/,::c/]]")?, @r#"
    raw:  :[::a/,:[::b/,::c/]] (cost 11)
    opt:  :[::a/,::b/,::c/] (cost 10)
    egg:  :[::a/,::b/,::c/] (cost 10)
    "#);
    Ok(())
}

#[test]
fn compose_flatten_deep() -> anyhow::Result<()> {
    assert_snapshot!(report(":[::a/,:[::b/,:[::c/,::d/]]]")?, @r#"
    raw:  :[::a/,:[::b/,::c/,::d/]] (cost 14)
    opt:  :[::a/,::b/,::c/,::d/] (cost 13)
    egg:  :[::a/,::b/,::c/,::d/] (cost 13)
    "#);
    Ok(())
}

#[test]
fn compose_two_subdirs() -> anyhow::Result<()> {
    assert_snapshot!(report(":[::a/,::b/]")?, @r#"
    raw:  :[::a/,::b/] (cost 7)
    opt:  :[::a/,::b/] (cost 7)
    egg:  :[::a/,::b/] (cost 7)
    "#);
    Ok(())
}

#[test]
fn compose_three_subdirs() -> anyhow::Result<()> {
    assert_snapshot!(report(":[::a/,::b/,::c/]")?, @r#"
    raw:  :[::a/,::b/,::c/] (cost 10)
    opt:  :[::a/,::b/,::c/] (cost 10)
    egg:  :[::a/,::b/,::c/] (cost 10)
    "#);
    Ok(())
}
