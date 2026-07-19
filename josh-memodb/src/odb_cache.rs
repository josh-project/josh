use crate::PassthroughHasher;
use libgit2_sys as raw;
use std::hash::{BuildHasherDefault, Hasher};

// In context of josh, most often per transaction
const OBJECT_CACHE_SIZE: usize = 300 * 1024 * 1024;

#[derive(Clone, Copy)]
struct ObjectCacheKey(raw::git_oid);

impl PartialEq<Self> for ObjectCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl From<raw::git_oid> for ObjectCacheKey {
    fn from(value: raw::git_oid) -> Self {
        ObjectCacheKey(value)
    }
}

impl Eq for ObjectCacheKey {}

impl std::hash::Hash for ObjectCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Bare write of the 20 raw bytes. `<[u8; 20]>::hash` would emit a length
        // prefix plus per-byte writes, which `PassthroughHasher` rejects (it
        // expects a single already-uniform digest). This mirrors `git2::Oid::hash`.
        state.write(&self.0.id);
    }
}

pub struct ObjectCache {
    data: lru::LruCache<
        ObjectCacheKey,
        (raw::git_object_t, Box<[u8]>),
        BuildHasherDefault<PassthroughHasher>,
    >,
    size: usize,
    target_size: usize,
}

impl Default for ObjectCache {
    fn default() -> Self {
        ObjectCache {
            data: lru::LruCache::unbounded_with_hasher(
                BuildHasherDefault::<PassthroughHasher>::new(),
            ),
            size: 0,
            target_size: OBJECT_CACHE_SIZE,
        }
    }
}

pub enum CacheObjectData<'a> {
    Allocated(Vec<u8>),
    Ref(&'a [u8]),
}

impl<'a> CacheObjectData<'a> {
    pub fn len(&self) -> usize {
        match self {
            CacheObjectData::Allocated(data) => data.len(),
            CacheObjectData::Ref(data) => data.len(),
        }
    }
}

impl ObjectCache {
    pub fn store(&mut self, id: raw::git_oid, kind: raw::git_object_t, data: CacheObjectData) {
        // Consider size of key too, even though it's not stored
        // exactly the same way; this is to avoid blowing
        // up the cache with lots of very small objects
        const KEY_SIZE: usize = size_of::<u64>();

        let len = data.len();
        let id = ObjectCacheKey::from(id);

        // Avoid double-lookup in the hashmap while at the same
        // time gating allocation triggered by .into() behind
        // existence check
        self.data.get_or_insert_mut_ref(&id, || {
            self.size += len + KEY_SIZE;

            let data = match data {
                CacheObjectData::Allocated(data) => data.into_boxed_slice(),
                CacheObjectData::Ref(data) => data.into(),
            };

            (kind, data)
        });

        while self.size > self.target_size
            && let Some((_, (_, data))) = self.data.pop_lru()
        {
            self.size -= data.len() + KEY_SIZE;
        }
    }

    pub fn load(&mut self, id: raw::git_oid) -> Option<(raw::git_object_t, &[u8])> {
        let id = ObjectCacheKey::from(id);
        self.data
            .get(&id)
            .map(|(object_type, data)| (*object_type, data.as_ref()))
    }
}
