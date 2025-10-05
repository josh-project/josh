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

    pub fn with_backend<T: CacheBackend + 'static>(mut self, backend: T) -> Self {
        self.backends.push(Box::new(backend));
        self
    }

    pub fn write_all(
        &self,
        filter: filter::Filter,
        from: git2::Oid,
        to: git2::Oid,
    ) -> JoshResult<()> {
        for backend in &self.backends {
            backend.write(filter, from, to)?;
        }

        Ok(())
    }

    // when reading,
    pub fn read_propagate(
        &self,
        filter: filter::Filter,
        from: git2::Oid,
    ) -> JoshResult<Option<git2::Oid>> {
        let values = self
            .backends
            .iter()
            .enumerate()
            .find_map(|(index, backend)| match backend.read(filter, from) {
                Ok(None) => None,
                Ok(Some(oid)) => Some(Ok((index, oid))),
                Err(e) => Some(Err(e)),
            });

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
            .try_for_each(|backend| backend.write(filter, from, oid))?;

        Ok(Some(oid))
    }
}
