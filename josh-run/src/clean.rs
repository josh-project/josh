use crate::CleanMode;
use crate::job_cache;
use crate::podman;

pub fn clean(mode: CleanMode) -> anyhow::Result<()> {
    // Remove out_* volumes
    let out_vols = podman::list_volumes_with_prefix("out_")?;
    for vol in out_vols {
        eprintln!("[clean] removing volume: {vol}");
        podman::volume_rm_force(&vol)?;
    }

    // Remove .josh/success and .josh/failed cache directories
    job_cache::clean();

    // Remove ws_image_* images
    let images = podman::list_images_with_prefix("ws_image_")?;
    for image in images {
        eprintln!("[clean] removing image: {image}");
        podman::rmi(&image)?;
    }

    if mode == CleanMode::CleanAll {
        let cache_vols = podman::list_volumes_with_prefix("")?
            .into_iter()
            .filter(|v| v.ends_with("_cache"))
            .collect::<Vec<_>>();
        for vol in cache_vols {
            eprintln!("[clean] removing cache volume: {vol}");
            podman::volume_rm(&vol)?;
        }
    }

    Ok(())
}
