//! Experimental egg-based filter optimizer (POC).
//!
//! This is a wiring/correctness proof that the [`egg`] e-graph crate can drive a
//! real josh filter optimization using the existing [`Filter`] representation
//! end-to-end. It implements two rewrites — the Chain/Compose distribute <->
//! factor pair, and a Prefix/Subdir cancellation — both as pure pattern
//! rewrites: Prefix/Subdir paths are structural children, so path equality is
//! handled by egg's unification rather than a Rust condition. Gated entirely
//! behind the `--use-new-opt` flag.
//!
//! The bar this module must meet is correctness, not speed or prettier output:
//! [`egg_optimize`] must never return a filter that is not semantically
//! equivalent to its input.

use crate::filter::Filter;
use crate::op::{BlobContent, Op};
use crate::opt;
use crate::persist::{to_filter, to_op};
use egg::{AstSize, Extractor, Id, RecExpr, Rewrite, Runner, Symbol, rewrite};
use std::collections::HashMap;

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
    ]
}

/// Canonicalize a filter via the trusted existing optimizer.
fn canon(f: Filter) -> Filter {
    opt::optimize(f)
}

/// Two filters are equivalent if they share a canonical form under the trusted
/// existing optimizer.
///
/// Note: `optimize` is not a complete normal form (that is the larger problem
/// this POC is a step toward solving), so this check is sufficient but not
/// necessary: genuinely equivalent filters may compare unequal, in which case
/// `egg_optimize` conservatively returns its input unchanged.
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
}
