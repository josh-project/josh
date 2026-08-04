//! The wasm filter is experimental-gated. This lives in its own test binary
//! (= its own process) because the gate is a process-wide LazyLock and
//! `wasm_op.rs` needs it enabled.

#[test]
fn wasm_requires_experimental_gate() {
    // SAFETY: test-only; this is the only test in this binary, and it runs
    // before the gate's LazyLock is first read.
    unsafe { std::env::remove_var("JOSH_EXPERIMENTAL_FEATURES") };
    let err = josh_filter::flang::parse::parse(":!st/config[]").unwrap_err();
    assert!(
        err.to_string()
            .contains("Wasm filter requires JOSH_EXPERIMENTAL_FEATURES=1"),
        "unexpected error: {}",
        err
    );
}
