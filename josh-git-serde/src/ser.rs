use crate::error::SerdeGitError;
use crate::value::GitValue;
use crate::wire::{
    DATA_FIELD, EXTRA_FIELD_VARIANT_BASE, MARKER_FIELD_MAP, MARKER_FIELD_NEWTYPE_STRUCT,
    MARKER_FIELD_NEWTYPE_VARIANT, MARKER_FIELD_NONE, MARKER_FIELD_SEQ, MARKER_FIELD_SOME,
    MARKER_FIELD_STRUCT, MARKER_FIELD_STRUCT_VARIANT, MARKER_FIELD_TUPLE,
    MARKER_FIELD_TUPLE_STRUCT, MARKER_FIELD_TUPLE_VARIANT, MARKER_FIELD_UNIT,
    MARKER_FIELD_UNIT_STRUCT, MARKER_FIELD_UNIT_VARIANT, encode_key,
};
use serde::ser::{
    Impossible, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};
use std::collections::BTreeMap;

struct GitValueSerializer;

impl GitValueSerializer {
    fn new() -> Self {
        GitValueSerializer
    }
}

fn make_empty_marker(marker: &str) -> GitValue {
    GitValue::Tree(BTreeMap::from([(
        marker.to_string(),
        Box::new(GitValue::Blob(Default::default())),
    )]))
}

fn make_wrapper_from_entries(
    marker_name: &str,
    marker_data: Option<Vec<u8>>,
    data_name: &str,
    data_contents: BTreeMap<String, Box<GitValue>>,
) -> GitValue {
    let mut wrapper_entries = BTreeMap::from([(
        marker_name.to_string(),
        Box::new(GitValue::Blob(marker_data.unwrap_or_default())),
    )]);

    if !data_contents.is_empty() {
        wrapper_entries.insert(
            data_name.to_string(),
            Box::new(GitValue::Tree(data_contents)),
        );
    }

    GitValue::Tree(wrapper_entries)
}

impl Serializer for GitValueSerializer {
    type Ok = GitValue;
    type Error = SerdeGitError;

    type SerializeSeq = GitValueGenericSeqSerializer;
    type SerializeTuple = GitValueGenericSeqSerializer;
    type SerializeTupleStruct = GitValueTupleStructSerializer;
    type SerializeTupleVariant = GitValueTupleVariantSerializer;
    type SerializeMap = GitValueMapSerializer;
    type SerializeStruct = GitValueStructSerializer;
    type SerializeStructVariant = GitValueStructVariantSerializer;

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Blob(v.to_le_bytes().to_vec()))
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Blob(v.to_le_bytes().to_vec()))
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Blob(v.to_le_bytes().to_vec()))
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Blob(v.to_le_bytes().to_vec()))
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Blob(v.to_le_bytes().to_vec()))
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Blob(v.to_le_bytes().to_vec()))
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Blob(v.to_le_bytes().to_vec()))
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Blob(v.to_le_bytes().to_vec()))
    }

    fn serialize_f32(self, _v: f32) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_f32` not supported"))
    }

    fn serialize_f64(self, _v: f64) -> Result<Self::Ok, Self::Error> {
        Err(serde::ser::Error::custom("`serialize_f64` not supported"))
    }

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::blob_from_str(v.to_string()))
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::blob_from_str(v.to_string()))
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::blob_from_str(v))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Blob(v.to_vec()))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(make_empty_marker(MARKER_FIELD_NONE))
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize + ?Sized,
    {
        let value = value.serialize(GitValueSerializer::new())?;
        Ok(GitValue::Tree(BTreeMap::from([
            (
                MARKER_FIELD_SOME.to_string(),
                Box::new(GitValue::empty_blob()),
            ),
            (DATA_FIELD.to_string(), Box::new(value)),
        ])))
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(make_empty_marker(MARKER_FIELD_UNIT))
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Tree(BTreeMap::from([(
            MARKER_FIELD_UNIT_STRUCT.to_string(),
            Box::new(GitValue::blob_from_str(name)),
        )])))
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Ok(GitValue::Tree(BTreeMap::from([
            (
                EXTRA_FIELD_VARIANT_BASE.to_string(),
                Box::new(GitValue::blob_from_str(name)),
            ),
            (
                MARKER_FIELD_UNIT_VARIANT.to_string(),
                Box::new(GitValue::blob_from_str(variant)),
            ),
        ])))
    }

    fn serialize_newtype_struct<T>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize + ?Sized,
    {
        let value = value.serialize(GitValueSerializer::new())?;
        Ok(GitValue::Tree(BTreeMap::from([
            (
                MARKER_FIELD_NEWTYPE_STRUCT.to_string(),
                Box::new(GitValue::blob_from_str(name)),
            ),
            (DATA_FIELD.to_string(), Box::new(value)),
        ])))
    }

    fn serialize_newtype_variant<T>(
        self,
        name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize + ?Sized,
    {
        let value = value.serialize(GitValueSerializer::new())?;
        Ok(GitValue::Tree(BTreeMap::from([
            (
                EXTRA_FIELD_VARIANT_BASE.to_string(),
                Box::new(GitValue::blob_from_str(name)),
            ),
            (
                MARKER_FIELD_NEWTYPE_VARIANT.to_string(),
                Box::new(GitValue::blob_from_str(variant)),
            ),
            (DATA_FIELD.to_string(), Box::new(value)),
        ])))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(GitValueGenericSeqSerializer::new())
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(GitValueGenericSeqSerializer::new())
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(GitValueTupleStructSerializer::new(name))
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(GitValueTupleVariantSerializer::new(name, variant))
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(GitValueMapSerializer::default())
    }

    fn serialize_struct(
        self,
        name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(GitValueStructSerializer::new(name))
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(GitValueStructVariantSerializer::new(name, variant))
    }
}

struct GitValueGenericSeqSerializer {
    entries: Vec<GitValue>,
}

fn make_marker_from_size(
    size: usize,
    marker: &str,
) -> Result<(String, Box<GitValue>), SerdeGitError> {
    let size = u64::try_from(size)
        .map_err(|_| SerdeGitError("Failed to compose size field for sequence".to_string()))?;

    let marker_contents = size.to_le_bytes().to_vec();
    Ok((
        marker.to_string(),
        Box::new(GitValue::blob_from_str(marker_contents)),
    ))
}

fn make_indexed_contents(
    entries: Vec<GitValue>,
) -> Result<BTreeMap<String, Box<GitValue>>, SerdeGitError> {
    // Fixed-width zero-padded hex indices keep lexicographic name order
    // identical to numeric order, which is what the deserializer's
    // sequential walk relies on -- and they cap the entry count at u32::MAX.
    if entries.len() > u32::MAX as usize {
        return Err(SerdeGitError("Too many entries".to_string()));
    }

    Ok(entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| (format!("{:08x}", index), Box::new(entry)))
        .collect())
}

impl GitValueGenericSeqSerializer {
    fn new() -> Self {
        GitValueGenericSeqSerializer {
            entries: Vec::new(),
        }
    }

    fn seq_serialize_element<T>(&mut self, value: &T) -> Result<(), SerdeGitError>
    where
        T: Serialize + ?Sized,
    {
        let entry = value.serialize(GitValueSerializer::new())?;
        self.entries.push(entry);
        Ok(())
    }

    fn seq_end(self, marker: &str) -> Result<GitValue, SerdeGitError> {
        let marker_entry = make_marker_from_size(self.entries.len(), marker)?;

        // Empty containers carry no `data` entry, matching maps and structs.
        let mut entries = BTreeMap::from([marker_entry]);
        if !self.entries.is_empty() {
            let subtree_contents = make_indexed_contents(self.entries)?;
            entries.insert(
                DATA_FIELD.to_string(),
                Box::new(GitValue::Tree(subtree_contents)),
            );
        }

        Ok(GitValue::Tree(entries))
    }
}

impl SerializeSeq for GitValueGenericSeqSerializer {
    type Ok = GitValue;
    type Error = SerdeGitError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize + ?Sized,
    {
        self.seq_serialize_element(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.seq_end(MARKER_FIELD_SEQ)
    }
}

impl SerializeTuple for GitValueGenericSeqSerializer {
    type Ok = GitValue;
    type Error = SerdeGitError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize + ?Sized,
    {
        self.seq_serialize_element(value)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.seq_end(MARKER_FIELD_TUPLE)
    }
}

struct GitValueTupleStructSerializer {
    name: &'static str,
    entries: Vec<GitValue>,
}

impl GitValueTupleStructSerializer {
    fn new(name: &'static str) -> Self {
        GitValueTupleStructSerializer {
            name,
            entries: Vec::new(),
        }
    }
}

impl SerializeTupleStruct for GitValueTupleStructSerializer {
    type Ok = GitValue;
    type Error = SerdeGitError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize + ?Sized,
    {
        let entry = value.serialize(GitValueSerializer::new())?;
        self.entries.push(entry);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let subtree_contents = make_indexed_contents(self.entries)?;

        Ok(GitValue::Tree(BTreeMap::from([
            (
                MARKER_FIELD_TUPLE_STRUCT.to_string(),
                Box::new(GitValue::blob_from_str(self.name)),
            ),
            (
                DATA_FIELD.to_string(),
                Box::new(GitValue::Tree(subtree_contents)),
            ),
        ])))
    }
}

struct GitValueTupleVariantSerializer {
    name: &'static str,
    variant: &'static str,
    entries: Vec<GitValue>,
}

impl GitValueTupleVariantSerializer {
    fn new(name: &'static str, variant: &'static str) -> Self {
        GitValueTupleVariantSerializer {
            name,
            variant,
            entries: Vec::new(),
        }
    }
}

impl SerializeTupleVariant for GitValueTupleVariantSerializer {
    type Ok = GitValue;
    type Error = SerdeGitError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize + ?Sized,
    {
        let entry = value.serialize(GitValueSerializer::new())?;
        self.entries.push(entry);
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let subtree_contents = make_indexed_contents(self.entries)?;

        let mut entries = BTreeMap::from([
            (
                MARKER_FIELD_TUPLE_VARIANT.to_string(),
                Box::new(GitValue::blob_from_str(self.variant)),
            ),
            (
                EXTRA_FIELD_VARIANT_BASE.to_string(),
                Box::new(GitValue::blob_from_str(self.name)),
            ),
        ]);
        if !subtree_contents.is_empty() {
            entries.insert(
                DATA_FIELD.to_string(),
                Box::new(GitValue::Tree(subtree_contents)),
            );
        }

        Ok(GitValue::Tree(entries))
    }
}

#[derive(Default)]
struct GitValueMapSerializer {
    next_key: Option<String>,
    result: BTreeMap<String, Box<GitValue>>,
}

impl SerializeMap for GitValueMapSerializer {
    type Ok = GitValue;
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

        let encoded_key = encode_key(&key);
        let entry = value.serialize(GitValueSerializer::new())?;
        self.result.insert(encoded_key, Box::new(entry));

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let size = u64::try_from(self.result.len())
            .map_err(|_| SerdeGitError("Too many entries".to_string()))?;

        let marker_contents = size.to_le_bytes().to_vec();
        let contents = make_wrapper_from_entries(
            MARKER_FIELD_MAP,
            Some(marker_contents),
            DATA_FIELD,
            self.result,
        );

        Ok(contents)
    }
}

struct GitValueStructVariantSerializer {
    name: &'static str,
    variant: &'static str,
    fields: BTreeMap<String, Box<GitValue>>,
}

impl GitValueStructVariantSerializer {
    fn new(name: &'static str, variant: &'static str) -> Self {
        GitValueStructVariantSerializer {
            name,
            variant,
            fields: Default::default(),
        }
    }
}

impl SerializeStructVariant for GitValueStructVariantSerializer {
    type Ok = GitValue;
    type Error = SerdeGitError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize + ?Sized,
    {
        let entry = value.serialize(GitValueSerializer::new())?;
        // Field names share the map-key namespace and its filename-safe
        // encoding; the deserializer decodes both through `decode_key`.
        self.fields.insert(encode_key(key), Box::new(entry));
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let mut entries = BTreeMap::from([
            (
                MARKER_FIELD_STRUCT_VARIANT.to_string(),
                Box::new(GitValue::blob_from_str(self.variant)),
            ),
            (
                EXTRA_FIELD_VARIANT_BASE.to_string(),
                Box::new(GitValue::blob_from_str(self.name)),
            ),
        ]);
        if !self.fields.is_empty() {
            entries.insert(
                DATA_FIELD.to_string(),
                Box::new(GitValue::Tree(self.fields)),
            );
        }

        Ok(GitValue::Tree(entries))
    }
}

struct GitValueStructSerializer {
    name: &'static str,
    entries: BTreeMap<String, Box<GitValue>>,
}

impl GitValueStructSerializer {
    fn new(name: &'static str) -> Self {
        GitValueStructSerializer {
            name,
            entries: Default::default(),
        }
    }
}

impl SerializeStruct for GitValueStructSerializer {
    type Ok = GitValue;
    type Error = SerdeGitError;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize + ?Sized,
    {
        let entry = value.serialize(GitValueSerializer::new())?;
        // Same encoding as map keys: field names share their namespace and
        // the deserializer decodes both through `decode_key`.
        self.entries.insert(encode_key(key), Box::new(entry));
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let name = self.name.as_bytes().to_vec();
        let contents =
            make_wrapper_from_entries(MARKER_FIELD_STRUCT, Some(name), DATA_FIELD, self.entries);

        Ok(contents)
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
    value.serialize(GitValueSerializer::new())
}
