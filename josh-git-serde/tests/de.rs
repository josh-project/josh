use josh_git_serde::{GitValue, from_value};
use serde::Deserialize;
use std::collections::BTreeMap;

#[test]
#[allow(dead_code)]
fn test_simple() -> anyhow::Result<()> {
    #[derive(Deserialize, Debug)]
    struct MyStruct {
        pub field1: String,
        pub field2: String,
    }

    let value = GitValue::Tree(BTreeMap::from([
        (
            "field1".to_string(),
            Box::new(GitValue::Blob(b"value1".to_vec())),
        ),
        (
            "field2".to_string(),
            Box::new(GitValue::Blob(b"value2".to_vec())),
        ),
    ]));

    let my_struct: MyStruct = from_value(&value)?;

    assert_eq!(
        format!("{my_struct:#?}"),
        "MyStruct {\n    field1: \"value1\",\n    field2: \"value2\",\n}"
    );

    Ok(())
}

#[test]
#[allow(dead_code)]
fn test_nested() -> anyhow::Result<()> {
    #[derive(Deserialize, Debug)]
    struct Inner {
        pub field1: String,
    }

    #[derive(Deserialize, Debug)]
    struct Outer {
        pub inner: Inner,
    }

    let inner = GitValue::Tree(BTreeMap::from([(
        "field1".to_string(),
        Box::new(GitValue::Blob(b"value1".to_vec())),
    )]));

    let value = GitValue::Tree(BTreeMap::from([("inner".to_string(), Box::new(inner))]));

    let outer: Outer = from_value(&value)?;

    assert_eq!(
        format!("{outer:#?}"),
        "Outer {\n    inner: Inner {\n        field1: \"value1\",\n    },\n}"
    );

    Ok(())
}

#[test]
#[allow(dead_code)]
fn test_number() -> anyhow::Result<()> {
    #[derive(Deserialize, Debug)]
    struct MyStruct {
        pub field1: u64,
    }

    let value = GitValue::Tree(BTreeMap::from([(
        "field1".to_string(),
        Box::new(GitValue::Blob(u64::MAX.to_le_bytes().to_vec())),
    )]));

    let my_struct: MyStruct = from_value(&value)?;

    assert_eq!(
        format!("{my_struct:#?}"),
        "MyStruct {\n    field1: 18446744073709551615,\n}"
    );

    Ok(())
}

#[test]
fn test_non_canonical_key_rejected() -> anyhow::Result<()> {
    // Two encodings of one key must be rejected, not collapsed into one.
    let value = GitValue::Tree(BTreeMap::from([
        ("a".to_string(), Box::new(GitValue::blob_from_str("one"))),
        ("%61".to_string(), Box::new(GitValue::blob_from_str("two"))),
    ]));

    let err = from_value::<BTreeMap<String, String>>(&value).unwrap_err();
    assert!(
        err.to_string().contains("invalid key encoding"),
        "unexpected error: {err}"
    );
    Ok(())
}

#[test]
fn test_enum_single_entry_rule() -> anyhow::Result<()> {
    #[derive(Deserialize, Debug, PartialEq)]
    enum TestEnum {
        A(String),
        B,
    }

    let err = from_value::<TestEnum>(&GitValue::Tree(BTreeMap::new())).unwrap_err();
    assert!(
        err.to_string()
            .contains("enum tree must have exactly one entry"),
        "unexpected error: {err}"
    );

    let two = GitValue::Tree(BTreeMap::from([
        ("A".to_string(), Box::new(GitValue::blob_from_str("x"))),
        ("B".to_string(), Box::new(GitValue::Tree(BTreeMap::new()))),
    ]));
    let err = from_value::<TestEnum>(&two).unwrap_err();
    assert!(
        err.to_string()
            .contains("enum tree must have exactly one entry"),
        "unexpected error: {err}"
    );

    let unknown = GitValue::Tree(BTreeMap::from([(
        "C".to_string(),
        Box::new(GitValue::Tree(BTreeMap::new())),
    )]));
    let err = from_value::<TestEnum>(&unknown).unwrap_err();
    assert!(
        err.to_string().contains("unknown variant"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn test_unit_from_empty_tree() -> anyhow::Result<()> {
    #[derive(Deserialize, Debug, PartialEq)]
    struct Unit;

    let empty = GitValue::Tree(BTreeMap::new());
    let _: () = from_value(&empty)?;
    let unit: Unit = from_value(&empty)?;
    assert_eq!(unit, Unit);

    let non_empty = GitValue::Tree(BTreeMap::from([(
        "x".to_string(),
        Box::new(GitValue::blob_from_str("v")),
    )]));
    let err = from_value::<()>(&non_empty).unwrap_err();
    assert!(
        err.to_string().contains("expected empty tree for unit"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn test_option_field_absent_vs_present() -> anyhow::Result<()> {
    #[derive(Deserialize, Debug, PartialEq)]
    struct WithOption {
        field: Option<String>,
    }

    // Absent entry decodes to None via serde's generated default.
    let absent = GitValue::Tree(BTreeMap::new());
    let parsed: WithOption = from_value(&absent)?;
    assert_eq!(parsed, WithOption { field: None });

    // A present empty blob is `Some("")`, distinct from absent.
    let present = GitValue::Tree(BTreeMap::from([(
        "field".to_string(),
        Box::new(GitValue::empty_blob()),
    )]));
    let parsed: WithOption = from_value(&present)?;
    assert_eq!(
        parsed,
        WithOption {
            field: Some(String::new())
        }
    );

    // A missing required field errors instead of defaulting.
    #[derive(Deserialize, Debug)]
    struct Required {
        #[allow(dead_code)]
        field: String,
    }
    let err = from_value::<Required>(&GitValue::Tree(BTreeMap::new())).unwrap_err();
    assert!(
        err.to_string().contains("missing field"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn test_seq_deserialize_unsupported() -> anyhow::Result<()> {
    let value = GitValue::Tree(BTreeMap::from([(
        "x".to_string(),
        Box::new(GitValue::blob_from_str("v")),
    )]));
    let err = from_value::<Vec<String>>(&value).unwrap_err();
    assert!(
        err.to_string().contains("sequences are not supported"),
        "unexpected error: {err}"
    );
    Ok(())
}
