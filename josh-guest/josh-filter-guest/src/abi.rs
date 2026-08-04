//! Raw ABI v1 import declarations for the host module `"josh"`.
//!
//! Prefer the safe wrappers ([`crate::Filter`], [`crate::Tree`],
//! [`crate::args`]) — this module exists so that guests with unusual needs
//! (or non-Rust-shaped codegen) can reach the ABI directly.
//!
//! Conventions: filters are opaque `u32` handles; guest-to-host strings are
//! UTF-8 `(ptr, len)` pairs; host-to-guest strings come back as
//! `(ptr << 32) | len` packed into a `u64` with the buffer allocated via the
//! guest's exported `josh_alloc` (`0` means the empty string — do not
//! dereference the pointer). An invalid handle, non-UTF-8 string, invalid
//! glob or out-of-bounds pointer traps, which the host turns into an
//! evaluation error.

#[link(wasm_import_module = "josh")]
unsafe extern "C" {
    // Constructors (no receiver).
    pub fn nop() -> u32;
    pub fn empty() -> u32;

    // Combinators.
    pub fn chain(a: u32, b: u32) -> u32;
    /// `count` little-endian `u32` handles at `ptr`.
    pub fn compose(ptr: *const u32, count: u32) -> u32;

    // Builder methods (receiver handle first).
    pub fn subdir(f: u32, p: *const u8, l: u32) -> u32;
    pub fn prefix(f: u32, p: *const u8, l: u32) -> u32;
    pub fn file(f: u32, p: *const u8, l: u32) -> u32;
    /// NOTE: the DESTINATION path comes first, then the source, matching
    /// `josh_filter::Filter::rename` on the host.
    pub fn rename(f: u32, dst: *const u8, dst_len: u32, src: *const u8, src_len: u32) -> u32;
    pub fn pattern(f: u32, glob: *const u8, l: u32) -> u32;
    pub fn linear(f: u32) -> u32;
    pub fn workspace(f: u32, p: *const u8, l: u32) -> u32;
    pub fn stored(f: u32, p: *const u8, l: u32) -> u32;
    pub fn author(f: u32, name: *const u8, name_len: u32, email: *const u8, email_len: u32) -> u32;
    pub fn committer(
        f: u32,
        name: *const u8,
        name_len: u32,
        email: *const u8,
        email_len: u32,
    ) -> u32;
    pub fn message(f: u32, m: *const u8, l: u32) -> u32;
    pub fn unsign(f: u32) -> u32;
    pub fn prune_trivial_merge(f: u32) -> u32;
    pub fn hook(f: u32, h: *const u8, l: u32) -> u32;
    pub fn with_meta(f: u32, key: *const u8, key_len: u32, value: *const u8, value_len: u32)
    -> u32;
    pub fn insert(f: u32, p: *const u8, p_len: u32, content: *const u8, content_len: u32) -> u32;
    pub fn treeid(f: u32, p: *const u8, l: u32, sub: u32) -> u32;
    /// `args` is the newline-joined argument list; empty means no args.
    pub fn wasm(f: u32, p: *const u8, p_len: u32, args: *const u8, args_len: u32, ctx: u32) -> u32;
    pub fn peel(f: u32) -> u32;
    /// Returns 0 or 1.
    pub fn is_nop(f: u32) -> u32;

    // Tree access (context-filtered tree only; packed-string returns).
    pub fn tree_file(p: *const u8, l: u32) -> u64;
    pub fn tree_files(p: *const u8, l: u32) -> u64;
    pub fn tree_dirs(p: *const u8, l: u32) -> u64;
    pub fn tree_entry_oid(p: *const u8, l: u32) -> u64;

    // Invocation context (newline-joined `=arg,...` list; packed-string).
    pub fn invocation_args() -> u64;
}
