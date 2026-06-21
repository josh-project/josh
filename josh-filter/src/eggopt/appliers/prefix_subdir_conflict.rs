use crate::eggopt::lang::{Josh, JoshAnalysis};
use egg::{Applier, EGraph, Id, PatternAst, Subst, Symbol, Var};
use std::path::Path;

/// `Chain[Prefix(a), Subdir(b)]` where `a` and `b` are *different* paths of the
/// *same* component count → `Empty`.
///
/// Mirrors the trusted optimizer's conflict case (`opt.rs` step): after
/// `Prefix(a)` re-roots the tree at `a`, a same-depth-but-different `Subdir(b)`
/// selects a subtree that cannot exist there, so the whole chain is the empty
/// tree. The same-path case (`a == b`) is the pure-pattern `cancel-prefix-subdir`
/// rule (→ Nop); this applier covers the complementary conflict case.
///
/// It cannot be a pure pattern: the guard needs both a *disequality* (`a != b`)
/// and a *component-count* comparison (same depth), and egg patterns can neither
/// inspect a `Symbol`'s bytes nor express disequality. Like the cancel rule, it
/// matches only the exact two-element chain; a conflicting pair inside a longer
/// chain is a follow-up.
///
/// Soundness does not rest on matching `opt`'s path-counting exactly: the
/// equivalence gate re-canonicalizes through `opt`, so an over-eager fire is
/// simply rejected and the input is returned unchanged.
pub(crate) struct PrefixSubdirConflict {
    a: Var,
    b: Var,
}

impl PrefixSubdirConflict {
    pub(crate) fn new() -> Self {
        Self {
            a: "?a".parse().expect("var ?a"),
            b: "?b".parse().expect("var ?b"),
        }
    }
}

impl Applier<Josh, JoshAnalysis> for PrefixSubdirConflict {
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
        let a = egraph.find(*subst.get(self.a).expect("bound ?a"));
        let b = egraph.find(*subst.get(self.b).expect("bound ?b"));

        // Same path is the cancel case (-> Nop), not a conflict: unioning Nop
        // and Empty into one e-class would be unsound. E-class identity mirrors
        // Filter-OID equality (build hash-conses by OID), so this is exactly
        // `opt`'s `a == b` test.
        if a == b {
            return vec![];
        }

        let Some(pa) = symbol_str(egraph, a) else {
            return vec![];
        };
        let Some(pb) = symbol_str(egraph, b) else {
            return vec![];
        };

        // Same depth, different paths -> the empty tree (opt's conflict case).
        // Component counts use the same `Path::components` as `opt`, so the two
        // agree and the equivalence gate accepts the result.
        if Path::new(pa).components().count() != Path::new(pb).components().count() {
            return vec![];
        }

        let empty = egraph.add(Josh::Symbol(Symbol::from("empty")));
        egraph.union(eclass, empty);
        vec![egraph.find(eclass)]
    }
}

/// The string of the first `Symbol` node in `id`'s e-class, if it contains one.
///
/// Returns a `&'static str` because egg interns `Symbol`s in a global table, so
/// the slice outlives the `&EGraph` borrow and never conflicts with a later
/// `&mut EGraph` call (e.g. `egraph.add`).
fn symbol_str(egraph: &EGraph<Josh, JoshAnalysis>, id: Id) -> Option<&'static str> {
    egraph[id].nodes.iter().find_map(|node| match node {
        Josh::Symbol(s) => Some(s.as_str()),
        _ => None,
    })
}
