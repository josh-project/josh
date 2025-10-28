use crate::cache::CacheBackend;
use crate::cache_sled::SledCacheBackend;
use crate::{JoshResult, filter};

pub struct CacheStack {
    backends: Vec<Box<dyn CacheBackend>>,
}

impl Default for CacheStack {
    fn default() -> Self {
        CacheStack::new().with_backend(SledCacheBackend::default())
    }
}

impl CacheStack {
    pub fn new() -> Self {
        Self {
            backends: Default::default(),
        }
    }

    /// Add a cache backend to existing [CacheStack] instance
    /// with builder-like pattern.
    /// The newly added backend will be queried _after_ the existing ones.
    pub fn with_backend<T: CacheBackend + 'static>(mut self, backend: T) -> Self {
        self.backends.push(Box::new(backend));
        self
    }

    /// Write a record to all cache backends.
    pub fn write_all(
        &self,
        filter: filter::Filter,
        from: git2::Oid,
        to: git2::Oid,
        sequence_number: u128,
    ) -> JoshResult<()> {
        for backend in &self.backends {
            backend.write(filter, from, to, sequence_number)?;
        }

        Ok(())
    }

    /// Try to read from the cache backend stack.
    ///
    /// When a record is found, it's propagated to the backends
    /// "below" it, meaning to the backends that were added earlier.
    /// This behaviour can be used for example to provide faster
    /// ephemeral cache alongside slower persistent one.
    pub fn read_propagate(
        &self,
        filter: filter::Filter,
        from: git2::Oid,
        sequence_number: u128,
    ) -> JoshResult<Option<git2::Oid>> {
        let values = self
            .backends
            .iter()
            .enumerate()
            .find_map(
                |(index, backend)| match backend.read(filter, from, sequence_number) {
                    Ok(None) => None,
                    Ok(Some(oid)) => Some(Ok((index, oid))),
                    Err(e) => Some(Err(e)),
                },
            );

        let (index, oid) = match values {
            // None of the backends had the value
            None => return Ok(None),
            // Some backend encountered error
            Some(Err(e)) => return Err(e),
            Some(Ok(value)) => value,
        };

        // Propagate value to "lower" backends if found in "higher" ones
        self.backends
            .iter()
            .take(index)
            .try_for_each(|backend| backend.write(filter, from, oid, sequence_number))?;

        Ok(Some(oid))
    }
}
