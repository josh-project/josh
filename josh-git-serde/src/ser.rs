use crate::error::SerdeGitError;
use crate::value::GitValue;
use crate::wire::encode_key;
use serde::ser::{
    Impossible, Serialize, SerializeMap, SerializeStruct, SerializeStructVariant, Serializer,
};
use std::collections::BTreeMap;

/// `None` has no representation of its own; `Absent` lets the parent drop
/// the entry (struct field, map value) or reject it where omission is
/// impossible (root, enum payload, nested `None`).
enum Node {
    Value(GitValue),
    Absent,
}

struct GitValueSerializer;

impl GitValueSerializer {
    fn new() -> Self {
        GitValueSerializer
    }
}

impl Serializer for GitValueSerializer {
    type Ok = Node;
    type Error = SerdeGitError;

    type SerializeSeq = Impossible<Node, SerdeGitError>;
    type SerializeTuple = Impossible<Node, SerdeGitError>;
    type SerializeTupleStruct = Impossible<Node, SerdeGitError>;
    type SerializeTupleVariant = Impossible<Node, SerdeGitError>;
    type SerializeMap = GitValueMapSerializer;
    type SerializeStruct = GitValueStructSerializer;
    type SerializeStructVariant = GitValueStructVariantSerializer;

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Blob(v.to_le_bytes().to_vec())))
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Blob(v.to_le_bytes().to_vec())))
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Blob(v.to_le_bytes().to_vec())))
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Blob(v.to_le_bytes().to_vec())))
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Blob(v.to_le_bytes().to_vec())))
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Blob(v.to_le_bytes().to_vec())))
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Blob(v.to_le_bytes().to_vec())))
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Blob(v.to_le_bytes().to_vec())))
    }

    fn serialize_f32(self, _v: f32) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_f32` not supported"))
    }

    fn serialize_f64(self, _v: f64) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_f64` not supported"))
    }

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::blob_from_str(v.to_string())))
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::blob_from_str(v.to_string())))
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::blob_from_str(v)))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Blob(v.to_vec())))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Absent)
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize + ?Sized,
    {
        match value.serialize(GitValueSerializer::new())? {
            Node::Absent => Err(serde::ser::Error::custom(
                "`serialize_some` cannot serialize nested `None`",
            )),
            node => Ok(node),
        }
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Tree(BTreeMap::new())))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Tree(BTreeMap::new())))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Tree(BTreeMap::from([(
            encode_key(variant),
            Box::new(GitValue::Tree(BTreeMap::new())),
        )]))))
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(GitValueSerializer::new())
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize + ?Sized,
    {
        match value.serialize(GitValueSerializer::new())? {
            Node::Absent => Err(serde::ser::Error::custom(
                "`serialize_newtype_variant` cannot serialize `None` payload",
            )),
            Node::Value(inner) => Ok(Node::Value(GitValue::Tree(BTreeMap::from([(
                encode_key(variant),
                Box::new(inner),
            )])))),
        }
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_seq` not supported"))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_tuple` not supported"))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(serde::ser::Error::custom(
            "`serialize_tuple_struct` not supported",
        ))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(serde::ser::Error::custom(
            "`serialize_tuple_variant` not supported",
        ))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(GitValueMapSerializer::default())
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(GitValueStructSerializer::default())
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(GitValueStructVariantSerializer::new(variant))
    }
}

#[derive(Default)]
struct GitValueMapSerializer {
    next_key: Option<String>,
    result: BTreeMap<String, Box<GitValue>>,
}

impl SerializeMap for GitValueMapSerializer {
    type Ok = Node;
    type Error = SerdeGitError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: Serialize + ?Sized,
    {
        let key = key.serialize(StringSerializer)?;
        self.next_key = Some(key);
        Ok(())
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize + ?Sized,
    {
        let key = self
            .next_key
            .take()
            .expect("serialize_value without serialize_key");

        // `None` values omit the entry entirely; the key is still consumed.
        if let Node::Value(entry) = value.serialize(GitValueSerializer::new())? {
            self.result.insert(encode_key(&key), Box::new(entry));
        }

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Tree(self.result)))
    }
}

struct GitValueStructVariantSerializer {
    variant: &'static str,
    fields: BTreeMap<String, Box<GitValue>>,
}

impl GitValueStructVariantSerializer {
    fn new(variant: &'static str) -> Self {
        GitValueStructVariantSerializer {
            variant,
            fields: Default::default(),
        }
    }
}

impl SerializeStructVariant for GitValueStructVariantSerializer {
    type Ok = Node;
    type Error = SerdeGitError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize + ?Sized,
    {
        // Field names share the map-key namespace; the deserializer decodes
        // both through `decode_key`.
        if let Node::Value(entry) = value.serialize(GitValueSerializer::new())? {
            self.fields.insert(encode_key(key), Box::new(entry));
        }
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        // Always wrapped, even with zero fields: an all-`None` struct variant
        // intentionally degenerates to the unit-variant shape.
        Ok(Node::Value(GitValue::Tree(BTreeMap::from([(
            encode_key(self.variant),
            Box::new(GitValue::Tree(self.fields)),
        )]))))
    }
}

#[derive(Default)]
struct GitValueStructSerializer {
    entries: BTreeMap<String, Box<GitValue>>,
}

impl SerializeStruct for GitValueStructSerializer {
    type Ok = Node;
    type Error = SerdeGitError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize + ?Sized,
    {
        // Field names share the map-key namespace; the deserializer decodes
        // both through `decode_key`.
        if let Node::Value(entry) = value.serialize(GitValueSerializer::new())? {
            self.entries.insert(encode_key(key), Box::new(entry));
        }
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(Node::Value(GitValue::Tree(self.entries)))
    }
}

pub(crate) struct StringSerializer;

impl Serializer for StringSerializer {
    type Ok = String;
    type Error = SerdeGitError;

    type SerializeSeq = Impossible<String, SerdeGitError>;
    type SerializeTuple = Impossible<String, SerdeGitError>;
    type SerializeTupleStruct = Impossible<String, SerdeGitError>;
    type SerializeTupleVariant = Impossible<String, SerdeGitError>;
    type SerializeMap = Impossible<String, SerdeGitError>;
    type SerializeStruct = Impossible<String, SerdeGitError>;
    type SerializeStructVariant = Impossible<String, SerdeGitError>;

    fn serialize_bool(self, _v: bool) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_bool` not supported"))
    }

    fn serialize_i8(self, _v: i8) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_i8` not supported"))
    }

    fn serialize_i16(self, _v: i16) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_i16` not supported"))
    }

    fn serialize_i32(self, _v: i32) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_i32` not supported"))
    }

    fn serialize_i64(self, _v: i64) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_i64` not supported"))
    }

    fn serialize_u8(self, _v: u8) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_u8` not supported"))
    }

    fn serialize_u16(self, _v: u16) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_u16` not supported"))
    }

    fn serialize_u32(self, _v: u32) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_u32` not supported"))
    }

    fn serialize_u64(self, _v: u64) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_u64` not supported"))
    }

    fn serialize_f32(self, _v: f32) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_f32` not supported"))
    }

    fn serialize_f64(self, _v: f64) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_f64` not supported"))
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        Ok(value.to_string())
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        Ok(value.to_owned())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_bytes` not supported"))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_none` not supported"))
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_unit` not supported"))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom(
            "`serialize_unit_struct` not supported",
        ))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom(
            "`serialize_unit_variant` not supported",
        ))
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize + ?Sized,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize + ?Sized,
    {
        Err(serde::ser::Error::custom(
            "`serialize_newtype_variant` not supported",
        ))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_seq` not supported"))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_tuple` not supported"))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(serde::ser::Error::custom(
            "`serialize_tuple_struct` not supported",
        ))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(serde::ser::Error::custom(
            "`serialize_tuple_variant` not supported",
        ))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_map` not supported"))
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(serde::ser::Error::custom(
            "`serialize_struct` not supported",
        ))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(serde::ser::Error::custom(
            "`serialize_struct_variant` not supported",
        ))
    }
}

pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<GitValue, SerdeGitError> {
    match value.serialize(GitValueSerializer::new())? {
        Node::Value(value) => Ok(value),
        Node::Absent => Err(serde::ser::Error::custom(
            "`None` at the top level has no representation",
        )),
    }
}
