use crate::shell;
use josh_core::cache::CacheStack;
use josh_core::housekeeping;
use std::path::{Path, PathBuf};

const OVERLAY_PACK_GC_THRESHOLD: usize = (josh_core::cache::GIX_PACK_SLOTS as usize)
    .div_ceil(10)
    .next_multiple_of(100);
const REPACK_REQUEST_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);
const PERIODIC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
const GC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60 * 60);

#[derive(Debug, PartialEq, Eq)]
enum Request {
    RepackIfNeeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    PeriodicMaintenance,
    PeriodicGc,
    RepackIfNeeded,
}

pub struct Actor {
    repo_path: PathBuf,
    gc: bool,
    cache: std::sync::Arc<CacheStack>,
    receiver: tokio::sync::mpsc::Receiver<Request>,
}

impl Actor {
    pub fn new(repo_path: PathBuf, gc: bool) -> (Self, ThrottledRequest) {
        let (request, receiver) = ThrottledRequest::channel();
        (
            Self {
                repo_path,
                gc,
                cache: std::sync::Arc::new(CacheStack::default()),
                receiver,
            },
            request,
        )
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut maintenance = tokio::time::interval(PERIODIC_INTERVAL);
        maintenance.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut gc = tokio::time::interval(GC_INTERVAL);
        gc.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut on_demand_open = true;

        loop {
            tokio::select! {
                biased;
                _ = maintenance.tick() => {
                    execute_periodic(
                        Operation::PeriodicMaintenance,
                        &mut maintenance,
                        &self.repo_path,
                        &self.cache,
                    ).await?;
                }
                _ = gc.tick(), if self.gc => {
                    execute_periodic(
                        Operation::PeriodicGc,
                        &mut gc,
                        &self.repo_path,
                        &self.cache,
                    ).await?;
                }
                request = self.receiver.recv(), if on_demand_open => {
                    match request {
                        Some(Request::RepackIfNeeded) => {
                            if let Err(error) = execute(
                                Operation::RepackIfNeeded,
                                &self.repo_path,
                                &self.cache,
                            ).await {
                                tracing::error!("on-demand housekeeping failed: {error:#}");
                            }
                        }
                        None => on_demand_open = false,
                    }
                }
            }
        }
    }
}

async fn execute(
    operation: Operation,
    repo_path: &Path,
    cache: &std::sync::Arc<CacheStack>,
) -> anyhow::Result<()> {
    let repo_path = repo_path.to_owned();
    let cache = cache.clone();
    tokio::task::spawn_blocking(move || match operation {
        Operation::PeriodicMaintenance => run_maintenance(&repo_path, cache),
        Operation::PeriodicGc => run_repack(&repo_path),
        Operation::RepackIfNeeded => repack_if_needed(&repo_path),
    })
    .await??;
    Ok(())
}

async fn execute_periodic(
    operation: Operation,
    timer: &mut tokio::time::Interval,
    repo_path: &Path,
    cache: &std::sync::Arc<CacheStack>,
) -> anyhow::Result<()> {
    execute(operation, repo_path, cache).await?;
    timer.reset();
    Ok(())
}

#[derive(Clone)]
pub struct ThrottledRequest {
    sender: tokio::sync::mpsc::Sender<Request>,
    last_sent: std::sync::Arc<std::sync::Mutex<Option<std::time::Instant>>>,
}

impl ThrottledRequest {
    fn channel() -> (Self, tokio::sync::mpsc::Receiver<Request>) {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        (
            Self {
                sender,
                last_sent: Default::default(),
            },
            receiver,
        )
    }

    pub fn repack_if_needed(&self) {
        self.repack_if_needed_at(std::time::Instant::now());
    }

    fn repack_if_needed_at(&self, now: std::time::Instant) {
        let mut last_sent = self.last_sent.lock().unwrap();
        if last_sent
            .is_some_and(|last| now.saturating_duration_since(last) < REPACK_REQUEST_INTERVAL)
        {
            return;
        }
        if self.sender.try_send(Request::RepackIfNeeded).is_ok() {
            *last_sent = Some(now);
        }
    }
}

macro_rules! trace_object_count {
    ($object_count:expr, $repo:expr) => {
        match $object_count {
            ParsedCommandResult::Parsed { value } => {
                tracing::info!(
                    repo = $repo,
                    "count" = value.count,
                    "size" = value.size,
                    "in_pack" = value.in_pack,
                    "packs" = value.packs,
                    "size_pack" = value.size_pack,
                    "prune_packable" = value.prune_packable,
                    "garbage" = value.garbage,
                    "size_garbage" = value.size_garbage,
                    "count_objects"
                )
            }
            ParsedCommandResult::Fallback { stdout } => {
                tracing::info!(repo = $repo, fallback = stdout, "count_objects")
            }
            ParsedCommandResult::Error { code, stderr } => {
                tracing::error!(
                    repo = $repo,
                    code = code.get(),
                    stderr = stderr,
                    "count_objects"
                )
            }
        }
    };
}

macro_rules! trace_command_result {
    ($command_result:expr, $repo:expr, $operation:expr) => {
        match $command_result {
            CommandResult::Ok { stdout } => {
                tracing::info!(repo = $repo, stdout = stdout, $operation)
            }
            CommandResult::Err { code, stderr } => {
                tracing::error!(repo = $repo, code = code.get(), stderr = stderr, $operation)
            }
        }
    };
}

enum CommandResult {
    Ok {
        stdout: String,
    },
    Err {
        code: std::num::NonZero<i32>,
        stderr: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CountObjectsOutput {
    count: usize,
    size: usize,
    in_pack: usize,
    packs: usize,
    size_pack: usize,
    prune_packable: usize,
    garbage: usize,
    size_garbage: usize,
}

enum ParsedCommandResult<T> {
    Parsed {
        value: T,
    },
    Fallback {
        stdout: String,
    },
    Error {
        code: std::num::NonZero<i32>,
        stderr: String,
    },
}

fn try_parse_count_objects_output(value: &str) -> Option<CountObjectsOutput> {
    let map = value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|value| value.split_once(':').map(|(k, v)| (k.trim(), v.trim())))
        .collect::<Option<std::collections::HashMap<_, _>>>()?;

    let try_get =
        |key: &str| -> Option<usize> { map.get(key).and_then(|v| v.parse::<usize>().ok()) };

    Some(CountObjectsOutput {
        count: try_get("count")?,
        size: try_get("size")?,
        in_pack: try_get("in-pack")?,
        packs: try_get("packs")?,
        size_pack: try_get("size-pack")?,
        prune_packable: try_get("prune-packable")?,
        garbage: try_get("garbage")?,
        size_garbage: try_get("size-garbage")?,
    })
}

impl From<CommandResult> for ParsedCommandResult<CountObjectsOutput> {
    fn from(value: CommandResult) -> Self {
        match value {
            CommandResult::Ok { stdout } => match try_parse_count_objects_output(&stdout) {
                Some(value) => ParsedCommandResult::Parsed { value },
                None => ParsedCommandResult::Fallback { stdout },
            },
            CommandResult::Err { code, stderr } => ParsedCommandResult::Error { code, stderr },
        }
    }
}

fn run_command(path: &Path, cmd: &[&str]) -> CommandResult {
    run_command_env(path, cmd, &[])
}

fn run_command_env(path: &Path, cmd: &[&str], env: &[(&str, &str)]) -> CommandResult {
    let shell = shell::Shell {
        cwd: path.to_owned(),
    };

    let (stdout, stderr, code) = if env.is_empty() {
        shell.command(cmd)
    } else {
        shell.command_env(cmd, env, &[])
    };

    if let Some(code) = std::num::NonZero::new(code) {
        CommandResult::Err { code, stderr }
    } else {
        CommandResult::Ok { stdout }
    }
}

fn repack_needed(object_count: &ParsedCommandResult<CountObjectsOutput>) -> bool {
    matches!(
        object_count,
        ParsedCommandResult::Parsed { value } if value.packs >= OVERLAY_PACK_GC_THRESHOLD
    )
}

#[tracing::instrument(name = "housekeeping_repack_if_needed", skip_all)]
fn repack_if_needed(repo_path: &Path) -> anyhow::Result<()> {
    let mirror_objects = repo_path.join("mirror").join("objects").canonicalize()?;
    let mirror_objects = mirror_objects.to_str().unwrap();
    let overlay_git_env = [("GIT_ALTERNATE_OBJECT_DIRECTORIES", mirror_objects)];
    let object_count: ParsedCommandResult<CountObjectsOutput> = run_command_env(
        &repo_path.join("overlay"),
        &["git", "count-objects", "-v"],
        &overlay_git_env,
    )
    .into();
    let repack_needed = repack_needed(&object_count);
    trace_object_count!(object_count, "overlay");

    if repack_needed {
        run_repack(repo_path)?;
    }
    Ok(())
}

#[tracing::instrument(name = "housekeeping_run", skip_all)]
fn run_maintenance(
    repo_path: &std::path::Path,
    cache: std::sync::Arc<josh_core::cache::CacheStack>,
) -> anyhow::Result<()> {
    use josh_core::cache::TransactionContext;

    let transaction_mirror = TransactionContext::new(repo_path.join("mirror"), cache.clone())
        .with_mem_odb_limit(crate::MAX_MEM_PACK_SIZE)
        .open()?;
    let transaction_overlay = TransactionContext::new(repo_path.join("overlay"), cache)
        .with_mem_odb_limit(crate::MAX_MEM_PACK_SIZE)
        .open()?;

    let mirror_objects = repo_path.join("mirror").join("objects").canonicalize()?;
    let mirror_objects = mirror_objects.to_str().unwrap();
    transaction_overlay.add_disk_alternate(mirror_objects)?;
    let overlay_git_env = [("GIT_ALTERNATE_OBJECT_DIRECTORIES", mirror_objects)];

    let mirror_object_count: ParsedCommandResult<CountObjectsOutput> =
        run_command(transaction_mirror.path(), &["git", "count-objects", "-v"]).into();

    trace_object_count!(mirror_object_count, "mirror");

    let overlay_object_count: ParsedCommandResult<CountObjectsOutput> = run_command_env(
        transaction_overlay.path(),
        &["git", "count-objects", "-v"],
        &overlay_git_env,
    )
    .into();

    trace_object_count!(overlay_object_count, "overlay");

    if std::env::var("JOSH_NO_DISCOVER").is_err() {
        housekeeping::discover_filter_candidates(&transaction_mirror)?;
    }

    if std::env::var("JOSH_NO_REFRESH").is_err() {
        josh_core::housekeeping::refresh_known_filters(&transaction_mirror, &transaction_overlay)?;
    }

    Ok(())
}

#[tracing::instrument(name = "housekeeping_repack", skip_all)]
fn run_repack(repo_path: &Path) -> anyhow::Result<()> {
    const CRUFT_PACK_SIZE: usize = 1024 * 1024 * 64;

    let mirror_path = repo_path.join("mirror");
    let overlay_path = repo_path.join("overlay");
    let mirror_objects = mirror_path.join("objects").canonicalize()?;
    let mirror_objects = mirror_objects.to_str().unwrap();
    let overlay_git_env = [("GIT_ALTERNATE_OBJECT_DIRECTORIES", mirror_objects)];

    trace_command_result!(
        run_command(
            &mirror_path,
            &[
                "git",
                "repack",
                "-adn",
                "--keep-unreachable",
                "--pack-kept-objects",
                "--no-write-bitmap-index",
                "--threads=4"
            ]
        ),
        "mirror",
        "repack"
    );

    trace_command_result!(
        run_command(
            &mirror_path,
            &["git", "multi-pack-index", "write", "--bitmap"]
        ),
        "mirror",
        "multi_pack_index"
    );

    trace_command_result!(
        run_command_env(
            &overlay_path,
            &[
                "git",
                "repack",
                "-dn",
                "-l",
                "--cruft",
                &format!("--max-cruft-size={}", CRUFT_PACK_SIZE),
                "--no-write-bitmap-index",
                "--window-memory=128m",
                "--threads=4",
            ],
            &overlay_git_env
        ),
        "overlay",
        "repack"
    );

    trace_command_result!(
        run_command_env(
            &overlay_path,
            &["git", "multi-pack-index", "write"],
            &overlay_git_env
        ),
        "overlay",
        "multi_pack_index"
    );

    let final_object_count: ParsedCommandResult<CountObjectsOutput> =
        run_command(&mirror_path, &["git", "count-objects", "-v"]).into();

    trace_object_count!(final_object_count, "mirror");

    Ok(())
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    #[test]
    fn test_parse_count_objects() {
        let output = indoc! {r#"
            count: 0
            size: 0
            in-pack: 17267
            packs: 1
            size-pack: 10890
            prune-packable: 0
            garbage: 0
            size-garbage: 0
        "#};

        let result = super::try_parse_count_objects_output(output);

        let expected = super::CountObjectsOutput {
            count: 0,
            size: 0,
            in_pack: 17267,
            packs: 1,
            size_pack: 10890,
            prune_packable: 0,
            garbage: 0,
            size_garbage: 0,
        };

        assert_eq!(result, Some(expected));
    }

    #[test]
    fn test_repack_if_needed_threshold() {
        assert_eq!(super::OVERLAY_PACK_GC_THRESHOLD, 500);
        let mut value = super::CountObjectsOutput {
            count: 0,
            size: 0,
            in_pack: 0,
            packs: super::OVERLAY_PACK_GC_THRESHOLD - 1,
            size_pack: 0,
            prune_packable: 0,
            garbage: 0,
            size_garbage: 0,
        };
        let object_count = super::ParsedCommandResult::Parsed {
            value: value.clone(),
        };
        assert!(!super::repack_needed(&object_count));

        value.packs = super::OVERLAY_PACK_GC_THRESHOLD;
        let object_count = super::ParsedCommandResult::Parsed { value };
        assert!(super::repack_needed(&object_count));
    }

    #[test]
    fn throttled_request_shares_ten_second_interval_across_clones() {
        let (request, mut receiver) = super::ThrottledRequest::channel();
        let now = std::time::Instant::now();
        let clone = request.clone();

        request.repack_if_needed_at(now);
        assert!(matches!(
            receiver.try_recv(),
            Ok(super::Request::RepackIfNeeded)
        ));

        clone.repack_if_needed_at(now + std::time::Duration::from_secs(9));
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        clone.repack_if_needed_at(now + std::time::Duration::from_secs(10));
        assert!(matches!(
            receiver.try_recv(),
            Ok(super::Request::RepackIfNeeded)
        ));
    }

    #[test]
    fn throttled_request_ignores_full_and_disconnected_channels() {
        let (request, mut receiver) = super::ThrottledRequest::channel();
        let now = std::time::Instant::now();

        request.repack_if_needed_at(now);
        request.repack_if_needed_at(now + std::time::Duration::from_secs(10));
        assert!(matches!(
            receiver.try_recv(),
            Ok(super::Request::RepackIfNeeded)
        ));

        request.repack_if_needed_at(now + std::time::Duration::from_secs(11));
        assert!(matches!(
            receiver.try_recv(),
            Ok(super::Request::RepackIfNeeded)
        ));

        drop(receiver);
        request.repack_if_needed_at(now + std::time::Duration::from_secs(30));
    }
}
