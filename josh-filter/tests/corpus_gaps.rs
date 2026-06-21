//! eggopt corpus: cases expected to expose quality gaps vs `opt`.
//!
//! These are specs where egg's rule set is known to be incomplete. The snapshot
//! locks the current (possibly-worse-than-opt) egg output so that a future rule
//! closing the gap shows up as a clear snapshot diff — and so a regression shows
//! up too. See `common::report` for the raw/opt/egg format.

mod common;
use common::report;
use insta::assert_snapshot;

/// `opt` factors the shared `:/a` prefix out of both branches (E12 `common_pre`).
/// egg has no general common_pre, so it should leave the branches un-factored —
/// a visible cost gap (egg cost > opt cost).
#[test]
fn workspace_common_prefix_factor() -> anyhow::Result<()> {
    assert_snapshot!(report(":[x=:/a/:/b/,y=:/a/:/c/]")?, @r#"
    raw:  :[:/a/b:prefix=x,:/a/c:prefix=y] (cost 9)
    opt:  :/a:[:/b:prefix=x,:/c:prefix=y] (cost 9)
    egg:  :/a:[:/b:prefix=x,:/c:prefix=y] (cost 9)
    "#);
    Ok(())
}
