use crate::eggopt::lang::{Josh, JoshAnalysis, cons_fold};
use egg::{Applier, EGraph, Id, PatternAst, Subst, Symbol, Var};

/// Factor a shared chain *head* out of a cons-list `Compose` of `Chain`s
/// (`common_pre`).
///
/// A cons-list whose every element is `Chain[?p, …rest_i]` for one common head
/// `?p` (≥2 elements) is equivalent to `Chain[?p, Compose(rest_1, …)]` — the
/// reverse of `distribute-chain-cons`, and the common-pre factoring `opt.rs`
/// does for `Compose(Chain[p, …], …)`. It generalizes the old 2-element-only
/// `FactorChain` to chains of **any** length: the head is peeled off and each
/// element's *rest* — the remaining chain kids — becomes an element of the inner
/// `Compose` (a bare kid if only one remains, a `Chain` if several, `Nop` if the
/// element was just the head).
///
/// It cannot be a pure pattern: the result rebuilds the whole list spine, so —
/// like [`super::SubtractComposeDiff`] — it is a custom applier that self-guards
/// and constructs the factored term. Cons-lists make the *traversal* declarative
/// ([`crate::eggopt::lang::cons_elems`] walks the spine); the "all elements
/// share `?p`" check and the RHS reconstruction are whole-list. It fires only
/// when ≥2 elements share a head and at least one element has a non-empty rest,
/// so it shrinks each time and never loops on its own output (a `Chain`, not a
/// `Cons`). Soundness also rests on the equivalence gate: an over-eager fire is
/// rejected and `egg_optimize` returns the input unchanged.
pub(crate) struct CommonPre {
    h: Var,
    tail: Var,
}

impl CommonPre {
    pub(crate) fn new() -> Self {
        Self {
            h: "?h".parse().expect("var ?h"),
            tail: "?tail".parse().expect("var ?tail"),
        }
    }
}

impl Applier<Josh, JoshAnalysis> for CommonPre {
    fn vars(&self) -> Vec<Var> {
        // The searcher pattern `(cons ?h ?tail)` binds these; this applier walks
        // the matched e-class directly, but declares them to match the other
        // appliers' convention.
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
        // Every element must be a `Chain` sharing one head. `rest` is the
        // remainder after peeling the head (bare kid / Chain / Nop — see below).
        let mut head: Option<Id> = None;
        let mut rests: Vec<Id> = Vec::with_capacity(elems.len());
        for &e in &elems {
            let Some((h, rest)) = chain_head_rest(egraph, e) else {
                return vec![];
            };
            let h = egraph.find(h);
            match head {
                None => head = Some(h),
                Some(rh) if rh != h => return vec![],
                _ => {}
            }
            rests.push(rest);
        }
        let Some(h) = head else {
            return vec![];
        };
        // Result: Chain[head, Compose(rest_1, rest_2, …)].
        let inner = cons_fold(egraph, &rests);
        let factored = egraph.add(Josh::Chain(vec![h, inner].into_boxed_slice()));
        egraph.union(eclass, factored);
        vec![egraph.find(eclass)]
    }
}

/// Split `id`'s e-class, if it holds a `Chain`, into `(head, rest_id)` where
/// `rest_id` is the remaining kids: the bare kid if exactly one remains (avoids
/// singleton chains), a fresh `Chain` of the remaining kids if several, or the
/// `nop` atom if the chain was only the head. Returns `None` if the e-class is
/// not a `Chain` or is empty.
fn chain_head_rest(egraph: &mut EGraph<Josh, JoshAnalysis>, id: Id) -> Option<(Id, Id)> {
    let kids = egraph[id]
        .nodes
        .iter()
        .find_map(|n| match n {
            Josh::Chain(k) => Some(k.clone()),
            _ => None,
        })?;
    if kids.is_empty() {
        return None;
    }
    let head = kids[0];
    let rest = match kids.len() {
        1 => egraph.add(Josh::Symbol(Symbol::from("nop"))),
        2 => kids[1],
        _ => egraph.add(Josh::Chain(kids[1..].to_vec().into_boxed_slice())),
    };
    Some((head, rest))
}
