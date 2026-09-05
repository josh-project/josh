use anyhow::Context;
use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::Path;
use std::process::Stdio;
use std::str::FromStr;

use josh_core::cache::{Expected, Transaction};
use josh_core::filter::tree;
use josh_core::objects;
use josh_git_serde::GitValue;
use serde::Serialize;

pub const REF_NAME: &str = "refs/josh/compose";
const REMOTE_REF: &str = "refs/josh/compose-remote";

struct Blob<'a>(&'a [u8]);

impl Serialize for Blob<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(self.0)
    }
}

#[derive(Serialize)]
struct JobResult<'a> {
    stdout: Blob<'a>,
    stderr: Blob<'a>,
}

#[derive(Debug)]
enum PendingUpdate {
    Touch,
    Result {
        success: bool,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
}

#[derive(Debug, Default)]
pub(crate) struct PendingResults {
    updates: HashMap<gix_hash::ObjectId, PendingUpdate>,
}

impl PendingResults {
    pub(crate) fn touch(&mut self, hash: gix_hash::ObjectId) {
        self.updates.entry(hash).or_insert(PendingUpdate::Touch);
    }

    pub(crate) fn record(
        &mut self,
        hash: gix_hash::ObjectId,
        success: bool,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    ) {
        self.updates.insert(
            hash,
            PendingUpdate::Result {
                success,
                stdout,
                stderr,
            },
        );
    }
}
fn result_path(status: &str, hash: gix_hash::ObjectId) -> std::path::PathBuf {
    Path::new(status).join(objects::oid_fanout_path(hash))
}

fn ref_base(
    transaction: &Transaction,
) -> anyhow::Result<(Option<gix_hash::ObjectId>, gix_hash::ObjectId)> {
    let previous = transaction.resolve_ref(REF_NAME)?;
    let root = match previous {
        Some(oid) => objects::CommitData::read(transaction.odb(), oid)?.tree_id()?,
        None => tree::empty_id(),
    };
    Ok((previous, root))
}
fn commit_ref_tree(
    transaction: &Transaction,
    previous: Option<gix_hash::ObjectId>,
    root: gix_hash::ObjectId,
    extra_parents: &[gix_hash::ObjectId],
    message: &str,
) -> anyhow::Result<()> {
    let signature = josh_core::git::josh_actor_signature()?;
    let parents: Vec<_> = previous
        .into_iter()
        .chain(extra_parents.iter().copied())
        .collect();
    let commit = objects::write_commit(
        transaction.odb(),
        root,
        &parents,
        &signature,
        &signature,
        message,
    )?;
    transaction.update_ref(
        REF_NAME,
        previous.map_or(Expected::Absent, Expected::At),
        commit,
        message,
    )
}

/// Commit every result update and cache hit from one compose run as a single ref update.
pub(crate) fn commit_results(
    transaction: &Transaction,
    pending: PendingResults,
) -> anyhow::Result<()> {
    if pending.updates.is_empty() {
        return Ok(());
    }

    let mut updates: Vec<_> = pending.updates.into_iter().collect();
    updates.sort_by_key(|(hash, _)| *hash);

    let (previous, mut root) = ref_base(transaction)?;
    let mut message = format!("update {REF_NAME} batch\n\n");
    for (hash, update) in updates {
        match update {
            PendingUpdate::Touch => {
                use std::fmt::Write;
                writeln!(&mut message, "touch {hash}")?;
            }
            PendingUpdate::Result {
                success,
                stdout,
                stderr,
            } => {
                let result = JobResult {
                    stdout: Blob(&stdout),
                    stderr: Blob(&stderr),
                };
                let (status, opposite) = if success {
                    ("success", "failed")
                } else {
                    ("failed", "success")
                };

                let value = josh_git_serde::to_value(&result)?;
                anyhow::ensure!(
                    matches!(value, GitValue::Tree(_)),
                    "compose result must serialize to a tree"
                );
                let result_tree = josh_git_serde::to_tree_oid(transaction.odb(), &value)?;
                let update_tree = tree::insert_oid(
                    transaction.odb(),
                    tree::empty_id(),
                    &result_path(status, hash),
                    result_tree,
                    0o040000,
                )?;
                let without_opposite = tree::insert_oid(
                    transaction.odb(),
                    root,
                    &result_path(opposite, hash),
                    tree::empty_id(),
                    0o040000,
                )?;
                root = tree::overlay(transaction, update_tree, without_opposite)?;

                use std::fmt::Write;
                writeln!(&mut message, "result {status}/{hash}")?;
            }
        }
    }

    commit_ref_tree(transaction, previous, root, &[], &message)
}

/// Persist one run's result metadata through a transaction whose object store is separate from
/// the ephemeral transaction used to build the filtered workspace.
pub(crate) fn commit_results_persistent(
    source: &Transaction,
    pending: PendingResults,
) -> anyhow::Result<()> {
    if pending.updates.is_empty() {
        return Ok(());
    }

    let transaction = josh_core::cache::TransactionContext::new(
        source.path(),
        std::sync::Arc::new(josh_core::cache::CacheStack::new()),
    )
    .open()
    .context("could not open compose result transaction")?;
    commit_results(&transaction, pending)?;
    transaction
        .flush_mem_odb()
        .context("could not persist compose results")
}

/// Returns true if `refs/josh/compose` records a successful run for `hash`.
pub fn is_cached_success(
    transaction: &Transaction,
    hash: gix_hash::ObjectId,
) -> anyhow::Result<bool> {
    let Some(tip) = transaction.resolve_ref(REF_NAME)? else {
        return Ok(false);
    };
    let root = objects::CommitData::read(transaction.odb(), tip)?.tree_id()?;
    Ok(tree::get_path_entry(
        transaction,
        transaction.odb(),
        root,
        &result_path("success", hash),
    )?
    .is_some_and(|entry| entry.mode.is_tree()))
}
fn fetch_remote_ref(
    transaction: &Transaction,
    remote: &str,
) -> anyhow::Result<Option<gix_hash::ObjectId>> {
    let output = transaction
        .git_command(&["ls-remote", "--refs", remote, REF_NAME], &[])?
        .with_stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not inspect {REF_NAME} on {remote}"))?;
    if output.stdout.is_empty() {
        return Ok(None);
    }

    let refspec = format!("+{REF_NAME}:{REMOTE_REF}");
    transaction
        .spawn_git(&["fetch", "--no-tags", remote, &refspec], &[])
        .with_context(|| format!("could not fetch {REF_NAME} from {remote}"))?;
    transaction
        .resolve_ref(REMOTE_REF)?
        .with_context(|| format!("fetch from {remote} did not create {REMOTE_REF}"))
        .map(Some)
}

fn integrate_remote_ref(
    transaction: &Transaction,
    remote_tip: gix_hash::ObjectId,
) -> anyhow::Result<()> {
    let Some(local_tip) = transaction.resolve_ref(REF_NAME)? else {
        return transaction.update_ref(
            REF_NAME,
            Expected::Absent,
            remote_tip,
            "pull refs/josh/compose\n",
        );
    };
    if local_tip == remote_tip
        || josh_gix_ext::is_descendant_of(transaction.odb(), local_tip, remote_tip)?
    {
        return Ok(());
    }
    if josh_gix_ext::is_descendant_of(transaction.odb(), remote_tip, local_tip)? {
        return transaction.update_ref(
            REF_NAME,
            Expected::At(local_tip),
            remote_tip,
            "fast-forward refs/josh/compose\n",
        );
    }

    let root = josh_gix_ext::merge_commits(transaction.odb(), local_tip, remote_tip, None)?;
    commit_ref_tree(
        transaction,
        Some(local_tip),
        root,
        &[remote_tip],
        "merge concurrent refs/josh/compose updates\n",
    )
}

fn remove_remote_ref(transaction: &Transaction) -> anyhow::Result<()> {
    if let Some(tip) = transaction.resolve_ref(REMOTE_REF)? {
        transaction.delete_ref(REMOTE_REF, Expected::At(tip))?;
    }
    Ok(())
}

pub fn pull_results(transaction: &Transaction, remote: &str) -> anyhow::Result<()> {
    let Some(remote_tip) = fetch_remote_ref(transaction, remote)? else {
        eprintln!("[compose] {REF_NAME} not present on {remote}");
        return Ok(());
    };
    integrate_remote_ref(transaction, remote_tip)?;
    remove_remote_ref(transaction)?;
    eprintln!("[compose] pulled {REF_NAME} from {remote}");
    Ok(())
}

pub fn push_results(transaction: &Transaction, remote: &str) -> anyhow::Result<()> {
    if transaction.resolve_ref(REF_NAME)?.is_none() {
        eprintln!("[compose] {REF_NAME} not present locally");
        return Ok(());
    }

    let refspec = format!("{REF_NAME}:{REF_NAME}");
    for attempt in 1..=3 {
        match transaction.spawn_git(&["push", remote, &refspec], &[]) {
            Ok(()) => {
                remove_remote_ref(transaction)?;
                eprintln!("[compose] pushed {REF_NAME} to {remote}");
                return Ok(());
            }
            Err(_error) if attempt < 3 => {
                eprintln!("[compose] push raced with another update; merging attempt {attempt}");
                let remote_tip = fetch_remote_ref(transaction, remote)?
                    .with_context(|| format!("{REF_NAME} disappeared from {remote}"))?;
                integrate_remote_ref(transaction, remote_tip)?;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not push {REF_NAME} after 3 attempts"));
            }
        }
    }
    unreachable!()
}

#[derive(Debug, Default)]
pub(crate) struct JobUsage {
    /// Last time an output volume was created or read.
    pub output: HashMap<gix_hash::ObjectId, i64>,
    /// Last time a workspace execution used its environment images.
    pub execution: HashMap<gix_hash::ObjectId, i64>,
}

/// Read local artifact usage from the commit history behind `refs/josh/compose`.
pub(crate) fn job_usage(transaction: &Transaction) -> anyhow::Result<JobUsage> {
    let Some(tip) = transaction.resolve_ref(REF_NAME)? else {
        return Ok(JobUsage::default());
    };

    let mut usage = JobUsage::default();
    let mut walk = josh_gix_ext::RevWalk::new(transaction.odb());
    walk.push(tip)?;
    walk.discover(|oid| {
        let commit = objects::CommitData::read(transaction.odb(), oid)?;
        let message = std::str::from_utf8(commit.message_raw()?)
            .context("compose result commit message is not UTF-8")?;
        let mut timestamp = None;
        for line in message.lines() {
            let Some((hash, executed)) = parse_usage_event(line) else {
                continue;
            };
            let timestamp = match timestamp {
                Some(timestamp) => timestamp,
                None => {
                    let value = commit.parsed()?.committer()?.time()?.seconds;
                    timestamp = Some(value);
                    value
                }
            };
            update_timestamp(&mut usage.output, hash, timestamp);
            if executed {
                update_timestamp(&mut usage.execution, hash, timestamp);
            }
        }
        Ok(ControlFlow::Continue(()))
    })?;
    Ok(usage)
}

fn parse_usage_event(line: &str) -> Option<(gix_hash::ObjectId, bool)> {
    if let Some(path) = line.strip_prefix("result ") {
        return parse_result_path(path).map(|hash| (hash, true));
    }
    if let Some(path) = line
        .strip_prefix("update ")
        .and_then(|value| value.strip_prefix(REF_NAME))
        .and_then(|value| value.strip_prefix(' '))
    {
        return parse_result_path(path).map(|hash| (hash, true));
    }

    let hash = line.strip_prefix("touch ")?;
    let hash = hash
        .strip_prefix(REF_NAME)
        .and_then(|value| value.strip_prefix(' '))
        .unwrap_or(hash);
    gix_hash::ObjectId::from_str(hash)
        .ok()
        .map(|hash| (hash, false))
}

fn parse_result_path(path: &str) -> Option<gix_hash::ObjectId> {
    let (status, hash) = path.split_once('/')?;
    if status != "success" && status != "failed" {
        return None;
    }
    gix_hash::ObjectId::from_str(hash).ok()
}

fn update_timestamp(
    timestamps: &mut HashMap<gix_hash::ObjectId, i64>,
    hash: gix_hash::ObjectId,
    timestamp: i64,
) {
    timestamps
        .entry(hash)
        .and_modify(|current| *current = (*current).max(timestamp))
        .or_insert(timestamp);
}

/// Remove all compose result metadata.
pub fn clean(transaction: &Transaction) -> anyhow::Result<()> {
    let Some(previous) = transaction.resolve_ref(REF_NAME)? else {
        return Ok(());
    };
    transaction.delete_ref(REF_NAME, Expected::At(previous))?;
    eprintln!("[clean] removed {REF_NAME}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit_result(
        transaction: &Transaction,
        hash: gix_hash::ObjectId,
        success: bool,
        stdout: &[u8],
        stderr: &[u8],
    ) {
        let mut pending = PendingResults::default();
        pending.record(hash, success, stdout.to_vec(), stderr.to_vec());
        commit_results(transaction, pending).unwrap();
    }

    fn commit_touch(transaction: &Transaction, hash: gix_hash::ObjectId) {
        let mut pending = PendingResults::default();
        pending.touch(hash);
        commit_results(transaction, pending).unwrap();
    }

    #[test]
    fn stores_results_in_compose_ref() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let context = josh_core::cache::TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        let hash =
            gix_hash::ObjectId::from_hex(b"0123456789abcdef0123456789abcdef01234567").unwrap();

        assert!(!is_cached_success(&transaction, hash).unwrap());

        commit_result(&transaction, hash, false, b"failed\xff", b"error");
        assert!(!is_cached_success(&transaction, hash).unwrap());

        commit_result(&transaction, hash, true, b"passed\xff", b"warning");
        assert!(is_cached_success(&transaction, hash).unwrap());

        let tip = transaction.resolve_ref(REF_NAME).unwrap().unwrap();
        let root = objects::CommitData::read(transaction.odb(), tip)
            .unwrap()
            .tree_id()
            .unwrap();
        assert!(
            tree::get_path_entry(
                &transaction,
                transaction.odb(),
                root,
                &result_path("failed", hash),
            )
            .unwrap()
            .is_none()
        );
        let stdout_path = Path::new("success")
            .join("01")
            .join("234")
            .join("56789abcdef0123456789abcdef01234567")
            .join("stdout");
        let stdout = tree::get_path_entry(&transaction, transaction.odb(), root, &stdout_path)
            .unwrap()
            .unwrap();
        let stdout_bytes = tree::blob_bytes(transaction.odb(), stdout.oid).unwrap();
        assert_eq!(&*stdout_bytes, b"passed\xff");

        commit_result(&transaction, hash, true, b"passed\xff", b"warning");
        let refreshed = transaction.resolve_ref(REF_NAME).unwrap().unwrap();
        assert_ne!(refreshed, tip);
        assert_eq!(
            objects::CommitData::read(transaction.odb(), refreshed)
                .unwrap()
                .tree_id()
                .unwrap(),
            root
        );

        commit_touch(&transaction, hash);
        let touched = transaction.resolve_ref(REF_NAME).unwrap().unwrap();
        assert_ne!(touched, refreshed);
        assert_eq!(
            objects::CommitData::read(transaction.odb(), touched)
                .unwrap()
                .tree_id()
                .unwrap(),
            root
        );

        let usage = job_usage(&transaction).unwrap();
        assert!(usage.output[&hash] >= usage.execution[&hash]);

        clean(&transaction).unwrap();
        assert_eq!(transaction.resolve_ref(REF_NAME).unwrap(), None);
    }

    #[test]
    fn persists_results_without_ephemeral_workspace_objects() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let transaction = josh_core::cache::TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        )
        .ephemeral()
        .open()
        .unwrap();
        let filtered_object =
            objects::write_blob(transaction.odb(), b"ephemeral filtered object").unwrap();
        let hash =
            gix_hash::ObjectId::from_hex(b"0123456789abcdef0123456789abcdef01234567").unwrap();
        let mut pending = PendingResults::default();
        pending.record(hash, true, b"passed".to_vec(), Vec::new());

        commit_results_persistent(&transaction, pending).unwrap();
        drop(transaction);

        let verifier = josh_core::cache::TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        )
        .open()
        .unwrap();
        assert!(is_cached_success(&verifier, hash).unwrap());
        assert!(!verifier.odb().contains(filtered_object));
    }

    #[test]
    fn commits_run_results_as_one_ref_update() {
        let dir = tempfile::tempdir().unwrap();
        gix::init_bare(dir.path()).unwrap();
        let context = josh_core::cache::TransactionContext::new(
            dir.path(),
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        );
        let transaction = context.open().unwrap();
        let seed =
            gix_hash::ObjectId::from_hex(b"1111111111111111111111111111111111111111").unwrap();
        let succeeded =
            gix_hash::ObjectId::from_hex(b"2222222222222222222222222222222222222222").unwrap();
        let failed =
            gix_hash::ObjectId::from_hex(b"3333333333333333333333333333333333333333").unwrap();
        commit_result(&transaction, seed, true, b"seed", b"");
        let previous = transaction.resolve_ref(REF_NAME).unwrap().unwrap();

        let mut pending = PendingResults::default();
        pending.touch(seed);
        pending.record(succeeded, true, b"passed".to_vec(), Vec::new());
        pending.record(failed, false, Vec::new(), b"failed".to_vec());
        commit_results(&transaction, pending).unwrap();

        let tip = transaction.resolve_ref(REF_NAME).unwrap().unwrap();
        let commit = objects::CommitData::read(transaction.odb(), tip).unwrap();
        assert_eq!(commit.parent_ids().collect::<Vec<_>>(), [previous]);
        assert!(is_cached_success(&transaction, succeeded).unwrap());
        assert!(!is_cached_success(&transaction, failed).unwrap());

        let usage = job_usage(&transaction).unwrap();
        assert!(usage.output.contains_key(&seed));
        assert!(usage.execution.contains_key(&succeeded));
        assert!(usage.execution.contains_key(&failed));
    }

    #[test]
    fn transfers_and_merges_concurrent_results() {
        let remote = tempfile::tempdir().unwrap();
        gix::init_bare(remote.path()).unwrap();
        let remote = remote.path().to_string_lossy().into_owned();

        let producer_dir = tempfile::tempdir().unwrap();
        let runner_a_dir = tempfile::tempdir().unwrap();
        let runner_b_dir = tempfile::tempdir().unwrap();
        let verifier_dir = tempfile::tempdir().unwrap();
        for dir in [&producer_dir, &runner_a_dir, &runner_b_dir, &verifier_dir] {
            gix::init_bare(dir.path()).unwrap();
        }

        let producer_context = josh_core::cache::TransactionContext::new(
            producer_dir.path(),
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        );
        let runner_a_context = josh_core::cache::TransactionContext::new(
            runner_a_dir.path(),
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        );
        let runner_b_context = josh_core::cache::TransactionContext::new(
            runner_b_dir.path(),
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        );
        let verifier_context = josh_core::cache::TransactionContext::new(
            verifier_dir.path(),
            std::sync::Arc::new(josh_core::cache::CacheStack::new()),
        );

        let producer = producer_context.open().unwrap();
        let seed =
            gix_hash::ObjectId::from_hex(b"1111111111111111111111111111111111111111").unwrap();
        commit_result(&producer, seed, true, b"seed", b"");
        push_results(&producer, &remote).unwrap();

        let runner_a = runner_a_context.open().unwrap();
        let runner_b = runner_b_context.open().unwrap();
        pull_results(&runner_a, &remote).unwrap();
        pull_results(&runner_b, &remote).unwrap();

        let hash_a =
            gix_hash::ObjectId::from_hex(b"2222222222222222222222222222222222222222").unwrap();
        commit_result(&runner_a, hash_a, true, b"a", b"");
        push_results(&runner_a, &remote).unwrap();

        let hash_b =
            gix_hash::ObjectId::from_hex(b"3333333333333333333333333333333333333333").unwrap();
        commit_result(&runner_b, hash_b, true, b"b", b"");
        push_results(&runner_b, &remote).unwrap();

        let verifier = verifier_context.open().unwrap();
        pull_results(&verifier, &remote).unwrap();
        assert!(is_cached_success(&verifier, seed).unwrap());
        assert!(is_cached_success(&verifier, hash_a).unwrap());
        assert!(is_cached_success(&verifier, hash_b).unwrap());
    }
}
