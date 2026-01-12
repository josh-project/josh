use crate::service::{JoshProxyService, UpstreamProtocol};
use crate::{FetchError, auth, proxy_commit_signature, run_git_with_auth};

use josh_core::cache::{CacheStack, TransactionContext};
use josh_core::changes::{PushMode, baseref_and_options, build_to_push};
use josh_core::josh_error;

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
    pub port: String,
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
    head_ref: Option<&str>,
    head_ref_resolved: Option<&str>,
) -> Result<bool, FetchError> {
    let fetch_timer_ok = {
        let last = {
            let fetch_timers = service.fetch_timers.read()?;
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

    let resolve_cache_ref = |cache_ref: &str| {
        let cache_ref = cache_ref.to_string();
        let upstream_repo = upstream_repo.to_string();
        let service = service.clone();

        tokio::task::spawn_blocking(move || {
            let transaction = service.open_mirror(Some(&format!(
                "refs/josh/upstream/{}/",
                &josh_core::to_ns(&upstream_repo)
            )))?;

            match transaction
                .repo()
                .refname_to_id(&transaction.refname(&cache_ref))
            {
                Ok(oid) => Ok(Some(oid)),
                Err(_) => Ok(None),
            }
        })
    };

    match (force, fetch_timer_ok, head_ref, head_ref_resolved) {
        (false, true, None, _) => return Ok(false),
        (false, true, Some(head_ref), _) => {
            if (resolve_cache_ref(head_ref)
                .await?
                .map_err(FetchError::from_josh_error)?)
            .is_some()
            {
                tracing::trace!("cache ref resolved");
                return Ok(false);
            }
        }
        (false, false, Some(head_ref), Some(head_ref_resolved)) => {
            if let Some(oid) = resolve_cache_ref(head_ref)
                .await?
                .map_err(FetchError::from_josh_error)?
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
    head_ref: Option<&str>,
    head_ref_resolved: Option<&str>,
    force: bool,
) -> Result<(), FetchError> {
    let upstream_repo = upstream_repo.to_owned();
    let refs_to_fetch = match head_ref {
        Some(head_ref) if head_ref != "HEAD" && !head_ref.starts_with("refs/heads/") => {
            vec![
                "HEAD*",
                "refs/josh/*",
                "refs/heads/*",
                "refs/tags/*",
                head_ref,
            ]
        }
        _ => {
            vec!["HEAD*", "refs/josh/*", "refs/heads/*", "refs/tags/*"]
        }
    };

    let refs_to_fetch: Vec<_> = refs_to_fetch.iter().map(|x| x.to_string()).collect();

    // Check if we really need to fetch before locking the semaphore. This avoids
    // A "no fetch" case waiting for some already running fetch just to do nothing.
    if !fetch_needed(
        service.clone(),
        &remote_url,
        &upstream_repo,
        force,
        head_ref,
        head_ref_resolved,
    )
    .await?
    {
        return Ok(());
    }

    let semaphore = service
        .fetch_permits
        .lock()?
        .entry(upstream_repo.clone())
        .or_insert(Arc::new(tokio::sync::Semaphore::new(1)))
        .clone();
    let permit = semaphore.acquire().await;

    // Check the fetch condition once again after locking the semaphore, as an unknown
    // amount of time might have passed and the outcome of this check might have changed
    // while waiting.
    if !fetch_needed(
        service.clone(),
        &remote_url,
        &upstream_repo,
        force,
        head_ref,
        head_ref_resolved,
    )
    .await?
    {
        return Ok(());
    }

    let fetch_result = {
        let span = tracing::span!(tracing::Level::INFO, "fetch_refs_from_url");

        let mirror_path = service.repo_path.join("mirror");
        let upstream_repo = upstream_repo.clone();
        let remote_url = remote_url.clone();
        let remote_auth = remote_auth.clone();

        tokio::task::spawn_blocking(move || {
            let _span_guard = span.enter();
            crate::fetch_refs_from_url(
                &mirror_path,
                &upstream_repo,
                &remote_url,
                &refs_to_fetch,
                &remote_auth,
            )
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

    let fetch_timers = service.fetch_timers.clone();
    let heads_map = service.head_symref_map.clone();

    if let Ok(hres) = hres {
        heads_map.write()?.insert(upstream_repo.clone(), hres);
    }

    std::mem::drop(permit);

    if fetch_result.is_ok() {
        fetch_timers
            .write()?
            .insert(remote_url.clone(), std::time::Instant::now());
    }

    match (fetch_result, remote_auth) {
        (Ok(_), RemoteAuth::Http { auth }) => {
            if let Some((auth_user, _)) = auth.parse()
                && matches!(&service.poll_user, Some(user) if auth_user == user.as_str())
            {
                service
                    .poll
                    .lock()?
                    .insert((upstream_repo, auth.clone(), remote_url));
            }

            Ok(())
        }
        (Ok(_), _) => Ok(()),
        (Err(e), _) => Err(e),
    }
}

pub fn process_repo_update(repo_update: RepoUpdate) -> josh_core::JoshResult<String> {
    let push_options_path = std::path::PathBuf::from(&repo_update.git_dir)
        .join("refs/namespaces")
        .join(&repo_update.git_ns)
        .join("push_options");

    let push_options = std::fs::read_to_string(&push_options_path)?;
    std::fs::remove_file(push_options_path).ok();

    let push_options: PushOptions = serde_json::from_str(&push_options)
        .map_err(|e| josh_error(&format!("Failed to parse push options: {}", e)))?;

    tracing::debug!(
        push_options = ?push_options,
        "process_repo_update"
    );

    let cache = std::sync::Arc::new(CacheStack::default());
    let transaction_ctx = TransactionContext::new(&repo_update.git_dir, cache.clone());
    let transaction_mirror_ctx =
        TransactionContext::new(&repo_update.mirror_git_dir, cache.clone());

    if let Some((refname, (old, new))) = repo_update.refs.iter().next() {
        let transaction = transaction_ctx.open(Some(&format!(
            "refs/josh/upstream/{}/",
            repo_update.base_ns
        )))?;

        let transaction_mirror = transaction_mirror_ctx.open(Some(&format!(
            "refs/josh/upstream/{}/",
            repo_update.base_ns
        )))?;

        transaction.repo().odb()?.add_disk_alternate(
            transaction_mirror
                .repo()
                .path()
                .join("objects")
                .to_str()
                .unwrap(),
        )?;

        let old = git2::Oid::from_str(old)?;
        let (baseref, push_to, options, push_mode) = baseref_and_options(refname)?;

        let old = if old == git2::Oid::zero() {
            let rev = format!("refs/namespaces/{}/{}", repo_update.git_ns, &baseref);
            let oid = if let Ok(resolved) = transaction.repo().revparse_single(&rev) {
                resolved.id()
            } else {
                old
            };

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
                .repo()
                .refname_to_id(&full_path_base_refname)
                .is_ok()
            {
                full_path_base_refname
            } else {
                transaction_mirror.refname(base)
            }
        } else {
            transaction_mirror.refname(&baseref)
        };

        let original_target = if let Ok(oid) = transaction_mirror
            .repo()
            .refname_to_id(&original_target_ref)
        {
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

            return Err(josh_core::josh_error(&indoc::formatdoc!(
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

        let author = if let Some(author) = &push_options.author {
            author.to_string()
        } else {
            "".to_string()
        };

        let mut changes =
            if push_mode == PushMode::Stack || push_mode == PushMode::Split || !author.is_empty() {
                Some(vec![])
            } else {
                None
            };

        let filter = josh_core::filter::parse(&repo_update.filter_spec)?;
        let new_oid = git2::Oid::from_str(new)?;
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
                &mut changes,
            )?;

            tracing::debug!(
                processed_old = ?old,
                unapply_result = ?unapply_result,
                "unapply_filter"
            );

            unapply_result
        };

        let oid_to_push = if push_options.merge {
            let backward_commit = transaction.repo().find_commit(backward_new_oid)?;
            if let Ok(base_commit_id) = transaction_mirror
                .repo()
                .revparse_single(&original_target_ref)
                .map(|x| x.id())
            {
                let signature = proxy_commit_signature()?;
                let base_commit = transaction.repo().find_commit(base_commit_id)?;
                let merged_tree = transaction
                    .repo()
                    .merge_commits(&base_commit, &backward_commit, None)?
                    .write_tree_to(transaction.repo())?;
                transaction.repo().commit(
                    None,
                    &signature,
                    &signature,
                    &format!("Merge from {}", &repo_update.filter_spec),
                    &transaction.repo().find_tree(merged_tree)?,
                    &[&base_commit, &backward_commit],
                )?
            } else {
                return Err(josh_core::josh_error("josh_merge failed"));
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
            transaction.repo(),
            changes,
            push_mode,
            &baseref,
            &author,
            &ref_with_options,
            oid_to_push,
            old,
        )?;

        let mut resp = vec![];

        for (reference, oid, display_name) in to_push {
            let force_push = push_mode != PushMode::Normal || push_options.force;

            let (text, status) = push_head_url(
                transaction.repo(),
                &format!("{}/objects", repo_update.mirror_git_dir),
                oid,
                &reference,
                &repo_update.remote_url,
                &repo_update.remote_auth,
                &repo_update.git_ns,
                &display_name,
                force_push,
            )?;

            if status != 0 {
                return Err(josh_core::josh_error(&text));
            }

            resp.push(text.to_string());
            let mut warnings = josh_core::filter::compute_warnings(
                &transaction,
                filter,
                transaction.repo().find_commit(oid)?.tree()?,
            );

            if !warnings.is_empty() {
                resp.push("warnings:".to_string());
                resp.append(&mut warnings);
            }
        }

        let reapply = josh_core::filter::apply_to_commit(
            filter,
            &transaction.repo().find_commit(oid_to_push)?,
            &transaction,
        )?;

        if new_oid != reapply {
            if std::env::var("JOSH_REWRITE_REFS").is_ok() {
                transaction.repo().reference(
                    &format!(
                        "refs/josh/rewrites/{}/{:?}/r_{}",
                        repo_update.base_ns,
                        filter.id(),
                        reapply
                    ),
                    reapply,
                    true,
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
    repo: &git2::Repository,
    alternate: &str,
    oid: git2::Oid,
    refname: &str,
    url: &str,
    remote_auth: &RemoteAuth,
    namespace: &str,
    display_name: &str,
    force: bool,
) -> josh_core::JoshResult<(String, i32)> {
    let push_temp_ref = format!("refs/{}", &namespace);
    let push_refspec = format!("{}:{}", &push_temp_ref, &refname);

    let mut cmd = vec!["git", "push"];
    if force {
        cmd.push("--force")
    }
    cmd.push(url);
    cmd.push(&push_refspec);

    let mut fake_head = repo.reference(&push_temp_ref, oid, true, "push_head_url")?;
    let (stdout, stderr, status) =
        run_git_with_auth(repo.path(), &cmd, remote_auth, Some(alternate.to_owned()))?;
    fake_head.delete()?;

    tracing::debug!(
        stdout = %stdout,
        stderr = %stderr,
        status = %status,
        "push_head_url: run_git"
    );

    let stderr = stderr.replace(&push_temp_ref, display_name);
    Ok((stderr, status))
}
