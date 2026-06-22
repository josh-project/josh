use crate::eggopt::lang::{Josh, JoshAnalysis, chain_elems, chain_fold, compose_of, cons_elems};
use egg::{Applier, EGraph, Id, PatternAst, Subst, Symbol, Var};

/// `Compose[Chain[b1, S], Chain[b2, S], …]` (every element a chain sharing the
/// same last element `S`) → `Chain[Compose[b1, b2, …], S]`.
///
/// This is opt's `common_post` factoring (opt.rs:509-538, applied at opt.rs:649-650
/// as `Chain[Compose(rest), common]`). It cannot be a pairwise pattern rule:
/// run-to-fixpoint over N chains merges two at a time, producing O(N²) intermediate
/// e-classes (the same bloat class as the dropped `distribute`), and a pairwise RHS
/// left a malformed `ChainCons` (a Compose `Cons` in a chain-tail slot) that tied the
/// dedup form under `AstSize` — extraction picked it and `rebuild` bailed. Instead
/// this applier factors all N shared-tail chains in one O(N) Rust pass and emits only
/// well-formed nodes.
///
/// Soundness does not rest on opt's self-inverse/Prefix/Message guard
/// (opt.rs:525-534): the rewrite is `(b1;S) ∪ … ∪ (bn;S) = (b1 ∪ … ∪ bn);S` —
/// right-distribution of sequence (chain) over parallel-union (compose), valid for
/// *any* `S`. So factoring is semantically unconditional; the equivalence gate is the
/// safety net for the gated `egg_optimize` path, and `egg_candidate`/the benchmark is
/// ungated.
///
/// Two practical guards (sound subsets — factoring less often, never wrongly):
/// * **All-or-nothing** (mirrors opt.rs:519-521): every compose element must be a
///   chain whose last element is the same `S`. Factoring a *subset* and replacing the
///   whole compose would drop the non-sharing elements — unsound — so any non-chain
///   element, or any disagreement on the last, bails. An empty resulting body-set
///   (every chain was the single element `[S]`) also bails, leaving `compose-dedup`
///   to handle it (otherwise `Chain[Compose[], S]` would reduce to `empty`).
/// * **Compose-tail only**: the shared tail `S` must itself be a `Compose`. This
///   targets the wide-pin shape (chains sharing a large pinned subtree) and breaks a
///   mutual explosion with `common-pre-factor`: without it, common-pre and
///   common-post feed each other (each factors the other's output composites) and the
///   e-graph grows without bound. Low-value atom-tail cases (a shared `Prefix`/`Subdir`)
///   are left to `compose-dedup`/`common-pre`.
///
/// The LHS pattern is any Compose (`(cons ?h ?tail)`); the applier walks the matched
/// e-class, so it ignores `subst` and declares no variables. The factored form is a
/// `ChainCons` (a chain), so it never re-matches the `Cons` LHS — no fixpoint loop —
/// and a re-fire guard short-circuits once the union has landed.
pub(crate) struct CommonPost;

impl CommonPost {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Applier<Josh, JoshAnalysis> for CommonPost {
    fn vars(&self) -> Vec<Var> {
        // The applier walks `eclass`, not `subst`, so it declares no variables.
        vec![]
    }

    fn apply_one(
        &self,
        egraph: &mut EGraph<Josh, JoshAnalysis>,
        eclass: Id,
        _subst: &Subst,
        _searcher_ast: Option<&PatternAst<Josh>>,
        _rule_name: Symbol,
    ) -> Vec<Id> {
        let root = egraph.find(eclass);
        let Some(elems) = cons_elems(egraph, root) else {
            return vec![];
        };
        if elems.len() < 2 {
            return vec![];
        }

        // Split every element into (body, last); bail if any element is not a chain,
        // or if the lasts disagree (all-or-nothing, mirroring opt.rs:519-521).
        let mut shared: Option<Id> = None;
        let mut bodies: Vec<Id> = Vec::with_capacity(elems.len());
        for &e in &elems {
            let Some((body, last)) = chain_body_last(egraph, e) else {
                return vec![];
            };
            let last = egraph.find(last);
            match shared {
                None => shared = Some(last),
                Some(s) if s != last => return vec![],
                _ => {}
            }
            // Canonicalize the body (chain minus its last): drop empty (the identity
            // in the union), collapse a singleton to the bare element, else fold a
            // chain spine. Same canonicalization rationale as `compose_of`.
            if let Some(b) = chain_of(egraph, &body) {
                bodies.push(b);
            }
        }
        // Every chain was the single element [S] — let compose-dedup handle it.
        if bodies.is_empty() {
            return vec![];
        }
        let shared = shared.expect(">=2 elements => >=1 chain => shared set");

        // Only factor when the shared tail is itself a Compose (a `Cons` e-class).
        // This targets the wide-pin shape (Compose of chains sharing a large pinned
        // subtree) and, crucially, breaks a mutual explosion with `common-pre-factor`:
        // common-pre's output composites have single-atom tails, so without this guard
        // common-pre and common-post feed each other (each factors the other's output),
        // growing the e-graph without bound. Factoring is sound for any tail, so
        // restricting to Compose tails is a sound subset — low-value atom-tail cases
        // are left to compose-dedup / common-pre.
        if !egraph[shared]
            .nodes
            .iter()
            .any(|n| matches!(n, Josh::Cons(_)))
        {
            return vec![];
        }

        // Chain[Compose[bodies], shared] as two nested ChainCons ending in ChainNil.
        // A bare element or a Cons in the chain-tail slot is the malformed-node class
        // that broke the pairwise rule; the Compose sits as a chain ELEMENT (head of
        // the outer ChainCons), the same shape common-pre-factor produces.
        let inner = compose_of(egraph, &bodies);
        let nil = egraph.add(Josh::ChainNil);
        let tail = egraph.add(Josh::ChainCons([shared, nil]));
        let factored = egraph.add(Josh::ChainCons([egraph.find(inner), egraph.find(tail)]));

        // Re-fire guard: once unioned, the hash-consed factored node lands in root's
        // class, so later iterations bail without re-unioning.
        if egraph.find(factored) == root {
            return vec![];
        }
        egraph.union(root, factored);
        vec![egraph.find(root)]
    }
}

/// Split a `ChainCons` spine at `id` into its body (all elements but the last) and
/// its last element. Returns `None` if `id` is not a pure chain spine or is empty.
/// Pure (no egraph mutation); the caller canonicalizes the body.
fn chain_body_last(egraph: &EGraph<Josh, JoshAnalysis>, id: Id) -> Option<(Vec<Id>, Id)> {
    let chain = chain_elems(egraph, id)?;
    if chain.is_empty() {
        return None;
    }
    let last = *chain.last().expect("non-empty chain");
    let body = chain[..chain.len() - 1].to_vec();
    Some((body, last))
}

/// Build a canonical chain from an element list: empty -> `None` (caller drops it),
/// singleton -> the bare element, else a `ChainCons` spine. Mirrors `compose_of`'s
/// canonicalization so `AstSize` has no bare-element-vs-`ChainCons(x, ChainNil)` tie
/// to mis-pick.
fn chain_of(egraph: &mut EGraph<Josh, JoshAnalysis>, elems: &[Id]) -> Option<Id> {
    match elems.len() {
        0 => None,
        1 => Some(egraph.find(elems[0])),
        _ => Some(chain_fold(egraph, elems)),
    }
}
