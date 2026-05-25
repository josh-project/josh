use dioxus::prelude::*;

fn main() {
    dioxus::LaunchBuilder::new()
        .with_cfg(
            dioxus::desktop::Config::new().with_window(
                dioxus::desktop::WindowBuilder::new().with_title("Josh"),
            ),
        )
        .launch(app);
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
            div { class: "header",
                svg {
                    width: "28",
                    height: "28",
                    view_box: "0 0 200 203",
                    fill: "none",
                    xmlns: "http://www.w3.org/2000/svg",
                    path {
                        fill_rule: "evenodd",
                        clip_rule: "evenodd",
                        d: "M195.422 0C198.366 0 200.508 2.6764 199.883 5.53125L153.937 198.594C153.491 200.646 151.617 202.073 149.476 202.073H125.567C122.623 202.073 120.392 199.397 121.106 196.542L168.479 3.39062C169.014 1.42796 170.799 0.000105731 172.94 0H195.422ZM115.68 8.0332C117.018 2.50188 122.639 -0.977528 128.259 0.271484C133.88 1.52057 137.359 7.05174 136.11 12.583L93.5541 194.048C92.2157 199.579 86.5953 203.058 80.975 201.81C75.3544 200.561 71.8744 195.028 73.1234 189.497L84.7221 139.805L47.6078 178.881C43.5039 183.163 36.7234 183.341 32.3519 179.416C27.9808 175.491 27.8025 168.8 31.8168 164.518L69.7338 124.549L16.5609 140.607C10.8512 142.302 4.87352 139.18 3.08924 133.648C1.3943 128.028 4.51722 122.139 10.2269 120.444L64.6488 104.029L9.24549 91.6279C2.91124 90.1112 -1.19304 83.8661 0.323614 77.6211C1.84026 71.3762 8.17469 67.4508 14.598 68.8779L64.0238 79.9404L27.5346 46.7529C23.163 42.8274 22.9847 36.1359 26.9994 31.8535C31.1033 27.5713 37.8837 27.3929 42.2553 31.3184L83.0267 68.4316L66.6107 16.4189C64.916 10.7985 68.0388 4.91086 73.7484 3.21582C79.4582 1.52073 85.4358 4.64345 87.2201 10.1748L103.279 61.2061L115.68 8.0332Z",
                        fill: "#E62200",
                    }
                }
                span { class: "header-dir",
                    {
                        std::env::current_dir()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default()
                    }
                }
            }
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
            div { class: "scroll-table",
                table { class: "changes",
                thead {
                    tr {
                        th { "Change-Id" }
                        th { "SHA" }
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
                                        td { code { "{&sha[..8]}" } }
                                        td { "{subject}" }
                                        td { "{author}" }
                                        td { "{series}" }
                                    }
                                }
                            },
                            Row::Contributing { change_id, sha, subject, author, series } => rsx! {
                                tr { class: "contributing",
                                    td { code { class: "muted", "{change_id}" } }
                                    td { code { "{&sha[..8]}" } }
                                    td { "{subject}" }
                                    td { "{author}" }
                                    td { "{series}" }
                                }
                            },
                        }
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
                div { class: "scroll-table",
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
            d.files.iter().position(|f| f.path == path).map(|i| {
                let prev = if i > 0 {
                    d.files[i - 1].path.clone()
                } else {
                    d.files.last().map(|f| f.path.clone()).unwrap_or_default()
                };
                let next = if i + 1 < d.files.len() {
                    d.files[i + 1].path.clone()
                } else {
                    d.files.first().map(|f| f.path.clone()).unwrap_or_default()
                };
                (prev, next)
            })
        })
        .unwrap_or_default();

    let sha2 = sha.clone();
    let sha3 = sha.clone();
    let prev = prev_file.clone();
    let next = next_file.clone();
    let nav = rsx! {
        div { class: "diff-nav",
            button {
                class: "nav-btn",
                onclick: {
                    let s = sha2.clone();
                    let p = prev.clone();
                    move |_| page.set(Page::FileDiff { sha: s.clone(), path: p.clone() })
                },
                "\u{2190} {prev_file}"
            }
            span { class: "diff-nav-pos", "{path}" }
            button {
                class: "nav-btn",
                onclick: {
                    let s = sha3.clone();
                    let n = next.clone();
                    move |_| page.set(Page::FileDiff { sha: s.clone(), path: n.clone() })
                },
                "{next_file} \u{2192}"
            }
        }
    };

    let mut context_lines = use_signal(|| 5u32);
    let mut show_all = use_signal(|| false);

    let ctx: u32 = if *show_all.read() {
        u32::MAX
    } else {
        *context_lines.read()
    };
    let all_lines = load_file_diff(&sha, &path, ctx);
    let mut scroll_offset = use_signal(|| 0usize);
    let mut selected_line = use_signal(|| None::<usize>);

    match &all_lines {
        Err(e) => rsx! {
            {back}
            p { class: "error", "Error: {e}" }
        },
        Ok(lines) => {
            let total = lines.len();
            let row_h = 20;
            let visible = 40;
            let overscan = 40;

            let offset = *scroll_offset.read();
            let start = (offset / row_h).saturating_sub(overscan);
            let end = ((offset / row_h) + visible + overscan).min(total);

            let top_spacer_h = start * row_h;
            let bottom_spacer_h = (total.saturating_sub(end)) * row_h;

            let ln_ch = format!("{}", total).len() + 1;

            rsx! {
                div { class: "diff-page",
                    {back}
                    {nav}
                    div { class: "diff-toolbar",
                        label {
                            "Context: "
                            input {
                                r#type: "number",
                                min: "0",
                                max: "999",
                                value: "{context_lines}",
                                disabled: *show_all.read(),
                                oninput: move |e| {
                                    if let Ok(v) = e.value().parse::<u32>() {
                                        context_lines.set(v.min(999));
                                    }
                                },
                            }
                        }
                        label {
                            input {
                                r#type: "checkbox",
                                checked: *show_all.read(),
                                oninput: move |e| show_all.set(e.checked()),
                            }
                            " Show all"
                        }
                    }
                    h2 { "{path}" }
                    div {
                        class: "diff-container",
                        tabindex: "0",
                        onscroll: move |e| {
                            scroll_offset.set(e.data.scroll_top() as usize);
                        },
                        onkeydown: move |e| {
                            let total = all_lines.as_ref().ok().map(|l| l.len()).unwrap_or(0);
                            if total == 0 {
                                return;
                            }
                            let cur = selected_line.read().unwrap_or(0);
                            let new = match e.key() {
                                Key::ArrowDown => Some((cur + 1).min(total - 1)),
                                Key::ArrowUp => Some(if cur > 0 { cur - 1 } else { 0 }),
                                _ => return,
                            };
                            if let Some(n) = new {
                                selected_line.set(Some(n));
                                let off = *scroll_offset.read();
                                let vis_start = off / row_h;
                                let vis_end = vis_start + visible;
                                if n < vis_start + overscan {
                                    scroll_offset.set(n.saturating_sub(overscan) * row_h);
                                } else if n >= vis_end.saturating_sub(overscan) {
                                    scroll_offset.set(
                                        (n + overscan + 1).saturating_sub(visible) * row_h,
                                    );
                                }
                            }
                        },
                        table { class: "diff-table",
                            colgroup {
                                col { style: "width: {ln_ch}ch" }
                                col { style: "width: 2ch" }
                                col {}
                            }
                            tbody {
                                if top_spacer_h > 0 {
                                    tr { style: "height: {top_spacer_h}px",
                                        td {}
                                        td {}
                                        td {}
                                    }
                                }
                                for line in lines[start..end].iter() {
                                    {
                                        let sel = *selected_line.read();
                                        let is_sel = sel == Some(line.line_number);
                                        let ln = line.line_number;
                                        rsx! {
                                            tr {
                                                class: "diff-line diff-line-{line.kind:?}",
                                                class: if is_sel { "diff-line-sel" },
                                                onclick: move |_| selected_line.set(Some(ln)),
                                                td { class: "diff-ln", "{line.line_number}" }
                                                td { class: "diff-sign", {line.kind.sign()} }
                                                td {
                                                    class: "diff-content",
                                                    pre { "{line.text}" }
                                                }
                                            }
                                        }
                                    }
                                }
                                if bottom_spacer_h > 0 {
                                    tr { style: "height: {bottom_spacer_h}px",
                                        td {}
                                        td {}
                                        td {}
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum DiffLineKind {
    Context,
    Add,
    Del,
    Hunk,
}

impl DiffLineKind {
    fn sign(&self) -> &str {
        match self {
            DiffLineKind::Add => "+",
            DiffLineKind::Del => "-",
            _ => "",
        }
    }
}

struct DiffLine {
    kind: DiffLineKind,
    text: String,
    line_number: usize,
}

fn load_file_diff(sha: &str, path: &str, context_lines: u32) -> anyhow::Result<Vec<DiffLine>> {
    let repo = git2::Repository::discover(".")?;
    let oid = git2::Oid::from_str(sha)?;
    let commit = repo.find_commit(oid)?;

    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let mut opts = git2::DiffOptions::new();
    opts.context_lines(context_lines);
    let diff =
        repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), Some(&mut opts))?;

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
            let mut n = 0;
            for h in 0..patch.num_hunks() {
                let (hunk, hunk_lines) = patch.hunk(h)?;
                n += 1;
                lines.push(DiffLine {
                    kind: DiffLineKind::Hunk,
                    text: String::from_utf8_lossy(hunk.header()).to_string(),
                    line_number: n,
                });
                for l in 0..hunk_lines {
                    n += 1;
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
                        line_number: n,
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
        let commit = repo.find_commit(change.commit())?;
        let subject = commit
            .message()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        let change_row = Row::Change {
            change_id: change.id().unwrap_or("").to_string(),
            sha: change.commit().to_string(),
            subject,
            author: change.author().to_string(),
            series: change.series().join(", "),
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
