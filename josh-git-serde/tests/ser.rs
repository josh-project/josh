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
        (
            "data".to_string(),
            Box::new(GitValue::Tree(BTreeMap::from([
                ("a".to_string(), blob("content of a")),
                ("b".to_string(), blob("content of b")),
            ]))),
        ),
        ("struct".to_string(), blob("TestStruct")),
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

    let expected = GitValue::Tree(BTreeMap::from([
        (
            "data".to_string(),
            Box::new(GitValue::Tree(BTreeMap::from([
                ("a".to_string(), blob("content of a")),
                ("b".to_string(), blob("content of b")),
            ]))),
        ),
        (
            "map".to_string(),
            Box::new(GitValue::Blob(2u64.to_le_bytes().to_vec())),
        ),
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

    let expected = GitValue::Tree(BTreeMap::from([
        (
            "data".to_string(),
            Box::new(GitValue::Tree(BTreeMap::from([(
                "value".to_string(),
                blob("content of a"),
            )]))),
        ),
        ("struct_variant".to_string(), blob("A")),
        ("variant_base".to_string(), blob("TestEnum")),
    ]));
    assert_eq!(result, expected);

    let roundtrip: TestEnum = from_value(&result)?;
    assert_eq!(roundtrip, value);

    Ok(())
}

#[test]
fn test_unit_and_unit_struct_roundtrip() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Unit;

    // Distinct wire shapes (`unit` vs `unit_struct` marker); each decodes
    // back to its own type.
    let _: () = from_value(&to_value(&())?)?;
    let _: Unit = from_value(&to_value(&Unit)?)?;
    Ok(())
}

#[test]
fn test_empty_containers_roundtrip() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Empty {}

    // Every empty container carries its own marker and no `data` entry.
    let _: Empty = from_value(&to_value(&Empty {})?)?;
    let _: BTreeMap<String, String> = from_value(&to_value(&BTreeMap::<String, String>::new())?)?;
    let _: Vec<String> = from_value(&to_value(&Vec::<String>::new())?)?;
    let _: (String,) = from_value(&to_value(&(String::new(),))?)?;
    Ok(())
}

#[test]
fn test_tuple_variant_roundtrip() -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    enum E {
        Pair(u32, String),
        Empty,
    }

    for original in [E::Pair(7, "seven".into()), E::Empty] {
        assert_eq!(from_value::<E>(&to_value(&original)?)?, original);
    }
    Ok(())
}

#[test]
fn test_scalar_roots_roundtrip() -> anyhow::Result<()> {
    assert_eq!(from_value::<String>(&to_value("hello")?)?, "hello");
    assert_eq!(from_value::<u64>(&to_value(&42u64)?)?, 42);
    assert_eq!(
        from_value::<Vec<u8>>(&to_value(&vec![1u8, 2])?)?,
        vec![1, 2]
    );
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
    let expected = GitValue::Tree(BTreeMap::from([
        (
            "data".to_string(),
            Box::new(GitValue::Tree(BTreeMap::from([(
                "s".to_string(),
                blob("borrowed"),
            )]))),
        ),
        ("struct".to_string(), blob("Borrowed")),
    ]));
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
