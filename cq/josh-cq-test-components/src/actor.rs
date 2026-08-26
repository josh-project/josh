use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::response::Response;
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
        response: oneshot::Sender<anyhow::Result<gix::ObjectId>>,
    },
    CreateBranch {
        name: String,
        from_ref: String,
        response: oneshot::Sender<anyhow::Result<gix::ObjectId>>,
    },
    GetHead {
        branch_ref: String,
        response: oneshot::Sender<anyhow::Result<gix::ObjectId>>,
    },
    ServeGitHttp {
        request: axum::extract::Request,
        response: oneshot::Sender<Response<Body>>,
    },
}

fn signature() -> gix::actor::Signature {
    gix::actor::Signature {
        name: GIT_AUTHOR_NAME.into(),
        email: GIT_AUTHOR_EMAIL.into(),
        time: gix::actor::date::Time {
            seconds: 0,
            offset: 0,
        },
    }
}

fn do_commit(
    repo_path: &Path,
    mode: &TreeMode,
    message: &str,
    branch_ref: &str,
) -> anyhow::Result<gix::ObjectId> {
    let repo = gix::open(repo_path)?;
    let parent_commit = repo.rev_parse_single(branch_ref).ok().map(|id| id.detach());
    let parent_tree = match (mode, parent_commit) {
        (TreeMode::Overlay(_), Some(parent)) => repo.find_commit(parent)?.tree_id()?.detach(),
        _ => gix::ObjectId::empty_tree(repo.object_hash()),
    };
    let entries = match mode {
        TreeMode::Overlay(entries) | TreeMode::Replace(entries) => entries,
    };

    let mut editor = repo.edit_tree(parent_tree)?;
    for entry in entries {
        let blob_oid = repo.write_blob(entry.content.as_bytes())?;
        editor.upsert(
            entry.path.as_str(),
            gix::objs::tree::EntryKind::Blob,
            blob_oid,
        )?;
    }
    let tree_oid = editor.write()?.detach();
    let parents = parent_commit.into_iter();
    let sig = signature();
    let mut committer_time = gix::date::parse::TimeBuf::default();
    let mut author_time = gix::date::parse::TimeBuf::default();
    let commit_oid = repo
        .commit_as(
            sig.to_ref(&mut committer_time),
            sig.to_ref(&mut author_time),
            branch_ref,
            message,
            tree_oid,
            parents,
        )?
        .detach();

    Ok(commit_oid)
}

fn do_create_branch(repo_path: &Path, name: &str, from_ref: &str) -> anyhow::Result<gix::ObjectId> {
    let repo = gix::open(repo_path)?;
    let branch_ref = format!("{REFS_HEADS_PREFIX}{name}");
    let from_oid = repo.rev_parse_single(from_ref)?.detach();
    repo.reference(
        branch_ref.as_str(),
        from_oid,
        gix::refs::transaction::PreviousValue::Any,
        "create branch",
    )?;
    Ok(from_oid)
}

fn do_get_head(repo_path: &Path, branch_ref: &str) -> anyhow::Result<gix::ObjectId> {
    let repo = gix::open(repo_path)?;
    Ok(repo.rev_parse_single(branch_ref)?.detach())
}

pub(crate) async fn run_actor(mut rx: mpsc::UnboundedReceiver<ActorMsg>, repo_path: PathBuf) {
    fn send_response<T>(tx: oneshot::Sender<T>, value: T) {
        if tx.send(value).is_err() {
            tracing::error!("failed to send response");
        }
    }

    fn send_join_result(
        tx: oneshot::Sender<anyhow::Result<gix::ObjectId>>,
        result: Result<anyhow::Result<gix::ObjectId>, tokio::task::JoinError>,
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
