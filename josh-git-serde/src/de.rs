use std::collections::BTreeMap;

use serde::de::{self, DeserializeSeed, IntoDeserializer, Visitor};
use serde::forward_to_deserialize_any;

use crate::error::SerdeGitError;
use crate::value::GitValue;
use crate::wire;

type TreeContents = BTreeMap<String, Box<GitValue>>;

struct TreeDeserializer<'a> {
    tree: &'a TreeContents,
}

impl<'a> TreeDeserializer<'a> {
    fn from_tree(tree: &'a TreeContents) -> Self {
        TreeDeserializer { tree }
    }
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
        // `None` is the absent entry, never a tree: a tree in this position
        // is always `Some` payload.
        visitor.visit_some(self)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if !self.tree.is_empty() {
            return Err(SerdeGitError("expected empty tree for unit".to_string()));
        }
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
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        // Newtypes are transparent: the tree is the inner value.
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(SerdeGitError("sequences are not supported".to_string()))
    }

    fn deserialize_tuple<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(SerdeGitError("tuples are not supported".to_string()))
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
        visitor.visit_map(TreeMapAccess::new(self.tree))
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
        // Structs and maps share the same flat-tree shape; absent entries
        // surface as serde's own missing-field/`None` handling.
        visitor.visit_map(TreeMapAccess::new(self.tree))
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
        let mut entries = self.tree.iter();
        let (encoded_name, payload) = entries.next().ok_or_else(|| {
            SerdeGitError("enum tree must have exactly one entry; found none".to_string())
        })?;
        if entries.next().is_some() {
            return Err(SerdeGitError(
                "enum tree must have exactly one entry".to_string(),
            ));
        }

        let name = wire::decode_key(encoded_name)?;
        visitor.visit_enum(TreeEnumAccess::new(payload, name))
    }

    forward_to_deserialize_any! {
        // Scalar reads happen on blob payloads via BlobDeserializer; a tree
        // carries only maps/structs, options, newtypes and enums.
        // Floating point is unsupported everywhere.
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char string byte_buf str bytes
        identifier ignored_any
    }
}

struct TreeEnumAccess<'de> {
    payload: &'de GitValue,
    name: String,
}

impl<'de> TreeEnumAccess<'de> {
    fn new(payload: &'de GitValue, name: String) -> Self {
        TreeEnumAccess { payload, name }
    }
}

impl<'de> de::EnumAccess<'de> for TreeEnumAccess<'de> {
    type Error = SerdeGitError;
    type Variant = TreeVariantAccess<'de>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let variant = TreeVariantAccess::new(self.payload);

        seed.deserialize(self.name.into_deserializer())
            .map(|v| (v, variant))
    }
}

struct TreeVariantAccess<'de> {
    payload: &'de GitValue,
}

impl<'de> TreeVariantAccess<'de> {
    fn new(payload: &'de GitValue) -> Self {
        TreeVariantAccess { payload }
    }
}

impl<'de> de::VariantAccess<'de> for TreeVariantAccess<'de> {
    type Error = SerdeGitError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        match self.payload {
            GitValue::Tree(tree) if tree.is_empty() => Ok(()),
            _ => Err(SerdeGitError(
                "unit variant payload must be an empty tree".to_string(),
            )),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.payload {
            GitValue::Blob(bytes) => seed.deserialize(BlobDeserializer::from_blob(bytes)),
            GitValue::Tree(tree) => seed.deserialize(TreeDeserializer::from_tree(tree)),
        }
    }

    fn tuple_variant<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(SerdeGitError(
            "tuple variants are not supported".to_string(),
        ))
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.payload {
            GitValue::Tree(tree) => visitor.visit_map(TreeMapAccess::new(tree)),
            _ => Err(SerdeGitError(
                "struct variant payload must be a tree".to_string(),
            )),
        }
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

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        // `None` is the absent entry, never a blob: a blob in this position
        // is always `Some` payload.
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        // Newtypes are transparent: the blob is the inner value.
        visitor.visit_newtype_struct(self)
    }

    forward_to_deserialize_any! {
        // Floating point is not supported
        f32 f64
        // Containers are handled by TreeDeserializer
        unit_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

pub fn from_value<'de, T: serde::Deserialize<'de>>(
    value: &'de GitValue,
) -> Result<T, SerdeGitError> {
    // Either shape is a valid root; the target type must match what
    // `to_value` wrote.
    match value {
        GitValue::Tree(tree) => T::deserialize(TreeDeserializer::from_tree(tree)),
        GitValue::Blob(bytes) => T::deserialize(BlobDeserializer::from_blob(bytes)),
    }
}
