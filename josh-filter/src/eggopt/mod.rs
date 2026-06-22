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
//! decomposition (opt E6/E7) is present (structural paths promoted). `common_pre`
//! (shared head) and `common_post` (shared tail) factoring are NOT pattern rules:
//! each was O(N²)/non-convergent as a rule, so both are targeted passes applied
//! between saturation rounds (see [`egg_candidate`] and [`rules`]); together they
//! let egg match `opt`'s shared-prefix/shared-suffix factoring on the corpus (see
//! `corpus_gaps`).

mod appliers;
mod convert;
mod lang;
mod rules;
pub mod spike_conslist;
pub mod spike_paths;

use crate::eggopt::appliers::{factor_all_common_post, factor_all_common_pre, factor_all_subtract};
use crate::eggopt::convert::{build, rebuild};
use crate::eggopt::lang::{Josh, JoshAnalysis};
use crate::eggopt::rules::rules;
use crate::filter::Filter;
use crate::opt;
use egg::{AstSize, EGraph, Extractor, Id, Language, RecExpr, Runner};
use std::collections::{HashMap, HashSet};

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

/// Extract the minimum-AstSize `RecExpr` rooted at `root`.
///
/// Replaces egg's `Extractor::find_best`, whose `find_costs` is a fixpoint over
/// ALL classes that re-iterates every class on each cost improvement — O(depth)
/// passes × O(classes) = O(N²) for the deep cons spines this optimizer produces
/// (e.g. the factored `Compose[file_0..file_{N-1}]`, a spine of depth N). This
/// instead does a single bottom-up pass in topological order (children before
/// parents), O(total enodes) = O(N), and matches `AstSize` exactly (same cost
/// fn, same first-min-enode tie-break). Falls back to egg's `Extractor` if the
/// class graph has a cycle — the filter shapes here are acyclic, but unions can
/// in principle create one.
///
/// Recursion depth is the e-graph's deepest spine (~N for the wide-pin shape);
/// fine at the benchmark sizes but not protected against a stack overflow on
/// pathologically deep inputs.
fn extract_best(egraph: &EGraph<Josh, JoshAnalysis>, root: Id) -> (usize, RecExpr<Josh>) {
    // Topological order (post-order DFS): each class is pushed after all of its
    // children, so iterating `order` costs children before parents.
    let mut order: Vec<Id> = Vec::new();
    let mut visited: HashSet<Id> = HashSet::new();
    let mut onstack: HashSet<Id> = HashSet::new();
    if !topo_order(egraph, root, &mut visited, &mut onstack, &mut order) {
        return Extractor::new(egraph, AstSize).find_best(root);
    }

    // Single bottom-up pass: each class's cheapest enode from its children's
    // (already-set) costs. `cost < min` (strict) keeps the first enode at the
    // minimum, matching egg's `Iterator::min_by`.
    let mut best: HashMap<Id, (usize, Josh)> = HashMap::new();
    for &id in &order {
        let id = egraph.find(id);
        let mut min: Option<(usize, Josh)> = None;
        for enode in &egraph[id].nodes {
            let mut cost = 1usize;
            let mut ok = true;
            for &child in enode.children() {
                match best.get(&egraph.find(child)) {
                    Some((cc, _)) => cost = cost.saturating_add(*cc),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                match &min {
                    None => min = Some((cost, enode.clone())),
                    Some((mc, _)) if cost < *mc => min = Some((cost, enode.clone())),
                    _ => {}
                }
            }
        }
        if let Some(m) = min {
            best.insert(id, m);
        }
    }

    let root = egraph.find(root);
    let (cost, root_node) = best
        .get(&root)
        .cloned()
        .expect("DAG root has a computed min-cost enode");
    let expr = root_node.build_recexpr(|id| best[&egraph.find(id)].1.clone());
    (cost, expr)
}

/// DFS post-order over the class graph (following every enode's children). Pushes
/// each class after all of its children, so `order` ends up children-before-parents.
/// Returns `false` if a back-edge (class-graph cycle) is found.
fn topo_order(
    egraph: &EGraph<Josh, JoshAnalysis>,
    id: Id,
    visited: &mut HashSet<Id>,
    onstack: &mut HashSet<Id>,
    order: &mut Vec<Id>,
) -> bool {
    let id = egraph.find(id);
    if !visited.insert(id) {
        return true;
    }
    onstack.insert(id);
    for enode in &egraph[id].nodes {
        for &child in enode.children() {
            let child = egraph.find(child);
            if onstack.contains(&child) {
                return false; // back-edge: class-graph cycle
            }
            if !topo_order(egraph, child, visited, onstack, order) {
                return false;
            }
        }
    }
    onstack.remove(&id);
    order.push(id);
    true
}

/// Run the egg pipeline (build → saturate → extract → rebuild) and return the
/// reduced candidate, WITHOUT the equivalence gate.
///
/// Exposed for experiments (`optimize_compare`) that want to see what egg
/// produced even when the sound-but-incomplete [`equivalent`] gate rejects it
/// (in which case [`egg_optimize`] conservatively returns the input unchanged).
/// Returns `None` if any `Op` in the tree is not representable by the egg
/// language (`build`/`rebuild` bail out).
///
/// `common_post` (shared tail) and `common_pre` (shared head) factoring are both
/// applied as **targeted passes** between saturation rounds, not as matched
/// rewrites. As a rule on `(cons ?h ?tail)` each was O(N²): that LHS matches
/// ~every cons cell of a large compose, and a matched applier re-factors each
/// suffix in O(N) (common_post); and for common_pre the pairwise rule's
/// intermediate is larger than its input, so `AstSize` rejects the partial
/// factoring and run-to-fixpoint never reaches the fully-factored form (leaving
/// multi-entry namespaces un-reduced). The targeted [`factor_all_common_pre`] and
/// [`factor_all_common_post`] each factor every compose-of-chains "spine top" once,
/// in total O(e-graph size) — so the cheap rules saturate linearly and neither
/// pass re-walks the whole graph. The subtract analogue [`factor_all_subtract`]
/// (shared head / shared Prefix tail over a `Subtract`) runs in the same loop.
/// `Subtract` is binary so it needs no pairwise care — each factoring shrinks it.
pub fn egg_candidate(filter: Filter) -> Option<Filter> {
    let mut expr = RecExpr::default();
    let mut seen_build = HashMap::new();
    build(&mut expr, &mut seen_build, filter)?;

    let rules = rules();
    let mut runner = Runner::<Josh, JoshAnalysis>::default()
        // Mandatory limits: e-graphs can blow up, and extraction always runs
        // (best-so-far) even when a limit is hit.
        .with_node_limit(100_000)
        .with_iter_limit(30)
        .with_expr(&expr);

    // Cheap-saturate to fixpoint, then factor all compose-of-chains (shared head
    // AND shared tail), repeated until a round factors nothing. `Runner::run`
    // asserts `stop_reason.is_none()` on entry (it is set when a run stops), so
    // reset it before each re-entry; the e-graph is left consistent by the passes
    // (only adds/unions). `|` (not `||`) runs BOTH passes each round so a shared
    // head factored by `common_pre` can have its inner composite's shared tail
    // picked up by `common_post` in the same round. Bounded so a pathological
    // interaction cannot loop forever; each pass's re-fire guard makes it
    // idempotent, so a `true` is genuine progress.
    runner = runner.run(&rules);
    for _ in 0..8 {
        let factored = factor_all_common_pre(&mut runner.egraph)
            | factor_all_common_post(&mut runner.egraph)
            | factor_all_subtract(&mut runner.egraph);
        if !factored {
            break;
        }
        runner.stop_reason = None;
        runner = runner.run(&rules);
    }

    let root = runner.roots[0];
    let (_cost, best) = extract_best(&runner.egraph, root);

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
