use crate::filter::Filter;
use crate::op::{BlobContent, Op};
use crate::persist::to_filter;
use egg::{Analysis, DidMerge, EGraph, Id, Symbol};
use std::collections::HashSet;

/// Field separator for the opaque atom symbols below.
///
/// NUL is invalid inside git tree entry names, so paths never contain it, which
/// keeps the separator unambiguous. Atom symbols are never re-parsed by egg's
/// s-expression machinery (they are only ever built programmatically and fed to
/// the e-graph as opaque tokens), so a literal NUL in a symbol is harmless.
pub(crate) const SEP: char = '\x00';

egg::define_language! {
    /// Mirror of the `Op` variants needed to round-trip the supported rules.
    ///
    /// `Compose` is represented as a cons-list (`Cons`/`Nil`) rather than a
    /// variadic `Box<[Id]>`: egg matches `Box<[Id]>` by exact child arity (the
    /// "variadic wall"), so any rule that removes a variable number of elements
    /// (dedup, set-difference) is impossible as a pure pattern. A cons-list
    /// sidesteps that — a 2-child `Cons` pattern matches a list of any length.
    /// `Compose` is a set (order-independent), so cons order is irrelevant and the
    /// element-set [`JoshAnalysis`] annotation is the canonical membership check.
    ///
    /// `Chain` is also a cons-list (`ChainCons`/`ChainNil`), DISJOINT from
    /// `Compose`'s `Cons`/`Nil`: although `Chain` is *ordered* where `Compose` is
    /// a set, no chain rule reorders elements — flatten is associative,
    /// `common_pre` factors the head and keeps tail order, and cancel/conflict
    /// match positions — so the cons-order non-determinism that would be wrong for
    /// an ordered sequence does not arise. The cons-list form is what lets
    /// head/tail patterns (flatten, `common_pre`, path decompose) match a chain of
    /// any length at a fixed arity, which the variadic `Box<[Id]>` "wall"
    /// forbids. `Subtract`/`Exclude`/`Pin` are structural containers.
    /// `Prefix`/`Subdir` carry their path as a structural child so a pattern
    /// variable can unify two equal paths (see `cancel-prefix-subdir`). `Message`
    /// is structural too, so a pattern can match "any message" regardless of its
    /// format/regex payload (see `subtract-message-message`); the rest of the leaf
    /// data ops are opaque atoms.
    pub(crate) enum Josh {
        // Cons-list Compose: a list is Cons(head, tail) chained down to Nil, so a
        // 2-child pattern matches a list of any length. Compose is a set.
        "cons" = Cons([Id; 2]),
        "nil" = Nil,
        // Ordered sequence container as its OWN cons-list (ChainCons/ChainNil),
        // disjoint from Compose's Cons/Nil: a 2-child pattern matches a chain of
        // any length at a fixed arity (needed for flatten/common_pre/decompose).
        // No chain rule reorders elements, so ordered semantics are preserved.
        "chaincons" = ChainCons([Id; 2]),
        "chainnil" = ChainNil,
        "subtract" = Subtract([Id; 2]),
        "exclude" = Exclude(Id),
        "pin" = Pin(Id),
        // Path representation, DISJOINT from `Compose`'s `Cons`/`Nil`: a path is a
        // cons-list of component `Symbol`s via PathCons/PathNil. Reusing `Cons` is
        // unsound — `compose-dedup` (`(cons ?x ?tail)`) would match a path-Cons and
        // silently delete a repeated path component (e.g. `Subdir("a/b/a")`).
        // Separate constructors keep the two list types invisible to each other's
        // rules. A single component `"a"` is `PathCons(Sym, PathNil)` — always a
        // spine, never a bare `Symbol`, so a pattern variable unifies two equal
        // paths.
        "pathcons" = PathCons([Id; 2]),
        "pathnil" = PathNil,
        // Path-carrying ops. The path is a PathCons/PathNil spine child, so two
        // equal paths share an e-class and unify under one pattern variable, and
        // decomposition (opt E6/E7) / join (E3/E4) are honest in-egg rewrites.
        "prefix" = Prefix(Id),
        "subdir" = Subdir(Id),
        // Message is structural (not an opaque atom) so a pattern can recognize
        // "any message". Its single child `Symbol` carries the NUL-separated
        // format/regex payload, which is never inspected by a pattern.
        "message" = Message(Id),
        // Opaque leaf atoms; the carried data is encoded into the symbol string.
        Symbol(Symbol),
    }
}

/// Per-e-class element-set annotation — the cons-list pivot's enabling mechanism.
///
/// For a cons-list e-class this is the set of canonical element `Id`s it contains
/// (its "membership set"); for `Nil`, atoms, and non-Compose nodes it is empty. It
/// lets a rewrite *guard* on membership without a variadic matcher: dedup is
/// `(cons ?x ?tail) => ?tail` when the tail's set contains `?x`, and absorb is
/// `(subtract ?x ?l) => empty` when `?l`'s set contains `?x`. Both are variadic
/// under `Box<[Id]>` (impossible as patterns); both are 2-arity patterns under cons
/// + this analysis.
///
/// Computed by [`Analysis::make`] — a `Cons` node's set is its tail's set plus the
/// canonical head — and unioned by [`Analysis::merge`] on class merge. egg
/// re-derives the data via `remake` on rebuild, so no `modify` hook is needed to
/// keep it sound.
#[derive(Default)]
pub(crate) struct JoshAnalysis;

impl Analysis<Josh> for JoshAnalysis {
    type Data = HashSet<Id>;

    fn make(egraph: &mut EGraph<Josh, Self>, enode: &Josh, _id: Id) -> Self::Data {
        match enode {
            // `EGraph::Index` canonicalizes on access, so `egraph[*t].data` is
            // already the canonical tail set; `find` is needed only for the head
            // `Id` we store into the set, so membership compares canonical reps.
            Josh::Cons([h, t]) => {
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

/// Walk a cons-list spine from `id`, collecting canonical head `Id`s until `Nil`.
/// Returns `None` if `id`'s e-class holds neither a `Cons` nor a `Nil` node (i.e.
/// it is not a pure cons-list) — the caller then leaves it untouched. Shared by the
/// appliers that inspect a `Compose` ([`crate::eggopt::appliers`]).
///
/// Cycle-safe: `dedup` / `drop-empty` legitimately union a cons-list with its own
/// tail (or with `Nil`), so a class can contain both a `Cons` and the `Nil`/tail it
/// points at. A naive walk would loop forever there, so visited classes break the
/// walk and return the elements seen so far. That partial list is fine for the
/// callers — the appliers self-guard on it, and the equivalence gate checks the
/// final result regardless.
pub(crate) fn cons_elems(egraph: &EGraph<Josh, JoshAnalysis>, start: Id) -> Option<Vec<Id>> {
    let mut out = Vec::new();
    let mut visited = HashSet::<Id>::new();
    let mut id = egraph.find(start);
    loop {
        if !visited.insert(id) {
            break;
        }
        match egraph[id].nodes.iter().find_map(|n| match n {
            Josh::Cons([h, t]) => Some((*h, *t)),
            _ => None,
        }) {
            Some((h, t)) => {
                out.push(egraph.find(h));
                id = egraph.find(t);
            }
            None => {
                if egraph[id].nodes.iter().any(|n| matches!(n, Josh::Nil)) {
                    break;
                }
                return None;
            }
        }
    }
    Some(out)
}

/// Build a cons-list of `elems` (empty -> `Nil`), prepending each canonical head and
/// adding the nodes to the e-graph. The mirror of [`cons_elems`].
pub(crate) fn cons_fold(egraph: &mut EGraph<Josh, JoshAnalysis>, elems: &[Id]) -> Id {
    let mut tail = egraph.add(Josh::Nil);
    for &h in elems.iter().rev() {
        tail = egraph.add(Josh::Cons([egraph.find(h), tail]));
    }
    tail
}

/// Walk a `ChainCons` spine from `id`, collecting canonical head `Id`s until
/// `ChainNil`. The chain analogue of [`cons_elems`]. Returns `None` if `id`'s
/// e-class holds neither a `ChainCons` nor a `ChainNil` (i.e. it is not a pure
/// chain spine) — the caller then leaves it untouched.
///
/// Cycle-safe, for the same reason as [`cons_elems`]: `chain-flatten` can union a
/// chain with its own tail, so a class can hold both a `ChainCons` and the
/// `ChainNil`/tail it points at. The visited-set breaks that loop and returns the
/// elements seen so far, which is fine for the (self-guarding) callers.
pub(crate) fn chain_elems(egraph: &EGraph<Josh, JoshAnalysis>, start: Id) -> Option<Vec<Id>> {
    let mut out = Vec::new();
    let mut visited = HashSet::<Id>::new();
    let mut id = egraph.find(start);
    loop {
        if !visited.insert(id) {
            break;
        }
        match egraph[id].nodes.iter().find_map(|n| match n {
            Josh::ChainCons([h, t]) => Some((*h, *t)),
            _ => None,
        }) {
            Some((h, t)) => {
                out.push(egraph.find(h));
                id = egraph.find(t);
            }
            None => {
                if egraph[id].nodes.iter().any(|n| matches!(n, Josh::ChainNil)) {
                    break;
                }
                return None;
            }
        }
    }
    Some(out)
}

/// Build a `ChainCons` spine of `elems` (empty -> `ChainNil`), prepending each
/// canonical head and adding the nodes to the e-graph. The mirror of
/// [`chain_elems`] (and the chain analogue of [`cons_fold`]).
pub(crate) fn chain_fold(egraph: &mut EGraph<Josh, JoshAnalysis>, elems: &[Id]) -> Id {
    let mut tail = egraph.add(Josh::ChainNil);
    for &h in elems.iter().rev() {
        tail = egraph.add(Josh::ChainCons([egraph.find(h), tail]));
    }
    tail
}

/// Build a canonical `Compose` operand from an element list: empty becomes the
/// `empty` atom (`Op::Empty`, the Compose identity), a singleton becomes the bare
/// element, otherwise a `Cons`/`Nil` spine. Construction-time canonicalization
/// keeps `AstSize` ties out of the e-graph — a bare element costs less than
/// `Cons(x, Nil)`, so without this the extractor could pick the wrapped form and
/// leave a redundant node. Shared by the appliers that build a `Compose`.
pub(crate) fn compose_of(egraph: &mut EGraph<Josh, JoshAnalysis>, elems: &[Id]) -> Id {
    match elems.len() {
        0 => egraph.add(Josh::Symbol(Symbol::from("empty"))),
        1 => egraph.find(elems[0]),
        _ => cons_fold(egraph, elems),
    }
}

/// Encode a leaf `Op` as an opaque atom symbol. Returns `None` for any `Op`
/// variant (or payload) the egg language does not model, which makes `build`
/// bail out and `egg_optimize` fall back to the identity filter.
///
/// `Prefix`/`Subdir`/`Message` are intentionally absent here: they are
/// structural nodes, not atoms, and are handled directly in `build`/`rebuild`.
pub(crate) fn op_to_atom(op: &Op) -> Option<String> {
    Some(match op {
        Op::File(dst, src) => format!("file{SEP}{}{SEP}{}", dst.to_str()?, src.to_str()?),
        Op::Blob(p, BlobContent::Inline(c)) => {
            format!("blob{SEP}{}{SEP}inline{SEP}{}", p.to_str()?, c)
        }
        Op::Blob(p, BlobContent::Oid(o)) => format!("blob{SEP}{}{SEP}oid{SEP}{}", p.to_str()?, o),
        Op::Nop => "nop".to_string(),
        Op::Empty => "empty".to_string(),
        Op::Pattern(p) => format!("pattern{SEP}{p}"),
        _ => return None,
    })
}

/// Decode an atom symbol back into a leaf `Filter`. Returns `None` if the symbol
/// is not a recognized atom (which only happens for a malformed symbol; in
/// practice every symbol in an extracted term was produced by `op_to_atom`).
pub(crate) fn atom_to_filter(s: &str) -> Option<Filter> {
    let (tag, rest) = match s.split_once(SEP) {
        Some((t, r)) => (t, Some(r)),
        None => (s, None),
    };
    let op = match tag {
        "nop" => Op::Nop,
        "empty" => Op::Empty,
        "pattern" => Op::Pattern(rest?.to_string()),
        "file" => {
            let (dst, src) = rest?.split_once(SEP)?;
            Op::File(dst.into(), src.into())
        }
        "blob" => {
            let (path, after) = rest?.split_once(SEP)?;
            let (kind, value) = after.split_once(SEP)?;
            let content = match kind {
                "inline" => BlobContent::Inline(value.to_string()),
                "oid" => BlobContent::Oid(git2::Oid::from_str(value).ok()?),
                _ => return None,
            };
            Op::Blob(path.into(), content)
        }
        _ => return None,
    };
    Some(to_filter(op))
}
