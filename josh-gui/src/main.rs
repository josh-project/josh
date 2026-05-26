use std::sync::OnceLock;

use dioxus::prelude::*;

fn main() {
    dioxus::LaunchBuilder::new()
        .with_cfg(
            dioxus::desktop::Config::new()
                .with_window(dioxus::desktop::WindowBuilder::new().with_title("Josh")),
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
                {breadcrumb(&page.read(), page, rows)}
            }
            match &*page.read() {
                Page::List => list_view(rows, page),
                Page::Detail { sha } => detail_view(sha.clone(), page),
                Page::FileDiff { sha, path } => file_diff_view(sha.clone(), path.clone(), page),
            }
        }
    }
}

fn repo_name() -> String {
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

fn subject_from_rows(rows: &anyhow::Result<Vec<Row>>, sha: &str) -> String {
    let Ok(rows) = rows else {
        return String::new();
    };
    rows.iter()
        .find_map(|r| match r {
            Row::Change {
                sha: s, subject, ..
            }
            | Row::Contributing {
                sha: s, subject, ..
            } if s == sha => Some(subject.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn breadcrumb(
    page: &Page,
    mut page_signal: Signal<Page>,
    rows: Signal<anyhow::Result<Vec<Row>>>,
) -> Element {
    let repo = repo_name();
    let subject = match page {
        Page::List => String::new(),
        Page::Detail { sha } | Page::FileDiff { sha, .. } => subject_from_rows(&rows.read(), sha),
    };

    match page {
        Page::List => rsx! {
            span { class: "header-dir",
                span { class: "breadcrumb-seg-last", "{repo}" }
            }
        },
        Page::Detail { .. } => rsx! {
            span { class: "header-dir",
                span {
                    class: "breadcrumb-seg",
                    onclick: move |_| page_signal.set(Page::List),
                    "{repo}"
                }
                span { class: "breadcrumb-sep", " \u{203A} " }
                span { class: "breadcrumb-seg-last", "{subject}" }
            }
        },
        Page::FileDiff { sha, path } => {
            let sha_c = sha.clone();
            rsx! {
                span { class: "header-dir",
                    span {
                        class: "breadcrumb-seg",
                        onclick: move |_| page_signal.set(Page::List),
                        "{repo}"
                    }
                    span { class: "breadcrumb-sep", " \u{203A} " }
                    span {
                        class: "breadcrumb-seg",
                        onclick: {
                            let s = sha_c.clone();
                            move |_| page_signal.set(Page::Detail { sha: s.clone() })
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
                            Row::Stack { change_id, sha: _, subject, author, series } => rsx! {
                                tr { class: "stack",
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
                    }
                }
            }
        }
    }
}

fn file_diff_view(sha: String, path: String, mut page: Signal<Page>) -> Element {
    let detail = load_detail(&sha);
    let (prev_file, next_file) = detail
        .as_ref()
        .ok()
        .and_then(|d| {
            let i = d.files.iter().position(|f| f.path == path)?;
            let n = d.files.len();
            let prev = d.files[(i + n - 1) % n].path.clone();
            let next = d.files[(i + 1) % n].path.clone();
            Some((prev, next))
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

    // Reset scroll position when navigating to a different file
    let mut last_path = use_signal(|| String::new());
    let current_file = format!("{}:{}", sha, path);
    if *last_path.read() != current_file {
        scroll_offset.set(0);
        selected_line.set(None);
        last_path.set(current_file);
    }

    match &all_lines {
        Err(e) => rsx! {
            p { class: "error", "Error: {e}" }
        },
        Ok(lines) => {
            let comments: Vec<josh_changes::Comment> = detail
                .as_ref()
                .ok()
                .map(|d| d.comments.clone())
                .unwrap_or_default();
            let items = build_diff_with_comments(lines, &comments, &path);
            let total = items.len();

            let heights: Vec<u32> = {
                let mut sum = 0u32;
                items
                    .iter()
                    .map(|item| {
                        sum += estimate_item_height(item);
                        sum
                    })
                    .collect()
            };

            let visible = 40;
            let overscan = 40;

            let offset = *scroll_offset.read() as u32;
            let viewport_px = (visible * 20) as u32;
            let overscan_px = (overscan * 20) as u32;

            let start = heights.partition_point(|&h| h <= offset.saturating_sub(overscan_px));
            let end = heights
                .partition_point(|&h| h <= offset + viewport_px + overscan_px)
                .min(heights.len());

            let total_h = *heights.last().unwrap_or(&0);
            let top_spacer_h = if start > 0 { heights[start - 1] } else { 0 };
            let bottom_spacer_h = if end > 0 {
                total_h - heights[end - 1]
            } else {
                total_h
            };

            let ln_ch = lines
                .iter()
                .flat_map(|l| [l.old_ln, l.new_ln])
                .flatten()
                .max()
                .map(|m| format!("{}", m).len())
                .unwrap_or(1)
                + 1;

            rsx! {
                div { class: "diff-page",
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
                    div {
                        class: "diff-container",
                        tabindex: "0",
                        onscroll: move |e| {
                            scroll_offset.set(e.data.scroll_top() as usize);
                        },
                        onkeydown: move |e| {
                            if total == 0 {
                                return;
                            }
                            let cur = selected_line
                                .read()
                                .unwrap_or(0)
                                .min(total.saturating_sub(1));
                            let new = match e.key() {
                                Key::ArrowDown => Some((cur + 1).min(total - 1)),
                                Key::ArrowUp => Some(if cur > 0 { cur - 1 } else { 0 }),
                                _ => return,
                            };
                            if let Some(n) = new {
                                selected_line.set(Some(n));
                                let vis_start = start;
                                let vis_end = end;
                                if n < vis_start + overscan {
                                    let target =
                                        if n > 0 { heights[n - 1] } else { 0 };
                                    scroll_offset
                                        .set(target.saturating_sub(overscan_px) as usize);
                                } else if n >= vis_end.saturating_sub(overscan) {
                                    let target_top =
                                        if n > 0 { heights[n - 1] } else { 0 };
                                    let item_h = if n < heights.len() {
                                        heights[n] - heights[n - 1]
                                    } else {
                                        20
                                    };
                                    let new_offset = (target_top + item_h)
                                        .saturating_sub(viewport_px);
                                    scroll_offset.set(new_offset as usize);
                                }
                            }
                        },
                        table { class: "diff-table",
                            colgroup {
                                col { style: "width: {ln_ch}ch" }
                                col { style: "width: {ln_ch}ch" }
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
                                for (i, item) in items[start..end].iter().enumerate() {
                                    {
                                        let item_idx = start + i;
                                        let sel = *selected_line.read();
                                        let is_sel = sel == Some(item_idx);
                                        match item {
                                            DiffItem::Line(line) => {
                                                let ln = item_idx;
                                                let old = line
                                                    .old_ln
                                                    .map(|n| n.to_string())
                                                    .unwrap_or_default();
                                                let new = line
                                                    .new_ln
                                                    .map(|n| n.to_string())
                                                    .unwrap_or_default();
                                                rsx! {
                                                    tr {
                                                        class: "diff-line diff-line-{line.kind:?}",
                                                        class: if is_sel { "diff-line-sel" },
                                                        onclick: move |_| selected_line.set(Some(ln)),
                                                        td { class: "diff-ln", "{old}" }
                                                        td { class: "diff-ln", "{new}" }
                                                        td {
                                                            class: "diff-content",
                                                            pre { "{line.text}" }
                                                        }
                                                    }
                                                }
                                            }
                                            DiffItem::Comment(flat) => {
                                                let author =
                                                    flat.comment.author.clone().unwrap_or_default();
                                                let ts = flat
                                                    .comment
                                                    .timestamp
                                                    .as_deref()
                                                    .and_then(|s| {
                                                        s.parse::<i64>()
                                                            .ok()
                                                            .and_then(|secs| {
                                                                chrono::DateTime::from_timestamp(
                                                                    secs, 0,
                                                                )
                                                            })
                                                    })
                                                    .map(|dt| {
                                                        dt.format("%Y-%m-%d %H:%M").to_string()
                                                    })
                                                    .unwrap_or_default();
                                                let indent = flat.depth * 16;
                                                let ln = item_idx;
                                                rsx! {
                                                    tr {
                                                        class: "diff-comment-row",
                                                        class: if is_sel { "diff-line-sel" },
                                                        onclick: move |_| selected_line.set(Some(ln)),
                                                        td { colspan: "3",
                                                            div {
                                                                class: "diff-comment-inline",
                                                                style: "margin-left: {indent}px",
                                                                {render_comment_card(&author, &ts, &flat.comment.message)}
                                                            }
                                                        }
                                                    }
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

#[derive(Clone)]
struct DiffLine {
    kind: DiffLineKind,
    text: String,
    old_ln: Option<usize>,
    new_ln: Option<usize>,
    line_number: usize,
}

#[derive(Clone)]
struct FlatComment {
    comment: josh_changes::Comment,
    depth: usize,
}

enum DiffItem {
    Line(DiffLine),
    Comment(FlatComment),
}

fn estimate_item_height(item: &DiffItem) -> u32 {
    match item {
        DiffItem::Line(_) => 20,
        DiffItem::Comment(c) => {
            let lines = c.comment.message.lines().count().max(1) as u32;
            40 + lines * 20
        }
    }
}

fn parse_hunk_header(header: &str) -> (usize, usize) {
    // Format: @@ -old_start[,old_count] +new_start[,new_count] @@
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
                let header = String::from_utf8_lossy(hunk.header());
                let (mut old_ln, mut new_ln) = parse_hunk_header(&header);
                n += 1;
                lines.push(DiffLine {
                    kind: DiffLineKind::Hunk,
                    text: header.to_string(),
                    old_ln: None,
                    new_ln: None,
                    line_number: n,
                });
                for l in 0..hunk_lines {
                    n += 1;
                    let line = patch.line_in_hunk(h, l)?;
                    let origin = line.origin();
                    let (kind, old, new) = match origin {
                        '+' => {
                            let curr = new_ln;
                            new_ln += 1;
                            (DiffLineKind::Add, None, Some(curr))
                        }
                        '-' => {
                            let curr = old_ln;
                            old_ln += 1;
                            (DiffLineKind::Del, Some(curr), None)
                        }
                        ' ' => {
                            let o = old_ln;
                            let n = new_ln;
                            old_ln += 1;
                            new_ln += 1;
                            (DiffLineKind::Context, Some(o), Some(n))
                        }
                        _ => (DiffLineKind::Context, None, None),
                    };
                    lines.push(DiffLine {
                        kind,
                        text: String::from_utf8_lossy(line.content()).to_string(),
                        old_ln: old,
                        new_ln: new,
                        line_number: n,
                    });
                }
            }
        }
    }

    Ok(lines)
}

fn flatten_thread(
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

fn build_diff_with_comments(
    lines: &[DiffLine],
    comments: &[josh_changes::Comment],
    file_path: &str,
) -> Vec<DiffItem> {
    let matching: Vec<&josh_changes::Comment> = comments
        .iter()
        .filter(|c| c.file.as_deref() == Some(file_path))
        .collect();
    if matching.is_empty() {
        return lines.iter().map(|l| DiffItem::Line(l.clone())).collect();
    }

    let all_comment_indices: Vec<usize> = (0..comments.len()).collect();

    // Flatten per-file comments, keyed by start_line.
    let mut flat: Vec<(u32, FlatComment)> = Vec::new();
    let file_indices: Vec<usize> = all_comment_indices
        .iter()
        .filter(|&&i| comments[i].file.as_deref() == Some(file_path))
        .copied()
        .collect();
    let roots: Vec<usize> = file_indices
        .iter()
        .filter(|&&i| comments[i].reply_to.is_none())
        .copied()
        .collect();
    for &root in &roots {
        let target = comments[root]
            .location
            .as_ref()
            .map(|loc| loc.start_line)
            .unwrap_or(0);
        flatten_thread(comments, &[root], target, 0, &mut flat);
    }

    // Build lookup: new_ln -> list of indices into flat.
    let mut by_line: std::collections::HashMap<u32, Vec<usize>> = std::collections::HashMap::new();
    for (fi, (target, _)) in flat.iter().enumerate() {
        by_line.entry(*target).or_default().push(fi);
    }

    let mut inserted = vec![false; flat.len()];
    let mut items: Vec<DiffItem> = Vec::new();

    // Walk diff lines; insert comments after matching lines.
    for line in lines {
        items.push(DiffItem::Line(line.clone()));
        if let Some(ln) = line.new_ln {
            if let Some(indices) = by_line.get(&(ln as u32)) {
                for &fi in indices {
                    if !inserted[fi] {
                        inserted[fi] = true;
                        items.push(DiffItem::Comment(flat[fi].1.clone()));
                    }
                }
            }
        }
    }

    // Append any remaining unmatched comments.
    for (fi, _) in flat.iter().enumerate() {
        if !inserted[fi] {
            items.push(DiffItem::Comment(flat[fi].1.clone()));
        }
    }

    items
}

fn render_comment_card(author: &str, ts: &str, message: &str) -> Element {
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

fn render_threads(
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

struct StackCommit {
    sha: String,
    subject: String,
    author: String,
    series: String,
}

struct DetailData {
    change_id: String,
    sha: String,
    subject: String,
    message: String,
    author: String,
    date: String,
    series: String,
    files: Vec<FileStat>,
    comments: Vec<josh_changes::Comment>,
    revisions: Vec<josh_changes::Revision>,
    stack: Vec<StackCommit>,
    pr_info: Option<PrInfo>,
}

struct PrInfo {
    url: String,
    title: String,
    state: String,
}

fn load_detail(sha: &str) -> anyhow::Result<DetailData> {
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
        if let Ok(tree) = repo.find_reference("refs/josh/changes")
            .and_then(|r| r.peel_to_tree())
        {
            let pr_path = std::path::Path::new("gh")
                .join(josh_changes::encode_change_id_path(cid));
            pr_info = tree.get_path(&pr_path).ok()
                .and_then(|e| e.to_object(&repo).ok())
                .and_then(|o| o.peel_to_tree().ok())
                .and_then(|t| t.iter().next()
                    .and_then(|e| e.to_object(&repo).ok())
                    .and_then(|o| o.peel_to_blob().ok())
                )
                .and_then(|b| {
                    let content = String::from_utf8_lossy(b.content());
                    serde_json::from_str::<serde_json::Value>(&content).ok()
                        .map(|v| PrInfo {
                            url: v["url"].as_str().unwrap_or("").to_string(),
                            title: v["title"].as_str().unwrap_or("").to_string(),
                            state: v["state"].as_str().unwrap_or("").to_string(),
                        })
                });

            let path = std::path::Path::new("diffs")
                .join(josh_changes::encode_change_id_path(cid));
            if let Some(entry) = tree.get_path(&path).ok()
                .and_then(|e| e.to_object(&repo).ok())
                .and_then(|o| o.peel_to_tree().ok())
            {
                if let Some(blob_entry) = entry.iter().next()
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
    Stack {
        change_id: String,
        sha: String,
        subject: String,
        author: String,
        series: String,
    },
}

fn load_rows() -> anyhow::Result<Vec<Row>> {
    let repo = git2::Repository::discover(".")?;

    let changes = josh_changes::list_changes(&repo)?;

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

        let mut stack_rows = Vec::new();
        for oid in change.contributing(&repo)? {
            if let Ok(c) = repo.find_commit(oid) {
                let msg = c.message().unwrap_or("");
                let c_subject = msg.lines().next().unwrap_or("").to_string();
                let c_author = c.author().email().unwrap_or("").to_string();
                let (c_change_id, c_series) = josh_changes::parse_change_meta(msg);
                stack_rows.push(Row::Stack {
                    change_id: c_change_id.unwrap_or_default(),
                    sha: oid.to_string(),
                    subject: c_subject,
                    author: c_author,
                    series: c_series.join(", "),
                });
            }
        }
        groups.push((change_row, stack_rows));
    }

    groups.sort_by_key(|(_, contrib)| contrib.len());

    let mut rows = Vec::new();
    for (change_row, stack_rows) in groups {
        rows.push(change_row);
        rows.extend(stack_rows);
    }
    Ok(rows)
}
