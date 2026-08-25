use josh_memodb::PassthroughHasher;
use std::hash::BuildHasherDefault;

/// Raw bytes of a tree object, either as the odb decompressed them (first read) or from the
/// per-transaction tree cache (repeated reads). Derefs to the byte slice either way.
pub enum TreeBytes {
    Odb(Vec<u8>),
    Cached(std::sync::Arc<[u8]>),
}

impl std::ops::Deref for TreeBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            TreeBytes::Odb(data) => data,
            TreeBytes::Cached(bytes) => bytes,
        }
    }
}

type OidMap<V> =
    std::collections::HashMap<gix_hash::ObjectId, V, BuildHasherDefault<PassthroughHasher>>;
type OidSet = std::collections::HashSet<gix_hash::ObjectId, BuildHasherDefault<PassthroughHasher>>;

/// Raw tree bytes cached after their second read.
///
/// Entries never go stale because trees are content-addressed. The cache resets when it
/// exceeds [`LIMIT`](Self::LIMIT).
#[derive(Default)]
pub(crate) struct TreeCache {
    map: OidMap<std::sync::Arc<[u8]>>,
    bytes: usize,
    seen: OidSet,
}

impl TreeCache {
    const LIMIT: usize = 64 * 1024 * 1024;

    pub(crate) fn get(&self, oid: gix_hash::ObjectId) -> Option<std::sync::Arc<[u8]>> {
        self.map.get(&oid).cloned()
    }

    /// Return true from the second read onward.
    pub(crate) fn should_promote(&mut self, oid: gix_hash::ObjectId) -> bool {
        !self.seen.insert(oid)
    }

    pub(crate) fn insert(&mut self, oid: gix_hash::ObjectId, bytes: std::sync::Arc<[u8]>) {
        if self.bytes > Self::LIMIT {
            self.map.clear();
            self.bytes = 0;
        }
        self.bytes += bytes.len();
        self.map.insert(oid, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(n: u8) -> gix_hash::ObjectId {
        gix_hash::ObjectId::from_bytes_or_panic(&[n; 20])
    }

    #[test]
    fn promotes_on_second_read_only() {
        let mut cache = TreeCache::default();
        assert!(!cache.should_promote(oid(1)));
        assert!(cache.get(oid(1)).is_none());
        assert!(cache.should_promote(oid(1)));
        cache.insert(oid(1), b"tree bytes"[..].into());
        assert_eq!(&*cache.get(oid(1)).unwrap(), b"tree bytes");
    }

    #[test]
    fn clears_wholesale_over_limit() {
        let mut cache = TreeCache::default();
        cache.insert(oid(1), b"x"[..].into());
        cache.bytes = TreeCache::LIMIT + 1;
        cache.insert(oid(2), b"y"[..].into());
        assert!(cache.get(oid(1)).is_none());
        assert_eq!(&*cache.get(oid(2)).unwrap(), b"y");
        assert_eq!(cache.bytes, 1);
    }
}
