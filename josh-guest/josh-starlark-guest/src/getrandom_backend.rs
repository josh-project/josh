//! Deterministic `getrandom` backend stub.
//!
//! `getrandom` arrives transitively (starlark → pagable → sorted_vector_map →
//! quickcheck → rand) and refuses to compile for wasm32-unknown-unknown
//! without a backend. The build selects the "custom" backend
//! (`--cfg getrandom_backend="custom"`, see `josh-guest/.cargo/config.toml`),
//! which resolves to this symbol. Filter evaluation must be a pure function
//! of its inputs, so the only correct "randomness" is a constant: hash seeds
//! and the like become fixed, which is exactly what the evaluation cache
//! requires.

#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    // SAFETY: the caller (getrandom) passes a valid buffer of `len` bytes.
    unsafe { core::ptr::write_bytes(dest, 0, len) };
    Ok(())
}
