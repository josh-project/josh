use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::response::Response;
use git2::Signature;
use tokio::sync::{mpsc, oneshot};

use crate::repo::TreeMode;

const GIT_AUTHOR_NAME: &str = "test";
const GIT_AUTHOR_EMAIL: &str = "test@test.com";
const REFS_HEADS_PREFIX: &str = "refs/heads/";

pub(crate) enum ActorMsg {
    Commit {
        mode: TreeMode,
        message: String,
        branch_ref: String,
        response: oneshot::Sender<anyhow::Result<git2::Oid>>,
    },
    CreateBranch {
        name: String,
        from_ref: String,
        response: oneshot::Sender<anyhow::Result<git2::Oid>>,
    },
    GetHead {
        branch_ref: String,
        response: oneshot::Sender<anyhow::Result<git2::Oid>>,
    },
    ServeGitHttp {
        request: axum::extract::Request,
        response: oneshot::Sender<Response<Body>>,
    },
}

fn signature() -> Signature<'static> {
    Signature::new(GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, &git2::Time::new(0, 0))
        .expect("creating git signature")
}

fn do_commit(
    repo_path: &Path,
    mode: &TreeMode,
    message: &str,
    branch_ref: &str,
) -> anyhow::Result<git2::Oid> {
    let repo = git2::Repository::open(repo_path)?;
    let sig = signature();

    let parent_commit = repo.revparse_single(branch_ref).ok().and_then(|obj| {
        if let Ok(commit) = obj.into_commit() {
            Some(commit)
        } else {
            None
        }
    });

    let parent_tree = match mode {
        TreeMode::Overlay(_) => parent_commit.as_ref().and_then(|c| c.tree().ok()),
        TreeMode::Replace(_) => None,
    };

    let entries = match mode {
        TreeMode::Overlay(e) | TreeMode::Replace(e) => e,
    };

    let mut treebuilder = repo.treebuilder(parent_tree.as_ref())?;
    for entry in entries {
        let blob_oid = repo.blob(entry.content.as_bytes())?;
        treebuilder.insert(
            entry.path.as_str(),
            blob_oid,
            i32::from(git2::FileMode::Blob),
        )?;
    }
    let tree_oid = treebuilder.write()?;
    let tree = repo.find_tree(tree_oid)?;

    let parents: Vec<&git2::Commit> = parent_commit.iter().collect();
    let commit_oid = repo.commit(Some(branch_ref), &sig, &sig, message, &tree, &parents)?;

    Ok(commit_oid)
}

fn do_create_branch(repo_path: &Path, name: &str, from_ref: &str) -> anyhow::Result<git2::Oid> {
    let repo = git2::Repository::open(repo_path)?;
    let branch_ref = format!("{REFS_HEADS_PREFIX}{name}");
    let from_oid = repo.revparse_single(from_ref)?.id();
    repo.reference(&branch_ref, from_oid, true, "create branch")?;
    Ok(from_oid)
}

fn do_get_head(repo_path: &Path, branch_ref: &str) -> anyhow::Result<git2::Oid> {
    let repo = git2::Repository::open(repo_path)?;
    let obj = repo.revparse_single(branch_ref)?;
    Ok(obj.id())
}

pub(crate) async fn run_actor(mut rx: mpsc::UnboundedReceiver<ActorMsg>, repo_path: PathBuf) {
    fn send_response<T>(tx: oneshot::Sender<T>, value: T) {
        if tx.send(value).is_err() {
            tracing::error!("failed to send response");
        }
    }

    fn send_join_result(
        tx: oneshot::Sender<anyhow::Result<git2::Oid>>,
        result: Result<anyhow::Result<git2::Oid>, tokio::task::JoinError>,
        label: &str,
    ) {
        let value = match result {
            Ok(r) => r,
            Err(e) => Err(anyhow::anyhow!("{} task panicked: {}", label, e)),
        };
        send_response(tx, value);
    }

    while let Some(msg) = rx.recv().await {
        let repo_path = repo_path.clone();
        match msg {
            ActorMsg::Commit {
                mode,
                message,
                branch_ref,
                response,
            } => {
                let result = tokio::task::spawn_blocking(move || {
                    do_commit(&repo_path, &mode, &message, &branch_ref)
                })
                .await;
                send_join_result(response, result, "commit");
            }
            ActorMsg::CreateBranch {
                name,
                from_ref,
                response,
            } => {
                let result = tokio::task::spawn_blocking(move || {
                    do_create_branch(&repo_path, &name, &from_ref)
                })
                .await;
                send_join_result(response, result, "create_branch");
            }
            ActorMsg::GetHead {
                branch_ref,
                response,
            } => {
                let result =
                    tokio::task::spawn_blocking(move || do_get_head(&repo_path, &branch_ref)).await;
                send_join_result(response, result, "get_head");
            }
            ActorMsg::ServeGitHttp { request, response } => {
                let result = crate::git_http::serve(&repo_path, request).await;
                send_response(response, result);
            }
        }
    }
}
