use std::hash::Hasher;

/// A hasher that uses the first 8 bytes of a SHA1 hash directly as the hash value.
/// This is designed to be used with HashMaps where the key is already a SHA1 hash,
/// e.g. Filter or Oid, avoiding double-hashing.
#[derive(Default)]
pub struct PassthroughHasher {
    buffer: [u8; 8],
    done: bool,
}

impl Hasher for PassthroughHasher {
    fn finish(&self) -> u64 {
        if !self.done {
            panic!("not enough data supplied to hasher")
        }

        u64::from_le_bytes(self.buffer)
    }

    // hyper-specialized: reject everything that's not sha1 length
    fn write(&mut self, bytes: &[u8]) {
        if self.done {
            panic!("hasher data already written");
        }

        // skip length prefix
        // FIXME remove this when https://github.com/rust-lang/rust/issues/96762 is stabilized
        if bytes.len() == size_of::<usize>() {
            return;
        }

        if bytes.len() != 20 {
            panic!("unexpected data length {} in hasher", bytes.len())
        }

        self.buffer.as_mut().copy_from_slice(&bytes[..8]);
        self.done = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passthrough_hasher() {
        let mut hasher = PassthroughHasher::default();
        let sha1: [u8; 20] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14,
        ];

        hasher.write(&sha1);

        // First 8 bytes as little-endian u64
        let expected = u64::from_le_bytes([0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
        assert_eq!(hasher.finish(), expected);
    }

    #[test]
    fn test_hashmap_with_filter() {
        use crate::filter::Filter;
        use std::collections::HashMap;
        use std::hash::BuildHasherDefault;

        type FilterHashMap<V> = HashMap<Filter, V, BuildHasherDefault<PassthroughHasher>>;
        let mut map: FilterHashMap<String> = HashMap::default();

        let filter1 = Filter::new().subdir("a");
        let filter2 = Filter::new().subdir("b");
        let filter3 = Filter::new().subdir("a"); // same as filter1

        map.insert(filter1.clone(), "value1".to_string());
        map.insert(filter2.clone(), "value2".to_string());

        assert_eq!(map.get(&filter1), Some(&"value1".to_string()));
        assert_eq!(map.get(&filter2), Some(&"value2".to_string()));
        assert_eq!(map.get(&filter3), Some(&"value1".to_string())); // should match filter1
        assert_eq!(map.len(), 2);
    }
}
