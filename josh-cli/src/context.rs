use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use josh_core::cache::{
    CacheStack, DistributedCacheBackend, SledCacheBackend, Transaction, TransactionContext,
};

#[derive(Debug, Clone, Copy)]
pub struct CachePolicy {
    distributed: bool,
}

impl CachePolicy {
    pub fn new(distributed: bool) -> Self {
        Self { distributed }
    }

    pub fn distributed(self) -> bool {
        self.distributed
    }

    fn stack(self, common_dir: &Path, sled: bool) -> anyhow::Result<Arc<CacheStack>> {
        let mut stack = CacheStack::new();
        if sled {
            stack = stack.with_backend(SledCacheBackend::new(common_dir));
        }
        if self.distributed {
            stack = stack.with_backend(
                DistributedCacheBackend::new(common_dir)
                    .context("Failed to create DistributedCacheBackend")?,
            );
        }
        Ok(Arc::new(stack))
    }

    pub fn open(
        self,
        repository: &Path,
        common_dir: &Path,
        sled: bool,
        ephemeral: bool,
    ) -> anyhow::Result<ExecutionContext> {
        let mut context = TransactionContext::new(repository, self.stack(common_dir, sled)?);
        if ephemeral {
            context = context.ephemeral();
        } else {
            context = context.with_mem_odb_limit(crate::MAX_MEM_PACK_SIZE);
        }
        let transaction = context.open().context("Failed TransactionContext::open")?;
        Ok(ExecutionContext {
            transaction,
            cache_policy: self,
        })
    }
}

pub struct ExecutionContext {
    transaction: Transaction,
    cache_policy: CachePolicy,
}

impl ExecutionContext {
    pub fn transaction(&self) -> &Transaction {
        &self.transaction
    }

    pub fn cache_policy(&self) -> CachePolicy {
        self.cache_policy
    }
}
