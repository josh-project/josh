//! Tests for the cons-list + `egg::Analysis` spike (`spike_conslist`).
//!
//! These assert on the extracted `RecExpr` directly (not through the `Filter`
//! round-trip), keeping the spike minimal. Dedup is checked by the element
//! `atom_set` plus a node-count shrink, since a given set can extract in more than
//! one cons order; the distribute cost-check asserts the full s-expression.

use std::collections::HashSet;

use egg::{Id, RecExpr};
use josh_filter::eggopt::spike_conslist::{ConsJosh, run_spike, spike_reachable};

/// Collect the leaf `Symbol` atoms reachable from `expr`'s root, skipping `nil`.
fn atom_set(expr: &RecExpr<ConsJosh>) -> HashSet<String> {
    fn walk(expr: &RecExpr<ConsJosh>, id: Id, out: &mut HashSet<String>) {
        match &expr[id] {
            ConsJosh::Symbol(s) => {
                if s.as_str() != "nil" {
                    out.insert(s.as_str().to_string());
                }
            }
            ConsJosh::Nil => {}
            ConsJosh::Cons([a, b]) | ConsJosh::Chain2([a, b]) => {
                walk(expr, *a, out);
                walk(expr, *b, out);
            }
        }
    }
    let mut out = HashSet::new();
    walk(expr, expr.root(), &mut out);
    out
}

/// Count the distinct nodes reachable from `expr`'s root (a DAG, so shared subterms
/// count once). Used to prove egg did real work (the output is strictly smaller).
fn node_count(expr: &RecExpr<ConsJosh>) -> usize {
    fn walk(expr: &RecExpr<ConsJosh>, id: Id, seen: &mut HashSet<Id>) {
        if !seen.insert(id) {
            return;
        }
        match &expr[id] {
            ConsJosh::Symbol(_) | ConsJosh::Nil => {}
            ConsJosh::Cons([a, b]) | ConsJosh::Chain2([a, b]) => {
                walk(expr, *a, seen);
                walk(expr, *b, seen);
            }
        }
    }
    let mut seen = HashSet::new();
    walk(expr, expr.root(), &mut seen);
    seen.len()
}

fn set(xs: &[&str]) -> HashSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

fn run(input: &str) -> RecExpr<ConsJosh> {
    let expr: RecExpr<ConsJosh> = input.parse().expect("parsed spike input");
    run_spike(&expr)
}

#[test]
fn dedup_consecutive() {
    // [a, a, b] -> [a, b]. opt's consecutive `Vec::dedup` also catches this one.
    let input = "(cons a (cons a (cons b nil)))";
    let out = run(input);
    assert_eq!(atom_set(&out), set(&["a", "b"]));
    assert!(node_count(&out) < node_count(&input.parse().unwrap()));
}

#[test]
fn dedup_nonconsecutive() {
    // [a, b, a] -> {a, b}. This is the case opt.rs's consecutive-only `Vec::dedup`
    // MISSES — the headline payoff of doing dedup via the element-set analysis.
    let input = "(cons a (cons b (cons a nil)))";
    let out = run(input);
    assert_eq!(atom_set(&out), set(&["a", "b"]));
    assert!(node_count(&out) < node_count(&input.parse().unwrap()));
}

#[test]
fn dedup_collapses_to_singleton() {
    // [a, a] -> [a] (left as `Cons(a, Nil)` — there is no singleton-collapse rule;
    // see the spike's "lost singleton collapse" finding).
    let input = "(cons a (cons a nil))";
    let out = run(input);
    assert_eq!(atom_set(&out), set(&["a"]));
    assert_eq!(out.to_string(), "(cons a nil)");
}

#[test]
fn dedup_noop_on_unique() {
    // [a, b] is already unique: nothing fires, and there is no singleton rule to
    // mangle the inner `Cons(b, Nil)` (which would break the list spine).
    let input = "(cons a (cons b nil))";
    let out = run(input);
    assert_eq!(atom_set(&out), set(&["a", "b"]));
    assert_eq!(out.to_string(), input);
}

#[test]
fn distribute_cost_check() {
    // Cost-check: the cons-form distribute rules (`chain2-nil`, `chain2-cons`) fire
    // over a list of any arity — verified by the distributed form being reachable in
    // the saturated e-graph. Two rules replace the current per-arity
    // distribute-compose-2/3/4. We check reachability rather than the extracted expr
    // because distribute is a node-count *loss* (7 -> 8 nodes here), so `AstSize`
    // correctly extracts the factored form — the same size-driven behavior as the
    // main POC, where factor usually wins.
    let input: RecExpr<ConsJosh> = "(chain2 p (cons z1 (cons z2 nil)))".parse().unwrap();
    assert!(
        spike_reachable(&input, "(cons (chain2 p z1) ?tail)"),
        "cons-form distribute should fire, producing a cons of distributed chain2s"
    );
}
