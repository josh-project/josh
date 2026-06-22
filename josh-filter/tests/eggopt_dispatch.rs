//! Tests for the production dispatch layer (`egg_or_opt`, `parse_v2_egg`, the
//! `parse`/`optimize` dispatchers). These exercise the opt-fallback path that the
//! raw-fallback `egg_optimize` does NOT cover, and that no corpus test touched
//! before. They call the explicit functions directly, so they are deterministic
//! regardless of `USE_EGG_OPT`.

use josh_filter::eggopt::{egg_optimize, egg_or_opt};
use josh_filter::flang::parse::{parse, parse_egg, parse_v1, parse_v2_egg};
use josh_filter::flang::spec_egg;
use josh_filter::op::Op;
use josh_filter::opt::optimize_v1;
use josh_filter::persist::to_filter;

/// When egg reduces the filter AND its gate accepts, `egg_or_opt` returns the same
/// egg result as `egg_optimize`. `:subtract[::a/,::a/]` is an accept-and-reduce
/// case (corpus `subtract_self`: egg -> `:empty`, gate accepts).
#[test]
fn egg_or_opt_matches_egg_optimize_when_accepted() -> anyhow::Result<()> {
    let f = parse_egg(":subtract[::a/,::a/]")?;
    // Sanity: egg genuinely reduced it (so we are in the accept regime, not a
    // bail/reject no-op where both functions trivially agree).
    assert_ne!(
        egg_optimize(f),
        f,
        "egg should reduce subtract-self; test premise is stale"
    );
    assert_eq!(egg_or_opt(f), egg_optimize(f));
    Ok(())
}

/// When egg BAILS (an unrepresentable Op), `egg_optimize` returns the raw input
/// but `egg_or_opt` falls back to the trusted optimizer — so production never ships
/// a raw/un-reduced filter. This also covers the gate-REJECT arm (both hit the
/// same `_ =>` fallback in `egg_or_opt`).
#[test]
fn egg_or_opt_falls_back_to_opt_when_egg_bails() {
    // Op::Author is not modelled by the egg language.
    let f = to_filter(Op::Author("name".into(), "e@mail".into()));
    assert_eq!(
        egg_optimize(f),
        f,
        "egg_optimize returns the raw input on bail"
    );
    assert_eq!(
        egg_or_opt(f),
        optimize_v1(f),
        "egg_or_opt falls back to the trusted optimizer, never the raw input"
    );
}

/// `parse_v2_egg` is `egg_or_opt(parse_egg(spec))` — confirm the wiring and that
/// it renders without re-simplification (apples-to-apples with the raw path).
#[test]
fn parse_v2_egg_is_egg_or_opt_of_raw_parse() -> anyhow::Result<()> {
    let spec = ":subtract[::a/,::a/]";
    assert_eq!(
        spec_egg(parse_v2_egg(spec)?),
        spec_egg(egg_or_opt(parse_egg(spec)?))
    );
    Ok(())
}

/// By default (`USE_EGG_OPT` unset), the `parse` dispatcher routes to the trusted
/// `parse_v1`. (The egg branch is covered by `parse_v2_egg` above; flipping the
/// process-global `OnceLock` from inside a test binary is not reliable.)
#[test]
fn parse_dispatcher_defaults_to_v1() -> anyhow::Result<()> {
    assert_eq!(spec_egg(parse(":/a:/b")?), spec_egg(parse_v1(":/a:/b")?));
    Ok(())
}
