//! Structural-paths de-risking spike.
//!
//! Isolated experiment — NOT wired into [`super::egg_optimize`], the CLI flag, or
//! [`crate::opt`]. It de-risks *promoting `Subdir`/`Prefix` paths to structural
//! cons-list-of-components* before that change touches the main
//! [`super::lang::Josh`] language — the same role [`spike_conslist`] played for the
//! cons-list `Compose` pivot.
//!
//! Today `Prefix`/`Subdir` carry their whole path as a single opaque [`Symbol`]
//! child, so egg cannot split a path on its `/` boundary — path decomposition
//! (opt.rs E6/E7) and path-join (E3/E4) are impossible as rewrites. The reverted
//! experiment decomposed paths *at the `build()` boundary* (outside the e-graph),
//! which broke the Prefix/Subdir conflict rules and bloated the graph. Representing
//! a path as a `PathCons`/`PathNil` cons-list of component [`Symbol`]s lets
//! decomposition and join be *honest, non-destructive in-egg rewrites* (both forms
//! coexist; `AstSize` picks), fixing the root cause.
//!
//! ## Why `PathCons`/`PathNil`, not the existing `Cons`/`Nil`
//!
//! `Cons`/`Nil` are `Compose`'s representation, and `compose-dedup`
//! (`(cons ?x ?tail)`) matches *any* `Cons`. A path with a repeated component like
//! `Subdir("a/b/a")` would have its outer path-`Cons` matched and the first `a`
//! silently deleted. Paths get their OWN constructors so the two list types are
//! invisible to each other's rules.
//!
//! ## The four de-risking questions (one test each, in `tests/eggopt_paths.rs`)
//!
//! 1. **Mechanism** — do `PathCons` paths and the decompose/merge rewrites fire?
//! 2. **Decompose ⇄ whole coexist, `AstSize` non-destructive** — decomposing
//!    `Subdir("a/b")` (≈6 nodes) into `Chain[Subdir(a),Subdir(b)]` (≈9) is a
//!    node-count *loss*, so `AstSize` extracts the whole path back. This is the
//!    same distribute⇄factor tension that caused the rollback, made concrete: both
//!    forms coexist, extraction does NOT commit to the decomposed form.
//! 3. **Does `common_pre` factoring win under `AstSize`?** — the open question.
//!    With a shared leading component the factored form shares that node once, so
//!    `AstSize` *should* pick factored. If it does not, structural paths alone do
//!    not reclaim factoring (a cost tweak / directional rules would be needed) —
//!    the actionable finding.
//! 4. **cancel/conflict over structural paths** — cancel is a pure pattern
//!    (`?p == ?p`); conflict is an applier comparing `PathCons` depth (cleaner than
//!    today's `Path::components()` on a symbol string).
//!
//! A known interaction NOT covered here (a promotion-time concern): the decompose
//! rules fire on a `Subdir`/`Prefix` *anywhere*, including nested in a chain, so a
//! multi-component path adjacent to its cancel/conflict partner decomposes first
//! (mirroring `opt.rs` step, which recurses `step()` into each chain element before
//! the cancel/conflict pass). That richer interaction is left for the promotion.
//!
//! ## No custom `Analysis`
//!
//! Unlike [`spike_conslist`], no rule needs an element-set condition, so the
//! e-graph uses `()` analysis — leaner, and free of the cyclic-annotation gotcha.
//! Every rule is a pure pattern or a finite-spine-walking applier.

use egg::{
    Applier, AstSize, EGraph, Extractor, Id, Pattern, PatternAst, RecExpr, Rewrite, Runner,
    Searcher, Subst, Symbol, Var, rewrite,
};
use std::collections::HashSet;

egg::define_language! {
    /// Self-contained language for the structural-paths spike. A path is a cons-list
    /// of component [`Symbol`]s via `PathCons`/`PathNil` — DISJOINT from `Compose`'s
    /// `Cons`/`Nil` (see the module docs for why sharing them is unsound). A single
    /// component `"a"` is `PathCons(Sym, PathNil)` — always a spine, never a bare
    /// `Symbol`, so a pattern variable unifies two equal paths.
    pub enum PathJosh {
        "pathcons" = PathCons([Id; 2]),
        "pathnil" = PathNil,
        "subdir" = Subdir(Id),
        "prefix" = Prefix(Id),
        "chain2" = Chain2([Id; 2]),
        "path-append" = PathAppend([Id; 2]),
        "cons" = Cons([Id; 2]),
        "nil" = Nil,
        Symbol(Symbol),
    }
}

/// The spike's rewrites. See the module docs for the four de-risking questions.
pub fn path_rules() -> Vec<Rewrite<PathJosh, ()>> {
    vec![
        // E5 — an empty path (PathNil) is identity.
        rewrite!("subdir-empty"; "(subdir pathnil)" => "nop"),
        rewrite!("prefix-empty"; "(prefix pathnil)" => "nop"),
        // E6 Subdir decompose — FORWARD (opt.rs step has no .rev()). The two-deep
        // LHS guard (pathcons ?h (pathcons ?h2 ?t)) matches only ≥2-component paths,
        // so a single component (pathcons h pathnil) is the base case and does not
        // oscillate against subdir-empty.
        rewrite!("subdir-decompose";
            "(subdir (pathcons ?h (pathcons ?h2 ?t)))"
            => "(chain2 (subdir (pathcons ?h pathnil)) (subdir (pathcons ?h2 ?t)))"),
        // E7 Prefix decompose — REVERSED (opt.rs step does .rev()): the LAST
        // component becomes the FIRST chain element. Asymmetric with subdir-decompose.
        rewrite!("prefix-decompose";
            "(prefix (pathcons ?h (pathcons ?h2 ?t)))"
            => "(chain2 (prefix (pathcons ?h2 ?t)) (prefix (pathcons ?h pathnil)))"),
        // Chain flatten (both brackets, node-neutral) so adjacent pairs meet at any
        // nesting; nop-elim so cancel/nop results don't inflate cost.
        rewrite!("chain2-flatten-l"; "(chain2 (chain2 ?a ?b) ?c)" => "(chain2 ?a (chain2 ?b ?c))"),
        rewrite!("chain2-flatten-r"; "(chain2 ?a (chain2 ?b ?c))" => "(chain2 (chain2 ?a ?b) ?c)"),
        rewrite!("chain2-nop-l"; "(chain2 nop ?x)" => "?x"),
        rewrite!("chain2-nop-r"; "(chain2 ?x nop)" => "?x"),
        // E4/E3 path-join. path-append is append-via-cons: peels arg-1 to fixpoint
        // (terminating — paths never self-reference, so no cyclic-class risk) and
        // leaves a flat PathCons spine. prefix-merge swaps the join order to mirror
        // opt.rs's `y.join(x)`.
        rewrite!("subdir-merge";
            "(chain2 (subdir ?p1) (subdir ?p2))" => "(subdir (path-append ?p1 ?p2))"),
        rewrite!("prefix-merge";
            "(chain2 (prefix ?p1) (prefix ?p2))" => "(prefix (path-append ?p2 ?p1))"),
        rewrite!("path-append-cons";
            "(path-append (pathcons ?h ?t) ?p2)" => "(pathcons ?h (path-append ?t ?p2))"),
        rewrite!("path-append-nil";
            "(path-append pathnil ?p2)" => "?p2"),
        // common_pre factoring — a pure pattern. ?shared unifies by e-class identity
        // across both Chain heads = opt.rs's "first chain element equal" test.
        rewrite!("common-pre-factor";
            "(cons (chain2 ?shared ?rest1) (chain2 ?shared ?rest2))"
            => "(chain2 ?shared (cons ?rest1 ?rest2))"),
        // cancel — pure pattern (same path e-class unifies ?p).
        rewrite!("cancel-prefix-subdir"; "(chain2 (prefix ?p) (subdir ?p))" => "nop"),
        // conflict — custom Applier: same PathCons depth, different path -> empty.
        rewrite!("prefix-subdir-conflict";
            "(chain2 (prefix ?pa) (subdir ?pb))" => { PathConflict::new() }),
    ]
}

/// Saturate `expr` under [`path_rules`]. Mirrors the main optimizer's Runner config.
fn saturate(expr: &RecExpr<PathJosh>) -> Runner<PathJosh, ()> {
    Runner::<PathJosh, ()>::default()
        .with_expr(expr)
        .with_node_limit(10_000)
        .with_iter_limit(30)
        .run(&path_rules())
}

/// Run the spike: saturate `expr` and extract the cheapest (`AstSize`) form.
pub fn run_spike(expr: &RecExpr<PathJosh>) -> RecExpr<PathJosh> {
    let runner = saturate(expr);
    let (_cost, best) = Extractor::new(&runner.egraph, AstSize).find_best(runner.roots[0]);
    best
}

/// Whether `pattern` matches anywhere in the saturated e-graph. Used to confirm a
/// cost-loss rule (decompose) *fired* even when `AstSize` extracts the other form.
pub fn spike_reachable(expr: &RecExpr<PathJosh>, pattern: &str) -> bool {
    let runner = saturate(expr);
    let pat: Pattern<PathJosh> = pattern.parse().expect("parsed spike pattern");
    !pat.search(&runner.egraph).is_empty()
}

/// `Chain2[Prefix(pa), Subdir(pb)]` where `pa`, `pb` are *different* paths of the
/// *same* `PathCons` depth → `empty`. Mirrors opt.rs's conflict (step ~line 698):
/// after `Prefix` re-roots, a same-depth different `Subdir` selects a subtree that
/// cannot exist. Depth is the `PathCons` spine length — structural, no string
/// parsing (cleaner than the main language's `Path::components()` applier). The
/// same-path case is the pure-pattern `cancel-prefix-subdir` rule (→ nop).
pub struct PathConflict {
    pa: Var,
    pb: Var,
}

impl PathConflict {
    pub fn new() -> Self {
        Self {
            pa: "?pa".parse().expect("var ?pa"),
            pb: "?pb".parse().expect("var ?pb"),
        }
    }
}

impl Applier<PathJosh, ()> for PathConflict {
    fn vars(&self) -> Vec<Var> {
        vec![self.pa, self.pb]
    }

    fn apply_one(
        &self,
        egraph: &mut EGraph<PathJosh, ()>,
        eclass: Id,
        subst: &Subst,
        _searcher_ast: Option<&PatternAst<PathJosh>>,
        _rule_name: Symbol,
    ) -> Vec<Id> {
        let pa = egraph.find(*subst.get(self.pa).expect("bound ?pa"));
        let pb = egraph.find(*subst.get(self.pb).expect("bound ?pb"));
        // Same path is the cancel case (-> nop); unioning nop and empty would be
        // unsound. E-class identity mirrors structural path equality.
        if pa == pb {
            return vec![];
        }
        let (Some(da), Some(db)) = (path_depth(egraph, pa), path_depth(egraph, pb)) else {
            return vec![];
        };
        if da != db {
            return vec![];
        }
        let empty = egraph.add(PathJosh::Symbol(Symbol::from("empty")));
        egraph.union(eclass, empty);
        vec![egraph.find(eclass)]
    }
}

/// Length of the `PathCons` spine at `id`'s e-class (0 for `PathNil`), or `None` if
/// the class is not a pure path spine. Paths are finite and never self-referential
/// (no rewrite unions a path with its own tail), so the walk cannot cycle; the
/// visited-set is defensive only.
fn path_depth(egraph: &EGraph<PathJosh, ()>, start: Id) -> Option<usize> {
    let mut depth = 0;
    let mut visited = HashSet::new();
    let mut id = egraph.find(start);
    loop {
        if !visited.insert(id) {
            return None;
        }
        match egraph[id].nodes.iter().find_map(|n| match n {
            PathJosh::PathNil => Some(None),
            PathJosh::PathCons([_, t]) => Some(Some(*t)),
            _ => None,
        }) {
            Some(None) => return Some(depth),
            Some(Some(t)) => {
                depth += 1;
                id = egraph.find(t);
            }
            None => return None,
        }
    }
}
