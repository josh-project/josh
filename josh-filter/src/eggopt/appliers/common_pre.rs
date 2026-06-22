use crate::eggopt::lang::{Josh, JoshAnalysis, chain_elems, chain_fold, compose_of, cons_elems};
use egg::{EGraph, Id};
use std::collections::HashSet;

/// `Compose[Chain[S, b1], Chain[S, b2], …]` (every element a chain sharing the
/// same first element `S`) → `Chain[S, Compose[b1, b2, …]]`.
///
/// This is opt's `common_pre` factoring (opt.rs:481-507, applied at opt.rs:647-648
/// as `Chain[common, Compose(rest)]`). It is the head analogue of `common_post`'s
/// tail factoring, and like that one it CANNOT be a
/// pairwise pattern rule: `common-pre-factor`'s intermediate result is *larger*
/// than the input (each remaining un-factored chain still carries the shared
/// element `S`), so `AstSize` rejects the partial factoring and run-to-fixpoint
/// never reaches the fully-factored form. Concretely this leaves ordinary
/// multi-entry namespace specs (`:[x=:/a/:/b/,y=:/a/:/c/,z=:/a/:/d/]`) un-reduced —
/// see `corpus_gaps::common_prefix_factor_3way`.
///
/// So `common_pre` is a **targeted pass**, the mirror of `factor_all_common_post`:
/// it factors all N shared-head chains in one O(N) walk, applied between cheap
/// saturation rounds alongside the common_post pass (see `egg_candidate`).
///
/// Soundness is unconditional — left-distribution of sequence (chain) over
/// parallel-union (compose): `(S;b1) ∪ … ∪ (S;bn) = S;(b1 ∪ … ∪ bn)`, valid for
/// any `S`. (opt's `common_pre` has no element-type guard either, unlike
/// `common_post`.) The equivalence gate is the safety net for `egg_optimize`.
///
/// Guards (sound subsets):
/// * **All-or-nothing** (mirrors opt.rs:494-498): every compose element must be a
///   chain whose *first* element is the same `S`. Any non-chain element or any
///   disagreement bails. An empty resulting tail-set (every chain was the single
///   element `[S]`) also bails, leaving `compose-dedup` to handle it.
/// * **No shared-element-type guard** — unlike `common_post`'s Compose-tail guard.
///   That guard exists to break a *mutual* common-pre⇄common-post explosion
///   (common_post factoring atom-tails whose output common_pre then re-factors).
///   With common_post already refusing atom-tails, the cycle is broken on its
///   side, so common_pre needs no symmetric head guard — and could not have one,
///   since its target shared head is a path op (`Subdir`/`Prefix`), not a Compose.
///   The two passes are also disjoint on the stress shapes: wide-pin chains share
///   a *tail* (common_pre's head-agreement check bails), namespaces share a *head*
///   (common_post's tail-agreement check bails).
///
/// The factored form is a `ChainCons`, so it does not re-present a `Cons` compose
/// for re-factoring, and a re-fire guard short-circuits once unioned.

/// Factor the compose-of-chains at `id` in one O(spine) pass. Returns `true` if a
/// new factored term was added and unioned into `id`'s class. Leaves the e-graph
/// untouched (returns `false`) if `id` is not such a compose, or the guards fail.
pub(crate) fn factor_common_pre(egraph: &mut EGraph<Josh, JoshAnalysis>, id: Id) -> bool {
    let root = egraph.find(id);
    let Some(elems) = cons_elems(egraph, root) else {
        return false;
    };
    if elems.len() < 2 {
        return false;
    }

    // Split every element into (head, tail); bail if any element is not a chain,
    // or if the heads disagree (all-or-nothing, mirroring opt.rs:494-498).
    let mut shared: Option<Id> = None;
    let mut tails: Vec<Id> = Vec::with_capacity(elems.len());
    for &e in &elems {
        let Some((head, tail)) = chain_head_tail(egraph, e) else {
            return false;
        };
        let head = egraph.find(head);
        match shared {
            None => shared = Some(head),
            Some(s) if s != head => return false,
            _ => {}
        }
        // Canonicalize the tail (chain minus its head): drop empty (a `[S]`
        // singleton), collapse a singleton to the bare element, else fold a chain
        // spine. Same canonicalization rationale as `compose_of`.
        if let Some(t) = chain_of(egraph, &tail) {
            tails.push(t);
        }
    }
    // Every chain was the single element [S] — let compose-dedup handle it.
    if tails.is_empty() {
        return false;
    }
    let shared = shared.expect(">=2 elements => >=1 chain => shared set");

    // Chain[S, Compose[tails]] as two nested ChainCons ending in ChainNil: the
    // shared head FIRST, the inner Compose as the tail element. A bare element or
    // a Cons in the chain-tail slot is the malformed-node class; the Compose sits
    // as a chain element (tail of the outer ChainCons), mirroring common_post's
    // shape from the other end.
    let inner = compose_of(egraph, &tails);
    let nil = egraph.add(Josh::ChainNil);
    let tail = egraph.add(Josh::ChainCons([egraph.find(inner), nil]));
    let factored = egraph.add(Josh::ChainCons([shared, egraph.find(tail)]));

    // Re-fire guard: once unioned, the hash-consed factored node lands in root's
    // class, so a repeat call (or another spine top merged into it) bails without
    // re-unioning.
    if egraph.find(factored) == root {
        return false;
    }
    egraph.union(root, factored);
    true
}

/// Factor every maximal compose-of-chains in the e-graph, once each — the head
/// analogue of `factor_all_common_post`. A compose `Cons`
/// spine has one "top" (the outermost `Cons`) whose class is never used as the
/// tail of any other `Cons`; factoring only the tops keeps the total work linear.
/// Returns `true` if any spine was factored.
pub(crate) fn factor_all_common_pre(egraph: &mut EGraph<Josh, JoshAnalysis>) -> bool {
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
        if factor_common_pre(egraph, top) {
            changed = true;
        }
    }
    changed
}

/// Split a `ChainCons` spine at `id` into its first element (head) and the rest
/// (tail). Returns `None` if `id` is not a pure chain spine or is empty. Pure (no
/// egraph mutation); the caller canonicalizes the tail. The head analogue of
/// `common_post`'s `chain_body_last`.
fn chain_head_tail(egraph: &EGraph<Josh, JoshAnalysis>, id: Id) -> Option<(Id, Vec<Id>)> {
    let chain = chain_elems(egraph, id)?;
    if chain.is_empty() {
        return None;
    }
    let head = *chain.first().expect("non-empty chain");
    let tail = chain[1..].to_vec();
    Some((head, tail))
}

/// Build a canonical chain from an element list: empty -> `None` (caller drops
/// it), singleton -> the bare element, else a `ChainCons` spine. Mirrors
/// `compose_of`'s canonicalization so `AstSize` has no bare-element-vs-
/// `ChainCons(x, ChainNil)` tie to mis-pick.
fn chain_of(egraph: &mut EGraph<Josh, JoshAnalysis>, elems: &[Id]) -> Option<Id> {
    match elems.len() {
        0 => None,
        1 => Some(egraph.find(elems[0])),
        _ => Some(chain_fold(egraph, elems)),
    }
}
