//! Shared helpers for the eggopt integration tests.
//!
//! These mirror the private helpers that used to live inside `eggopt.rs`'s
//! `#[cfg(test)] mod tests`, so the moved tests could be ported verbatim. Each
//! topic test file pulls in only the subset of helpers it needs via
//! `use common::*;`, so unused-function warnings are expected and suppressed
//! crate-wide here.
#![allow(dead_code)]

use josh_filter::Filter;
use josh_filter::op::Op;
use josh_filter::persist::to_filter;

pub fn subdir(p: &str) -> Filter {
    to_filter(Op::Subdir(p.into()))
}

pub fn prefix(p: &str) -> Filter {
    to_filter(Op::Prefix(p.into()))
}

pub fn compose(fs: &[Filter]) -> Filter {
    to_filter(Op::Compose(fs.to_vec()))
}

/// A Message filter rewriting the commit message with `fmt`, selecting
/// commits whose message matches `re`. Only the tree (not the message) is
/// observable downstream, so any two messages have an empty tree difference.
pub fn message(fmt: &str, re: &str) -> Filter {
    to_filter(Op::Message(fmt.to_string(), regex::Regex::new(re).unwrap()))
}

// ---- eggopt corpus quality harness -----------------------------------------
//
// These power the `corpus_*.rs` snapshot tests. Each takes a filter spec string,
// parses it RAW (no opt — see `parse_egg`), and renders with `spec_egg` (no
// re-simplify), so snapshots show each optimizer's true output. `report` shows
// the raw / opt / egg forms side by side plus a structural cost, so a divergence
// between opt and egg is a visible quality gap.
use josh_filter::eggopt::egg_optimize;
use josh_filter::flang::parse::parse_egg;
use josh_filter::flang::spec_egg;
use josh_filter::opt;
use josh_filter::persist::to_op;

/// egg's output for the raw parse of `spec`, rendered without re-simplification.
pub fn egg_opt(spec: &str) -> anyhow::Result<String> {
    Ok(spec_egg(egg_optimize(parse_egg(spec)?)))
}

/// The trusted optimizer's output for the same raw parse — for side-by-side.
pub fn opt_ref(spec: &str) -> anyhow::Result<String> {
    Ok(spec_egg(opt::optimize(parse_egg(spec)?)))
}

/// raw / opt / egg rendered side by side with a structural cost each, so each
/// snapshot shows the quality gap at a glance: where `opt` and `egg` differ, egg
/// is leaving reduction on the table. All three use `spec_egg` (no re-simplify)
/// for an apples-to-apples compare.
pub fn report(spec: &str) -> anyhow::Result<String> {
    let raw = parse_egg(spec)?;
    let o = opt::optimize(raw);
    let e = egg_optimize(raw);
    Ok(format!(
        "raw:  {} (cost {})\nopt:  {} (cost {})\negg:  {} (cost {})",
        spec_egg(raw),
        cost(raw),
        spec_egg(o),
        cost(o),
        spec_egg(e),
        cost(e),
    ))
}

/// Structural size of a filter — the node count of its `Op` DAG. A rough "how
/// reduced" proxy: lower is more reduced (a leaf is 1; `Compose[a, b]` is 3).
pub fn cost(f: Filter) -> usize {
    fn count(op: &Op) -> usize {
        1 + match op {
            Op::Compose(v) | Op::Chain(v) => v.iter().map(|c| count(&to_op(*c))).sum(),
            Op::Subtract(a, b) => count(&to_op(*a)) + count(&to_op(*b)),
            Op::Exclude(b) | Op::Pin(b) => count(&to_op(*b)),
            _ => 0,
        }
    }
    count(&to_op(f))
}
