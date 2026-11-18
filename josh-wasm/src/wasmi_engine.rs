//! The wasmi implementation of the engine traits: a fuel- and memory-limited
//! store per evaluation, all `"josh"` imports linked, then
//! `josh_abi_version` (must be 1) followed by `josh_run`.

use crate::engine::{CompiledModule, Engine, EvalContext, Limits};
use crate::host::{self, HostState};
use std::sync::Arc;

pub(crate) struct WasmiEngine {
    engine: wasmi::Engine,
}

impl WasmiEngine {
    pub(crate) fn new() -> WasmiEngine {
        let mut config = wasmi::Config::default();
        config.consume_fuel(true);
        WasmiEngine {
            engine: wasmi::Engine::new(&config),
        }
    }
}

impl Engine for WasmiEngine {
    fn load(&self, bytes: &[u8], _limits: &Limits) -> anyhow::Result<Arc<dyn CompiledModule>> {
        let module = wasmi::Module::new(&self.engine, bytes)?;
        Ok(Arc::new(WasmiModule {
            engine: self.engine.clone(),
            module,
        }))
    }
}

struct WasmiModule {
    engine: wasmi::Engine,
    module: wasmi::Module,
}

impl CompiledModule for WasmiModule {
    fn evaluate(
        &self,
        ctx: EvalContext<'_>,
        limits: &Limits,
    ) -> anyhow::Result<josh_filter::Filter> {
        let state = HostState::new(&ctx, limits);
        let mut store = wasmi::Store::new(&self.engine, state);
        store.set_fuel(limits.fuel)?;
        store.limiter(|state| state.limiter());
        let mut linker = wasmi::Linker::new(&self.engine);
        host::add_to_linker(&mut linker)?;
        let instance = linker.instantiate_and_start(&mut store, &self.module)?;
        let abi_version = instance
            .get_typed_func::<(), u32>(&store, "josh_abi_version")?
            .call(&mut store, ())?;
        anyhow::ensure!(
            abi_version == 1,
            "unsupported josh ABI version: {}",
            abi_version
        );
        let handle = instance
            .get_typed_func::<(), u32>(&store, "josh_run")?
            .call(&mut store, ())?;
        store.data().handle(handle).ok_or_else(|| {
            anyhow::anyhow!("josh_run returned an invalid filter handle: {}", handle)
        })
    }
}
