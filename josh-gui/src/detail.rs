use dioxus::prelude::*;
use std::str::FromStr;

use crate::Page;
use crate::common::{
    render_threads, review_decision_display, review_decision_label, vote_state_display,
};

#[derive(Clone)]
pub struct FileStat {
    pub path: String,
    pub adds: usize,
    pub dels: usize,
}

pub struct StackCommit {
    pub sha: String,
    pub subject: String,
    pub author: String,
    #[allow(dead_code)]
    pub series: String,
}

pub struct DetailData {
    pub change_id: String,
    pub sha: String,
    #[allow(dead_code)]
    pub subject: String,
    pub message: String,
    pub author: String,
    pub date: String,
    pub series: String,
    pub files: Vec<FileStat>,
    pub comments: Vec<josh_changes::Comment>,
    pub revisions: Vec<josh_changes::Revision>,
    pub stack: Vec<StackCommit>,
    pub pr_info: Option<PrInfo>,
    pub local_vote: Option<josh_changes::VoteData>,
}

pub struct PrInfo {
    pub url: String,
    pub title: String,
    pub state: String,
    pub review_decision: String,
}

#[component]
pub fn DetailView(sha: String, scope: josh_changes::ChangesRef, mut page: Signal<Page>) -> Element {
    let changes_ref_oid = use_context::<Signal<Option<gix_hash::ObjectId>>>();
    // Establish a reactive dependency on ref changes.
    let _ = changes_ref_oid.read();
    let data = load_detail(&sha, &scope);
    let mut vote_body = use_signal(String::new);

    match &data {
        Err(e) => rsx! {
            p { class: "error", "Error: {e}" }
        },
        Ok(data) => {
            let stats_total = format!(
                "{} files changed, +{} / -{}",
                data.files.len(),
                data.files.iter().map(|f| f.adds).sum::<usize>(),
                data.files.iter().map(|f| f.dels).sum::<usize>(),
            );
            rsx! {
                div { class: "scroll-table detail-layout",
                    div { class: "detail-left",
                        table { class: "detail-meta",
                            tbody {
                                tr { td { "Change-Id" } td { code { "{data.change_id}" } } }
                                tr { td { "SHA" } td { code { "{data.sha}" } } }
                                tr { td { "Author" } td { "{data.author}" } }
                                tr { td { "Date" } td { "{data.date}" } }
                                tr { td { "Series" } td { "{data.series}" } }
                                if let Some(ref pr) = data.pr_info {
                                    tr {
                                        td { "PR" }
                                        td {
                                            a {
                                                href: "{pr.url}",
                                                target: "_blank",
                                                rel: "noopener noreferrer",
                                                class: "pr-link",
                                                "{pr.title}"
                                            }
                                            span { class: "pr-state", " {pr.state}" }
                                            if !pr.review_decision.is_empty() {
                                                span {
                                                    class: "pr-state review-{review_decision_label(&pr.review_decision)}",
                                                    " {review_decision_display(&pr.review_decision)}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if !data.stack.is_empty() {
                            h2 { "Stack" }
                            div { class: "stack-list",
                                for cc in data.stack.iter() {
                                    {
                                        let short_sha = &cc.sha[..cc.sha.len().min(8)];
                                        rsx! {
                                            div { class: "stack-item",
                                                code { class: "stack-sha", "{short_sha}" }
                                                span { class: "stack-subject", "{cc.subject}" }
                                                span { class: "stack-author", "{cc.author}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if !data.revisions.is_empty() {
                            h2 { "Revisions" }
                            div { class: "revision-list",
                                for rev in data.revisions.iter() {
                                    {
                                        let is_current = rev.diff.commit == data.sha;
                                        let row_class = if is_current {
                                            "revision-item current"
                                        } else {
                                            "revision-item"
                                        };
                                        let short_sha = &rev.diff.commit[..rev.diff.commit.len().min(8)];
                                        let ts = rev.timestamp.parse::<i64>().ok()
                                            .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
                                            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                                            .unwrap_or_else(|| rev.timestamp.clone());
                                        rsx! {
                                            div { class: "{row_class}",
                                                code { class: "revision-sha", "{short_sha}" }
                                                span { class: "revision-author", "{rev.author}" }
                                                span { class: "revision-ts", "{ts}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "detail-right",
                        pre { class: "commit-message", "{data.message}" }
                        h2 { "Changed files" }
                        p { class: "diff-summary", "{stats_total}" }
                        {
                            let top_roots: Vec<&josh_changes::Comment> = data
                                .comments
                                .iter()
                                .filter(|c| c.meta.file.is_none() && c.meta.reply_to.is_none())
                                .collect();
                            if !top_roots.is_empty() {
                                rsx! {
                                    div { class: "file-comments",
                                        {render_threads(&data.comments, &top_roots, 0)}
                                    }
                                }
                            } else {
                                rsx! {}
                            }
                        }
                        table { class: "files",
                            thead {
                                tr {
                                    th { "File" }
                                    th { class: "num", "+" }
                                    th { class: "num", "-" }
                                }
                            }
                            tbody {
                                for f in data.files.iter() {
                                    {
                                        let s = data.sha.clone();
                                        let p = f.path.clone();
                                        let file_roots: Vec<&josh_changes::Comment> = data
                                            .comments
                                            .iter()
                                            .filter(|c| c.meta.file.as_deref() == Some(p.as_str())
                                                && c.meta.reply_to.is_none())
                                            .collect();
                                        let has_comments = !file_roots.is_empty();
                                        rsx! {
                                            tr {
                                                class: "file-row",
                                                onclick: move |_| page.set(Page::FileDiff {
                                                    sha: s.clone(),
                                                    path: p.clone(),
                                                }),
                                                td { "{f.path}" }
                                                td { class: "num adds", "{f.adds}" }
                                                td { class: "num dels", "{f.dels}" }
                                            }
                                            if has_comments {
                                                tr {
                                                    class: "file-comment-row",
                                                    td { colspan: "3",
                                                        div { class: "file-comments",
                                                            {render_threads(
                                                                &data.comments,
                                                                &file_roots,
                                                                0,
                                                            )}
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "vote-section",
                            h2 { "Vote" }
                            if let Some(ref vote) = data.local_vote {
                                div { class: "current-vote",
                                    span { class: "vote-state vote-{vote.state}",
                                        "{vote_state_display(&vote.state)}"
                                    }
                                }
                            }
                            textarea {
                                class: "vote-textarea",
                                placeholder: "Review comment (optional)...",
                                value: "{vote_body}",
                                oninput: move |evt| vote_body.set(evt.value()),
                            }
                            div { class: "vote-actions",
                                {
                                    let sha = sha.clone();
                                    let scope = scope.clone();
                                    rsx! {
                                        button {
                                            class: "vote-btn approve",
                                            onclick: move |_| {
                                                let body = vote_body.read().clone();
                                                if save_vote(&sha, "approve", &body, &scope).is_ok() {
                                                    bump_changes_ref_oid(changes_ref_oid, &scope);
                                                }
                                                vote_body.set(String::new());
                                            },
                                            "Approve"
                                        }
                                    }
                                }
                                {
                                    let sha = sha.clone();
                                    let scope = scope.clone();
                                    rsx! {
                                        button {
                                            class: "vote-btn discuss",
                                            onclick: move |_| {
                                                let body = vote_body.read().clone();
                                                if save_vote(&sha, "discuss", &body, &scope).is_ok() {
                                                    bump_changes_ref_oid(changes_ref_oid, &scope);
                                                }
                                                vote_body.set(String::new());
                                            },
                                            "Discuss"
                                        }
                                    }
                                }
                                {
                                    let sha = sha.clone();
                                    let scope = scope.clone();
                                    rsx! {
                                        button {
                                            class: "vote-btn revise",
                                            onclick: move |_| {
                                                let body = vote_body.read().clone();
                                                if save_vote(&sha, "revise", &body, &scope).is_ok() {
                                                    bump_changes_ref_oid(changes_ref_oid, &scope);
                                                }
                                                vote_body.set(String::new());
                                            },
                                            "Revise"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn load_detail(sha: &str, scope: &josh_changes::ChangesRef) -> anyhow::Result<DetailData> {
    let transaction = crate::common::open_transaction()?;
    let oid = gix_hash::ObjectId::from_str(sha)?;
    let commit = josh_core::objects::CommitData::read(transaction.odb(), oid)?;
    let parsed = commit.parsed()?;
    let msg = std::str::from_utf8(commit.message_raw()?.as_ref()).unwrap_or("");
    let subject = msg.lines().next().unwrap_or("").to_string();
    let message = msg.to_string();
    let author = std::str::from_utf8(parsed.author()?.email.as_ref())
        .unwrap_or("")
        .to_string();
    let date = chrono::DateTime::from_timestamp(parsed.time()?.seconds, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    let (change_id, series) = josh_changes::parse_change_meta(msg);

    let files = josh_core::git::file_stats(&transaction, oid)?
        .into_iter()
        .map(|stat| FileStat {
            path: stat.path,
            adds: stat.adds,
            dels: stat.dels,
        })
        .collect();

    let mut change = josh_changes::Change::new(&transaction, oid)?;

    let mut stack: Vec<StackCommit> = Vec::new();
    let mut pr_info: Option<PrInfo> = None;
    if let Some(ref cid) = change_id {
        pr_info = josh_github_changes::read_pr_data(&transaction, cid, scope)
            .ok()
            .flatten()
            .map(|v| PrInfo {
                url: v.url,
                title: v.title,
                state: v.state,
                review_decision: v.review_decision.unwrap_or_default(),
            });

        // Adopt the base oid recorded under the selected scope, if present.
        if let Ok(all) = josh_changes::list_changes(&transaction, scope) {
            if let Some(c) = all.iter().find(|c| c.id() == Some(cid.as_str())) {
                let base = c.base();
                if base != gix_hash::ObjectId::null(gix_hash::Kind::Sha1) {
                    change.set_base(base);
                }
            }
        }
        for oid in change.contributing(&transaction).unwrap_or_default() {
            if let Ok(commit) = josh_core::objects::CommitData::read(transaction.odb(), oid)
                && let Ok(parsed) = commit.parsed()
            {
                let msg = commit
                    .message_raw()
                    .ok()
                    .and_then(|msg| std::str::from_utf8(msg.as_ref()).ok())
                    .unwrap_or("");
                let c_subject = msg.lines().next().unwrap_or("").to_string();
                let c_author = parsed
                    .author()
                    .ok()
                    .and_then(|signature| std::str::from_utf8(signature.email.as_ref()).ok())
                    .unwrap_or("")
                    .to_string();
                let (_, c_series) = josh_changes::parse_change_meta(msg);
                stack.push(StackCommit {
                    sha: oid.to_string(),
                    subject: c_subject,
                    author: c_author,
                    series: c_series.join(", "),
                });
            }
        }
    }

    let comments = change_id
        .as_deref()
        .map(|cid| josh_changes::read_comments(&transaction, cid, scope).unwrap_or_default())
        .unwrap_or_default();
    let revisions = josh_changes::read_revisions(&transaction, &change, scope).unwrap_or_default();
    let local_vote = change_id
        .as_ref()
        .and_then(|cid| josh_changes::read_vote(&transaction, cid, None, scope).ok())
        .flatten();

    Ok(DetailData {
        change_id: change_id.unwrap_or_default(),
        sha: sha.to_string(),
        subject,
        message,
        author,
        date,
        series: series.join(", "),
        files,
        comments,
        revisions,
        stack,
        pr_info,
        local_vote,
    })
}

pub fn save_comment(
    sha: &str,
    file_path: &str,
    line_num: u32,
    message: &str,
    scope: &josh_changes::ChangesRef,
) -> anyhow::Result<String> {
    let transaction = crate::common::open_transaction()?;
    let oid = gix_hash::ObjectId::from_str(sha)?;
    let change = josh_changes::Change::new(&transaction, oid)?;

    let meta = josh_changes::CommentMeta {
        message: message.to_string(),
        file: Some(file_path.to_string()),
        location: Some(josh_changes::Location {
            start_line: line_num,
            end_line: line_num,
            start_col: 1,
            end_col: 1,
        }),
        reply_to: None,
        update_of: None,
    };

    let namespace = josh_changes::CommentNamespace::for_scope(scope);
    let content_hash =
        josh_changes::write_comment(&transaction, &change, &meta, None, None, scope, namespace)?;
    // The write went into the transaction's in-memory odb; make it durable (and
    // surface flush errors) before reporting success.
    transaction.flush_mem_odb()?;
    Ok(content_hash)
}

/// Refresh the shared OID after a local ref mutation.
pub fn bump_changes_ref_oid(
    mut changes_ref_oid: Signal<Option<gix_hash::ObjectId>>,
    scope: &josh_changes::ChangesRef,
) {
    let new_oid = crate::common::open_transaction()
        .ok()
        .and_then(|transaction| josh_changes::read_ref_oid(&transaction, scope));
    if new_oid != *changes_ref_oid.peek() {
        changes_ref_oid.set(new_oid);
    }
}

pub fn save_vote(
    sha: &str,
    state: &str,
    body: &str,
    scope: &josh_changes::ChangesRef,
) -> anyhow::Result<()> {
    let transaction = crate::common::open_transaction()?;
    let oid = gix_hash::ObjectId::from_str(sha)?;
    let change = josh_changes::Change::new(&transaction, oid)?;

    let body_meta = || josh_changes::CommentMeta {
        message: body.to_string(),
        file: None,
        location: None,
        reply_to: None,
        update_of: None,
    };

    if !body.trim().is_empty() {
        josh_changes::write_comment(
            &transaction,
            &change,
            &body_meta(),
            None,
            None,
            scope,
            josh_changes::CommentNamespace::for_scope(scope),
        )?;
    }
    josh_changes::write_vote(
        &transaction,
        &change,
        state,
        None,
        None,
        scope,
        josh_changes::VoteNamespace::for_scope(scope),
    )?;
    // The write went into the transaction's in-memory odb; make it durable (and
    // surface flush errors) before reporting success.
    transaction.flush_mem_odb()?;
    Ok(())
}
