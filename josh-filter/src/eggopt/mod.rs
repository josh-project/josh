//! Experimental egg-based filter optimizer (POC).
//!
//! A wiring/correctness proof that the [`egg`] e-graph crate can drive a real
//! josh filter optimization over the existing [`Filter`] representation
//! end-to-end. The bar is semantic: [`egg_optimize`] may only ever return a
//! filter that produces an equivalent tree and history to its input. [`opt`] is
//! used as a semantic *reference* (and as the equivalence oracle), not a
//! mechanical spec — where egg expresses an optimization more cleanly than
//! `opt`'s ordered passes, the cleaner form wins. Gated behind `--use-new-opt`.
//!
//! The rewrite set spans six families, all gated on the same semantic bar
//! (the output must produce an equivalent tree and history): Prefix/Subdir
//! cancellation and its conflict case; Compose identity, dedup, Empty-removal,
//! and flatten; Exclude/Pin identity; a Subtract algebra (identity,
//! Message-Message, pluck/absorb) as pure patterns; and a bidirectional Compose
//! set-difference. Two rules use custom appliers — the Prefix/Subdir conflict (a
//! disequality + component-count guard) and the Compose set-difference (the one
//! variadic rewrite) — because their guards are not expressible as patterns. Path
//! and Message data are structural children, so path equality and message
//! recognition are egg's unification rather than Rust conditions. Path
//! decomposition (opt E6/E7) and a unidirectional `common_pre` factoring rule are
//! present (structural paths promoted); together they let egg match `opt`'s
//! shared-prefix factoring on the corpus (see `corpus_gaps`). `common_post`
//! factoring is an Applier, not a pattern rule (see [`rules`]).

mod appliers;
mod convert;
mod lang;
mod rules;
pub mod spike_conslist;
pub mod spike_paths;

use crate::eggopt::convert::{build, rebuild};
use crate::eggopt::lang::{Josh, JoshAnalysis};
use crate::eggopt::rules::rules;
use crate::filter::Filter;
use crate::opt;
use egg::{AstSize, Extractor, RecExpr, Runner};
use std::collections::HashMap;

/// Canonicalize a filter via the trusted existing optimizer.
fn canon(f: Filter) -> Filter {
    opt::optimize(f)
}

/// Two filters are equivalent if they share a canonical form under the trusted
/// existing optimizer.
///
/// The true correctness bar is equivalent tree and history; `optimize` is used
/// here only as a sound-but-incomplete proxy oracle for that (verifying real
/// tree equivalence needs a repo, out of scope for this crate). So this check is
/// sufficient but not necessary: genuinely equivalent filters may compare
/// unequal — for instance if egg composes rules into something `opt` itself
/// wouldn't reach — in which case `egg_optimize` conservatively returns its
/// input unchanged. Strengthening this oracle is the follow-up that would let
/// egg exploit optimizations beyond `opt`'s reach.
pub fn equivalent(a: Filter, b: Filter) -> bool {
    canon(a) == canon(b)
}

/// Run the egg pipeline (build → saturate → extract → rebuild) and return the
/// reduced candidate, WITHOUT the equivalence gate.
///
/// Exposed for experiments (`optimize_compare`) that want to see what egg
/// produced even when the sound-but-incomplete [`equivalent`] gate rejects it
/// (in which case [`egg_optimize`] conservatively returns the input unchanged).
/// Returns `None` if any `Op` in the tree is not representable by the egg
/// language (`build`/`rebuild` bail out).
pub fn egg_candidate(filter: Filter) -> Option<Filter> {
    let mut expr = RecExpr::default();
    let mut seen_build = HashMap::new();
    build(&mut expr, &mut seen_build, filter)?;

    let rules = rules();
    let runner = Runner::<Josh, JoshAnalysis>::default()
        .with_expr(&expr)
        // Mandatory limits: e-graphs can blow up, and extraction always runs
        // (best-so-far) even when a limit is hit.
        .with_node_limit(10_000)
        .with_iter_limit(30)
        .run(&rules);

    let root = runner.roots[0];
    let (_cost, best) = Extractor::new(&runner.egraph, AstSize).find_best(root);

    let mut seen_rebuild = HashMap::new();
    rebuild(&best, &mut seen_rebuild, best.root())
}

/// Run the experimental egg-based optimizer over `filter`.
///
/// - Idempotent-ish and deterministic.
/// - Never returns a non-equivalent filter. If any `Op` in the tree is not
///   representable by the egg language, the input is returned unchanged; and as
///   a final guard the output is checked for equivalence, falling back to the
///   input if the egg output could not be proven equivalent.
pub fn egg_optimize(filter: Filter) -> Filter {
    let Some(candidate) = egg_candidate(filter) else {
        return filter;
    };
    if equivalent(filter, candidate) {
        candidate
    } else {
        filter
    }
}
