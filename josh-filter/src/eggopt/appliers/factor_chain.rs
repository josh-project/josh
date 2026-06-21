use crate::eggopt::lang::{Josh, JoshAnalysis, cons_fold};
use egg::{Applier, EGraph, Id, PatternAst, Subst, Symbol, Var};

/// Factor a shared chain prefix out of a cons-list `Compose`.
///
/// A cons-list whose every element is `chain ?p ?_` for one `?p` is equivalent to
/// `chain ?p <Compose of the second children>` — the reverse of the
/// `distribute-chain-cons` rule and the common-pre factoring `opt.rs` does for
/// `Chain[p, Compose(..)]`. It cannot be a pure pattern: the result rebuilds the
/// whole list spine, so — like [`super::SubtractComposeDiff`] — it is a custom
/// applier that self-guards and constructs the factored term.
///
/// Cons-lists make the *traversal* declarative ([`cons_elems`] walks the spine), but
/// the "all elements share `?p`" check and the RHS reconstruction are whole-list, so
/// a richer `common_pre` analysis (phase 2) could later subsume this. It fires only
/// when ≥2 elements all share a prefix, so it never loops on its own output (which is
/// a `chain`, not a `cons`). Soundness also rests on the equivalence gate: an
/// over-eager fire is rejected and `egg_optimize` returns the input unchanged.
pub(crate) struct FactorChain {
    h: Var,
    tail: Var,
}

impl FactorChain {
    pub(crate) fn new() -> Self {
        Self {
            h: "?h".parse().expect("var ?h"),
            tail: "?tail".parse().expect("var ?tail"),
        }
    }
}

impl Applier<Josh, JoshAnalysis> for FactorChain {
    fn vars(&self) -> Vec<Var> {
        // The searcher pattern `(cons ?h ?tail)` binds these; this applier walks the
        // matched e-class directly, but declares them to match the other appliers'
        // convention.
        vec![self.h, self.tail]
    }

    fn apply_one(
        &self,
        egraph: &mut EGraph<Josh, JoshAnalysis>,
        eclass: Id,
        _subst: &Subst,
        _searcher_ast: Option<&PatternAst<Josh>>,
        _rule_name: Symbol,
    ) -> Vec<Id> {
        let eclass = egraph.find(eclass);
        let Some(elems) = crate::eggopt::lang::cons_elems(egraph, eclass) else {
            return vec![];
        };
        if elems.len() < 2 {
            return vec![];
        }
        // Every element must be a 2-element `chain ?p ?_` for one common `?p`.
        let mut prefix: Option<Id> = None;
        let mut seconds: Vec<Id> = Vec::with_capacity(elems.len());
        for &e in &elems {
            let Some((p, z)) = chain_split(egraph, e) else {
                return vec![];
            };
            let p = egraph.find(p);
            match prefix {
                None => prefix = Some(p),
                Some(rp) if rp != p => return vec![],
                _ => {}
            }
            seconds.push(z);
        }
        let Some(p) = prefix else {
            return vec![];
        };
        // Result: chain[p, cons(z1, cons(z2, ..., nil))].
        let list = cons_fold(egraph, &seconds);
        let factored = egraph.add(Josh::Chain(vec![p, list].into_boxed_slice()));
        egraph.union(eclass, factored);
        vec![egraph.find(eclass)]
    }
}

/// Split a 2-element `chain ?p ?z` in `id`'s e-class into `(p, z)`, if it has one.
fn chain_split(egraph: &EGraph<Josh, JoshAnalysis>, id: Id) -> Option<(Id, Id)> {
    egraph[id].nodes.iter().find_map(|n| match n {
        Josh::Chain(kids) if kids.len() == 2 => Some((kids[0], kids[1])),
        _ => None,
    })
}
