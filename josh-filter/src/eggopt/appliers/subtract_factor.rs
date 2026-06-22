use crate::eggopt::lang::{Josh, JoshAnalysis, chain_elems, chain_fold};
use egg::{EGraph, Id};

/// Factor a shared element out of a `Subtract`'s two operands — the subtract
/// analogue of the `common_pre` (shared head) and `common_post` (shared tail)
/// compose passes:
///
///   Subtract(S;A, S;B) → S;Subtract(A,B)        [shared head, any S]
///   Subtract(A;P, B;P) → Subtract(A,B);P         [shared Prefix tail P]
///
/// Mirrors opt.rs:733-749 (shared-head) and opt.rs:761-764 (shared-tail, gated
/// on Prefix/self-inverse/Message). Both are sound by sequence-distributing-over-
/// subtract: the shared element is applied identically to both operands, so it
/// factors out. A shared Prefix tail factors like the compose `common_post`
/// (Prefix is a path bijection — the inverse of Subdir), so the difference taken
/// inside the shared namespace is preserved. The shared head needs no guard (opt
/// applies it unconditionally): even a non-injective head such as Subdir is
/// applied identically to both operands, so the difference is preserved.
///
/// This is deliberately NOT opt's one-sided prefix-hoist (opt.rs:750-759). That
/// rule strips a trailing Prefix from a *single* operand rather than a shared one,
/// e.g. reducing `Subtract(Prefix(p), Prefix(q))` with `p != q` to `empty` — which
/// is tree-unsound (two disjoint relocations' difference is the first relocation,
/// not empty). It is not a shared-element factoring, so egg does not replicate it;
/// the `::a/`-vs-`::b/` and compounded-namespace cases in `corpus_subtract` that
/// depend on it stay un-reduced (egg falls back to the input, which is correct).
///
/// `Subtract` is binary, so — unlike the N-ary compose passes — there is no
/// pairwise explosion: each factoring strictly shrinks the subtract (one fewer
/// shared element) and converges. Applied as a targeted pass between saturation
/// rounds alongside `factor_all_common_pre` / `factor_all_common_post`.

/// Try to factor the `Subtract` at `id`. Returns `true` if a new factored term was
/// added and unioned into `id`'s class. Leaves the e-graph untouched (`false`) if
/// `id` is not a `Subtract`, its operands share neither a head nor a Prefix tail,
/// or the shared element is the whole operand (left to `subtract-self`).
pub(crate) fn factor_subtract(egraph: &mut EGraph<Josh, JoshAnalysis>, id: Id) -> bool {
    let root = egraph.find(id);
    let Some((a, b)) = subtract_operands(egraph, root) else {
        return false;
    };

    // Shared head first, then shared Prefix tail. Whichever fires shrinks the
    // subtract; the new inner subtract is re-presented on the next round for any
    // further factoring. Each is an independent sound subset.
    if let Some(f) = factor_shared_head(egraph, a, b) {
        if egraph.find(f) != root {
            egraph.union(root, f);
            return true;
        }
    }
    if let Some(f) = factor_shared_tail(egraph, a, b) {
        if egraph.find(f) != root {
            egraph.union(root, f);
            return true;
        }
    }
    false
}

/// Factor every `Subtract` in the e-graph once — collect all classes holding a
/// `Subtract` enode and try to factor each. Returns `true` if any was factored.
/// New inner subtracts created by factoring are picked up on the next round.
pub(crate) fn factor_all_subtract(egraph: &mut EGraph<Josh, JoshAnalysis>) -> bool {
    let subtracts: Vec<Id> = egraph
        .classes()
        .filter(|c| c.nodes.iter().any(|n| matches!(n, Josh::Subtract(_))))
        .map(|c| egraph.find(c.id))
        .collect();
    let mut changed = false;
    for id in subtracts {
        if factor_subtract(egraph, id) {
            changed = true;
        }
    }
    changed
}

/// The two operands of a `Subtract` enode in `id`'s class, canonicalized. `None`
/// if the class holds no `Subtract`.
fn subtract_operands(egraph: &EGraph<Josh, JoshAnalysis>, id: Id) -> Option<(Id, Id)> {
    for node in &egraph[id].nodes {
        if let Josh::Subtract([a, b]) = node {
            return Some((egraph.find(*a), egraph.find(*b)));
        }
    }
    None
}

/// `Subtract(Chain[S, A…], Chain[S, B…]) → Chain[S, Subtract(A…, B…)]` when both
/// operands are chains sharing a first element `S`, and both have a non-empty tail
/// (a `Subtract(S,S)` singleton pair is left to `subtract-self`). Returns the
/// factored class id, or `None` if the guards fail.
fn factor_shared_head(egraph: &mut EGraph<Josh, JoshAnalysis>, a: Id, b: Id) -> Option<Id> {
    let ca = chain_elems(egraph, a)?;
    let cb = chain_elems(egraph, b)?;
    if ca.is_empty() || cb.is_empty() {
        return None;
    }
    let head = egraph.find(ca[0]);
    if head != egraph.find(cb[0]) {
        return None;
    }
    let tail_a = &ca[1..];
    let tail_b = &cb[1..];
    // Both tails non-empty — `Subtract([S],[S])` is left to subtract-self.
    if tail_a.is_empty() || tail_b.is_empty() {
        return None;
    }
    let inner_a = chain_of(egraph, tail_a)?;
    let inner_b = chain_of(egraph, tail_b)?;
    let inner_sub = egraph.add(Josh::Subtract([inner_a, inner_b]));
    Some(chain2(egraph, head, inner_sub))
}

/// `Subtract(Chain[A…, P], Chain[B…, P]) → Chain[Subtract(A…, B…), P]` when both
/// operands are chains sharing a last element `P` that is a `Prefix` (the soundness
/// guard — a path bijection), and both have a non-empty body (else left to
/// `subtract-self`). Returns the factored class id, or `None` if the guards fail.
fn factor_shared_tail(egraph: &mut EGraph<Josh, JoshAnalysis>, a: Id, b: Id) -> Option<Id> {
    let ca = chain_elems(egraph, a)?;
    let cb = chain_elems(egraph, b)?;
    if ca.is_empty() || cb.is_empty() {
        return None;
    }
    let last = egraph.find(*ca.last().expect("non-empty"));
    if last != egraph.find(*cb.last().expect("non-empty")) {
        return None;
    }
    // The shared tail must be a Prefix (a path bijection) — the soundness guard.
    if !egraph[last]
        .nodes
        .iter()
        .any(|n| matches!(n, Josh::Prefix(_)))
    {
        return None;
    }
    let body_a = &ca[..ca.len() - 1];
    let body_b = &cb[..cb.len() - 1];
    // Both bodies non-empty — `Subtract([P],[P])` is left to subtract-self.
    if body_a.is_empty() || body_b.is_empty() {
        return None;
    }
    let inner_a = chain_of(egraph, body_a)?;
    let inner_b = chain_of(egraph, body_b)?;
    let inner_sub = egraph.add(Josh::Subtract([inner_a, inner_b]));
    Some(chain2(egraph, inner_sub, last))
}

/// Build a 2-element chain `Chain[x, y]` as two nested `ChainCons` ending in
/// `ChainNil`. A bare element or a `Cons` in the chain-tail slot is the
/// malformed-node class that made `rebuild` bail in earlier passes, so both
/// elements sit as proper chain cells.
fn chain2(egraph: &mut EGraph<Josh, JoshAnalysis>, x: Id, y: Id) -> Id {
    let nil = egraph.add(Josh::ChainNil);
    let tail = egraph.add(Josh::ChainCons([egraph.find(y), nil]));
    egraph.add(Josh::ChainCons([egraph.find(x), egraph.find(tail)]))
}

/// Build a canonical chain from a non-empty element list: singleton -> the bare
/// element, else a `ChainCons` spine. Mirrors the `chain_of` helpers in the
/// sibling compose passes (so `AstSize` has no bare-vs-spine tie to mis-pick).
fn chain_of(egraph: &mut EGraph<Josh, JoshAnalysis>, elems: &[Id]) -> Option<Id> {
    match elems.len() {
        0 => None,
        1 => Some(egraph.find(elems[0])),
        _ => Some(chain_fold(egraph, elems)),
    }
}
