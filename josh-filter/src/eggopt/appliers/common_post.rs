use crate::eggopt::lang::{Josh, JoshAnalysis, chain_elems, chain_fold, compose_of, cons_elems};
use egg::{EGraph, Id};
use std::collections::HashSet;

/// `Compose[Chain[b1, S], Chain[b2, S], …]` (every element a chain sharing the
/// same last element `S`) → `Chain[Compose[b1, b2, …], S]`.
///
/// This is opt's `common_post` factoring (opt.rs:509-538, applied at opt.rs:649-650
/// as `Chain[Compose(rest), common]`). It was first tried as a pairwise pattern
/// rule and then as a matched `Applier` on the LHS `(cons ?h ?tail)`. Both were
/// O(N²): run-to-fixpoint over N chains merges two at a time producing O(N²)
/// intermediate e-classes (pairwise rule); and the `(cons ?h ?tail)` LHS matches
/// ~every cons cell of an N-element compose, with the applier re-factoring each
/// suffix in O(N) — O(N) firings × O(N) = O(N²), confirmed by per-iteration
/// profiling (`apply_time` dominated by `common-post-factor`).
///
/// Instead `common_post` is now a **targeted pass**, applied directly to the
/// e-graph between cheap saturation rounds (see `egg_candidate`): it factors all
/// N shared-tail chains in a single O(N) Rust walk, with no egg matcher involved.
/// [`factor_all_common_post`] finds every maximal compose-of-chains "spine top"
/// and factors each once, so the total cost is linear in the e-graph size.
///
/// Soundness does not rest on opt's self-inverse/Prefix/Message guard
/// (opt.rs:525-534): the rewrite is `(b1;S) ∪ … ∪ (bn;S) = (b1 ∪ … ∪ bn);S` —
/// right-distribution of sequence (chain) over parallel-union (compose), valid
/// for *any* `S`. So factoring is semantically unconditional; the equivalence
/// gate is the safety net for the gated `egg_optimize` path.
///
/// Two practical guards (sound subsets — factoring less often, never wrongly):
/// * **All-or-nothing** (mirrors opt.rs:519-521): every compose element must be a
///   chain whose last element is the same `S`. Factoring a *subset* and replacing
///   the whole compose would drop the non-sharing elements — unsound — so any
///   non-chain element, or any disagreement on the last, bails. An empty resulting
///   body-set (every chain was the single element `[S]`) also bails, leaving
///   `compose-dedup` to handle it (otherwise `Chain[Compose[], S]` → `empty`).
/// * **Compose-tail only**: the shared tail `S` must itself be a `Compose`. This
///   targets the wide-pin shape (chains sharing a large pinned subtree) and breaks
///   a mutual explosion with `common-pre-factor`: without it, common-pre and
///   common-post feed each other (each factors the other's output composites) and
///   the e-graph grows without bound. Low-value atom-tail cases (a shared
///   `Prefix`/`Subdir`) are left to `compose-dedup`/`common-pre`.
///
/// The factored form is a `ChainCons` (a chain), so it does not re-present a
/// `Cons` compose to factor — no fixpoint loop — and a re-fire guard
/// short-circuits once the union has landed.

/// Factor the compose-of-chains at `id` in one O(spine) pass. Returns `true` if a
/// new factored term was added and unioned into `id`'s class. Leaves the e-graph
/// untouched (returns `false`) if `id` is not such a compose, or the guards fail.
pub(crate) fn factor_common_post(egraph: &mut EGraph<Josh, JoshAnalysis>, id: Id) -> bool {
    let root = egraph.find(id);
    let Some(elems) = cons_elems(egraph, root) else {
        return false;
    };
    if elems.len() < 2 {
        return false;
    }

    // Split every element into (body, last); bail if any element is not a chain,
    // or if the lasts disagree (all-or-nothing, mirroring opt.rs:519-521).
    let mut shared: Option<Id> = None;
    let mut bodies: Vec<Id> = Vec::with_capacity(elems.len());
    for &e in &elems {
        let Some((body, last)) = chain_body_last(egraph, e) else {
            return false;
        };
        let last = egraph.find(last);
        match shared {
            None => shared = Some(last),
            Some(s) if s != last => return false,
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
        return false;
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
        return false;
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
    // class, so a repeat call (or another spine top merged into it) bails without
    // re-unioning.
    if egraph.find(factored) == root {
        return false;
    }
    egraph.union(root, factored);
    true
}

/// Factor every maximal compose-of-chains in the e-graph, once each.
///
/// A compose `Cons` spine `Cons(h0, Cons(h1, …))` has one "top" (the outermost
/// `Cons`) whose class is never used as the *tail* of any other `Cons`. The inner
/// cells ARE such tails, so they are skipped — factoring only the top of each
/// spine keeps the total work linear (each `Cons` cell is walked once, as part of
/// exactly one top's spine). Returns `true` if any spine was factored.
pub(crate) fn factor_all_common_post(egraph: &mut EGraph<Josh, JoshAnalysis>) -> bool {
    // Snapshot, before any mutation, the classes that hold a `Cons` and the
    // classes that appear as some `Cons`'s tail. A spine top is a `Cons`-class not
    // used as a tail.
    let mut cons_classes: HashSet<Id> = HashSet::new();
    let mut tail_classes: HashSet<Id> = HashSet::new();
    for class in egraph.classes() {
        let cid = egraph.find(class.id);
        let mut has_cons = false;
        for node in &class.nodes {
            if let Josh::Cons([_h, t]) = node {
                has_cons = true;
                tail_classes.insert(egraph.find(*t));
            }
        }
        if has_cons {
            cons_classes.insert(cid);
        }
    }
    let tops: Vec<Id> = cons_classes.difference(&tail_classes).copied().collect();

    let mut changed = false;
    for top in tops {
        if factor_common_post(egraph, top) {
            changed = true;
        }
    }
    changed
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
