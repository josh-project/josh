use josh_compose_runtime::Runtime;

use crate::CleanMode;
use crate::job_cache;
use crate::naming;

pub fn clean(mode: CleanMode, runtime: &dyn Runtime) -> anyhow::Result<()> {
    let out_vols = runtime.list_artifacts(naming::OUTPUT_PREFIX)?;
    for vol in out_vols {
        eprintln!("[clean] removing volume: {vol}");
        runtime.remove_artifact(&vol, true)?;
    }

    job_cache::clean();

    let images = runtime.list_envs(naming::ENV_PREFIX)?;
    for image in images {
        eprintln!("[clean] removing image: {image}");
        runtime.remove_env(&image)?;
    }

    if mode == CleanMode::CleanAll {
        let cache_vols = runtime.list_artifacts(naming::CACHE_PREFIX)?;
        for vol in cache_vols {
            eprintln!("[clean] removing cache volume: {vol}");
            runtime.remove_artifact(&vol, false)?;
        }
    }

    Ok(())
}
