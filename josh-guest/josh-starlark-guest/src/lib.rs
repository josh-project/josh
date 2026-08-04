//! A Starlark interpreter as a josh wasm filter guest module.
//!
//! This is the RFC's Starlark compatibility path (deployment variant 1): the
//! built blob is committed once into a user repository (e.g. as
//! `tools/starlark.wasm`) and every scripted filter site references it with
//! the script path as its first invocation argument:
//!
//! ```text
//! :!tools/starlark=st/config.star[::st/]
//! ```
//!
//! The script sees the same API as the old native `josh-starlark` filter:
//! pre-set module variables `filter` (starts as the nop filter) and `tree`
//! (the root of the context-filtered tree), plus the global `compose()`. The
//! script builds its result by reassigning `filter`; an empty script yields
//! the nop filter. The one API change: `filter.starlark(path, sub)` is
//! replaced by `filter.wasm(path, args, sub)`.
//!
//! The script must be visible in the context-filtered tree (`::st/` above
//! includes `st/config.star`). A missing invocation argument or an absent
//! script is a deterministic evaluation error: this module panics, the panic
//! aborts (trap), and josh degrades the filter to `:empty`. This differs
//! deliberately from the old native behavior, where a missing script was
//! treated as an empty script (nop).
//!
//! Build with `scripts/build-starlark-guest.sh`; see README.md for the
//! getrandom determinism notes.

mod evaluate;
mod filter;
mod getrandom_backend;
mod module;
mod tree;

use josh_filter_guest::{Filter, Tree, josh_filter_entry};

fn run(tree: Tree) -> Filter {
    let args = josh_filter_guest::args();
    let script_path = args
        .first()
        .expect("josh-starlark-guest: missing script path invocation argument");
    // `tree.file` cannot distinguish a missing script from an empty one, so
    // absence is detected via the entry OID and turned into a trap.
    if tree.entry_oid(script_path).is_none() {
        panic!("josh-starlark-guest: script not found in context tree: {script_path}");
    }
    let script = tree.file(script_path);
    match evaluate::evaluate(&script) {
        Ok(filter) => filter,
        Err(e) => panic!("josh-starlark-guest: {e}"),
    }
}

josh_filter_entry!(run);
