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
            "struct".to_string(),
            Box::new(GitValue::Blob(b"MyStruct".to_vec())),
        ),
        (
            "data".to_string(),
            Box::new(GitValue::Tree(BTreeMap::from([
                (
                    "field1".to_string(),
                    Box::new(GitValue::Blob(b"value1".to_vec())),
                ),
                (
                    "field2".to_string(),
                    Box::new(GitValue::Blob(b"value2".to_vec())),
                ),
            ]))),
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
        "data".to_string(),
        Box::new(GitValue::Tree(BTreeMap::from([(
            "field1".to_string(),
            Box::new(GitValue::Blob(b"value1".to_vec())),
        )]))),
    )]));

    let value = GitValue::Tree(BTreeMap::from([
        (
            "struct".to_string(),
            Box::new(GitValue::Blob(b"Outer".to_vec())),
        ),
        (
            "data".to_string(),
            Box::new(GitValue::Tree(BTreeMap::from([(
                "inner".to_string(),
                Box::new(inner),
            )]))),
        ),
    ]));

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

    let value = GitValue::Tree(BTreeMap::from([
        (
            "struct".to_string(),
            Box::new(GitValue::Blob(b"MyStruct".to_vec())),
        ),
        (
            "data".to_string(),
            Box::new(GitValue::Tree(BTreeMap::from([(
                "field1".to_string(),
                Box::new(GitValue::Blob(u64::MAX.to_le_bytes().to_vec())),
            )]))),
        ),
    ]));

    let my_struct: MyStruct = from_value(&value)?;

    assert_eq!(
        format!("{my_struct:#?}"),
        "MyStruct {\n    field1: 18446744073709551615,\n}"
    );

    Ok(())
}

#[test]
#[allow(dead_code)]
fn test_vec_of_string() -> anyhow::Result<()> {
    #[derive(Deserialize, Debug)]
    struct MyStruct {
        pub field1: Vec<String>,
    }

    let field1_contents: BTreeMap<String, Box<GitValue>> = BTreeMap::from([
        (
            "seq".to_string(),
            Box::new(GitValue::Blob(2u64.to_le_bytes().to_vec())),
        ),
        (
            "data".to_string(),
            Box::new(GitValue::Tree(BTreeMap::from([
                (
                    "00000000".to_string(),
                    Box::new(GitValue::Blob(b"value1".to_vec())),
                ),
                (
                    "00000001".to_string(),
                    Box::new(GitValue::Blob(b"value2".to_vec())),
                ),
            ]))),
        ),
    ]);

    let value = GitValue::Tree(BTreeMap::from([
        (
            "struct".to_string(),
            Box::new(GitValue::Blob(b"MyStruct".to_vec())),
        ),
        (
            "data".to_string(),
            Box::new(GitValue::Tree(BTreeMap::from([(
                "field1".to_string(),
                Box::new(GitValue::Tree(field1_contents)),
            )]))),
        ),
    ]));

    let my_struct: MyStruct = from_value(&value)?;

    assert_eq!(
        format!("{my_struct:#?}"),
        "MyStruct {\n    field1: [\n        \"value1\",\n        \"value2\",\n    ],\n}"
    );

    Ok(())
}

#[test]
fn test_non_canonical_key_rejected() -> anyhow::Result<()> {
    use std::collections::BTreeMap;

    // A hand-crafted tree whose entries decode to the same key: `a` and
    // `%61`. The strict decoder must refuse it instead of collapsing data.
    let value = GitValue::Tree(BTreeMap::from([
        (
            "map".to_string(),
            Box::new(GitValue::Blob(2u64.to_le_bytes().to_vec())),
        ),
        (
            "data".to_string(),
            Box::new(GitValue::Tree(BTreeMap::from([
                ("a".to_string(), Box::new(GitValue::blob_from_str("one"))),
                ("%61".to_string(), Box::new(GitValue::blob_from_str("two"))),
            ]))),
        ),
    ]));

    let err = from_value::<BTreeMap<String, String>>(&value).unwrap_err();
    assert!(
        err.to_string().contains("invalid key encoding"),
        "unexpected error: {err}"
    );
    Ok(())
}
