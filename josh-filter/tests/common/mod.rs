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

/// `Chain[p, Compose[z1, z2]]` — the factored form.
pub fn factored() -> Filter {
    to_filter(Op::Chain(vec![
        subdir("p"),
        to_filter(Op::Compose(vec![subdir("z1"), subdir("z2")])),
    ]))
}

/// `Compose[Chain[p, z1], Chain[p, z2]]` — the distributed form.
pub fn distributed() -> Filter {
    to_filter(Op::Compose(vec![
        to_filter(Op::Chain(vec![subdir("p"), subdir("z1")])),
        to_filter(Op::Chain(vec![subdir("p"), subdir("z2")])),
    ]))
}
