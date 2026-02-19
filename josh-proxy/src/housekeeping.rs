use josh_core::{housekeeping, shell};
use std::path::Path;

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
    let shell = shell::Shell {
        cwd: path.to_owned(),
    };

    let (stdout, stderr, code) = shell.command(cmd);

    if let Some(code) = std::num::NonZero::new(code) {
        CommandResult::Err { code, stderr }
    } else {
        CommandResult::Ok { stdout }
    }
}

#[tracing::instrument(name = "housekeeping_run", skip_all)]
pub fn run(
    repo_path: &std::path::Path,
    cache: std::sync::Arc<josh_core::cache::CacheStack>,
    do_gc: bool,
) -> anyhow::Result<()> {
    use josh_core::cache::TransactionContext;

    const CRUFT_PACK_SIZE: usize = 1024 * 1024 * 64;

    let transaction_mirror =
        TransactionContext::new(repo_path.join("mirror"), cache.clone()).open(None)?;
    let transaction_overlay =
        TransactionContext::new(repo_path.join("overlay"), cache).open(None)?;

    transaction_overlay
        .repo()
        .odb()?
        .add_disk_alternate(repo_path.join("mirror").join("objects").to_str().unwrap())?;

    let mirror_object_count: ParsedCommandResult<CountObjectsOutput> = run_command(
        transaction_mirror.repo().path(),
        &["git", "count-objects", "-v"],
    )
    .into();

    trace_object_count!(mirror_object_count, "mirror");

    let overlay_object_count: ParsedCommandResult<CountObjectsOutput> = run_command(
        transaction_overlay.repo().path(),
        &["git", "count-objects", "-v"],
    )
    .into();

    trace_object_count!(overlay_object_count, "overlay");

    if std::env::var("JOSH_NO_DISCOVER").is_err() {
        housekeeping::discover_filter_candidates(&transaction_mirror)?;
    }

    if std::env::var("JOSH_NO_REFRESH").is_err() {
        josh_core::housekeeping::refresh_known_filters(&transaction_mirror, &transaction_overlay)?;
    }

    if do_gc {
        trace_command_result!(
            run_command(
                transaction_mirror.repo().path(),
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
                transaction_mirror.repo().path(),
                &["git", "multi-pack-index", "write", "--bitmap"]
            ),
            "mirror",
            "multi_pack_index"
        );

        trace_command_result!(
            run_command(
                transaction_overlay.repo().path(),
                &[
                    "git",
                    "repack",
                    "-dn",
                    "--cruft",
                    &format!("--max-cruft-size={}", CRUFT_PACK_SIZE),
                    "--no-write-bitmap-index",
                    "--window-memory=128m",
                    "--threads=4",
                ]
            ),
            "overlay",
            "repack"
        );

        trace_command_result!(
            run_command(
                transaction_overlay.repo().path(),
                &["git", "multi-pack-index", "write", "--bitmap"]
            ),
            "overlay",
            "multi_pack_index"
        );

        let final_object_count: ParsedCommandResult<CountObjectsOutput> = run_command(
            transaction_mirror.repo().path(),
            &["git", "count-objects", "-v"],
        )
        .into();

        trace_object_count!(final_object_count, "mirror");
    }

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
}
