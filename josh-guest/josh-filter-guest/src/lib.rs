//! Guest-side SDK for writing josh wasm filter modules.
//!
//! A josh wasm filter is a `wasm32-unknown-unknown` module referenced from a
//! filter spec as `:!path=arg,...[context]`. Josh instantiates the blob at
//! `path.wasm` (looked up in the tree being filtered) in a sandboxed,
//! deterministic runtime, lets it inspect the context-filtered tree, and
//! composes the [`Filter`] it returns with the context filter. This crate
//! wraps the raw ABI in safe types so a module is just:
//!
//! ```ignore
//! #![no_std]
//!
//! use josh_filter_guest::{Filter, Tree, compose, josh_filter_entry, josh_guest_rt, nop};
//!
//! fn run(tree: Tree) -> Filter {
//!     compose(tree.dirs("").into_iter().map(|d| nop().subdir(&d).prefix(&d)))
//! }
//!
//! josh_filter_entry!(run);
//! josh_guest_rt!(); // no_std runtime: bump allocator + aborting panic handler
//! ```
//!
//! # ABI (v1)
//!
//! The module must export:
//!
//! * `memory` — its linear memory (rustc exports this by default for wasm
//!   `cdylib` targets),
//! * `josh_abi_version() -> u32` — must return `1`,
//! * `josh_alloc(len: u32) -> ptr` — allocates guest buffers for the host,
//! * `josh_run() -> u32` — the entry point, returning a filter handle.
//!
//! [`josh_filter_entry!`] emits all three functions. All host imports live in
//! the wasm module `"josh"` (see [`abi`]): filter builder methods operating
//! on opaque `u32` handles, read-only tree access, and `invocation_args`.
//! Guest-to-host strings are UTF-8 `(ptr, len)` pairs; host-to-guest strings
//! are written into buffers obtained by re-entrantly calling `josh_alloc` and
//! returned packed into an `i64` as `(ptr << 32) | len` (the empty string is
//! returned as `0` without any allocation).
//!
//! # Allocation model
//!
//! Allocations are never freed: an instance is discarded right after
//! `josh_run` returns, so everything — `josh_alloc` buffers and the `rt`
//! feature's bump allocator alike — simply leaks into instance memory that
//! dies with the evaluation.
//!
//! Pure-`no_std` guests install the runtime (global allocator + panic
//! handler) by invoking [`josh_guest_rt!`] once; guests that link `std`
//! must not, and should depend on this crate with `default-features =
//! false` (the lang items live in the guest crate, not this rlib, so one
//! workspace can build both kinds side by side).
//!
//! # Determinism
//!
//! There is no WASI and there are no clock, randomness, environment or
//! filesystem imports. Evaluation must be (and with this import set, is) a
//! pure function of the module blob, the invocation args and the visible
//! tree; josh caches results under exactly that key.
//!
//! # Building
//!
//! Inside the `josh-guest/` workspace:
//!
//! ```text
//! cargo build --release --target wasm32-unknown-unknown
//! ```
//!
//! The release profile is size-tuned (`opt-level = "z"`, fat LTO, `strip`,
//! `panic = "abort"`). Commit the resulting `.wasm` blob into the repository
//! that references it, next to its source.

#![no_std]

extern crate alloc;

pub mod abi;
mod args;
mod filter;
#[cfg(all(feature = "rt", target_arch = "wasm32"))]
pub mod rt;
mod tree;

pub use args::args;
pub use filter::{Filter, compose, empty, nop};
pub use tree::Tree;

use alloc::string::String;
use alloc::vec::Vec;

/// Decode a host-to-guest string return: `(ptr << 32) | len`, buffer
/// allocated through the guest's own `josh_alloc`. A `len` of zero means the
/// empty string and carries no valid pointer.
pub(crate) fn decode_string(packed: u64) -> String {
    let len = (packed & 0xffff_ffff) as usize;
    if len == 0 {
        return String::new();
    }
    let ptr = (packed >> 32) as usize as *const u8;
    // SAFETY: the host wrote `len` bytes at `ptr`, a buffer it obtained from
    // this instance's `josh_alloc` (leak-only, so it is never invalidated).
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Split a newline-joined host list; the empty string is the empty list.
pub(crate) fn split_list(joined: String) -> Vec<String> {
    if joined.is_empty() {
        return Vec::new();
    }
    joined.split('\n').map(String::from).collect()
}

/// Implementation of the exported `josh_alloc`: leak a buffer of at least
/// `len` bytes. Not part of the public API — use [`josh_filter_entry!`].
#[doc(hidden)]
pub fn __josh_alloc(len: u32) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity((len as usize).max(1));
    let ptr = buf.as_mut_ptr();
    core::mem::forget(buf);
    ptr
}

/// Emit the ABI v1 guest exports (`josh_abi_version`, `josh_alloc`,
/// `josh_run`) for an entry function `fn run(tree: Tree) -> Filter`, which is
/// invoked with the root of the context-filtered tree.
#[macro_export]
macro_rules! josh_filter_entry {
    ($run:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn josh_abi_version() -> u32 {
            1
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn josh_alloc(len: u32) -> *mut u8 {
            $crate::__josh_alloc(len)
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn josh_run() -> u32 {
            let run: fn($crate::Tree) -> $crate::Filter = $run;
            run($crate::Tree::root()).handle()
        }
    };
}
