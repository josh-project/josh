//! Experimental egg-based filter optimizer (POC).
//!
//! A wiring/correctness proof that the [`egg`] e-graph crate can drive a real
//! josh filter optimization over the existing [`Filter`] representation
//! end-to-end. The bar is semantic: [`egg_optimize`] may only ever return a
//! filter that produces an equivalent tree and history to its input. [`opt`] is
//! used as a semantic *reference* (and as the equivalence oracle), not a
//! mechanical spec — where egg expresses an optimization more cleanly than
//! `opt`'s ordered passes, the cleaner form wins. Gated behind `--use-new-opt`.
//!
//! Three rewrite families: the Chain/Compose distribute <-> factor pair; a
//! Prefix/Subdir cancellation (paths are structural children, so path equality
//! is egg's unification rather than a Rust condition); and a Subtract algebra —
//! identity rules as pure patterns plus a bidirectional Compose set-difference
//! via a custom applier (the one variadic rewrite).

use crate::filter::Filter;
use crate::op::{BlobContent, Op};
use crate::opt;
use crate::persist::{to_filter, to_op};
use egg::{
    Applier, AstSize, EGraph, Extractor, Id, PatternAst, RecExpr, Rewrite, Runner, Subst, Symbol,
    Var, rewrite,
};
use std::collections::{HashMap, HashSet};

/// Field separator for the opaque atom symbols below.
///
/// NUL is invalid inside git tree entry names, so paths never contain it, which
/// keeps the separator unambiguous. Atom symbols are never re-parsed by egg's
/// s-expression machinery (they are only ever built programmatically and fed to
/// the e-graph as opaque tokens), so a literal NUL in a symbol is harmless.
const SEP: char = '\x00';

egg::define_language! {
    /// Mirror of the `Op` variants needed to round-trip the supported rules.
    ///
    /// `Compose`/`Chain`/`Subtract`/`Exclude`/`Pin` are structural containers.
    /// `Prefix`/`Subdir` carry their path as a structural child so a pattern
    /// variable can unify two equal paths (see `cancel-prefix-subdir`); the rest
    /// of the leaf data ops are opaque atoms.
    enum Josh {
        // Variadic containers. Rewrite patterns match these by exact child count,
        // so a 2-child pattern only matches a 2-child node (see egg's
        // `define_language!` docs on `Box<[Id]>`).
        "compose" = Compose(Box<[Id]>),
        "chain" = Chain(Box<[Id]>),
        "subtract" = Subtract([Id; 2]),
        "exclude" = Exclude(Id),
        "pin" = Pin(Id),
        // Path-carrying ops. The path is a child `Symbol`, so two equal paths
        // share an e-class and unify under one pattern variable.
        "prefix" = Prefix(Id),
        "subdir" = Subdir(Id),
        // Opaque leaf atoms; the carried data is encoded into the symbol string.
        Symbol(Symbol),
    }
}

/// Encode a leaf `Op` as an opaque atom symbol. Returns `None` for any `Op`
/// variant (or payload) the egg language does not model, which makes `build`
/// bail out and `egg_optimize` fall back to the identity filter.
///
/// `Prefix`/`Subdir` are intentionally absent here: they are structural nodes,
/// not atoms, and are handled directly in `build`/`rebuild`.
fn op_to_atom(op: &Op) -> Option<String> {
    Some(match op {
        Op::File(dst, src) => format!("file{SEP}{}{SEP}{}", dst.to_str()?, src.to_str()?),
        Op::Blob(p, BlobContent::Inline(c)) => {
            format!("blob{SEP}{}{SEP}inline{SEP}{}", p.to_str()?, c)
        }
        Op::Blob(p, BlobContent::Oid(o)) => format!("blob{SEP}{}{SEP}oid{SEP}{}", p.to_str()?, o),
        Op::Nop => "nop".to_string(),
        Op::Empty => "empty".to_string(),
        Op::Pattern(p) => format!("pattern{SEP}{p}"),
        Op::Message(fmt, re) => format!("message{SEP}{fmt}{SEP}{}", re.as_str()),
        _ => return None,
    })
}

/// Decode an atom symbol back into a leaf `Filter`. Returns `None` if the symbol
/// is not a recognized atom (which only happens for a malformed symbol; in
/// practice every symbol in an extracted term was produced by `op_to_atom`).
fn atom_to_filter(s: &str) -> Option<Filter> {
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
        "message" => {
            let (fmt, regex_str) = rest?.split_once(SEP)?;
            Op::Message(fmt.to_string(), regex::Regex::new(regex_str).ok()?)
        }
        _ => return None,
    };
    Some(to_filter(op))
}

/// Convert a `Filter` into an egg `RecExpr`, memoizing by `Filter` OID so shared
/// subterms share an e-node. Returns `None` if any subterm is unrepresentable.
fn build(expr: &mut RecExpr<Josh>, seen: &mut HashMap<Filter, Id>, f: Filter) -> Option<Id> {
    if let Some(&id) = seen.get(&f) {
        return Some(id);
    }
    let id = match to_op(f) {
        Op::Compose(cs) => {
            let kids = cs
                .iter()
                .map(|&c| build(expr, seen, c))
                .collect::<Option<Vec<_>>>()?;
            expr.add(Josh::Compose(kids.into_boxed_slice()))
        }
        Op::Chain(cs) => {
            let kids = cs
                .iter()
                .map(|&c| build(expr, seen, c))
                .collect::<Option<Vec<_>>>()?;
            expr.add(Josh::Chain(kids.into_boxed_slice()))
        }
        Op::Subtract(a, b) => {
            let a = build(expr, seen, a)?;
            let b = build(expr, seen, b)?;
            expr.add(Josh::Subtract([a, b]))
        }
        Op::Exclude(b) => {
            let b = build(expr, seen, b)?;
            expr.add(Josh::Exclude(b))
        }
        Op::Pin(b) => {
            let b = build(expr, seen, b)?;
            expr.add(Josh::Pin(b))
        }
        Op::Prefix(path) => {
            let p = expr.add(Josh::Symbol(Symbol::from(path.to_str()?)));
            expr.add(Josh::Prefix(p))
        }
        Op::Subdir(path) => {
            let p = expr.add(Josh::Symbol(Symbol::from(path.to_str()?)));
            expr.add(Josh::Subdir(p))
        }
        other => {
            let atom = op_to_atom(&other)?;
            expr.add(Josh::Symbol(Symbol::from(atom)))
        }
    };
    seen.insert(f, id);
    Some(id)
}

/// Convert an extracted egg `RecExpr` back into a `Filter`, memoizing by e-node
/// `Id` since a `RecExpr` is a DAG. Returns `None` only if a symbol fails to
/// decode (should not happen for symbols this module produced).
fn rebuild(expr: &RecExpr<Josh>, seen: &mut HashMap<Id, Filter>, id: Id) -> Option<Filter> {
    if let Some(&f) = seen.get(&id) {
        return Some(f);
    }
    let f = match &expr[id] {
        Josh::Compose(kids) => {
            let v = kids
                .iter()
                .map(|&c| rebuild(expr, seen, c))
                .collect::<Option<Vec<_>>>()?;
            to_filter(Op::Compose(v))
        }
        Josh::Chain(kids) => {
            let v = kids
                .iter()
                .map(|&c| rebuild(expr, seen, c))
                .collect::<Option<Vec<_>>>()?;
            to_filter(Op::Chain(v))
        }
        Josh::Subtract([a, b]) => to_filter(Op::Subtract(
            rebuild(expr, seen, *a)?,
            rebuild(expr, seen, *b)?,
        )),
        Josh::Exclude(b) => to_filter(Op::Exclude(rebuild(expr, seen, *b)?)),
        Josh::Pin(b) => to_filter(Op::Pin(rebuild(expr, seen, *b)?)),
        Josh::Prefix(p) => match &expr[*p] {
            Josh::Symbol(s) => to_filter(Op::Prefix(s.as_str().into())),
            _ => return None,
        },
        Josh::Subdir(p) => match &expr[*p] {
            Josh::Symbol(s) => to_filter(Op::Subdir(s.as_str().into())),
            _ => return None,
        },
        Josh::Symbol(sym) => atom_to_filter(sym.as_str())?,
    };
    seen.insert(id, f);
    Some(f)
}

/// The rewrites this POC runs.
///
/// All gated on the same semantic bar — the output must produce an equivalent
/// tree and history, checked by [`equivalent`] using the trusted optimizer as a
/// sufficient-but-not-necessary oracle. They capture the *spirit* of `opt.rs`,
/// not its exact mechanism: several are one declarative step where `opt.rs`
/// recurses.
///
/// * distribute/factor: for a single chain prefix element `p`,
///   ```text
///   distribute: Chain[p, Compose(z1..zn)] == Compose(Chain[p,z1], ..., Chain[p,zn])
///   factor:     the reverse.
///   ```
///   Unconditionally semantics-preserving (Chain is sequential composition,
///   Compose is parallel merge). Written for one prefix element and compose
///   arities 2 and 3 — the minimal viable subset covering the `:/a:[...]` shapes
///   from `tests/filter/pretty_print.t`. Longer prefixes or larger composes are
///   left untouched (and thus stay equivalent); more fixed-arity cases are a
///   mechanical follow-up.
///
/// * cancel-prefix-subdir: `Chain[Prefix(p), Subdir(p)] == Nop`, mirroring the
///   adjacent-pair cancellation in the trusted optimizer (`opt.rs` flatten).
///   Because the path is a structural child, a single pattern variable `?p`
///   unifies the prefix's and subdir's path — egg's own matcher enforces path
///   equality, so this is a pure pattern rewrite with no Rust condition. Only
///   the exact two-element chain is handled; cancelling a pair inside a longer
///   chain is a follow-up (same arity limitation as distribute/factor).
///
/// * compose identity: `(compose)` is the empty tree and `(compose ?x)` is `?x`.
///   Pure patterns because `Box<[Id]>` matches by exact arity; mirrors `opt.rs`
///   Compose normalization and cleans up singleton/empty results from
///   [`SubtractComposeDiff`].
///
/// * subtract identity algebra (pure patterns): `x - x = empty`, `empty - x =
///   empty`, `x - nop = empty` (nop selects everything), `x - empty = x`.
///
/// * subtract pluck / absorb (pure patterns): when one operand is a single
///   element (not a compose), mirror `opt.rs` cases 11/12 — pluck it out of the
///   other side's compose, or collapse to empty if it is contained there. This
///   is the gap the variadic applier cannot cover (it needs both operands to be
///   composes). Fixed arities 2 and 3.
///
/// * subtract-compose-diff: `Subtract(Compose(A), Compose(B))` bidirectional set
///   difference, via [`SubtractComposeDiff`] — the one rewrite that needs a
///   custom applier because set difference is variadic.
fn rules() -> Vec<Rewrite<Josh, ()>> {
    vec![
        rewrite!("distribute-compose-2";
            "(chain ?p (compose ?z1 ?z2))" => "(compose (chain ?p ?z1) (chain ?p ?z2))"),
        rewrite!("factor-compose-2";
            "(compose (chain ?p ?z1) (chain ?p ?z2))" => "(chain ?p (compose ?z1 ?z2))"),
        rewrite!("distribute-compose-3";
            "(chain ?p (compose ?z1 ?z2 ?z3))" =>
            "(compose (chain ?p ?z1) (chain ?p ?z2) (chain ?p ?z3))"),
        rewrite!("factor-compose-3";
            "(compose (chain ?p ?z1) (chain ?p ?z2) (chain ?p ?z3))" =>
            "(chain ?p (compose ?z1 ?z2 ?z3))"),
        rewrite!("cancel-prefix-subdir";
            "(chain (prefix ?p) (subdir ?p))" => "nop"),
        // Compose identity: empty compose = empty tree; singleton compose = its
        // sole element. Exact-arity matching on Box<[Id]> makes these patterns.
        rewrite!("compose-empty"; "(compose)" => "empty"),
        rewrite!("compose-single"; "(compose ?x)" => "?x"),
        // Subtract identity / annihilator algebra — all pure patterns; `nop`
        // and `empty` are the opaque leaf atoms.
        rewrite!("subtract-self"; "(subtract ?x ?x)" => "empty"),
        rewrite!("subtract-empty-l"; "(subtract empty ?x)" => "empty"),
        rewrite!("subtract-nop-r"; "(subtract ?x nop)" => "empty"),
        rewrite!("subtract-empty-r"; "(subtract ?x empty)" => "?x"),
        // Subtract pluck / absorb (pure patterns), mirroring opt.rs cases 11/12
        // for the case one operand is a single element rather than a compose —
        // the gap the variadic applier below cannot cover (it needs both
        // operands to be composes). Pluck removes the element from the compose;
        // absorb collapses to empty when the element is contained in the right.
        // Fixed arities 2 and 3 (same convention as distribute/factor); larger
        // arities are a mechanical follow-up. Duplicate-element degenerate cases
        // are caught by the equivalence gate (see `equivalent`).
        rewrite!("pluck-compose-2-0"; "(subtract (compose ?a ?b) ?a)" => "?b"),
        rewrite!("pluck-compose-2-1"; "(subtract (compose ?a ?b) ?b)" => "?a"),
        rewrite!("pluck-compose-3-0";
            "(subtract (compose ?a ?b ?c) ?a)" => "(compose ?b ?c)"),
        rewrite!("pluck-compose-3-1";
            "(subtract (compose ?a ?b ?c) ?b)" => "(compose ?a ?c)"),
        rewrite!("pluck-compose-3-2";
            "(subtract (compose ?a ?b ?c) ?c)" => "(compose ?a ?b)"),
        rewrite!("absorb-compose-2-0"; "(subtract ?a (compose ?a ?b))" => "empty"),
        rewrite!("absorb-compose-2-1"; "(subtract ?a (compose ?b ?a))" => "empty"),
        rewrite!("absorb-compose-3-0";
            "(subtract ?a (compose ?a ?b ?c))" => "empty"),
        rewrite!("absorb-compose-3-1";
            "(subtract ?a (compose ?b ?a ?c))" => "empty"),
        rewrite!("absorb-compose-3-2";
            "(subtract ?a (compose ?b ?c ?a))" => "empty"),
        // Subtract set-difference over two composes — variadic, so a custom
        // applier rather than a pattern. See [`SubtractComposeDiff`].
        rewrite!("subtract-compose-diff";
            "(subtract ?a ?b)" => { SubtractComposeDiff::new() }),
    ]
}

/// `Subtract(Compose(A), Compose(B))` → the bidirectional set difference
/// `Subtract(Compose(A\B), Compose(B\A))`.
///
/// This is the one rewrite that cannot be a pure pattern: removing a *variable*
/// number of shared elements from a variadic `Compose` needs an applier that
/// builds the result programmatically (egg rewrite patterns are fixed-arity).
/// It captures the *spirit* of the trusted optimizer's set-difference case
/// (`opt.rs`), not its mechanism — `opt` reaches the same result via a recursive
/// single-element `retain` over a hashed `FilterSet`, whereas this adds the
/// fully-differenced term in one step and lets the `compose`-identity rules
/// clean up any singleton/empty result.
///
/// Bidirectional (rather than left-only `A\B`) because the equivalence gate
/// canonicalizes via `opt`, which differsences both sides — a left-only
/// candidate would be sound but fail `canon(input) == canon(candidate)` and so
/// never fire. Both forms give the same tree, so this is correct for the right
/// reason, not just to placate the gate.
///
/// Membership is by e-class identity (`egraph.find`): two elements are "the
/// same" iff they share an e-class, which is exactly the Filter-OID hash-consing
/// [`build`] establishes. Self-guarding: if either operand is not a `compose`,
/// or the element sets are disjoint, it adds nothing. Because the result is
/// disjoint, it will not re-fire on its own output.
struct SubtractComposeDiff {
    a: Var,
    b: Var,
}

impl SubtractComposeDiff {
    fn new() -> Self {
        Self {
            a: "?a".parse().expect("var ?a"),
            b: "?b".parse().expect("var ?b"),
        }
    }
}

impl Applier<Josh, ()> for SubtractComposeDiff {
    fn vars(&self) -> Vec<Var> {
        vec![self.a, self.b]
    }

    fn apply_one(
        &self,
        egraph: &mut EGraph<Josh, ()>,
        eclass: Id,
        subst: &Subst,
        _searcher_ast: Option<&PatternAst<Josh>>,
        _rule_name: Symbol,
    ) -> Vec<Id> {
        let a = *subst.get(self.a).expect("bound ?a");
        let b = *subst.get(self.b).expect("bound ?b");

        let Some(av) = compose_children(egraph, a) else {
            return vec![];
        };
        let Some(bv) = compose_children(egraph, b) else {
            return vec![];
        };

        // Canonicalize elements to their e-class representative so two equal
        // Filters (same OID -> same Id -> same class) compare as one element.
        let a_set: HashSet<Id> = av.iter().map(|i| egraph.find(*i)).collect();
        let b_set: HashSet<Id> = bv.iter().map(|i| egraph.find(*i)).collect();

        // Only fire when an element can actually be removed; otherwise this is a
        // no-op that would just re-match the original enode every iteration.
        let overlaps = av.iter().any(|i| b_set.contains(&egraph.find(*i)))
            || bv.iter().any(|i| a_set.contains(&egraph.find(*i)));
        if !overlaps {
            return vec![];
        }

        // A\B and B\A, deduped by canonical e-class (mirrors opt's two retains).
        // First-seen order is preserved to keep extraction stable.
        let mut diff_a = Vec::new();
        let mut seen_a: HashSet<Id> = HashSet::new();
        for &i in &av {
            let c = egraph.find(i);
            if !b_set.contains(&c) && seen_a.insert(c) {
                diff_a.push(i);
            }
        }
        let mut diff_b = Vec::new();
        let mut seen_b: HashSet<Id> = HashSet::new();
        for &i in &bv {
            let c = egraph.find(i);
            if !a_set.contains(&c) && seen_b.insert(c) {
                diff_b.push(i);
            }
        }

        // Build canonical operands (empty -> the empty atom, singleton -> the
        // bare element) rather than Compose([])/Compose([x]). This mirrors opt's
        // Compose normalization at construction time and, crucially, keeps those
        // nodes out of the e-graph so the AstSize extractor has no tie to break
        // between e.g. Compose([]) and the empty atom (both cost 1).
        let left = compose_of(egraph, diff_a);
        let right = compose_of(egraph, diff_b);
        let differenced = egraph.add(Josh::Subtract([left, right]));
        egraph.union(eclass, differenced);
        vec![egraph.find(eclass)]
    }
}

/// Children of the first `compose` node in `id`'s e-class, if it contains one.
fn compose_children(egraph: &EGraph<Josh, ()>, id: Id) -> Option<Vec<Id>> {
    egraph[id].nodes.iter().find_map(|node| match node {
        Josh::Compose(kids) => Some(kids.to_vec()),
        _ => None,
    })
}

/// Build a canonical compose operand from an element list: empty becomes the
/// `empty` atom, a singleton becomes the element itself, otherwise a `compose`.
/// See [`SubtractComposeDiff`] for why construction-time canonicalization
/// matters.
fn compose_of(egraph: &mut EGraph<Josh, ()>, elems: Vec<Id>) -> Id {
    match elems.len() {
        0 => egraph.add(Josh::Symbol(Symbol::from("empty"))),
        1 => elems[0],
        _ => egraph.add(Josh::Compose(elems.into_boxed_slice())),
    }
}

/// Canonicalize a filter via the trusted existing optimizer.
fn canon(f: Filter) -> Filter {
    opt::optimize(f)
}

/// Two filters are equivalent if they share a canonical form under the trusted
/// existing optimizer.
///
/// The true correctness bar is equivalent tree and history; `optimize` is used
/// here only as a sound-but-incomplete proxy oracle for that (verifying real
/// tree equivalence needs a repo, out of scope for this crate). So this check is
/// sufficient but not necessary: genuinely equivalent filters may compare
/// unequal — for instance if egg composes rules into something `opt` itself
/// wouldn't reach — in which case `egg_optimize` conservatively returns its
/// input unchanged. Strengthening this oracle is the follow-up that would let
/// egg exploit optimizations beyond `opt`'s reach.
fn equivalent(a: Filter, b: Filter) -> bool {
    canon(a) == canon(b)
}

/// Run the experimental egg-based optimizer over `filter`.
///
/// - Idempotent-ish and deterministic.
/// - Never returns a non-equivalent filter. If any `Op` in the tree is not
///   representable by the egg language, the input is returned unchanged; and as
///   a final guard the output is checked for equivalence, falling back to the
///   input if the egg output could not be proven equivalent.
pub fn egg_optimize(filter: Filter) -> Filter {
    let mut expr = RecExpr::default();
    let mut seen_build = HashMap::new();
    if build(&mut expr, &mut seen_build, filter).is_none() {
        return filter;
    }

    let rules = rules();
    let runner = Runner::<Josh, ()>::default()
        .with_expr(&expr)
        // Mandatory limits: e-graphs can blow up, and extraction always runs
        // (best-so-far) even when a limit is hit.
        .with_node_limit(10_000)
        .with_iter_limit(30)
        .run(&rules);

    let root = runner.roots[0];
    let (_cost, best) = Extractor::new(&runner.egraph, AstSize).find_best(root);

    let mut seen_rebuild = HashMap::new();
    let candidate = match rebuild(&best, &mut seen_rebuild, best.root()) {
        Some(f) => f,
        None => return filter,
    };

    if equivalent(filter, candidate) {
        candidate
    } else {
        filter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flang::parse::parse;

    fn subdir(p: &str) -> Filter {
        to_filter(Op::Subdir(p.into()))
    }

    fn prefix(p: &str) -> Filter {
        to_filter(Op::Prefix(p.into()))
    }

    fn compose(fs: &[Filter]) -> Filter {
        to_filter(Op::Compose(fs.to_vec()))
    }

    /// `Chain[p, Compose[z1, z2]]` — the factored form.
    fn factored() -> Filter {
        to_filter(Op::Chain(vec![
            subdir("p"),
            to_filter(Op::Compose(vec![subdir("z1"), subdir("z2")])),
        ]))
    }

    /// `Compose[Chain[p, z1], Chain[p, z2]]` — the distributed form.
    fn distributed() -> Filter {
        to_filter(Op::Compose(vec![
            to_filter(Op::Chain(vec![subdir("p"), subdir("z1")])),
            to_filter(Op::Chain(vec![subdir("p"), subdir("z2")])),
        ]))
    }

    #[test]
    fn factored_form_stays_equivalent() {
        let f = factored();
        assert!(
            equivalent(f, egg_optimize(f)),
            "egg_optimize must preserve equivalence on the factored form"
        );
    }

    #[test]
    fn distributed_form_factors_to_cheaper() {
        let dist = distributed();
        let out = egg_optimize(dist);
        // The extractor did real work: it picked the cheaper factored form
        // rather than returning the input unchanged. With structural Prefix/Subdir
        // each path-carrying op costs 2 (op + path symbol), so AstSize(factored)
        // = 8 < AstSize(distributed) = 11.
        assert_eq!(out, factored(), "expected the cheaper factored form");
        assert_ne!(out, dist, "egg must not have returned the input verbatim");
    }

    #[test]
    fn unrepresentable_filter_is_identity() {
        // Op::Author is not modelled by the egg language, so the whole filter is
        // returned unchanged (never a non-equivalent result).
        let f = to_filter(Op::Author("name".into(), "e@mail".into()));
        assert_eq!(egg_optimize(f), f);
    }

    #[test]
    fn real_filters_stay_equivalent() {
        for spec in [
            ":/a",
            ":/a:/b",
            ":[x=:/a:/b:/d,y=:/a:/c:/d]",
            ":subtract[a=:[::x/,::y/,::z/],b=:[::x/,::y/]]",
            ":subtract[a=:[::x/,::y/,::z/],b=:[::x/,::y/,::w/]]",
        ] {
            let f = parse(spec).expect(spec);
            assert!(
                equivalent(f, egg_optimize(f)),
                "egg_optimize broke equivalence for {spec:?}"
            );
        }
    }

    #[test]
    fn prefix_subdir_chain_cancels_to_nop() {
        // Chain[Prefix(p), Subdir(p)] mirrors the adjacent-pair cancellation in
        // opt.rs flatten; the trusted optimizer reduces it to Nop, so egg's
        // extracted Nop passes the equivalence gate and is returned.
        let input = to_filter(Op::Chain(vec![prefix("p"), subdir("p")]));
        let out = egg_optimize(input);
        assert_eq!(out, to_filter(Op::Nop), "expected cancellation to Nop");
        assert_ne!(out, input, "egg must not have returned the input verbatim");
    }

    #[test]
    fn mismatched_paths_do_not_cancel() {
        // Different paths must not cancel: the pattern requires the prefix's and
        // subdir's path to be the same variable `?p`, which mismatched paths
        // fail to satisfy, so the chain is returned unchanged.
        let input = to_filter(Op::Chain(vec![prefix("p"), subdir("q")]));
        let out = egg_optimize(input);
        assert_eq!(out, input, "mismatched paths must not be rewritten");
        assert_ne!(out, to_filter(Op::Nop), "mismatched paths must not cancel");
    }

    #[test]
    fn subtract_self_cancels_to_empty() {
        // Subtract(x, x) == Empty. Both children are the same Filter, so they
        // share one e-class and the (subtract ?x ?x) pattern matches natively —
        // no Rust condition, exactly like cancel-prefix-subdir.
        let x = subdir("a");
        let input = to_filter(Op::Subtract(x, x));
        let out = egg_optimize(input);
        assert_eq!(out, to_filter(Op::Empty), "x - x must cancel to Empty");
        assert_ne!(out, input, "egg must not have returned the input verbatim");
    }

    #[test]
    fn subtract_identity_rules() {
        // x - nop = Empty (nop selects everything); x - empty = x.
        let x = subdir("a");
        let nop = to_filter(Op::Nop);
        let empty = to_filter(Op::Empty);

        let out = egg_optimize(to_filter(Op::Subtract(x, nop)));
        assert_eq!(out, empty, "x - nop must be Empty");

        let out = egg_optimize(to_filter(Op::Subtract(x, empty)));
        assert_eq!(out, x, "x - empty must be x");
    }

    #[test]
    fn subtract_compose_set_difference() {
        // Subtract(Compose[a,b], Compose[a,c]): element `a` is shared, so it is
        // selected-then-subtracted on the left and present on the right — it
        // contributes nothing either way. The bidirectional difference leaves
        // Subtract(b, c), which opt also produces, so the gate accepts the
        // smaller extracted form.
        let input = to_filter(Op::Subtract(
            compose(&[subdir("a"), subdir("b")]),
            compose(&[subdir("a"), subdir("c")]),
        ));
        let out = egg_optimize(input);
        let expected = to_filter(Op::Subtract(subdir("b"), subdir("c")));
        assert_eq!(
            out, expected,
            "shared compose elements must be differenced away"
        );
        assert_ne!(out, input, "egg must not have returned the input verbatim");
    }

    #[test]
    fn subtract_subset_collapses_to_empty() {
        // A subset of B: A\B is empty, so the difference is Empty. Exercises the
        // compose-empty cleanup of the applier's empty left side.
        let input = to_filter(Op::Subtract(
            compose(&[subdir("a"), subdir("b")]),
            compose(&[subdir("a"), subdir("b"), subdir("c")]),
        ));
        let out = egg_optimize(input);
        assert_eq!(out, to_filter(Op::Empty), "subset subtract must be Empty");
        assert_ne!(out, input, "egg must not have returned the input verbatim");
    }

    #[test]
    fn disjoint_compose_subtract_stays_equivalent() {
        // No shared elements: the set-difference applier self-guards (no
        // overlap), so the subtract is left structurally as-is.
        let input = to_filter(Op::Subtract(
            compose(&[subdir("a"), subdir("b")]),
            compose(&[subdir("c"), subdir("d")]),
        ));
        let out = egg_optimize(input);
        assert!(
            equivalent(input, out),
            "disjoint subtract must stay equivalent"
        );
    }

    #[test]
    fn subtract_pluck_element_from_compose() {
        // Subtract(Compose(a,b), a): a is an element of the left compose, so it
        // is removed — opt.rs case 11. The variadic applier can't handle this
        // (the right operand is a single element, not a compose); the pure
        // pluck pattern fills that gap.
        let input = to_filter(Op::Subtract(
            compose(&[subdir("a"), subdir("b")]),
            subdir("a"),
        ));
        let out = egg_optimize(input);
        assert_eq!(out, subdir("b"), "plucked element must be removed");
        assert_ne!(out, input, "egg must not have returned the input verbatim");
    }

    #[test]
    fn subtract_pluck_middle_of_three() {
        // Subtract(Compose(a,b,c), b) -> Compose(a,c): the 3-ary pluck rules are
        // position-independent.
        let input = to_filter(Op::Subtract(
            compose(&[subdir("a"), subdir("b"), subdir("c")]),
            subdir("b"),
        ));
        let out = egg_optimize(input);
        assert_eq!(
            out,
            compose(&[subdir("a"), subdir("c")]),
            "middle element must be plucked"
        );
        assert_ne!(out, input, "egg must not have returned the input verbatim");
    }

    #[test]
    fn subtract_absorb_into_compose() {
        // Subtract(a, Compose(a,b)): a is contained in the right compose, so the
        // difference is empty — opt.rs case 12.
        let input = to_filter(Op::Subtract(
            subdir("a"),
            compose(&[subdir("a"), subdir("b")]),
        ));
        let out = egg_optimize(input);
        assert_eq!(
            out,
            to_filter(Op::Empty),
            "contained element must absorb to empty"
        );
        assert_ne!(out, input, "egg must not have returned the input verbatim");
    }
}
