use std::collections::BTreeMap;

use serde::de::{self, DeserializeSeed, IntoDeserializer, Visitor};
use serde::forward_to_deserialize_any;

use crate::error::SerdeGitError;
use crate::value::GitValue;
use crate::wire::{
    self, DATA_FIELD, MARKER_FIELD_MAP, MARKER_FIELD_NEWTYPE_VARIANT, MARKER_FIELD_NONE,
    MARKER_FIELD_SEQ, MARKER_FIELD_SOME, MARKER_FIELD_STRUCT_VARIANT, MARKER_FIELD_TUPLE,
    MARKER_FIELD_TUPLE_VARIANT, MARKER_FIELD_UNIT, MARKER_FIELD_UNIT_STRUCT,
    MARKER_FIELD_UNIT_VARIANT,
};

type TreeContents = BTreeMap<String, Box<GitValue>>;

struct TreeDeserializer<'a> {
    tree: &'a TreeContents,
}

impl<'a> TreeDeserializer<'a> {
    fn from_tree(tree: &'a TreeContents) -> Self {
        TreeDeserializer { tree }
    }
}

fn find_data_entry(tree_contents: &TreeContents) -> Result<&GitValue, SerdeGitError> {
    let value = tree_contents
        .get(DATA_FIELD)
        .ok_or_else(|| SerdeGitError("missing 'data' entry".to_string()))?;

    Ok(value)
}

fn find_data_subtree(tree_contents: &TreeContents) -> Result<&TreeContents, SerdeGitError> {
    let contents = find_data_entry(tree_contents)?;

    if let GitValue::Tree(contents) = contents {
        Ok(contents)
    } else {
        Err(SerdeGitError("invalid 'data' entry".to_string()))
    }
}

fn find_marker<'a>(
    tree_contents: &'a TreeContents,
    marker: &str,
) -> Result<&'a [u8], SerdeGitError> {
    let contents = tree_contents
        .get(marker)
        .ok_or_else(|| SerdeGitError(format!("missing marker `{marker}`")))?;

    let data = match contents.as_ref() {
        GitValue::Blob(data) => data,
        _ => return Err(SerdeGitError("marker must be a blob".to_string())),
    };

    Ok(data.as_slice())
}

fn size_from_marker(tree_contents: &TreeContents, marker: &str) -> Result<usize, SerdeGitError> {
    let data = find_marker(tree_contents, marker)?;
    let size = u64::from_le_bytes(
        data.try_into()
            .map_err(|_| SerdeGitError("invalid size marker".to_string()))?,
    ) as usize;

    Ok(size)
}

impl<'de> de::Deserializer<'de> for TreeDeserializer<'de> {
    type Error = SerdeGitError;

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(SerdeGitError(
            "TreeDeserializer: not implemented".to_string(),
        ))
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if find_marker(self.tree, MARKER_FIELD_NONE).is_ok() {
            visitor.visit_none()
        } else if let (Ok(_), Ok(entry)) = (
            find_marker(self.tree, MARKER_FIELD_SOME),
            find_data_entry(self.tree),
        ) {
            match entry {
                GitValue::Blob(bytes) => visitor.visit_some(BlobDeserializer::from_blob(bytes)),
                GitValue::Tree(tree) => visitor.visit_some(TreeDeserializer::from_tree(tree)),
            }
        } else {
            Err(SerdeGitError("missing optional kind marker".to_string()))
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        find_marker(self.tree, MARKER_FIELD_UNIT)?;
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        find_marker(self.tree, MARKER_FIELD_UNIT_STRUCT)?;
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let entry = find_data_entry(self.tree)?;

        match entry {
            GitValue::Blob(bytes) => {
                visitor.visit_newtype_struct(BlobDeserializer::from_blob(bytes))
            }
            GitValue::Tree(tree) => visitor.visit_newtype_struct(TreeDeserializer::from_tree(tree)),
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let size = size_from_marker(self.tree, MARKER_FIELD_SEQ)?;

        if size == 0 {
            visitor.visit_seq(TreeEmptySeqAccess {})
        } else {
            let data_contents = find_data_subtree(self.tree)?;
            visitor.visit_seq(TreeSeqAccess::new(data_contents))
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if size_from_marker(self.tree, MARKER_FIELD_TUPLE)? == 0 {
            visitor.visit_seq(TreeEmptySeqAccess {})
        } else {
            visitor.visit_seq(TreeSeqAccess::new(find_data_subtree(self.tree)?))
        }
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let size = size_from_marker(self.tree, MARKER_FIELD_MAP)?;

        if size == 0 {
            visitor.visit_map(TreeEmptyMapAccess {})
        } else {
            let data_contents = find_data_subtree(self.tree)?;
            visitor.visit_map(TreeMapAccess::new(data_contents))
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        // Fieldless structs are written without a `data` entry, like all
        // empty containers.
        match self.tree.get(DATA_FIELD).map(Box::as_ref) {
            None => visitor.visit_map(TreeEmptyMapAccess {}),
            Some(GitValue::Tree(data_contents)) => {
                visitor.visit_map(TreeMapAccess::new(data_contents))
            }
            Some(_) => Err(SerdeGitError("`struct` data must be a tree".to_string())),
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let variant = find_marker(self.tree, MARKER_FIELD_STRUCT_VARIANT)
            .or_else(|_| find_marker(self.tree, MARKER_FIELD_UNIT_VARIANT))
            .or_else(|_| find_marker(self.tree, MARKER_FIELD_NEWTYPE_VARIANT))
            .or_else(|_| find_marker(self.tree, MARKER_FIELD_TUPLE_VARIANT))?;

        let variant = std::str::from_utf8(variant)
            .map_err(|_| SerdeGitError("invalid variant marker".to_string()))?;
        visitor.visit_enum(TreeEnumAccess::new(self.tree, variant))
    }

    forward_to_deserialize_any! {
        // A tree carries only containers, options, newtypes, enums and unit
        // markers; scalar reads happen on blob payloads via BlobDeserializer.
        // Floating point is unsupported everywhere.
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char string byte_buf str bytes
        // Not supported
        identifier ignored_any
    }
}

struct TreeEnumAccess<'de> {
    tree: &'de TreeContents,
    name: &'de str,
}

impl<'de> TreeEnumAccess<'de> {
    fn new(tree: &'de TreeContents, name: &'de str) -> Self {
        TreeEnumAccess { tree, name }
    }
}

impl<'de> de::EnumAccess<'de> for TreeEnumAccess<'de> {
    type Error = SerdeGitError;
    type Variant = TreeVariantAccess<'de>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let name = self.name.to_string();

        seed.deserialize(name.into_deserializer())
            .map(|v| (v, TreeVariantAccess::new(self.tree)))
    }
}

struct TreeVariantAccess<'de> {
    tree: &'de TreeContents,
}

impl<'de> TreeVariantAccess<'de> {
    fn new(tree: &'de TreeContents) -> Self {
        TreeVariantAccess { tree }
    }
}

impl<'de> de::VariantAccess<'de> for TreeVariantAccess<'de> {
    type Error = SerdeGitError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        let entry = find_data_entry(self.tree)?;

        match entry {
            GitValue::Blob(bytes) => seed.deserialize(BlobDeserializer::from_blob(bytes)),
            GitValue::Tree(tree) => seed.deserialize(TreeDeserializer::from_tree(tree)),
        }
    }

    // The variant payload is a bare indexed map -- no `seq` marker of its
    // own -- so it is walked directly, like struct variant fields.
    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.tree.get(DATA_FIELD).map(Box::as_ref) {
            None => visitor.visit_seq(TreeEmptySeqAccess {}),
            Some(GitValue::Tree(data)) => visitor.visit_seq(TreeSeqAccess::new(data)),
            Some(_) => Err(SerdeGitError(
                "tuple variant payload must be a tree".to_string(),
            )),
        }
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        // Fieldless variants are written without a `data` entry, like all
        // empty containers.
        match self.tree.get(DATA_FIELD).map(Box::as_ref) {
            None => visitor.visit_map(TreeEmptyMapAccess {}),
            Some(GitValue::Tree(data)) => visitor.visit_map(TreeMapAccess::new(data)),
            Some(_) => Err(SerdeGitError(
                "struct variant payload must be a tree".to_string(),
            )),
        }
    }
}

struct TreeEmptyMapAccess {}

impl<'de> de::MapAccess<'de> for TreeEmptyMapAccess {
    type Error = SerdeGitError;

    fn next_key_seed<K>(&mut self, _seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        Ok(None)
    }

    fn next_value_seed<V>(&mut self, _seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        Err(SerdeGitError("unexpected value".to_string()))
    }
}

struct TreeMapAccess<'de> {
    iter: std::collections::btree_map::Iter<'de, String, Box<GitValue>>,
    value: Option<&'de GitValue>,
}

impl<'de> TreeMapAccess<'de> {
    fn new(tree: &'de TreeContents) -> Self {
        TreeMapAccess {
            iter: tree.iter(),
            value: None,
        }
    }
}

impl<'de> de::MapAccess<'de> for TreeMapAccess<'de> {
    type Error = SerdeGitError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        let (encoded_key, value) = match self.iter.next() {
            Some(entry) => entry,
            None => return Ok(None),
        };

        let key = wire::decode_key(encoded_key)?;

        self.value = Some(value.as_ref());
        seed.deserialize(key.into_deserializer()).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let value = match self.value {
            Some(entry) => entry,
            None => return Err(SerdeGitError("missing value".to_string())),
        };

        match value {
            GitValue::Blob(bytes) => seed.deserialize(BlobDeserializer::from_blob(bytes)),
            GitValue::Tree(tree) => seed.deserialize(TreeDeserializer::from_tree(tree)),
        }
    }
}

struct TreeEmptySeqAccess {}

impl<'de> de::SeqAccess<'de> for TreeEmptySeqAccess {
    type Error = SerdeGitError;

    fn next_element_seed<T>(&mut self, _seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        Ok(None)
    }
}

struct TreeSeqAccess<'de> {
    iter: std::collections::btree_map::Iter<'de, String, Box<GitValue>>,
    index: usize,
}

impl<'de> TreeSeqAccess<'de> {
    fn new(tree: &'de TreeContents) -> Self {
        TreeSeqAccess {
            iter: tree.iter(),
            index: 0,
        }
    }
}

impl<'de> de::SeqAccess<'de> for TreeSeqAccess<'de> {
    type Error = SerdeGitError;

    // Zero-padded hex index names keep stored order equal to numeric order.
    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        let (name, entry) = match self.iter.next() {
            Some(entry) => entry,
            None => return Ok(None),
        };

        let expected_name = format!("{:08x}", self.index);
        if name.as_str() != expected_name {
            return Err(SerdeGitError("sequence entry name mismatch".to_string()));
        }

        self.index += 1;
        let result = match entry.as_ref() {
            GitValue::Blob(bytes) => seed.deserialize(BlobDeserializer::from_blob(bytes)),
            GitValue::Tree(tree) => seed.deserialize(TreeDeserializer::from_tree(tree)),
        };

        result.map(Some)
    }
}

struct BlobDeserializer<'de> {
    blob: &'de [u8],
}

impl<'de> BlobDeserializer<'de> {
    fn from_blob(blob: &'de [u8]) -> Self {
        BlobDeserializer { blob }
    }

    fn try_str(&self) -> Result<&'de str, SerdeGitError> {
        std::str::from_utf8(self.blob).map_err(|_| SerdeGitError("invalid string blob".to_string()))
    }
}

impl<'de> de::Deserializer<'de> for BlobDeserializer<'de> {
    type Error = SerdeGitError;

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(SerdeGitError(
            "BlobDeserializer: can't `deserialize_any`; schema change?".to_string(),
        ))
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.blob.len() == 1 {
            let mut bytes = [0u8; 1];
            bytes.copy_from_slice(&self.blob[0..1]);
            let value = u8::from_le_bytes(bytes);

            visitor.visit_u8(value)
        } else {
            Err(SerdeGitError("invalid u8 blob".to_string()))
        }
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.blob.len() == 2 {
            let mut bytes = [0u8; 2];
            bytes.copy_from_slice(&self.blob[0..2]);
            let value = u16::from_le_bytes(bytes);

            visitor.visit_u16(value)
        } else {
            Err(SerdeGitError("invalid u16 blob".to_string()))
        }
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.blob.len() == 4 {
            let mut bytes = [0u8; 4];
            bytes.copy_from_slice(&self.blob[0..4]);
            let value = u32::from_le_bytes(bytes);

            visitor.visit_u32(value)
        } else {
            Err(SerdeGitError("invalid u32 blob".to_string()))
        }
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.blob.len() == 8 {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&self.blob[0..8]);
            let value = u64::from_le_bytes(bytes);

            visitor.visit_u64(value)
        } else {
            Err(SerdeGitError("invalid u64 blob".to_string()))
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.blob.len() == 1 {
            let mut bytes = [0u8; 1];
            bytes.copy_from_slice(&self.blob[0..1]);
            let value = i8::from_le_bytes(bytes);

            visitor.visit_i8(value)
        } else {
            Err(SerdeGitError("invalid i8 blob".to_string()))
        }
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.blob.len() == 2 {
            let mut bytes = [0u8; 2];
            bytes.copy_from_slice(&self.blob[0..2]);
            let value = i16::from_le_bytes(bytes);

            visitor.visit_i16(value)
        } else {
            Err(SerdeGitError("invalid i16 blob".to_string()))
        }
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.blob.len() == 4 {
            let mut bytes = [0u8; 4];
            bytes.copy_from_slice(&self.blob[0..4]);
            let value = i32::from_le_bytes(bytes);

            visitor.visit_i32(value)
        } else {
            Err(SerdeGitError("invalid i32 blob".to_string()))
        }
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.blob.len() == 8 {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&self.blob[0..8]);
            let value = i64::from_le_bytes(bytes);

            visitor.visit_i64(value)
        } else {
            Err(SerdeGitError("invalid i64 blob".to_string()))
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = self.try_str()?;
        let value = value
            .parse::<bool>()
            .map_err(|_| SerdeGitError("invalid bool blob".to_string()))?;
        visitor.visit_bool(value)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let value = self.try_str()?;
        let value = value
            .parse::<char>()
            .map_err(|_| SerdeGitError("invalid char blob".to_string()))?;
        visitor.visit_char(value)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let result = self.try_str()?;
        visitor.visit_borrowed_str(result)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let result = self.try_str()?;
        visitor.visit_string(result.to_string())
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_borrowed_bytes(self.blob)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_byte_buf(self.blob.to_vec())
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    forward_to_deserialize_any! {
        // Floating point is not supported
        f32 f64
        // Containers are handled by tree deserializer
        option unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

pub fn from_value<'de, T: serde::Deserialize<'de>>(
    value: &'de GitValue,
) -> Result<T, SerdeGitError> {
    // Top-level scalars are blobs; containers and markers are trees. Both
    // are valid roots -- the shape must simply match what `to_value` wrote.
    match value {
        GitValue::Tree(tree) => T::deserialize(TreeDeserializer::from_tree(tree)),
        GitValue::Blob(bytes) => T::deserialize(BlobDeserializer::from_blob(bytes)),
    }
}
