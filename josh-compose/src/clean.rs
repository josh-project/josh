use std::collections::{HashMap, HashSet};

use anyhow::Context;
use josh_compose_backend::{ArtifactBackend, EnvironmentBackend, Runtime, StorageStatus};

use crate::CleanMode;
use crate::job_cache;
use crate::naming;
use crate::plan;

const CLEANUP_TRIGGER_PERCENT: u64 = 90;
const CLEANUP_TARGET_PERCENT: u64 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ResourceKind {
    Environment,
    Output,
}

#[derive(Debug, PartialEq, Eq)]
struct Candidate {
    last_used: i64,
    kind: ResourceKind,
    name: String,
}

/// Reclaim local compose outputs and images in least-recently-used order when
/// Podman's storage filesystem reaches the high-water mark.
pub fn reclaim_if_needed(
    transaction: &josh_core::cache::Transaction,
    ws_tree: gix_hash::ObjectId,
    runtime: &dyn Runtime,
) -> anyhow::Result<()> {
    let Some(status) = runtime.storage_status()? else {
        return Ok(());
    };
    validate_status(status)?;
    if !usage_at_least(status, CLEANUP_TRIGGER_PERCENT) {
        return Ok(());
    }

    let odb = transaction.odb();
    let protected_outputs: HashSet<_> =
        plan::collect_job_hashes(transaction, odb, ws_tree, true, runtime)?
            .into_iter()
            .collect();
    let protected_images: HashSet<_> =
        plan::collect_image_oids(transaction, odb, ws_tree, false, runtime)?
            .into_iter()
            .collect();

    let usage = job_cache::job_usage(transaction)?;
    let mut image_usage = HashMap::new();
    for (job, timestamp) in usage.execution {
        match plan::collect_workspace_image_oids(transaction, odb, job) {
            Ok(images) => {
                for image in images {
                    update_timestamp(&mut image_usage, image, timestamp);
                }
            }
            Err(error) => {
                log::debug!("cannot resolve images used by compose job {job}: {error}");
            }
        }
    }

    let mut candidates = Vec::new();
    for name in runtime.list_envs(naming::ENV_PREFIX)? {
        let oid = naming::env_oid(&name);
        if oid.is_some_and(|oid| protected_images.contains(&oid)) {
            continue;
        }
        candidates.push(Candidate {
            last_used: oid
                .and_then(|oid| image_usage.get(&oid).copied())
                .unwrap_or(i64::MIN),
            kind: ResourceKind::Environment,
            name,
        });
    }
    for name in runtime.list_artifacts(naming::OUTPUT_PREFIX)? {
        let oid = naming::output_oid(&name);
        if oid.is_some_and(|oid| protected_outputs.contains(&oid)) {
            continue;
        }
        candidates.push(Candidate {
            last_used: oid
                .and_then(|oid| usage.output.get(&oid).copied())
                .unwrap_or(i64::MIN),
            kind: ResourceKind::Output,
            name,
        });
    }

    reclaim(runtime, status, candidates)
}

fn reclaim<R>(
    runtime: &R,
    mut status: StorageStatus,
    mut candidates: Vec<Candidate>,
) -> anyhow::Result<()>
where
    R: ArtifactBackend + EnvironmentBackend + ?Sized,
{
    candidates.sort_by(|left, right| {
        left.last_used
            .cmp(&right.last_used)
            .then(left.kind.cmp(&right.kind))
            .then(left.name.cmp(&right.name))
    });
    eprintln!(
        "[clean] local container storage is {}% full; reclaiming least-recently-used compose artifacts",
        status.used_percent()
    );
    for candidate in candidates {
        if !usage_above(status, CLEANUP_TARGET_PERCENT) {
            return Ok(());
        }

        match candidate.kind {
            ResourceKind::Environment => {
                eprintln!("[clean] removing image: {}", candidate.name);
                runtime.remove_env(&candidate.name)?;
            }
            ResourceKind::Output => {
                eprintln!("[clean] removing volume: {}", candidate.name);
                runtime.remove_artifact(&candidate.name, true)?;
            }
        }
        status = runtime
            .storage_status()?
            .context("container backend stopped reporting storage capacity during cleanup")?;
        validate_status(status)?;
    }

    anyhow::ensure!(
        !usage_above(status, CLEANUP_TARGET_PERCENT),
        "local container storage remains {}% full after removing all unused compose images and output volumes",
        status.used_percent()
    );
    Ok(())
}

fn validate_status(status: StorageStatus) -> anyhow::Result<()> {
    anyhow::ensure!(
        status.total_bytes > 0 && status.used_bytes <= status.total_bytes,
        "container backend reported invalid storage usage: {} of {} bytes",
        status.used_bytes,
        status.total_bytes
    );
    Ok(())
}

fn usage_at_least(status: StorageStatus, percent: u64) -> bool {
    u128::from(status.used_bytes) * 100 >= u128::from(status.total_bytes) * u128::from(percent)
}

fn usage_above(status: StorageStatus, percent: u64) -> bool {
    u128::from(status.used_bytes) * 100 > u128::from(status.total_bytes) * u128::from(percent)
}

fn update_timestamp(
    timestamps: &mut HashMap<gix_hash::ObjectId, i64>,
    oid: gix_hash::ObjectId,
    timestamp: i64,
) {
    timestamps
        .entry(oid)
        .and_modify(|current| *current = (*current).max(timestamp))
        .or_insert(timestamp);
}

pub fn clean<R>(
    transaction: &josh_core::cache::Transaction,
    mode: CleanMode,
    runtime: &R,
) -> anyhow::Result<()>
where
    R: ArtifactBackend + EnvironmentBackend + ?Sized,
{
    let out_vols = runtime.list_artifacts(naming::OUTPUT_PREFIX)?;
    for vol in out_vols {
        eprintln!("[clean] removing volume: {vol}");
        runtime.remove_artifact(&vol, true)?;
    }

    job_cache::clean(transaction)?;

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

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use josh_compose_backend::EnvRecipe;

    use super::*;

    struct FakeRuntime {
        status: Mutex<StorageStatus>,
        reclaimed_bytes: HashMap<String, u64>,
        removed: Mutex<Vec<String>>,
    }

    impl FakeRuntime {
        fn new(used_bytes: u64, reclaimed_bytes: HashMap<String, u64>) -> Self {
            Self {
                status: Mutex::new(StorageStatus {
                    total_bytes: 1_000,
                    used_bytes,
                }),
                reclaimed_bytes,
                removed: Mutex::new(Vec::new()),
            }
        }

        fn remove(&self, name: &str) {
            self.removed.lock().push(name.to_owned());
            let bytes = self.reclaimed_bytes.get(name).copied().unwrap_or(0);
            let mut status = self.status.lock();
            status.used_bytes = status.used_bytes.saturating_sub(bytes);
        }
    }

    impl ArtifactBackend for FakeRuntime {
        fn artifact_exists(&self, _name: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        fn create_artifact(&self, _name: &str) -> anyhow::Result<()> {
            Ok(())
        }

        fn import_artifact(&self, _name: &str, _tar: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }

        fn export_artifact(&self, _name: &str) -> anyhow::Result<Vec<u8>> {
            Ok(Vec::new())
        }

        fn extract_artifact(&self, _name: &str, _dest: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }

        fn remove_artifact(&self, name: &str, _force: bool) -> anyhow::Result<()> {
            self.remove(name);
            Ok(())
        }

        fn list_artifacts(&self, _prefix: &str) -> anyhow::Result<Vec<String>> {
            Ok(Vec::new())
        }

        fn storage_status(&self) -> anyhow::Result<Option<StorageStatus>> {
            Ok(Some(*self.status.lock()))
        }

        fn create_scratch_artifact(&self, _tar: &[u8]) -> anyhow::Result<String> {
            Ok(String::new())
        }
    }

    impl EnvironmentBackend for FakeRuntime {
        fn env_exists(&self, _key: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        fn prepare_env(&self, _key: &str, _recipe: EnvRecipe) -> anyhow::Result<()> {
            Ok(())
        }

        fn list_envs(&self, _prefix: &str) -> anyhow::Result<Vec<String>> {
            Ok(Vec::new())
        }

        fn remove_env(&self, key: &str) -> anyhow::Result<()> {
            self.remove(key);
            Ok(())
        }
    }

    #[test]
    fn reclaims_in_lru_order_until_target() {
        let runtime = FakeRuntime::new(
            950,
            HashMap::from([
                ("old-image".to_owned(), 100),
                ("old-volume".to_owned(), 100),
                ("new-volume".to_owned(), 100),
            ]),
        );
        let candidates = vec![
            Candidate {
                last_used: 30,
                kind: ResourceKind::Output,
                name: "new-volume".to_owned(),
            },
            Candidate {
                last_used: 10,
                kind: ResourceKind::Output,
                name: "old-volume".to_owned(),
            },
            Candidate {
                last_used: 10,
                kind: ResourceKind::Environment,
                name: "old-image".to_owned(),
            },
        ];

        let status = *runtime.status.lock();
        reclaim(&runtime, status, candidates).unwrap();

        assert_eq!(
            *runtime.removed.lock(),
            ["old-image".to_owned(), "old-volume".to_owned()]
        );
        assert_eq!(runtime.status.lock().used_bytes, 750);
    }

    #[test]
    fn rejects_insufficient_reclaimable_space() {
        let runtime = FakeRuntime::new(950, HashMap::from([("image".to_owned(), 50)]));
        let status = *runtime.status.lock();
        let result = reclaim(
            &runtime,
            status,
            vec![Candidate {
                last_used: 10,
                kind: ResourceKind::Environment,
                name: "image".to_owned(),
            }],
        );

        assert!(result.unwrap_err().to_string().contains("remains 90% full"));
    }

    #[test]
    fn starts_reclamation_at_high_water_mark() {
        let status = |used_bytes| StorageStatus {
            total_bytes: 1_000,
            used_bytes,
        };

        assert!(!usage_at_least(status(899), CLEANUP_TRIGGER_PERCENT));
        assert!(usage_at_least(status(900), CLEANUP_TRIGGER_PERCENT));
        assert!(usage_above(status(801), CLEANUP_TARGET_PERCENT));
        assert!(!usage_above(status(800), CLEANUP_TARGET_PERCENT));
    }
}
