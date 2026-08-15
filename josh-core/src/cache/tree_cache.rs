use josh_memodb::PassthroughHasher;
use std::hash::BuildHasherDefault;

/// Raw bytes of a tree object, either straight from the odb (first read, zero-copy) or from the
/// per-transaction tree cache (repeated reads). Derefs to the byte slice either way.
pub enum TreeBytes<'a> {
    Odb(git2::OdbObject<'a>),
    Cached(std::sync::Arc<[u8]>),
}

impl std::ops::Deref for TreeBytes<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            TreeBytes::Odb(obj) => obj.data(),
            TreeBytes::Cached(bytes) => bytes,
        }
    }
}

/// Cache key routing the oid digest into [`PassthroughHasher`]'s single-`write` path:
/// `git2::Oid`'s own `Hash` impl hashes its bytes as a slice, whose `write_usize` length
/// prefix the hasher rejects.
#[derive(PartialEq, Eq)]
struct OidKey(git2::Oid);

impl std::hash::Hash for OidKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(self.0.as_bytes());
    }
}

type OidMap<V> = std::collections::HashMap<OidKey, V, BuildHasherDefault<PassthroughHasher>>;
type OidSet = std::collections::HashSet<OidKey, BuildHasherDefault<PassthroughHasher>>;

/// Per-transaction cache of raw tree bytes, standing in for the parsed-object cache libgit2
/// kept behind `find_tree` (which the gix-object tree ops no longer go through). See
/// [`super::Transaction::read_tree_bytes`] for the read path.
///
/// Policy: a tree is only copied into the cache on its second read
/// ([`should_promote`](Self::should_promote)), so once-read trees cost nothing extra; trees are
/// content-addressed so cached entries never go stale; the cache is dropped wholesale when it
/// outgrows [`LIMIT`](Self::LIMIT).
#[derive(Default)]
pub(crate) struct TreeCache {
    map: OidMap<std::sync::Arc<[u8]>>,
    bytes: usize,
    seen: OidSet,
}

impl TreeCache {
    const LIMIT: usize = 64 * 1024 * 1024;

    pub(crate) fn get(&self, oid: git2::Oid) -> Option<std::sync::Arc<[u8]>> {
        self.map.get(&OidKey(oid)).cloned()
    }

    /// Whether `oid` is being read for the second time and should be copied into the cache
    /// now; the first read is only recorded, so it can hand out the odb buffer zero-copy.
    pub(crate) fn should_promote(&mut self, oid: git2::Oid) -> bool {
        !self.seen.insert(OidKey(oid))
    }

    pub(crate) fn insert(&mut self, oid: git2::Oid, bytes: std::sync::Arc<[u8]>) {
        if self.bytes > Self::LIMIT {
            self.map.clear();
            self.bytes = 0;
        }
        self.bytes += bytes.len();
        self.map.insert(OidKey(oid), bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(n: u8) -> git2::Oid {
        git2::Oid::from_bytes(&[n; 20]).unwrap()
    }

    // The first read is only recorded; the second read promotes, and only then does the cache
    // hold the bytes.
    #[test]
    fn promotes_on_second_read_only() {
        let mut cache = TreeCache::default();
        assert!(!cache.should_promote(oid(1)));
        assert!(cache.get(oid(1)).is_none());
        assert!(cache.should_promote(oid(1)));
        cache.insert(oid(1), b"tree bytes"[..].into());
        assert_eq!(&*cache.get(oid(1)).unwrap(), b"tree bytes");
    }

    // Growing past the budget drops the whole cache before the next insert, which itself
    // stays cached.
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
