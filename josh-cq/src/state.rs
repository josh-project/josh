use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Context;

use josh_core::git::spawn_git_command;
use josh_github_changes::admission::AdmissionState;
use josh_github_graphql::connection::GithubApiConnection;
use josh_github_graphql::operations::repo::RequiredStatusCheck;
use josh_link::make_signature;

use crate::types::{AdmissionRelevantEvent, GH_TOKEN_ENV};

#[derive(Debug, Clone)]
pub(crate) struct CandidatePr {
    pub node_id: String,
    pub number: i64,
    pub repo_url: String,
    pub head_sha: String,
    pub head_branch: String,
    pub base_sha: String,
    pub base_branch: String,
    pub title: String,
}

#[derive(Default, Clone)]
pub(crate) struct CqActorState {
    pub admission: BTreeMap<String, BTreeSet<RequiredStatusCheck>>,
    pub pr_admissions: BTreeMap<String, AdmissionState>,
    pub candidates: BTreeMap<String, CandidatePr>,
}

impl CqActorState {
    pub fn get_or_fetch_admission(
        &mut self,
        clone_url: &str,
        api: Option<&GithubApiConnection>,
    ) -> Option<BTreeSet<RequiredStatusCheck>> {
        if let Some(checks) = self.admission.get(clone_url) {
            return Some(checks.clone());
        }

        let Some(api) = api else {
            tracing::warn!(
                url = %clone_url,
                "skipping admission populate: {} not set",
                GH_TOKEN_ENV
            );
            return None;
        };

        let (owner, name) = match josh_github_changes::repo::parse_owner_repo(clone_url) {
            Ok(parts) => parts,
            Err(e) => {
                tracing::warn!(url = %clone_url, error = ?e, "could not parse owner/repo");
                return None;
            }
        };

        match tokio::runtime::Handle::current().block_on(fetch_required_checks(api, &owner, &name))
        {
            Ok(checks) => {
                tracing::info!(
                    url = %clone_url,
                    count = checks.len(),
                    "populated admission entry"
                );
                self.admission.insert(clone_url.to_string(), checks.clone());
                Some(checks)
            }
            Err(e) => {
                tracing::error!(
                    url = %clone_url,
                    error = ?e,
                    "failed to fetch required checks; will retry on next webhook"
                );
                None
            }
        }
    }

    pub fn get_or_init_pr_admission(
        &mut self,
        pr_node_id: &str,
        clone_url: &str,
        api: Option<&GithubApiConnection>,
    ) -> Option<&mut AdmissionState> {
        if !self.pr_admissions.contains_key(pr_node_id) {
            let required = self.get_or_fetch_admission(clone_url, api)?;
            let maintainers = fetch_maintainers(clone_url, api);
            let state = AdmissionState {
                required_checks: required.into_iter().map(|c| (c, false)).collect(),
                maintainer_reviews: BTreeMap::new(),
                maintainers: maintainers.into_iter().collect(),
            };
            tracing::info!(
                pr = %pr_node_id,
                url = %clone_url,
                "initialized pr_admission entry"
            );
            self.pr_admissions.insert(pr_node_id.to_string(), state);
        }
        self.pr_admissions.get_mut(pr_node_id)
    }

    pub fn upsert_candidate(&mut self, pr: CandidatePr) {
        self.candidates.insert(pr.node_id.clone(), pr);
    }

    pub fn remove_candidate(&mut self, pr_node_id: &str) {
        self.candidates.remove(pr_node_id);
        self.pr_admissions.remove(pr_node_id);
    }

    pub fn get_candidate(&self, pr_node_id: &str) -> Option<&CandidatePr> {
        self.candidates.get(pr_node_id)
    }
}

fn fetch_maintainers(clone_url: &str, api: Option<&GithubApiConnection>) -> Vec<String> {
    let Some(api) = api else {
        return Vec::new();
    };
    let (owner, name) = match josh_github_changes::repo::parse_owner_repo(clone_url) {
        Ok(parts) => parts,
        Err(_) => return Vec::new(),
    };
    match tokio::runtime::Handle::current().block_on(api.get_maintainers(&owner, &name)) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(url = %clone_url, error = ?e, "failed to fetch maintainers");
            Vec::new()
        }
    }
}

pub(crate) fn lookup_open_prs_by_sha(
    api: Option<&GithubApiConnection>,
    clone_url: &str,
    sha: &str,
) -> Vec<String> {
    let Some(api) = api else {
        return Vec::new();
    };
    let (owner, name) = match josh_github_changes::repo::parse_owner_repo(clone_url) {
        Ok(parts) => parts,
        Err(e) => {
            tracing::warn!(url = %clone_url, error = ?e, "could not parse owner/repo");
            return Vec::new();
        }
    };
    match tokio::runtime::Handle::current()
        .block_on(api.find_open_prs_by_head_sha(&owner, &name, sha))
    {
        Ok(prs) => prs.into_iter().map(|(id, _)| id).collect(),
        Err(e) => {
            tracing::warn!(url = %clone_url, sha = %sha, error = ?e, "failed to look up PRs by SHA");
            Vec::new()
        }
    }
}

async fn fetch_required_checks(
    api: &GithubApiConnection,
    owner: &str,
    name: &str,
) -> anyhow::Result<BTreeSet<RequiredStatusCheck>> {
    let rulesets = api.get_repository_rulesets(owner, name).await?;
    let mut checks = BTreeSet::new();
    for ruleset in rulesets {
        if !ruleset.is_active() {
            continue;
        }
        match api.get_ruleset_required_checks(&ruleset.id).await {
            Ok(rs_checks) => checks.extend(rs_checks),
            Err(e) => tracing::warn!(
                ruleset = %ruleset.id,
                error = ?e,
                "failed to fetch checks for ruleset; skipping"
            ),
        }
    }

    Ok(checks)
}

pub(crate) fn handle_fetch(
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    mut state: CqActorState,
) -> anyhow::Result<CqActorState> {
    let repo = transaction.repo();
    let head_commit = repo
        .head()
        .context("Failed to get HEAD")?
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let link_files =
        josh_core::link::find_link_files(repo, &head_tree).context("Failed to find link files")?;

    let mut remotes: Vec<(PathBuf, String, String)> = Vec::new();
    for (path, filter) in &link_files {
        if let (Some(remote), Some(commit)) = (filter.get_meta("remote"), filter.get_meta("commit"))
        {
            remotes.push((path.clone(), remote, commit));
        }
    }

    if remotes.is_empty() {
        tracing::info!("no tracked remotes found");
        return Ok(state);
    }

    let signature = josh_link::make_signature(repo)?;
    let mut links_to_update: Vec<(PathBuf, git2::Oid)> = Vec::new();

    for (path, url, current_commit) in &remotes {
        spawn_git_command(repo.path(), &["fetch", url.as_str()], &[])
            .with_context(|| format!("Failed to fetch from {}", url))?;

        let refs = crate::remote::list_refs(url)
            .with_context(|| format!("Failed to list refs for {}", url))?;

        if let Some(head_oid) = refs.get("HEAD") {
            if head_oid.to_string() != *current_commit {
                links_to_update.push((path.clone(), *head_oid));
            }
        }
    }

    if !links_to_update.is_empty() {
        let count = links_to_update.len();
        match josh_link::update_links(repo, transaction, &head_commit, links_to_update, &signature)?
        {
            Some(result) => {
                repo.head()?
                    .set_target(result.commit_with_updates, "josh-cq fetch")
                    .context("Failed to update HEAD")?;
            }
            None => {
                tracing::debug!("link files already up to date");
            }
        }
        tracing::info!(count, "updated link file(s)");
    }

    for (_, url, _) in &remotes {
        let (owner, repo_name) = match josh_github_changes::repo::parse_owner_repo(url) {
            Ok(parts) => parts,
            Err(e) => {
                tracing::warn!(url = %url, error = ?e, "could not parse owner/repo");
                continue;
            }
        };

        let Some(api) = api else {
            tracing::warn!(url = %url, "skipping PR discovery: no API connection");
            continue;
        };

        let prs = match tokio::runtime::Handle::current()
            .block_on(api.get_open_pull_requests(&owner, &repo_name))
        {
            Ok(prs) => prs,
            Err(e) => {
                tracing::warn!(url = %url, error = ?e, "failed to fetch open PRs");
                continue;
            }
        };

        for pr in &prs {
            state.upsert_candidate(CandidatePr {
                node_id: pr.node_id.clone(),
                number: pr.number,
                repo_url: url.clone(),
                head_sha: pr.head_sha.clone(),
                head_branch: pr.head_branch.clone(),
                base_sha: pr.base_sha.clone(),
                base_branch: pr.base_branch.clone(),
                title: pr.title.clone(),
            });

            state.get_or_init_pr_admission(&pr.node_id, url, Some(api));

            match tokio::runtime::Handle::current()
                .block_on(api.get_pr_reviews(&owner, &repo_name, pr.number))
            {
                Ok(reviews) => {
                    if let Some(admission) = state.pr_admissions.get_mut(&pr.node_id) {
                        admission.apply_review_states(&reviews);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        pr = %pr.node_id,
                        error = ?e,
                        "failed to fetch PR reviews"
                    );
                }
            }
        }

        tracing::info!(
            url = %url,
            count = prs.len(),
            "discovered open PRs"
        );
    }

    Ok(state)
}

/// Select the first admissible PR from the candidate pool.
///
/// Iterates candidates in insertion order (BTreeMap), checks each one's
/// admission state, and returns the first that passes `admissible()`.
fn select_candidate(state: &CqActorState) -> Option<CandidatePr> {
    for (node_id, candidate) in &state.candidates {
        if let Some(admission) = state.pr_admissions.get(node_id) {
            if admission.admissible() {
                tracing::info!(
                    pr = %node_id,
                    number = candidate.number,
                    repo = %candidate.repo_url,
                    "selected admissible PR"
                );
                return Some(candidate.clone());
            }
        }
    }
    None
}

/// Run evaluate→step while admissible PRs remain.
/// Called after every event (webhook or tick) to try to make progress.
pub(crate) fn run_queue_cycle(
    state: &mut CqActorState,
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
) {
    loop {
        let candidate = match select_candidate(state) {
            Some(c) => c,
            None => {
                tracing::debug!("no admissible PRs");
                break;
            }
        };

        match handle_step(&candidate, transaction, api, state) {
            Ok(()) => {
                tracing::info!(
                    pr = %candidate.node_id,
                    number = candidate.number,
                    repo = %candidate.repo_url,
                    "merged PR"
                );
            }
            Err(e) => {
                tracing::error!(
                    pr = %candidate.node_id,
                    number = candidate.number,
                    error = ?e,
                    "failed to merge PR; will retry next cycle"
                );
                break;
            }
        }
    }
}

fn spawn_git_command_stdout(repo_path: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(repo_path)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .context("failed to execute git command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "git {} exited with {}: {}",
            args.join(" "),
            output.status,
            stderr.trim()
        ));
    }

    String::from_utf8(output.stdout).context("git output was not valid UTF-8")
}

/// Merge an admissible PR: compute merge locally, push to remote main,
/// update `.link.josh`, close the PR, and remove from the candidate pool.
fn handle_step(
    candidate: &CandidatePr,
    transaction: &josh_core::cache::Transaction,
    api: Option<&GithubApiConnection>,
    state: &mut CqActorState,
) -> anyhow::Result<()> {
    let repo = transaction.repo();
    let head_commit = repo
        .head()
        .context("Failed to get HEAD")?
        .peel_to_commit()
        .context("Failed to peel HEAD to commit")?;
    let head_tree = head_commit.tree().context("Failed to get HEAD tree")?;

    let link_files =
        josh_core::link::find_link_files(repo, &head_tree).context("Failed to find link files")?;

    let mut main_sha = None;
    let mut link_path = None;
    for (path, filter) in &link_files {
        if filter.get_meta("remote").as_deref() == Some(candidate.repo_url.as_str()) {
            main_sha = filter.get_meta("commit");
            link_path = Some(path.clone());
            break;
        }
    }

    let main_sha = main_sha.context("No link file found for remote")?;
    let link_path = link_path.context("No link file found for remote")?;

    let merge_base =
        spawn_git_command_stdout(repo.path(), &["merge-base", &main_sha, &candidate.head_sha])?;
    let merge_base = merge_base.trim().to_string();

    let merged_tree = spawn_git_command_stdout(
        repo.path(),
        &["merge-tree", &merge_base, &main_sha, &candidate.head_sha],
    )?;
    let merged_tree = merged_tree.trim().to_string();

    if merged_tree.len() != 40 || !merged_tree.chars().all(|c| c.is_ascii_hexdigit()) {
        tracing::warn!(
            pr = %candidate.node_id,
            "merge conflict detected; skipping PR"
        );
        return Ok(());
    }

    let message = format!("Merge PR #{}: {}", candidate.number, candidate.title);
    let merge_commit = spawn_git_command_stdout(
        repo.path(),
        &[
            "commit-tree",
            "-p",
            &main_sha,
            "-p",
            &candidate.head_sha,
            "-m",
            &message,
            &merged_tree,
        ],
    )?;
    let merge_commit = merge_commit.trim().to_string();

    let target_ref = format!("refs/heads/{}", candidate.base_branch);
    let refspec = format!("{}:{}", merge_commit, target_ref);
    spawn_git_command(repo.path(), &["push", &candidate.repo_url, &refspec], &[])?;

    let merge_oid = merge_commit
        .parse::<git2::Oid>()
        .context("Failed to parse merge commit OID")?;
    let signature = make_signature(repo)?;
    match josh_link::update_links(
        repo,
        transaction,
        &head_commit,
        vec![(link_path, merge_oid)],
        &signature,
    )? {
        Some(result) => {
            repo.head()?
                .set_target(result.commit_with_updates, "josh-cq merge")
                .context("Failed to update HEAD")?;
        }
        None => {
            tracing::debug!("link file already up to date");
        }
    }

    if let Some(api) = api {
        let comment = format!("Merged by Josh merge queue as `{}`.", merge_commit);
        tokio::runtime::Handle::current().block_on(async {
            api.add_pr_comment(&candidate.node_id, &comment).await?;
            api.close_pull_request(&candidate.node_id).await
        })?;
    }

    state.remove_candidate(&candidate.node_id);

    Ok(())
}

pub(crate) fn process_admission_events(
    state: &mut CqActorState,
    events: &[(String, AdmissionRelevantEvent<'_>)],
    clone_url: &str,
    api: Option<&GithubApiConnection>,
) {
    for (pr_node_id, evt) in events {
        let Some(admission) = state.get_or_init_pr_admission(pr_node_id, clone_url, api) else {
            continue;
        };
        match evt {
            AdmissionRelevantEvent::PullRequestReview(e) => {
                admission.process_pr_review_events(std::slice::from_ref(e));
            }
            AdmissionRelevantEvent::CheckRun(e) => {
                admission.process_check_run_events(std::slice::from_ref(e));
            }
        }
    }
}
