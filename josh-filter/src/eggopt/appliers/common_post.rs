use crate::eggopt::lang::{Josh, JoshAnalysis, cons_fold};
use egg::{Applier, EGraph, Id, PatternAst, Subst, Symbol, Var};

/// Factor a shared chain *tail* out of a cons-list `Compose` of `Chain`s
/// (`common_post`) — the mirror of [`super::CommonPre`].
///
/// A cons-list whose every element is `Chain[…rest_i, ?t]` for one common tail
/// `?t` (≥2 elements) is equivalent to `Chain[Compose(rest_1, …), ?t]`. Suffix
/// factoring is sound only when `?t` commutes with `Compose` (overlay), i.e.
/// `t(overlay(A, B)) == overlay(t(A), t(B))` — which holds for `Prefix` (places
/// the overlay under `p/`) and `Message` (rewrites only commit metadata). This
/// mirrors `opt.rs`'s `common_post`, whose guard admits exactly those (plus
/// self-invertible ops); restricting to them also keeps the factored form one
/// `opt` itself produces, so the equivalence gate accepts it.
///
/// Like [`super::CommonPre`] it is a whole-list custom applier (the RHS rebuilds
/// the spine), self-guarding, never looping on its own output (a `Chain`).
pub(crate) struct CommonPost {
    h: Var,
    tail: Var,
}

impl CommonPost {
    pub(crate) fn new() -> Self {
        Self {
            h: "?h".parse().expect("var ?h"),
            tail: "?tail".parse().expect("var ?tail"),
        }
    }
}

impl Applier<Josh, JoshAnalysis> for CommonPost {
    fn vars(&self) -> Vec<Var> {
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
        // Every element must be a `Chain` sharing one tail.
        let mut tail: Option<Id> = None;
        let mut rests: Vec<Id> = Vec::with_capacity(elems.len());
        for &e in &elems {
            let Some((rest, t)) = chain_rest_tail(egraph, e) else {
                return vec![];
            };
            let t = egraph.find(t);
            match tail {
                None => tail = Some(t),
                Some(rt) if rt != t => return vec![],
                _ => {}
            }
            rests.push(rest);
        }
        let Some(t) = tail else {
            return vec![];
        };
        // Guard: the shared tail must commute with `Compose` (Prefix or Message).
        if !commutes_with_compose(egraph, t) {
            return vec![];
        }
        // Result: Chain[Compose(rest_1, rest_2, …), tail].
        let inner = cons_fold(egraph, &rests);
        let factored = egraph.add(Josh::Chain(vec![inner, t].into_boxed_slice()));
        egraph.union(eclass, factored);
        vec![egraph.find(eclass)]
    }
}

/// Split `id`'s e-class, if it holds a `Chain`, into `(rest_id, tail)` — the
/// mirror of `chain_head_rest`: `rest_id` is the chain minus its last kid (bare
/// kid / `Chain` / `Nop`), `tail` is the last kid. `None` if not a `Chain` or
/// empty.
fn chain_rest_tail(egraph: &mut EGraph<Josh, JoshAnalysis>, id: Id) -> Option<(Id, Id)> {
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
    let tail = kids[kids.len() - 1];
    let rest = match kids.len() {
        1 => egraph.add(Josh::Symbol(Symbol::from("nop"))),
        2 => kids[0],
        _ => egraph.add(Josh::Chain(kids[..kids.len() - 1].to_vec().into_boxed_slice())),
    };
    Some((rest, tail))
}

/// Whether the e-class `id` holds a `Prefix` or `Message` node — the ops that
/// commute with `Compose`, per `opt::common_post`'s guard.
fn commutes_with_compose(egraph: &EGraph<Josh, JoshAnalysis>, id: Id) -> bool {
    egraph[id]
        .nodes
        .iter()
        .any(|n| matches!(n, Josh::Prefix(_) | Josh::Message(_)))
}
