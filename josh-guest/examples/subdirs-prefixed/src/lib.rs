//! The RFC's doc example: for every top-level directory `d` of the context
//! tree, overlay `:/d:prefix=d` — i.e. keep each directory mounted at its own
//! name, dropping top-level files.
//!
//! Build (inside `josh-guest/`):
//! `cargo build --release --target wasm32-unknown-unknown`
//! → `target/wasm32-unknown-unknown/release/subdirs_prefixed.wasm`

#![no_std]

use josh_filter_guest::{Filter, Tree, compose, josh_filter_entry, josh_guest_rt, nop};

fn run(tree: Tree) -> Filter {
    compose(
        tree.dirs("")
            .into_iter()
            .map(|d| nop().subdir(&d).prefix(&d)),
    )
}

josh_filter_entry!(run);
josh_guest_rt!();
