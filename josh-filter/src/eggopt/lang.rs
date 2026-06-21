use crate::filter::Filter;
use crate::op::{BlobContent, Op};
use crate::persist::to_filter;
use egg::{Id, Symbol};

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
    /// `Compose`/`Chain`/`Subtract`/`Exclude`/`Pin` are structural containers.
    /// `Prefix`/`Subdir` carry their path as a structural child so a pattern
    /// variable can unify two equal paths (see `cancel-prefix-subdir`). `Message`
    /// is structural too, so a pattern can match "any message" regardless of its
    /// format/regex payload (see `subtract-message-message`); the rest of the leaf
    /// data ops are opaque atoms.
    pub(crate) enum Josh {
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
        // Message is structural (not an opaque atom) so a pattern can recognize
        // "any message". Its single child `Symbol` carries the NUL-separated
        // format/regex payload, which is never inspected by a pattern.
        "message" = Message(Id),
        // Opaque leaf atoms; the carried data is encoded into the symbol string.
        Symbol(Symbol),
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
