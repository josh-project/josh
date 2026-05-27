use dioxus::prelude::*;

use crate::Page;
use crate::common::{render_threads, review_decision_display, review_decision_label};

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
    pub series: String,
}

pub struct DetailData {
    pub change_id: String,
    pub sha: String,
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

fn vote_state_display(state: &str) -> &'static str {
    match state {
        "approved" => "Approved",
        "neutral" => "Neutral",
        "changes_requested" => "Changes requested",
        _ => "",
    }
}

pub fn detail_view(sha: String, mut page: Signal<Page>) -> Element {
    let data = load_detail(&sha);
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
                                        let is_current = rev.commit_oid == data.sha;
                                        let row_class = if is_current {
                                            "revision-item current"
                                        } else {
                                            "revision-item"
                                        };
                                        let short_sha = &rev.commit_oid[..rev.commit_oid.len().min(8)];
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
                                .filter(|c| c.file.is_none() && c.reply_to.is_none())
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
                                            .filter(|c| c.file.as_deref() == Some(p.as_str())
                                                && c.reply_to.is_none())
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
                                    if !vote.body.is_empty() {
                                        pre { class: "vote-body", "{vote.body}" }
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
                                    rsx! {
                                        button {
                                            class: "vote-btn approve",
                                            onclick: move |_| {
                                                let body = vote_body.read().clone();
                                                let _ = save_vote(
                                                    &sha, "approved", &body,
                                                );
                                                vote_body.set(String::new());
                                                page.set(Page::Detail {
                                                    sha: sha.clone(),
                                                });
                                            },
                                            "Approve"
                                        }
                                    }
                                }
                                {
                                    let sha = sha.clone();
                                    rsx! {
                                        button {
                                            class: "vote-btn neutral",
                                            onclick: move |_| {
                                                let body = vote_body.read().clone();
                                                let _ = save_vote(
                                                    &sha, "neutral", &body,
                                                );
                                                vote_body.set(String::new());
                                                page.set(Page::Detail {
                                                    sha: sha.clone(),
                                                });
                                            },
                                            "Neutral"
                                        }
                                    }
                                }
                                {
                                    let sha = sha.clone();
                                    rsx! {
                                        button {
                                            class: "vote-btn revise",
                                            onclick: move |_| {
                                                let body = vote_body.read().clone();
                                                let _ = save_vote(
                                                    &sha, "changes_requested", &body,
                                                );
                                                vote_body.set(String::new());
                                                page.set(Page::Detail {
                                                    sha: sha.clone(),
                                                });
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

pub fn load_detail(sha: &str) -> anyhow::Result<DetailData> {
    let repo = git2::Repository::discover(".")?;
    let oid = git2::Oid::from_str(sha)?;
    let commit = repo.find_commit(oid)?;

    let msg = commit.message().unwrap_or("");
    let subject = msg.lines().next().unwrap_or("").to_string();
    let message = msg.to_string();
    let author = commit.author().email().unwrap_or("").to_string();
    let date = chrono::DateTime::from_timestamp(commit.time().seconds(), 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    let (change_id, series) = josh_changes::parse_change_meta(msg);

    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), None)?;

    let mut files: Vec<FileStat> = Vec::new();
    for i in 0..diff.deltas().len() {
        let delta = diff.deltas().nth(i).unwrap();
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(|p| p.to_str())
            .unwrap_or("");
        let patch = git2::Patch::from_diff(&diff, i)?;
        let (_, adds, dels) = patch
            .as_ref()
            .map(|p| p.line_stats().unwrap_or((0, 0, 0)))
            .unwrap_or((0, 0, 0));
        files.push(FileStat {
            path: path.to_string(),
            adds,
            dels,
        });
    }

    let mut change = josh_changes::Change::new(&repo, &commit);

    let mut stack: Vec<StackCommit> = Vec::new();
    let mut pr_info: Option<PrInfo> = None;
    if let Some(ref cid) = change_id {
        if let Ok(tree) = repo
            .find_reference("refs/josh/changes")
            .and_then(|r| r.peel_to_tree())
        {
            let pr_path = std::path::Path::new("gh").join(josh_changes::encode_change_id_path(cid));
            pr_info = tree
                .get_path(&pr_path)
                .ok()
                .and_then(|e| e.to_object(&repo).ok())
                .and_then(|o| o.peel_to_tree().ok())
                .and_then(|t| {
                    t.iter()
                        .next()
                        .and_then(|e| e.to_object(&repo).ok())
                        .and_then(|o| o.peel_to_blob().ok())
                })
                .and_then(|b| {
                    let content = String::from_utf8_lossy(b.content());
                    serde_json::from_str::<serde_json::Value>(&content)
                        .ok()
                        .map(|v| PrInfo {
                            url: v["url"].as_str().unwrap_or("").to_string(),
                            title: v["title"].as_str().unwrap_or("").to_string(),
                            state: v["state"].as_str().unwrap_or("").to_string(),
                            review_decision: v["review_decision"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                        })
                });

            let path = std::path::Path::new("diffs").join(josh_changes::encode_change_id_path(cid));
            if let Some(entry) = tree
                .get_path(&path)
                .ok()
                .and_then(|e| e.to_object(&repo).ok())
                .and_then(|o| o.peel_to_tree().ok())
            {
                if let Some(blob_entry) = entry
                    .iter()
                    .next()
                    .and_then(|e| e.to_object(&repo).ok())
                    .and_then(|o| o.peel_to_blob().ok())
                {
                    let content = String::from_utf8_lossy(blob_entry.content());
                    if let Some((_, base_str)) = content.split_once('\n') {
                        if let Ok(base_oid) = git2::Oid::from_str(base_str) {
                            change.set_base(base_oid);
                        }
                    }
                }
            }
        }
        for oid in change.contributing(&repo).unwrap_or_default() {
            if let Ok(c) = repo.find_commit(oid) {
                let msg = c.message().unwrap_or("");
                let c_subject = msg.lines().next().unwrap_or("").to_string();
                let c_author = c.author().email().unwrap_or("").to_string();
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

    let comments = josh_changes::read_comments(&repo, &change).unwrap_or_default();
    let revisions = josh_changes::read_revisions(&repo, &change).unwrap_or_default();
    let local_vote = change_id
        .as_ref()
        .and_then(|cid| josh_changes::read_vote(&repo, cid).ok())
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
) -> anyhow::Result<String> {
    let repo = git2::Repository::discover(".")?;
    let oid = git2::Oid::from_str(sha)?;
    let commit = repo.find_commit(oid)?;
    let change = josh_changes::Change::new(&repo, &commit);

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

    josh_changes::write_comment(&repo, &change, &meta, None, None)
}

pub fn save_vote(sha: &str, state: &str, body: &str) -> anyhow::Result<String> {
    let repo = git2::Repository::discover(".")?;
    let oid = git2::Oid::from_str(sha)?;
    let commit = repo.find_commit(oid)?;
    let change = josh_changes::Change::new(&repo, &commit);
    josh_changes::write_vote(&repo, &change, state, body, None, None)
}
