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

/// `common_pre` factoring works for TWO chains sharing a prefix (above) but NOT
/// for three or more: the pairwise `common-pre-factor` rule's intermediate result
/// is *larger* than the input (each remaining un-factored chain still carries the
/// shared element), so `AstSize` rejects the partial factoring and egg never
/// reaches the fully-factored form opt produces. This is the same N-ary problem
/// the targeted `common_post` pass solves for the tail analogue — `common_pre`
/// still has it. The namespace spec (`:[x=…,y=…,z=…]`) is the realistic form users
/// write for "give me these subtrees", so this gap shows up on ordinary
/// multi-entry namespaces and widens with the entry count.
#[test]
fn common_prefix_factor_3way() -> anyhow::Result<()> {
    assert_snapshot!(report(":[x=:/a/:/b/,y=:/a/:/c/,z=:/a/:/d/]")?, @r#"
    raw:  :[:/a/b:prefix=x,:/a/c:prefix=y,:/a/d:prefix=z] (cost 13)
    opt:  :/a:[:/b:prefix=x,:/c:prefix=y,:/d:prefix=z] (cost 12)
    egg:  :[:/a/b:prefix=x,:/a/c:prefix=y,:/a/d:prefix=z] (cost 13)
    "#);
    Ok(())
}
