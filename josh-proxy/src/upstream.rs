use crate::http::RetryableError;
use crate::service::{JoshProxyService, UpstreamProtocol};
use crate::{FetchError, auth, run_git_with_auth};
use anyhow::{Context, anyhow};
use backon::BackoffBuilder;
use std::str::FromStr;

use josh_changes::{PushMode, build_to_push};
use josh_core::cache::{CacheStack, TransactionContext};
use josh_core::git::HeadRef;
use josh_gerrit_changes::baseref_and_options;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::IntoResponse;
use reqwest::StatusCode;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub enum RemoteAuth {
    Http { auth: auth::Handle },
    Ssh { auth_socket: PathBuf },
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct RepoUpdate {
    pub refs: std::collections::HashMap<String, (String, String)>,
    pub remote_url: String,
    pub remote_auth: RemoteAuth,
    pub port: u16,
    pub filter_spec: String,
    pub base_ns: String,
    pub git_ns: String,
    pub git_dir: String,
    pub mirror_git_dir: String,
    pub context_propagator: std::collections::HashMap<String, String>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(default)]
#[derive(Default)]
pub struct PushOptions {
    pub merge: bool,
    pub allow_orphans: bool,
    pub edit: bool,
    pub create: bool,
    pub force: bool,
    pub base: Option<String>,
    pub author: Option<String>,
}

pub trait Upstream {
    fn upstream(&self, protocol: UpstreamProtocol) -> Option<String>;
}

pub struct HttpUpstream(pub String);

impl<S: Upstream + Send + Sync> FromRequestParts<S> for HttpUpstream {
    type Rejection = axum::response::Response;

    async fn from_request_parts(_: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        if let Some(remote) = state.upstream(UpstreamProtocol::Http) {
            Ok(HttpUpstream(remote))
        } else {
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "Upstream of requested type is not configured",
            )
                .into_response())
        }
    }
}

async fn fetch_needed(
    service: Arc<JoshProxyService>,
    remote_url: &str,
    upstream_repo: &str,
    force: bool,
    head_refs: &nonempty::NonEmpty<HeadRef>,
    head_ref_resolved: Option<&str>,
) -> Result<bool, FetchError> {
    let fetch_timer_ok = {
        let last = {
            let fetch_timers = service.fetch_timers.read().unwrap();
            fetch_timers.get(remote_url).cloned()
        };

        if let Some(last) = last {
            let since = std::time::Instant::now().duration_since(last);
            let max = std::time::Duration::from_secs(service.cache_duration);

            tracing::trace!("last: {:?}, since: {:?}, max: {:?}", last, since, max);
            since < max
        } else {
            false
        }
    };

    let resolve_cache_ref = |head_ref: &HeadRef| {
        let head_ref = head_ref.clone();
        let upstream_repo = upstream_repo.to_string();
        let service = service.clone();

        tokio::task::spawn_blocking(move || {
            let transaction = service.open_mirror(Some(&format!(
                "refs/josh/upstream/{}/",
                &josh_core::to_ns(&upstream_repo)
            )))?;

            if let HeadRef::ExplicitSha(_, oid) = head_ref {
                return Ok(transaction.odb().contains(oid).then_some(oid));
            }

            transaction.resolve_ref(&transaction.refname(head_ref.get()))
        })
    };

    match (force, fetch_timer_ok, head_refs, head_ref_resolved) {
        (false, true, head_refs, _) => {
            for head_ref in head_refs {
                if resolve_cache_ref(head_ref)
                    .await?
                    .map_err(FetchError::from_anyhow)?
                    .is_none()
                {
                    return Ok(true);
                }
            }

            tracing::trace!("cache ref resolved");
            return Ok(false);
        }
        (false, false, head_refs, Some(head_ref_resolved)) if head_refs.tail.is_empty() => {
            if let Some(oid) = resolve_cache_ref(&head_refs.head)
                .await?
                .map_err(FetchError::from_anyhow)?
                && oid.to_string() == head_ref_resolved
            {
                tracing::trace!("cache ref resolved and matches");
                return Ok(false);
            }
        }
        _ => (),
    };

    Ok(true)
}

#[tracing::instrument(skip(service))]
pub async fn fetch_upstream(
    service: Arc<JoshProxyService>,
    upstream_repo: &str,
    remote_auth: &RemoteAuth,
    remote_url: String,
    head_refs: &nonempty::NonEmpty<HeadRef>,
    head_ref_resolved: Option<&str>,
    force: bool,
) -> Result<(), FetchError> {
    let upstream_repo = upstream_repo.to_owned();
    let to_mirror_ref = |r: &str| {
        format!(
            "+{}:refs/josh/upstream/{}/{}",
            r,
            josh_core::to_ns(&upstream_repo),
            r
        )
    };
    let refspecs: Vec<String> = ["HEAD*", "refs/josh/*", "refs/heads/*", "refs/tags/*"]
        .into_iter()
        .map(to_mirror_ref)
        .chain(head_refs.iter().filter_map(|head_ref| match head_ref {
            HeadRef::ExplicitHead | HeadRef::Implicit => None,
            HeadRef::ExplicitRef(r) if r.starts_with("refs/heads/") => None,
            HeadRef::ExplicitRef(r) => Some(to_mirror_ref(r)),
            // Objects fetched by SHA stay in the ODB without a ref, so do not create one per SHA
            HeadRef::ExplicitSha(sha, _) => Some(sha.clone()),
        }))
        .collect();

    // Check if we really need to fetch before locking the semaphore. This avoids
    // A "no fetch" case waiting for some already running fetch just to do nothing.
    if !fetch_needed(
        service.clone(),
        &remote_url,
        &upstream_repo,
        force,
        head_refs,
        head_ref_resolved,
    )
    .await?
    {
        return Ok(());
    }

    // Callers must pass `upstream_repo` in canonical form (no leading slash):
    // this string keys fetch_permits / head_symref_map / poll, and different
    // spellings of the same repo would split the lock and let concurrent
    // fetches race on the same mirror refs.
    let semaphore = service
        .fetch_permits
        .lock()
        .unwrap()
        .entry(upstream_repo.clone())
        .or_insert(Arc::new(tokio::sync::Semaphore::new(1)))
        .clone();

    let backoff = crate::http::make_exponential_backoff(service.http_retry).build();
    let mut fetch_result = Ok(());

    for delay in std::iter::once(None).chain(backoff.map(|d| Some(d))) {
        if let Some(delay) = delay {
            tracing::warn!(delay = ?delay, "fetch got server error, retrying");
            tokio::time::sleep(delay).await;
        }

        let permit = semaphore.acquire().await;

        // Check the fetch condition once again after locking the semaphore, as an unknown
        // amount of time might have passed and the outcome of this check might have changed
        // while waiting.
        if !fetch_needed(
            service.clone(),
            &remote_url,
            &upstream_repo,
            force,
            head_refs,
            head_ref_resolved,
        )
        .await?
        {
            return Ok(());
        }

        fetch_result = {
            let span = tracing::span!(tracing::Level::INFO, "fetch_refs_from_url");

            let mirror_path = service.repo_path.join("mirror");
            let remote_url = remote_url.clone();
            let remote_auth = remote_auth.clone();
            let refspecs = refspecs.clone();

            tokio::task::spawn_blocking(move || {
                let _span_guard = span.enter();
                crate::fetch_refs_from_url(&mirror_path, &remote_url, &refspecs, &remote_auth)
            })
            .await?
        };

        let hres = {
            let span = tracing::span!(tracing::Level::INFO, "get_head");

            let mirror_path = service.repo_path.join("mirror");
            let remote_url = remote_url.clone();
            let remote_auth = remote_auth.clone();

            tokio::task::spawn_blocking(move || {
                let _span_guard = span.enter();
                crate::get_head(&mirror_path, &remote_url, &remote_auth)
            })
            .await?
        };

        let heads_map = service.head_symref_map.clone();

        if let Ok(hres) = hres {
            heads_map
                .write()
                .unwrap()
                .insert(upstream_repo.clone(), hres);
        }

        // Set the timer before the permit is released so the next waiter sees a fresh fetch.
        if fetch_result.is_ok() {
            service
                .fetch_timers
                .write()
                .unwrap()
                .insert(remote_url.clone(), std::time::Instant::now());
        }

        std::mem::drop(permit);

        match &fetch_result {
            Err(RetryableError::Retryable(e)) => {
                tracing::warn!(?e, "fetch got retryable error, will retry if budget left");
                continue;
            }
            _ => break,
        }
    }

    match (fetch_result, remote_auth) {
        (Ok(_), RemoteAuth::Http { auth }) => {
            if let Some((auth_user, _)) = auth.parse()
                && matches!(&service.poll_user, Some(user) if auth_user == user.as_str())
            {
                service
                    .poll
                    .lock()
                    .unwrap()
                    .insert((upstream_repo, auth.clone(), remote_url));
            }

            Ok(())
        }
        (Ok(_), _) => Ok(()),
        (Err(e), _) => Err(e.into_inner()),
    }
}

pub fn process_repo_update(repo_update: RepoUpdate) -> anyhow::Result<String> {
    let push_options_path = std::path::PathBuf::from(&repo_update.git_dir)
        .join("refs/namespaces")
        .join(&repo_update.git_ns)
        .join("push_options");

    let push_options = std::fs::read_to_string(&push_options_path)?;
    std::fs::remove_file(push_options_path).ok();

    let push_options: PushOptions =
        serde_json::from_str(&push_options).context("Failed to parse push options")?;

    tracing::debug!(
        push_options = ?push_options,
        "process_repo_update"
    );

    let cache = std::sync::Arc::new(CacheStack::default());

    if let Some((refname, (old, new))) = repo_update.refs.iter().next() {
        let transaction_ctx = TransactionContext::new(&repo_update.git_dir, cache.clone())
            .with_ref_prefix(format!("refs/josh/upstream/{}/", repo_update.base_ns))
            .with_mem_odb_limit(crate::MAX_MEM_PACK_SIZE);

        let transaction_mirror_ctx =
            TransactionContext::new(&repo_update.mirror_git_dir, cache.clone())
                .with_ref_prefix(format!("refs/josh/upstream/{}/", repo_update.base_ns))
                .with_mem_odb_limit(crate::MAX_MEM_PACK_SIZE);

        let transaction = transaction_ctx.open()?;
        let transaction_mirror = transaction_mirror_ctx.open()?;

        transaction
            .add_disk_alternate(transaction_mirror.path().join("objects").to_str().unwrap())?;

        let old = gix_hash::ObjectId::from_str(old)?;
        let author = push_options.author.as_deref().unwrap_or("");
        let (baseref, push_to, options, push_mode) = baseref_and_options(refname, author)?;

        let old = if old == gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
            let rev = format!("refs/namespaces/{}/{}", repo_update.git_ns, &baseref);
            let oid = transaction.resolve_ref(&rev)?.unwrap_or(old);

            tracing::debug!(
                old_oid = ?oid,
                rev = %rev,
                "resolve_old"
            );

            oid
        } else {
            tracing::debug!(
                old_oid = ?old,
                refname = %refname,
                "resolve_old"
            );

            old
        };

        let original_target_ref = if let Some(base) = &push_options.base {
            // Allow user to use just the branch name as the base:
            let full_path_base_refname =
                transaction_mirror.refname(&format!("refs/heads/{}", base));
            if transaction_mirror
                .resolve_ref(&full_path_base_refname)?
                .is_some()
            {
                full_path_base_refname
            } else {
                transaction_mirror.refname(base)
            }
        } else {
            transaction_mirror.refname(&baseref)
        };

        let original_target =
            if let Some(oid) = transaction_mirror.resolve_ref(&original_target_ref)? {
                tracing::debug!(
                    original_target_oid = ?oid,
                    original_target_ref = %original_target_ref,
                    "resolve_original_target"
                );

                oid
            } else {
                tracing::debug!(
                    original_target_ref = %original_target_ref,
                    "resolve_original_target"
                );

                return Err(anyhow!(indoc::formatdoc!(
                    r###"
                    Reference {:?} does not exist on remote.
                    If you want to create it, pass "-o base=<basebranch>" or "-o base=path/to/ref"
                    to specify a base branch/reference.
                    "###,
                    baseref
                )));
            };

        let reparent_orphans = if push_options.create {
            Some(original_target)
        } else {
            None
        };

        let filter = josh_core::filter::parse(&repo_update.filter_spec)?;
        let new_oid = gix_hash::ObjectId::from_str(new)?;
        let backward_new_oid = {
            let unapply_result = josh_core::history::unapply_filter(
                &transaction,
                filter,
                original_target,
                old,
                new_oid,
                if push_options.merge || push_options.allow_orphans {
                    josh_core::history::OrphansMode::Keep
                } else if push_options.edit {
                    josh_core::history::OrphansMode::Remove
                } else {
                    josh_core::history::OrphansMode::Fail
                },
                reparent_orphans,
                None,
            )?;

            tracing::debug!(
                processed_old = ?old,
                unapply_result = ?unapply_result,
                "unapply_filter"
            );

            unapply_result
        };

        let oid_to_push = if push_options.merge {
            if let Some(base_commit_id) = transaction_mirror.resolve_ref(&original_target_ref)? {
                let signature = josh_core::git::josh_actor_signature()?;
                let odb = transaction.odb();
                let merged_tree =
                    josh_core::objects::merge_commits(odb, base_commit_id, backward_new_oid, None)?;
                josh_core::objects::write_commit(
                    odb,
                    merged_tree,
                    &[base_commit_id, backward_new_oid],
                    &signature,
                    &signature,
                    &format!("Merge from {}", &repo_update.filter_spec),
                )?
            } else {
                return Err(anyhow!("josh_merge failed"));
            }
        } else {
            backward_new_oid
        };

        let ref_with_options = if !options.is_empty() {
            format!("{}{}{}", push_to, "%", options.join(","))
        } else {
            push_to
        };

        let to_push = build_to_push(
            &transaction,
            &push_mode,
            &baseref,
            &ref_with_options,
            oid_to_push,
            original_target,
        )?;

        let mut resp = vec![];

        for push_ref in to_push {
            let force_push = !matches!(push_mode, PushMode::Normal) || push_options.force;

            let (text, status) = push_head_url(
                &transaction,
                &format!("{}/objects", repo_update.mirror_git_dir),
                push_ref.oid,
                &push_ref.ref_name,
                &repo_update.remote_url,
                &repo_update.remote_auth,
                &repo_update.git_ns,
                &push_ref.change_id,
                force_push,
            )?;

            if status != 0 {
                return Err(anyhow!("{}", text));
            }

            resp.push(text.to_string());
            let mut warnings = josh_core::filter::compute_warnings(
                &transaction,
                filter,
                josh_core::objects::CommitData::read(transaction.odb(), push_ref.oid)?.tree_id()?,
                Some(push_ref.oid),
            );

            if !warnings.is_empty() {
                resp.push("warnings:".to_string());
                resp.append(&mut warnings);
            }
        }

        let reapply = josh_core::filter::apply_to_commit(filter, oid_to_push, &transaction)?;

        if new_oid != reapply {
            if std::env::var("JOSH_REWRITE_REFS").is_ok() {
                transaction.update_ref(
                    &format!(
                        "refs/josh/rewrites/{}/{:?}/r_{}",
                        repo_update.base_ns,
                        filter.id(),
                        reapply
                    ),
                    josh_core::cache::Expected::Any,
                    reapply,
                    "reapply",
                )?;
            }

            tracing::debug!(
                new_oid = ?new_oid,
                reapply = ?reapply,
                "rewrite"
            );

            let text = format!("REWRITE({} -> {})", new_oid, reapply);
            resp.push(text);
        }

        return Ok(resp.join("\n"));
    }

    Ok("".to_string())
}

#[allow(clippy::too_many_arguments)]
pub fn push_head_url(
    transaction: &josh_core::cache::Transaction,
    alternate: &str,
    oid: gix_hash::ObjectId,
    refname: &str,
    url: &str,
    remote_auth: &RemoteAuth,
    namespace: &str,
    display_name: &str,
    force: bool,
) -> anyhow::Result<(String, i32)> {
    let push_temp_ref = format!("refs/{}", &namespace);
    let push_refspec = format!("{}:{}", &push_temp_ref, &refname);

    let mut cmd = vec!["git", "push"];
    if force {
        cmd.push("--force")
    }
    cmd.push(url);
    cmd.push(&push_refspec);

    transaction.update_ref(
        &push_temp_ref,
        josh_core::cache::Expected::Any,
        oid,
        "push_head_url",
    )?;
    // Flush before the external `git push` below reads these objects from disk.
    transaction.flush_mem_odb()?;
    let push_result = run_git_with_auth(
        transaction.path(),
        &cmd,
        remote_auth,
        Some(alternate.to_owned()),
    );
    transaction.delete_ref(&push_temp_ref, josh_core::cache::Expected::Any)?;
    let (stdout, stderr, status) = push_result?;

    tracing::debug!(
        stdout = %stdout,
        stderr = %stderr,
        status = %status,
        "push_head_url: run_git"
    );

    let stderr = stderr.replace(&push_temp_ref, display_name);
    Ok((stderr, status))
}
