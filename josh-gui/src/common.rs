use std::sync::OnceLock;

use dioxus::prelude::*;

pub fn vote_state_label(state: &str) -> &'static str {
    match state {
        "approve" => "approve",
        "discuss" => "discuss",
        "revise" => "revise",
        _ => "",
    }
}

pub fn vote_state_display(state: &str) -> &'static str {
    match state {
        "approve" => "Approve",
        "discuss" => "Discuss",
        "revise" => "Revise",
        _ => "",
    }
}

pub fn review_decision_label(rd: &str) -> &'static str {
    match rd {
        "Approved" => "approved",
        "ChangesRequested" => "changes-requested",
        "ReviewRequired" => "review-required",
        _ => "",
    }
}

pub fn review_decision_display(rd: &str) -> &'static str {
    match rd {
        "Approved" => "Approved",
        "ChangesRequested" => "Changes requested",
        "ReviewRequired" => "Review required",
        _ => "",
    }
}

pub fn check_status_label(cs: &str) -> &'static str {
    match cs {
        "Success" => "success",
        "Failure" | "Error" => "failure",
        "Pending" | "Expected" => "pending",
        _ => "",
    }
}

pub fn check_status_display(cs: &str) -> &'static str {
    match cs {
        "Success" => "Passed",
        "Failure" => "Failed",
        "Error" => "Error",
        "Pending" => "Pending",
        "Expected" => "Expected",
        _ => "",
    }
}

pub fn repo_name() -> String {
    static HOME: OnceLock<Option<String>> = OnceLock::new();
    let home = HOME.get_or_init(|| std::env::home_dir().map(|p| p.display().to_string()));
    let dir = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    if let Some(home) = home {
        if dir.starts_with(home) {
            return dir.replacen(home, "~", 1);
        }
    }
    dir
}

fn subject_from_rows(
    list_data: &anyhow::Result<crate::list::ListData>,
    sha: &str,
) -> String {
    let Ok(data) = list_data else {
        return String::new();
    };
    data.rows
        .iter()
        .find(|r| r.sha == sha)
        .map(|r| r.subject.clone())
        .unwrap_or_default()
}

pub fn breadcrumb(
    page: &crate::Page,
    mut page_signal: Signal<crate::Page>,
    list_data: Signal<anyhow::Result<crate::list::ListData>>,
) -> Element {
    let repo = repo_name();
    let subject = match page {
        crate::Page::List => String::new(),
        crate::Page::Detail { sha } | crate::Page::FileDiff { sha, .. } => {
            subject_from_rows(&list_data.read(), sha)
        }
    };

    match page {
        crate::Page::List => rsx! {
            span { class: "header-dir",
                span { class: "breadcrumb-seg-last", "{repo}" }
            }
        },
        crate::Page::Detail { .. } => rsx! {
            span { class: "header-dir",
                span {
                    class: "breadcrumb-seg",
                    onclick: move |_| page_signal.set(crate::Page::List),
                    "{repo}"
                }
                span { class: "breadcrumb-sep", " \u{203A} " }
                span { class: "breadcrumb-seg-last", "{subject}" }
            }
        },
        crate::Page::FileDiff { sha, path } => {
            let sha_c = sha.clone();
            rsx! {
                span { class: "header-dir",
                    span {
                        class: "breadcrumb-seg",
                        onclick: move |_| page_signal.set(crate::Page::List),
                        "{repo}"
                    }
                    span { class: "breadcrumb-sep", " \u{203A} " }
                    span {
                        class: "breadcrumb-seg",
                        onclick: {
                            let s = sha_c.clone();
                            move |_| page_signal.set(crate::Page::Detail { sha: s.clone() })
                        },
                        "{subject}"
                    }
                    span { class: "breadcrumb-sep", " \u{203A} " }
                    span { class: "breadcrumb-seg-last", "{path}" }
                }
            }
        }
    }
}

pub fn parse_hunk_header(header: &str) -> (usize, usize) {
    let mut parts = header.split_whitespace();
    let old_part = parts.nth(1).unwrap_or("");
    let new_part = parts.next().unwrap_or("");
    let old_start = old_part
        .trim_start_matches('-')
        .split(',')
        .next()
        .unwrap_or("1")
        .parse()
        .unwrap_or(1);
    let new_start = new_part
        .trim_start_matches('+')
        .split(',')
        .next()
        .unwrap_or("1")
        .parse()
        .unwrap_or(1);
    (old_start, new_start)
}

#[derive(Clone)]
pub struct FlatComment {
    pub comment: josh_changes::Comment,
    pub depth: usize,
}

pub fn flatten_thread(
    all: &[josh_changes::Comment],
    roots: &[usize],
    target_line: u32,
    depth: usize,
    out: &mut Vec<(u32, FlatComment)>,
) {
    for &idx in roots {
        let c = &all[idx];
        out.push((
            target_line,
            FlatComment {
                comment: c.clone(),
                depth,
            },
        ));
        let children: Vec<usize> = all
            .iter()
            .enumerate()
            .filter(|(_, x)| x.reply_to.as_deref() == Some(&c.id))
            .map(|(i, _)| i)
            .collect();
        if !children.is_empty() {
            flatten_thread(all, &children, target_line, depth + 1, out);
        }
    }
}

pub fn render_comment_card(author: &str, ts: &str, message: &str) -> Element {
    rsx! {
        div { class: "comment-header",
            if !author.is_empty() {
                span { class: "comment-author", "{author}" }
            }
            if !ts.is_empty() {
                span { class: "comment-ts", " {ts}" }
            }
        }
        pre { class: "comment-body", "{message}" }
    }
}

pub fn render_threads(
    all: &[josh_changes::Comment],
    comments: &[&josh_changes::Comment],
    depth: usize,
) -> Element {
    let children: Vec<Element> = comments
        .iter()
        .map(|c| {
            let children: Vec<&josh_changes::Comment> = all
                .iter()
                .filter(|x| x.reply_to.as_deref() == Some(&c.id))
                .collect();
            let indent = depth * 16;
            let author = c.author.as_deref().unwrap_or_default();
            let ts = c
                .timestamp
                .as_deref()
                .and_then(|s| {
                    s.parse::<i64>()
                        .ok()
                        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
                })
                .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();
            rsx! {
                div { style: "margin-left: {indent}px",
                    div { class: "diff-comment-inline",
                        {render_comment_card(author, &ts, &c.message)}
                    }
                    {render_threads(all, &children, depth + 1)}
                }
            }
        })
        .collect();
    rsx! { {children.into_iter().map(|e| e)} }
}
