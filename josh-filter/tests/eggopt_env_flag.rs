//! Verifies the `USE_EGG_OPT` env var actually flips the `parse` dispatcher to
//! the egg branch. This is its OWN test binary (one test) so the process-global
//! `OnceLock` is first read here, after the env var is set — no race with other
//! tests. Test binaries are separate processes, so the `set_var` does not leak to
//! other suites.

use josh_filter::eggopt::egg_or_opt;
use josh_filter::flang::parse::{parse, parse_v1, parse_v2_egg};
use josh_filter::flang::spec_egg;
use josh_filter::opt;

#[test]
fn use_egg_opt_routes_parse_to_egg() {
    // A spec where opt and egg provably differ in reduction (corpus
    // `subtract_namespaces_compounded`: opt -> cost 6, egg -> cost 25, gate
    // accepts both as equivalent). So routing to egg vs opt is observable.
    let spec = ":subtract[a=:[::x/,::y/,::z/],b=:[::x/,::y/,::w/]]";

    // edition 2024: mutating the process environment is unsafe (not thread-safe
    // with concurrent env reads). This binary is single-threaded before the call.
    unsafe {
        std::env::set_var("USE_EGG_OPT", "1");
    }

    let via_dispatch = spec_egg(parse(spec).unwrap());
    let via_egg = spec_egg(parse_v2_egg(spec).unwrap());
    let via_opt = spec_egg(parse_v1(spec).unwrap());

    // The dispatcher followed the egg branch, not the opt branch.
    assert_eq!(
        via_dispatch, via_egg,
        "USE_EGG_OPT=1 should route parse to egg"
    );
    assert_ne!(
        via_dispatch, via_opt,
        "opt and egg differ on this spec; if equal, the env flag did not flip the path"
    );

    // The `optimize` dispatcher also follows the egg branch under the flag, and
    // does not recurse (its gate's oracle is the pure `optimize_v1`). `apply_to_commit`
    // is the production caller of this path; this is the no-stack-overflow guard.
    let f = parse_v2_egg(spec).unwrap();
    assert_eq!(opt::optimize(f), egg_or_opt(f));
}
