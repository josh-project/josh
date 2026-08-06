//! Parse/spec and persist round-trips for `Op::Wasm`.
//!
//! Every test enables the experimental gate before its first parse, so the
//! gate's LazyLock always initializes to enabled in this binary regardless of
//! test order. The gate-disabled error message is tested in `wasm_gate.rs`,
//! which runs as a separate process.

use josh_filter::flang::parse::parse;
use josh_filter::persist::{as_tree, from_tree};
use josh_filter::{Filter, Op, spec};

fn enable_experimental() {
    // SAFETY: test-only; all tests in this binary set the same value before
    // the gate's LazyLock is first read.
    unsafe { std::env::set_var("JOSH_EXPERIMENTAL_FEATURES", "1") };
}

#[test]
fn parse_spec_roundtrip() {
    enable_experimental();
    for canonical in [
        ":!st/config[:empty]",
        ":!st/config[:/sub]",
        ":!tools/mod=a,b[::st/]",
        ":!st/config=x[::st/]",
        ":!\"a b\"[:/sub]",
        ":!tools/mod=\"a,b\",c[:/sub]",
    ] {
        let filter = parse(canonical).unwrap();
        assert_eq!(spec(filter), canonical, "spec mismatch for {}", canonical);
        assert_eq!(
            parse(&spec(filter)).unwrap(),
            filter,
            "parse(spec(F)) != F for {}",
            canonical
        );
    }
}

#[test]
fn parse_structure() {
    enable_experimental();
    let filter = parse(":!tools/mod=a,b[::st/]").unwrap();
    match josh_filter::persist::to_op(filter) {
        Op::Wasm(path, args, _sub) => {
            assert_eq!(path, std::path::PathBuf::from("tools/mod"));
            assert_eq!(args, vec!["a".to_string(), "b".to_string()]);
        }
        other => panic!("expected Op::Wasm, got {:?}", other),
    }

    // No args: empty args vector, and an empty group means an empty context.
    let filter = parse(":!st/config[]").unwrap();
    match josh_filter::persist::to_op(filter) {
        Op::Wasm(path, args, _sub) => {
            assert_eq!(path, std::path::PathBuf::from("st/config"));
            assert!(args.is_empty());
        }
        other => panic!("expected Op::Wasm, got {:?}", other),
    }
}

#[test]
fn bracketless_context_is_optional() {
    enable_experimental();
    // The [context] group is optional; omitting it means an empty context,
    // exactly like an empty group.
    assert_eq!(
        parse(":!tools/mod").unwrap(),
        parse(":!tools/mod[]").unwrap()
    );
    assert_eq!(
        parse(":!tools/mod=a,b").unwrap(),
        parse(":!tools/mod=a,b[]").unwrap()
    );
    // The documented chain form.
    let filter = parse(":/sub:!tools/mod").unwrap();
    assert_eq!(filter, parse(":/sub:!tools/mod[]").unwrap());
    // The canonical (bracketed) spec round-trips.
    assert_eq!(parse(&spec(filter)).unwrap(), filter);
}

#[test]
fn builder_matches_parser() {
    enable_experimental();
    let built = Filter::new()
        .wasm(
            "tools/mod",
            vec!["a".to_string(), "b".to_string()],
            parse(":/sub").unwrap(),
        )
        .unwrap();
    assert_eq!(built, parse(":!tools/mod=a,b[:/sub]").unwrap());
}

#[test]
fn persist_roundtrip() {
    enable_experimental();
    let td = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init_bare(td.path()).unwrap();
    for canonical in [
        ":!st/config[:empty]",
        ":!st/config[:/sub]",
        ":!tools/mod=a,b[::st/]",
    ] {
        let filter = parse(canonical).unwrap();
        let tree_oid = as_tree(&repo, filter).unwrap();
        let restored = from_tree(&repo, tree_oid).unwrap();
        assert_eq!(
            spec(restored),
            spec(filter),
            "persist round-trip failed for {}",
            canonical
        );
    }
}
