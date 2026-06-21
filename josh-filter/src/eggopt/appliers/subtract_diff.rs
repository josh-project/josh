use crate::eggopt::lang::{Josh, JoshAnalysis, cons_elems, cons_fold};
use egg::{Applier, EGraph, Id, PatternAst, Subst, Symbol, Var};
use std::collections::HashSet;

/// `Subtract(Compose(A), Compose(B))` → the bidirectional set difference
/// `Subtract(Compose(A\B), Compose(B\A))`.
///
/// This is the one rewrite that cannot be a pure pattern: removing a *variable*
/// number of shared elements from a cons-list `Compose` needs an applier that
/// builds the result programmatically. (The *single-element* cases are the pure
/// `pluck-head` / `pluck-deeper` / `absorb-into-list` rules; this applier handles
/// the full two-compose intersection.) It captures the *spirit* of the trusted
/// optimizer's set-difference case (`opt.rs`), not its mechanism — `opt` reaches
/// the same result via a recursive single-element `retain` over a hashed
/// `FilterSet`, whereas this adds the fully-differenced term in one step.
///
/// Bidirectional (rather than left-only `A\B`) because the equivalence gate
/// canonicalizes via `opt`, which differsences both sides — a left-only candidate
/// would be sound but fail `canon(input) == canon(candidate)` and so never fire.
/// Both forms give the same tree, so this is correct for the right reason, not just
/// to placate the gate.
///
/// Membership is by e-class identity (`egraph.find`): two elements are "the same"
/// iff they share an e-class, exactly the Filter-OID hash-consing
/// [`build`](crate::eggopt::convert::build) establishes. Self-guarding: if either
/// operand is not a cons-list, or the element sets are disjoint, it adds nothing.
/// Because the result is disjoint, it will not re-fire on its own output.
pub(crate) struct SubtractComposeDiff {
    a: Var,
    b: Var,
}

impl SubtractComposeDiff {
    pub(crate) fn new() -> Self {
        Self {
            a: "?a".parse().expect("var ?a"),
            b: "?b".parse().expect("var ?b"),
        }
    }
}

impl Applier<Josh, JoshAnalysis> for SubtractComposeDiff {
    fn vars(&self) -> Vec<Var> {
        vec![self.a, self.b]
    }

    fn apply_one(
        &self,
        egraph: &mut EGraph<Josh, JoshAnalysis>,
        eclass: Id,
        subst: &Subst,
        _searcher_ast: Option<&PatternAst<Josh>>,
        _rule_name: Symbol,
    ) -> Vec<Id> {
        let a = *subst.get(self.a).expect("bound ?a");
        let b = *subst.get(self.b).expect("bound ?b");

        let Some(av) = cons_elems(egraph, a) else {
            return vec![];
        };
        let Some(bv) = cons_elems(egraph, b) else {
            return vec![];
        };

        // Canonicalize elements to their e-class representative so two equal
        // Filters (same OID -> same Id -> same class) compare as one element.
        let a_set: HashSet<Id> = av.iter().map(|i| egraph.find(*i)).collect();
        let b_set: HashSet<Id> = bv.iter().map(|i| egraph.find(*i)).collect();

        // Only fire when an element can actually be removed; otherwise this is a
        // no-op that would just re-match the original enode every iteration.
        let overlaps = av.iter().any(|i| b_set.contains(&egraph.find(*i)))
            || bv.iter().any(|i| a_set.contains(&egraph.find(*i)));
        if !overlaps {
            return vec![];
        }

        // A\B and B\A, deduped by canonical e-class (mirrors opt's two retains).
        // First-seen order is preserved to keep extraction stable.
        let mut diff_a = Vec::new();
        let mut seen_a: HashSet<Id> = HashSet::new();
        for &i in &av {
            let c = egraph.find(i);
            if !b_set.contains(&c) && seen_a.insert(c) {
                diff_a.push(i);
            }
        }
        let mut diff_b = Vec::new();
        let mut seen_b: HashSet<Id> = HashSet::new();
        for &i in &bv {
            let c = egraph.find(i);
            if !a_set.contains(&c) && seen_b.insert(c) {
                diff_b.push(i);
            }
        }

        // Build canonical operands: empty -> the `empty` atom (so `subtract-empty-l`
        // then fires to empty), singleton -> the bare element, otherwise a
        // cons-list. This mirrors opt's Compose normalization at construction time
        // and keeps AstSize ties out of the graph (a bare element costs less than
        // `Cons(x, Nil)`).
        let left = compose_of(egraph, diff_a);
        let right = compose_of(egraph, diff_b);
        let differenced = egraph.add(Josh::Subtract([left, right]));
        egraph.union(eclass, differenced);
        vec![egraph.find(eclass)]
    }
}

/// Build a canonical compose operand from an element list: empty becomes the
/// `empty` atom, a singleton becomes the element itself, otherwise a cons-list.
/// See [`SubtractComposeDiff`] for why construction-time canonicalization matters.
fn compose_of(egraph: &mut EGraph<Josh, JoshAnalysis>, elems: Vec<Id>) -> Id {
    match elems.len() {
        0 => egraph.add(Josh::Symbol(Symbol::from("empty"))),
        1 => egraph.find(elems[0]),
        _ => cons_fold(egraph, &elems),
    }
}
