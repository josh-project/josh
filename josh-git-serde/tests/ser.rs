use josh_git_serde::{GitValue, from_value, to_value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn blob(s: &str) -> Box<GitValue> {
    Box::new(GitValue::blob_from_str(s))
}

#[test]
fn test_simple_struct() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct TestStruct {
        a: String,
        b: String,
    }

    let value = TestStruct {
        a: "content of a".to_string(),
        b: "content of b".to_string(),
    };

    let result = to_value(&value)?;

    let expected = GitValue::Tree(BTreeMap::from([
        ("a".to_string(), blob("content of a")),
        ("b".to_string(), blob("content of b")),
    ]));
    assert_eq!(result, expected);

    let roundtrip: TestStruct = from_value(&result)?;
    assert_eq!(roundtrip, value);

    Ok(())
}

#[test]
fn test_map() -> anyhow::Result<()> {
    let mut map = BTreeMap::new();
    map.insert("a".to_string(), "content of a".to_string());
    map.insert("b".to_string(), "content of b".to_string());

    let result = to_value(&map)?;

    // Same flat shape as a struct: the schema, not the tree, says "map".
    let expected = GitValue::Tree(BTreeMap::from([
        ("a".to_string(), blob("content of a")),
        ("b".to_string(), blob("content of b")),
    ]));
    assert_eq!(result, expected);

    let roundtrip: BTreeMap<String, String> = from_value(&result)?;
    assert_eq!(roundtrip, map);

    Ok(())
}

#[test]
fn test_struct_variant() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    enum TestEnum {
        A { value: String },
    }

    let value = TestEnum::A {
        value: "content of a".to_string(),
    };

    let result = to_value(&value)?;

    let expected = GitValue::Tree(BTreeMap::from([(
        "A".to_string(),
        Box::new(GitValue::Tree(BTreeMap::from([(
            "value".to_string(),
            blob("content of a"),
        )]))),
    )]));
    assert_eq!(result, expected);

    let roundtrip: TestEnum = from_value(&result)?;
    assert_eq!(roundtrip, value);

    Ok(())
}

#[test]
fn test_unit_and_unit_struct_roundtrip() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Unit;

    // Both are the same empty tree; the target type carries the meaning.
    let empty = GitValue::Tree(BTreeMap::new());
    assert_eq!(to_value(&())?, empty);
    assert_eq!(to_value(&Unit)?, empty);

    let _: () = from_value(&empty)?;
    let _: Unit = from_value(&empty)?;
    Ok(())
}

#[test]
fn test_empty_containers_roundtrip() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Empty {}

    let _: Empty = from_value(&to_value(&Empty {})?)?;
    let _: BTreeMap<String, String> = from_value(&to_value(&BTreeMap::<String, String>::new())?)?;
    Ok(())
}

#[test]
fn test_scalar_roots_roundtrip() -> anyhow::Result<()> {
    assert_eq!(from_value::<String>(&to_value("hello")?)?, "hello");
    assert_eq!(from_value::<u64>(&to_value(&42u64)?)?, 42);
    Ok(())
}

#[test]
fn test_borrowed_str_field() -> anyhow::Result<()> {
    #[derive(Deserialize)]
    struct Borrowed<'a> {
        s: &'a str,
    }

    // Same shape the serializer writes; constructed by hand so the test
    // isolates the borrow path of the deserializer.
    let expected = GitValue::Tree(BTreeMap::from([("s".to_string(), blob("borrowed"))]));
    let parsed: Borrowed = from_value(&expected)?;
    assert_eq!(parsed.s, "borrowed");
    Ok(())
}

#[test]
fn test_renamed_field_with_escape_roundtrip() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Renamed {
        #[serde(rename = "a%b")]
        field: String,
    }

    let original = Renamed { field: "v".into() };
    assert_eq!(from_value::<Renamed>(&to_value(&original)?)?, original);
    Ok(())
}

#[test]
fn test_map_key_encoding_roundtrip() -> anyhow::Result<()> {
    // Keys that exercise every escape class: separators, spaces, dots,
    // literal percent signs (including percent + hex digits).
    let original: BTreeMap<String, String> = [
        ("a/b", "path"),
        ("a b", "space"),
        ("a.b", "dot"),
        ("100%", "percent"),
        ("%61", "looks like an escape"),
        ("a%2Fb", "escape-like sequence"),
        ("键", "non-ascii"),
        ("-", "_"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    assert_eq!(
        from_value::<BTreeMap<String, String>>(&to_value(&original)?)?,
        original
    );
    Ok(())
}

#[test]
fn test_none_root_and_nested_option_error() -> anyhow::Result<()> {
    #[derive(Serialize)]
    enum TestEnum {
        A(Option<String>),
    }

    let err = to_value(&None::<String>).unwrap_err();
    assert!(
        err.to_string()
            .contains("`None` at the top level has no representation"),
        "unexpected error: {err}"
    );

    let err = to_value(&Some(None::<String>)).unwrap_err();
    assert!(
        err.to_string()
            .contains("`serialize_some` cannot serialize nested `None`"),
        "unexpected error: {err}"
    );

    let err = to_value(&TestEnum::A(None)).unwrap_err();
    assert!(
        err.to_string().contains("cannot serialize `None` payload"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn test_seq_tuple_unsupported() -> anyhow::Result<()> {
    #[derive(Serialize)]
    struct Pair(String, u32);

    let err = to_value(&vec!["a".to_string()]).unwrap_err();
    assert!(
        err.to_string().contains("`serialize_seq` not supported"),
        "unexpected error: {err}"
    );

    let err = to_value(&("a".to_string(), 1u32)).unwrap_err();
    assert!(
        err.to_string().contains("`serialize_tuple` not supported"),
        "unexpected error: {err}"
    );

    let err = to_value(&Pair("a".to_string(), 1)).unwrap_err();
    assert!(
        err.to_string()
            .contains("`serialize_tuple_struct` not supported"),
        "unexpected error: {err}"
    );

    Ok(())
}

#[test]
fn test_none_field_omission_roundtrip() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct WithOption {
        present: String,
        absent: Option<String>,
    }

    let value = WithOption {
        present: "p".to_string(),
        absent: None,
    };

    let result = to_value(&value)?;

    let expected = GitValue::Tree(BTreeMap::from([("present".to_string(), blob("p"))]));
    assert_eq!(result, expected);

    let roundtrip: WithOption = from_value(&result)?;
    assert_eq!(roundtrip, value);

    // Map values follow the same omission rule.
    let mut map = BTreeMap::new();
    map.insert("present".to_string(), Some("p".to_string()));
    map.insert("absent".to_string(), None);

    let result = to_value(&map)?;
    assert_eq!(result, expected);

    let roundtrip: BTreeMap<String, Option<String>> = from_value(&result)?;
    assert_eq!(
        roundtrip,
        BTreeMap::from([("present".to_string(), Some("p".to_string()))])
    );

    Ok(())
}

#[test]
fn test_some_empty_string_field_roundtrip() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct WithOption {
        field: Option<String>,
    }

    // `Some("")` and `None` must stay distinguishable on the wire.
    let some_empty = WithOption {
        field: Some(String::new()),
    };
    let none = WithOption { field: None };

    let some_value = to_value(&some_empty)?;
    let none_value = to_value(&none)?;

    assert_eq!(
        some_value,
        GitValue::Tree(BTreeMap::from([("field".to_string(), blob(""))]))
    );
    assert_eq!(none_value, GitValue::Tree(BTreeMap::new()));
    assert_ne!(some_value, none_value);

    assert_eq!(from_value::<WithOption>(&some_value)?, some_empty);
    assert_eq!(from_value::<WithOption>(&none_value)?, none);

    Ok(())
}

#[test]
fn test_unit_variant_and_newtype_variant_shape() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    enum TestEnum {
        Unit,
        Newtype(String),
    }

    let unit = to_value(&TestEnum::Unit)?;
    assert_eq!(
        unit,
        GitValue::Tree(BTreeMap::from([(
            "Unit".to_string(),
            Box::new(GitValue::Tree(BTreeMap::new())),
        )]))
    );
    assert_eq!(from_value::<TestEnum>(&unit)?, TestEnum::Unit);

    let newtype = to_value(&TestEnum::Newtype("payload".to_string()))?;
    assert_eq!(
        newtype,
        GitValue::Tree(BTreeMap::from([("Newtype".to_string(), blob("payload"))]))
    );
    assert_eq!(
        from_value::<TestEnum>(&newtype)?,
        TestEnum::Newtype("payload".to_string())
    );

    Ok(())
}
