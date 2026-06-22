use crate::eggopt::lang::{Josh, SEP, atom_to_filter, op_to_atom};
use crate::filter::Filter;
use crate::op::Op;
use crate::persist::{to_filter, to_op};
use egg::{Id, RecExpr, Symbol};
use std::collections::HashMap;

/// Convert a `Filter` into an egg `RecExpr`, memoizing by `Filter` OID so shared
/// subterms share an e-node. Returns `None` if any subterm is unrepresentable.
pub(crate) fn build(
    expr: &mut RecExpr<Josh>,
    seen: &mut HashMap<Filter, Id>,
    f: Filter,
) -> Option<Id> {
    if let Some(&id) = seen.get(&f) {
        return Some(id);
    }
    let id = match to_op(f) {
        Op::Compose(cs) => {
            // Fold right into a cons-list: Compose[a, b, c] -> Cons(a, Cons(b,
            // Cons(c, Nil))); an empty compose -> Nil. Iterating in reverse and
            // prepending keeps the element order, and shared subterms still
            // memoize by `Filter` OID via `seen` (RecExpr dedups the cons enodes).
            let mut tail = expr.add(Josh::Nil);
            for &c in cs.iter().rev() {
                let h = build(expr, seen, c)?;
                tail = expr.add(Josh::Cons([h, tail]));
            }
            tail
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
        // Path-carrying ops keep the WHOLE path as one structural `Symbol` child
        // (no boundary decomposition): two equal paths share an e-class and unify
        // under one pattern variable, and the Prefix/Subdir conflict rule sees
        // the full path to compare depth. Multi-component `File`s fall through to
        // the opaque-atom arm below.
        Op::Prefix(path) => {
            let p = expr.add(Josh::Symbol(Symbol::from(path.to_str()?)));
            expr.add(Josh::Prefix(p))
        }
        Op::Subdir(path) => {
            let p = expr.add(Josh::Symbol(Symbol::from(path.to_str()?)));
            expr.add(Josh::Subdir(p))
        }
        Op::Message(fmt, re) => {
            // Structural (not an atom) so a pattern can match "any message". The
            // payload is one symbol so the node count stays the same as an atom.
            let payload = expr.add(Josh::Symbol(Symbol::from(format!(
                "{fmt}{SEP}{}",
                re.as_str()
            ))));
            expr.add(Josh::Message(payload))
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
pub(crate) fn rebuild(
    expr: &RecExpr<Josh>,
    seen: &mut HashMap<Id, Filter>,
    id: Id,
) -> Option<Filter> {
    if let Some(&f) = seen.get(&id) {
        return Some(f);
    }
    let f = match &expr[id] {
        Josh::Cons(_) | Josh::Nil => {
            let elems = rebuild_cons(expr, seen, id)?;
            // Singleton/empty collapse at the boundary (E9 + singleton-flatten),
            // NOT an egg rule: a blanket (cons ?x nil) => ?x is unsound inside a
            // list's tail slot, but rebuild knows the spine, so it is sound here.
            // Required by tests that assert `out == a`, not `out == Compose[a]`.
            match elems.len() {
                0 => to_filter(Op::Empty),
                1 => elems.into_iter().next().unwrap(),
                _ => to_filter(Op::Compose(elems)),
            }
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
        Josh::Message(p) => match &expr[*p] {
            Josh::Symbol(s) => {
                let (fmt, re) = s.as_str().split_once(SEP)?;
                to_filter(Op::Message(fmt.to_string(), regex::Regex::new(re).ok()?))
            }
            _ => return None,
        },
        Josh::Symbol(sym) => atom_to_filter(sym.as_str())?,
    };
    seen.insert(id, f);
    Some(f)
}

/// Walk a cons-list spine starting at `id`, rebuilding each head element, until
/// `Nil`. Returns the element list, or `None` if the spine is malformed (e.g. an
/// atom where a list tail is expected), which makes `egg_optimize` fall back to
/// the identity filter.
fn rebuild_cons(
    expr: &RecExpr<Josh>,
    seen: &mut HashMap<Id, Filter>,
    mut id: Id,
) -> Option<Vec<Filter>> {
    let mut elems = Vec::new();
    loop {
        match &expr[id] {
            Josh::Nil => break,
            Josh::Cons([h, t]) => {
                elems.push(rebuild(expr, seen, *h)?);
                id = *t;
            }
            _ => return None,
        }
    }
    Some(elems)
}
