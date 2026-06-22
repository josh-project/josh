//! eggopt corpus: cases expected to expose quality gaps vs `opt`.
//!
//! These are specs where egg's rule set is known to be incomplete. The snapshot
//! locks the current (possibly-worse-than-opt) egg output so that a future rule
//! closing the gap shows up as a clear snapshot diff — and so a regression shows
//! up too. See `common::report` for the raw/opt/egg format.

mod common;
use common::report;
use insta::assert_snapshot;

/// `opt` factors the shared `:/a` prefix out of both branches (`common_pre`). egg
/// now MATCHES it: structural-path `subdir-decompose` splits each `:/a/b` /
/// `:/a/c` into `Chain[Subdir(/a), Subdir(/b|/c)]`, and `common-pre-factor` then
/// factors the shared `Subdir(/a)` head out of the Compose into
/// `Chain[Subdir(/a), Compose[…]]` — opt's exact shape (opt.rs:648). `AstSize`
/// picks the factored form here because the shared subtree (`Subdir(/a)`, a path
/// spine) is large enough to beat the decompose cost. Kept as a regression guard
/// (it was the open gap this rule set closed).
#[test]
fn workspace_common_prefix_factor() -> anyhow::Result<()> {
    assert_snapshot!(report(":[x=:/a/:/b/,y=:/a/:/c/]")?, @r#"
    raw:  :[:/a/b:prefix=x,:/a/c:prefix=y] (cost 9)
    opt:  :/a:[:/b:prefix=x,:/c:prefix=y] (cost 9)
    egg:  :/a:[:/b:prefix=x,:/c:prefix=y] (cost 9)
    "#);
    Ok(())
}
