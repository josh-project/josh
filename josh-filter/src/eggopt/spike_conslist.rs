//! Cons-list + `egg::Analysis` de-risking spike (Tier 2 fork, option B).
//!
//! Isolated experiment — NOT wired into [`super::egg_optimize`], the CLI flag, or
//! [`crate::opt`]. It answers one question: does representing `Compose` as a
//! cons-list plus an element-set [`egg::Analysis`] let the variadic rules that hit
//! the "variadic wall" (full dedup, set-difference, common_pre/post) become
//! declarative pattern+condition rewrites instead of bespoke [`egg::Applier`]s?
//!
//! See `EGG_OPTIMIZER_POC.md` → "Findings → Cons-list spike" for the four decision
//! criteria this answers and the resulting go/no-go.

use egg::{
    Analysis, AstSize, DidMerge, EGraph, Extractor, Id, Pattern, RecExpr, Rewrite, Runner,
    Searcher, Subst, Symbol, Var, rewrite,
};
use std::collections::HashSet;

egg::define_language! {
    /// A cons-list mirror of `Compose`, isolated from the main [`super::lang::Josh`].
    ///
    /// `Cons`/`Nil` replace `Compose(Box<[Id]>)`: a list is `Cons(head, tail)`
    /// chained down to `Nil`, so a 2-child pattern matches a list of any length
    /// (no exact-arity wall). `Chain2` is a minimal binary chain used only for the
    /// distribute cost-check. `Symbol` (catch-all, last) holds the leaf atoms.
    pub enum ConsJosh {
        "cons" = Cons([Id; 2]),
        "nil" = Nil,
        "chain2" = Chain2([Id; 2]),
        Symbol(Symbol),
    }
}

/// Per-e-class element-set annotation. For a cons-list e-class this is the set of
/// canonical element `Id`s it contains; for `Nil`/atoms it is empty. Computed by
/// [`Analysis::make`] and unioned by [`Analysis::merge`] — egg re-derives it on
/// rebuild, so no `modify` hook is needed.
#[derive(Default)]
pub struct ListAnalysis;

impl Analysis<ConsJosh> for ListAnalysis {
    type Data = HashSet<Id>;

    fn make(egraph: &mut EGraph<ConsJosh, Self>, enode: &ConsJosh, _id: Id) -> Self::Data {
        match enode {
            // `EGraph::Index` canonicalizes on access, so `egraph[*t].data` is already
            // the canonical tail set; `find` is only needed for the head `Id` we store
            // into the set, so membership compares canonical representatives.
            ConsJosh::Cons([h, t]) => {
                let mut s = egraph[*t].data.clone();
                s.insert(egraph.find(*h));
                s
            }
            _ => HashSet::new(),
        }
    }

    fn merge(&mut self, to: &mut HashSet<Id>, from: HashSet<Id>) -> DidMerge {
        let pre = to.len();
        to.extend(from);
        // `from` is always a subset of the merged `to`, so `b_merged` is false; a
        // conservative `DidMerge(true, _)` re-queues parents for annotation refresh.
        DidMerge(pre != to.len(), false)
    }
}

/// Condition for the dedup rule below: the tail list's element-set contains the
/// head element (i.e. the head duplicates something later in the list). A closure
/// of this shape implements [`egg::Condition`].
fn tail_contains(
    head: &str,
    tail: &str,
) -> impl Fn(&mut EGraph<ConsJosh, ListAnalysis>, Id, &Subst) -> bool {
    let head_var = head.parse::<Var>().expect("head var");
    let tail_var = tail.parse::<Var>().expect("tail var");
    move |egraph, _eclass, subst| {
        let h = *subst.get(head_var).expect("bound ?x");
        let t = *subst.get(tail_var).expect("bound ?tail");
        egraph[t].data.contains(&egraph.find(h))
    }
}

/// The spike's rewrites. Two groups: the **payoff** (variadic dedup as a 2-arity
/// pattern + an `Analysis` condition — impossible against `Compose(Box<[Id]>)`) and
/// the **cost-check** (cons-form distribute, any arity, vs the current
/// one-rule-per-arity exact-arity patterns).
pub fn cons_rules() -> Vec<Rewrite<ConsJosh, ListAnalysis>> {
    vec![
        // Payoff: drop a head element that also appears later in the list. Run to
        // fixpoint, this dedups the whole list at any position — including
        // non-consecutive duplicates, the case opt.rs's consecutive-only `Vec::dedup`
        // misses. Compose is order-independent (a set), so keeping any occurrence is
        // the same canonical tree.
        rewrite!("cons-dedup";
            "(cons ?x ?tail)" => "?tail" if tail_contains("?x", "?tail")),
        // Cost-check: distribute a binary `Chain2` over a cons-list of any length,
        // recursively. Two rules replace the current distribute-compose-2/3/4 (one
        // rule per fixed arity). `chain2(p, z)` where `z` is a leaf is left as-is
        // (neither rule matches) — that is the distributed element.
        //
        // Note there is deliberately NO `(cons ?x nil) => ?x` singleton rule here:
        // unlike the spine-free `Compose(Box<[Id]>)` (where `(compose ?x) => ?x` is
        // sound), collapsing an *inner* `Cons(x, Nil)` would leave a bare atom in
        // its parent's tail slot and break the list spine. Cons-lists lose that
        // cheap singleton-collapse — a real cost of the pivot (see the POC findings).
        rewrite!("chain2-nil"; "(chain2 ?p nil)" => "nil"),
        rewrite!("chain2-cons";
            "(chain2 ?p (cons ?z ?tail))" =>
            "(cons (chain2 ?p ?z) (chain2 ?p ?tail))"),
    ]
}

/// Saturate `expr` under [`cons_rules`]. Mirrors the main optimizer's Runner
/// config (node/iter limits) for a fair comparison.
fn saturate(expr: &RecExpr<ConsJosh>) -> Runner<ConsJosh, ListAnalysis> {
    Runner::<ConsJosh, ListAnalysis>::default()
        .with_expr(expr)
        .with_node_limit(10_000)
        .with_iter_limit(30)
        .run(&cons_rules())
}

/// Run the spike: saturate `expr` under [`cons_rules`] and extract the cheapest
/// (`AstSize`) form.
pub fn run_spike(expr: &RecExpr<ConsJosh>) -> RecExpr<ConsJosh> {
    let runner = saturate(expr);
    let (_cost, best) = Extractor::new(&runner.egraph, AstSize).find_best(runner.roots[0]);
    best
}

/// Whether `pattern` matches anywhere in the saturated e-graph. Used by the spike
/// tests to confirm a rule *fired* even when [`run_spike`]'s `AstSize` extraction
/// would pick a different (cheaper) representative — e.g. distribute, which is a
/// node-count loss and so extracts back to the factored form.
pub fn spike_reachable(expr: &RecExpr<ConsJosh>, pattern: &str) -> bool {
    let runner = saturate(expr);
    let pat: Pattern<ConsJosh> = pattern.parse().expect("parsed spike pattern");
    !pat.search(&runner.egraph).is_empty()
}
