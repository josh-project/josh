use dioxus::prelude::*;

fn main() {
    dioxus::launch(app);
}

#[derive(Clone, PartialEq)]
enum Page {
    List,
    Detail { sha: String },
    FileDiff { sha: String, path: String },
}

fn app() -> Element {
    let rows = use_signal(load_rows);
    let page = use_signal(|| Page::List);

    rsx! {
        style { {include_str!("style.css")} }
        div { class: "app",
            h1 { "josh-gui" }
            match &*page.read() {
                Page::List => list_view(rows, page),
                Page::Detail { sha } => detail_view(sha.clone(), page),
                Page::FileDiff { sha, path } => file_diff_view(sha.clone(), path.clone(), page),
            }
        }
    }
}

fn list_view(rows: Signal<anyhow::Result<Vec<Row>>>, mut page: Signal<Page>) -> Element {
    match &*rows.read() {
        Ok(rows) if rows.is_empty() => rsx! {
            p { "No outgoing changes found." }
        },
        Ok(rows) => rsx! {
            table { class: "changes",
                thead {
                    tr {
                        th { "Change-Id" }
                        th { "Subject" }
                        th { "Author" }
                        th { "Series" }
                    }
                }
                tbody {
                    for row in rows.iter() {
                        match row {
                            Row::Change { change_id, sha, subject, author, series } => {
                                let s = sha.clone();
                                rsx! {
                                    tr {
                                        onclick: move |_| page.set(Page::Detail { sha: s.clone() }),
                                        td { code { "{change_id}" } }
                                        td { "{subject}" }
                                        td { "{author}" }
                                        td { "{series}" }
                                    }
                                }
                            },
                            Row::Contributing { change_id, sha: _, subject, author, series } => rsx! {
                                tr { class: "contributing",
                                    td { code { class: "muted", "{change_id}" } }
                                    td { "{subject}" }
                                    td { "{author}" }
                                    td { "{series}" }
                                }
                            },
                        }
                    }
                }
            }
        },
        Err(e) => rsx! {
            p { class: "error", "Error: {e}" }
        },
    }
}

#[derive(Clone)]
struct FileStat {
    path: String,
    adds: usize,
    dels: usize,
}

fn detail_view(sha: String, mut page: Signal<Page>) -> Element {
    let data = load_detail(&sha);

    let back = rsx! {
        button { class: "back",
            onclick: move |_| page.set(Page::List),
            "\u{2190} Back to list"
        }
    };

    match &data {
        Err(e) => rsx! {
            {back}
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
                {back}
                table { class: "detail-meta",
                    tbody {
                        tr { td { "Change-Id" } td { code { "{data.change_id}" } } }
                        tr { td { "SHA" } td { code { "{data.sha}" } } }
                        tr { td { "Subject" } td { "{data.subject}" } }
                        tr { td { "Author" } td { "{data.author}" } }
                        tr { td { "Date" } td { "{data.date}" } }
                        tr { td { "Series" } td { "{data.series}" } }
                    }
                }
                h2 { "Changed files" }
                p { class: "diff-summary", "{stats_total}" }
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
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn file_diff_view(sha: String, path: String, mut page: Signal<Page>) -> Element {
    let detail_sha = sha.clone();
    let back = rsx! {
        button { class: "back",
            onclick: move |_| page.set(Page::Detail { sha: detail_sha.clone() }),
            "\u{2190} Back to change"
        }
    };

    let detail = load_detail(&sha);
    let (prev_file, next_file) = detail
        .as_ref()
        .ok()
        .and_then(|d| {
            let i = d.files.iter().position(|f| f.path == path)?;
            let prev = i.checked_sub(1).map(|j| d.files[j].path.clone());
            let next = d.files.get(i + 1).map(|f| f.path.clone());
            Some((prev, next))
        })
        .unwrap_or((None, None));

    let (prev_clone, next_clone, sha_clone) = (prev_file.clone(), next_file.clone(), sha.clone());
    let nav = rsx! {
        div { class: "diff-nav",
            if let Some(prev) = prev_clone {
                button {
                    class: "nav-btn",
                    onclick: {
                        let s = sha_clone.clone();
                        let p = prev.clone();
                        move |_| page.set(Page::FileDiff { sha: s.clone(), path: p.clone() })
                    },
                    "\u{2190} {prev}"
                }
            }
            if let Some(next) = next_clone {
                button {
                    class: "nav-btn",
                    onclick: {
                        let s = sha_clone.clone();
                        let n = next.clone();
                        move |_| page.set(Page::FileDiff { sha: s.clone(), path: n.clone() })
                    },
                    "{next} \u{2192}"
                }
            }
        }
    };

    match load_file_diff(&sha, &path) {
        Err(e) => rsx! {
            {back}
            p { class: "error", "Error: {e}" }
        },
        Ok(lines) => rsx! {
            {back}
            {nav}
            h2 { "{path}" }
            pre { class: "diff-view",
                for line in lines.iter() {
                    match line.kind {
                        DiffLineKind::Add => rsx! { span { class: "diff-add", "{line.text}\n" } },
                        DiffLineKind::Del => rsx! { span { class: "diff-del", "{line.text}\n" } },
                        DiffLineKind::Hunk => rsx! { span { class: "diff-hunk", "{line.text}\n" } },
                        DiffLineKind::Context => rsx! { span { "{line.text}\n" } },
                    }
                }
            }
        },
    }
}

enum DiffLineKind {
    Context,
    Add,
    Del,
    Hunk,
}

struct DiffLine {
    kind: DiffLineKind,
    text: String,
}

fn load_file_diff(sha: &str, path: &str) -> anyhow::Result<Vec<DiffLine>> {
    let repo = git2::Repository::discover(".")?;
    let oid = git2::Oid::from_str(sha)?;
    let commit = repo.find_commit(oid)?;

    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), None)?;

    // Find the patch matching this path.
    let mut patch_idx = None;
    for i in 0..diff.deltas().len() {
        let delta = diff.deltas().nth(i).unwrap();
        let p = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(|p| p.to_str())
            .unwrap_or("");
        if p == path {
            patch_idx = Some(i);
            break;
        }
    }

    let mut lines = Vec::new();
    if let Some(idx) = patch_idx {
        let patch = git2::Patch::from_diff(&diff, idx)?;
        if let Some(patch) = patch {
            for h in 0..patch.num_hunks() {
                let (hunk, hunk_lines) = patch.hunk(h)?;
                lines.push(DiffLine {
                    kind: DiffLineKind::Hunk,
                    text: String::from_utf8_lossy(hunk.header()).to_string(),
                });
                for l in 0..hunk_lines {
                    let line = patch.line_in_hunk(h, l)?;
                    let origin = line.origin();
                    let kind = match origin {
                        '+' => DiffLineKind::Add,
                        '-' => DiffLineKind::Del,
                        ' ' => DiffLineKind::Context,
                        _ => DiffLineKind::Context,
                    };
                    lines.push(DiffLine {
                        kind,
                        text: String::from_utf8_lossy(line.content()).to_string(),
                    });
                }
            }
        }
    }

    Ok(lines)
}

struct DetailData {
    change_id: String,
    sha: String,
    subject: String,
    author: String,
    date: String,
    series: String,
    files: Vec<FileStat>,
}

fn load_detail(sha: &str) -> anyhow::Result<DetailData> {
    let repo = git2::Repository::discover(".")?;
    let oid = git2::Oid::from_str(sha)?;
    let commit = repo.find_commit(oid)?;

    let msg = commit.message().unwrap_or("");
    let subject = msg.lines().next().unwrap_or("").to_string();
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

    Ok(DetailData {
        change_id: change_id.unwrap_or_default(),
        sha: sha.to_string(),
        subject,
        author,
        date,
        series: series.join(", "),
        files,
    })
}

#[derive(Clone)]
enum Row {
    Change {
        change_id: String,
        sha: String,
        subject: String,
        author: String,
        series: String,
    },
    Contributing {
        change_id: String,
        sha: String,
        subject: String,
        author: String,
        series: String,
    },
}

fn load_rows() -> anyhow::Result<Vec<Row>> {
    let repo = git2::Repository::discover(".")?;
    let head = repo.head()?.peel_to_commit()?;

    let branch = repo.head()?.shorthand().map(|s| s.to_string());

    let base = if let Some(ref name) = branch {
        let remote_ref = format!("refs/remotes/origin/{}", name);
        repo.find_reference(&remote_ref)
            .ok()
            .and_then(|r| r.peel_to_commit().ok())
            .map(|c| c.id())
            .unwrap_or(git2::Oid::zero())
    } else {
        git2::Oid::zero()
    };

    let changes = josh_changes::list_changes(&repo, head.id(), base)?;

    let mut groups: Vec<(Row, Vec<Row>)> = Vec::new();
    for change in &changes {
        let commit = repo.find_commit(change.commit)?;
        let subject = commit
            .message()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        let change_row = Row::Change {
            change_id: change.id.clone().unwrap_or_default(),
            sha: change.commit.to_string(),
            subject,
            author: change.author.clone(),
            series: change.series.join(", "),
        };

        let mut contrib_rows = Vec::new();
        for oid in change.contributing(&repo)? {
            if let Ok(c) = repo.find_commit(oid) {
                let msg = c.message().unwrap_or("");
                let c_subject = msg.lines().next().unwrap_or("").to_string();
                let c_author = c.author().email().unwrap_or("").to_string();
                let (c_change_id, c_series) = josh_changes::parse_change_meta(msg);
                contrib_rows.push(Row::Contributing {
                    change_id: c_change_id.unwrap_or_default(),
                    sha: oid.to_string(),
                    subject: c_subject,
                    author: c_author,
                    series: c_series.join(", "),
                });
            }
        }
        groups.push((change_row, contrib_rows));
    }

    groups.sort_by_key(|(_, contrib)| contrib.len());

    let mut rows = Vec::new();
    for (change_row, contrib_rows) in groups {
        rows.push(change_row);
        rows.extend(contrib_rows);
    }
    Ok(rows)
}
