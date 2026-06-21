//! eggopt corpus: Exclude/Pin identity rules.
//!
//! Each snapshot is a raw/opt/egg report (see `common::report`). Where `egg`
//! differs from `opt`, egg is leaving reduction on the table.

mod common;
use common::report;
use insta::assert_snapshot;

#[test]
fn exclude_subdir_stays() -> anyhow::Result<()> {
    assert_snapshot!(report(":exclude[::a/]")?, @r#"
    raw:  :exclude[:[::a/]] (cost 5)
    opt:  :exclude[::a/] (cost 4)
    egg:  :exclude[::a/] (cost 4)
    "#);
    Ok(())
}

#[test]
fn pin_subdir_stays() -> anyhow::Result<()> {
    assert_snapshot!(report(":pin[::a/]")?, @r#"
    raw:  :pin[:[::a/]] (cost 5)
    opt:  :pin[::a/] (cost 4)
    egg:  :pin[::a/] (cost 4)
    "#);
    Ok(())
}

#[test]
fn exclude_empty_to_nop() -> anyhow::Result<()> {
    assert_snapshot!(report(":exclude[:empty]")?, @r#"
    raw:  :exclude[:[:empty]] (cost 3)
    opt:  :/ (cost 1)
    egg:  :exclude[:empty] (cost 2)
    "#);
    Ok(())
}

#[test]
fn pin_empty_to_nop() -> anyhow::Result<()> {
    assert_snapshot!(report(":pin[:empty]")?, @r#"
    raw:  :pin[:[:empty]] (cost 3)
    opt:  :/ (cost 1)
    egg:  :pin[:empty] (cost 2)
    "#);
    Ok(())
}

#[test]
fn exclude_nop_to_empty() -> anyhow::Result<()> {
    assert_snapshot!(report(":exclude[:nop]")?, @r#"
    raw:  :exclude[:[:/]] (cost 3)
    opt:  :empty (cost 1)
    egg:  :exclude[:/] (cost 2)
    "#);
    Ok(())
}
