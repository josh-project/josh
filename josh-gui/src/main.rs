use std::collections::{HashMap, HashSet};
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

fn review_decision_label(rd: &str) -> &'static str {
    match rd {
        "Approved" => "approved",
        "ChangesRequested" => "changes-requested",
        "ReviewRequired" => "review-required",
        _ => "",
    }
}

fn review_decision_display(rd: &str) -> &'static str {
    match rd {
        "Approved" => "Approved",
        "ChangesRequested" => "Changes requested",
        "ReviewRequired" => "Review required",
        _ => "",
    }
}

fn app() -> Element {
    let list_data = use_signal(load_rows);
    let page = use_signal(|| Page::List);
    let mut selected_change = use_signal(|| None::<String>);

    use_effect(move || {
        if let Ok(data) = &*list_data.read() {
            if let Some(first) = data.rows.first() {
                let cid = first.change_id.clone();
                if selected_change.read().is_none() {
                    selected_change.set(Some(cid));
                }
            }
        }
    });

    use_effect(move || {
        if matches!(&*page.read(), Page::List) {
            let _ = dioxus::document::eval(
                "setTimeout(function(){var el=document.querySelector('.app');if(el)el.focus();},0)",
            );
        }
    });

    use_effect(move || {
        let cid = selected_change.read();
        if let Some(cid) = cid.as_deref() {
            if let Ok(data) = &*list_data.read() {
                if let Some(row) = data.rows.iter().find(|r| r.change_id == cid) {
                    let js = format!(
                        "var el=document.getElementById('change-{0}');if(el)el.scrollIntoView({{block:'nearest'}});",
                        row.sha
                    );
                    let _ = dioxus::document::eval(&js);
                }
            }
        }
    });

    let on_keydown = move |evt: dioxus::events::KeyboardEvent| {
        use dioxus::prelude::Key;
        let is_nav = match evt.key() {
            Key::ArrowDown | Key::ArrowUp => true,
            Key::Character(ref c) => c == "j" || c == "k",
            _ => false,
        };
        if !is_nav {
            return;
        }
        evt.prevent_default();

        if let Ok(data) = &*list_data.read() {
            let ids: Vec<&str> = data.rows.iter().map(|r| r.change_id.as_str()).collect();
            let cur = {
                let sel = selected_change.read();
                sel.as_deref()
                    .and_then(|cid| ids.iter().position(|id| *id == cid))
            };
            let next = match (evt.key(), cur) {
                (Key::ArrowDown, Some(i)) if i + 1 < ids.len() => Some(i + 1),
                (Key::ArrowUp, Some(i)) if i > 0 => Some(i - 1),
                (Key::Character(ref c), Some(i)) if c == "j" && i + 1 < ids.len() => Some(i + 1),
                (Key::Character(ref c), Some(i)) if c == "k" && i > 0 => Some(i - 1),
                _ => None,
            };
            if let Some(i) = next {
                selected_change.set(Some(ids[i].to_string()));
            }
        }
    };

    rsx! {
        style { {include_str!("style.css")} }
        div { class: "app", tabindex: "0", onkeydown: on_keydown,
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
                {breadcrumb(&page.read(), page, list_data)}
            }
            match &*page.read() {
                Page::List => list_view(list_data, page, selected_change),
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

fn subject_from_rows(list_data: &anyhow::Result<ListData>, sha: &str) -> String {
    let Ok(data) = list_data else {
        return String::new();
    };
    data.rows
        .iter()
        .find(|r| r.sha == sha)
        .map(|r| r.subject.clone())
        .unwrap_or_default()
}

fn breadcrumb(
    page: &Page,
    mut page_signal: Signal<Page>,
    list_data: Signal<anyhow::Result<ListData>>,
) -> Element {
    let repo = repo_name();
    let subject = match page {
        Page::List => String::new(),
        Page::Detail { sha } | Page::FileDiff { sha, .. } => {
            subject_from_rows(&list_data.read(), sha)
        }
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

fn list_view(
    list_data: Signal<anyhow::Result<ListData>>,
    mut page: Signal<Page>,
    mut selected_change: Signal<Option<String>>,
) -> Element {
    match &*list_data.read() {
        Ok(data) if data.rows.is_empty() => rsx! {
            p { "No outgoing changes found." }
        },
        Ok(data) => {
            let sel_cid = selected_change.read();
            let related: HashSet<&str> = if let Some(cid) = sel_cid.as_deref() {
                let mut set = HashSet::new();
                if let Some(deps) = data.dependencies.get(cid) {
                    for d in deps {
                        set.insert(d.as_str());
                    }
                }
                if let Some(dep_by) = data.dependents.get(cid) {
                    for d in dep_by {
                        set.insert(d.as_str());
                    }
                }
                set
            } else {
                HashSet::new()
            };

            let row_items: Vec<(String, String, String)> = data
                .rows
                .iter()
                .map(|row| {
                    let is_sel = sel_cid.as_deref() == Some(&row.change_id);
                    let class = if is_sel {
                        "selected"
                    } else if related.contains(row.change_id.as_str()) {
                        "related"
                    } else {
                        ""
                    };
                    (class.to_string(), row.sha.clone(), row.change_id.clone())
                })
                .collect();

            rsx! {
                div { class: "scroll-table",
                    table { class: "changes",
                        thead {
                            tr {
                                th { "Change-Id" }
                                th { "Subject" }
                                th { "Author" }
                                th { "Series" }
                                th { "Review" }
                            }
                        }
                        tbody {
                            for (i, item) in row_items.iter().enumerate() {
                                {
                                    let row = &data.rows[i];
                                    let class = &item.0;
                                    let sha = &item.1;
                                    let cid = &item.2;
                                    rsx! {
                                        tr {
                                            id: "change-{row.sha}",
                                            class: "{class}",
                                            onclick: {
                                                let s = sha.clone();
                                                move |_| page.set(Page::Detail { sha: s.clone() })
                                            },
                                            td {
                                                onclick: {
                                                    let c = cid.clone();
                                                    move |evt| {
                                                        evt.stop_propagation();
                                                        selected_change.set(Some(c.clone()));
                                                    }
                                                },
                                                code { "{row.change_id}" }
                                            }
                                            td { "{row.subject}" }
                                            td { "{row.author}" }
                                            td { "{row.series}" }
                                            td {
                                                class: "review-{review_decision_label(&row.review_decision)}",
                                                "{review_decision_display(&row.review_decision)}"
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
                    }
                }
            }
        }
    }
}

fn file_diff_view(sha: String, path: String, mut page: Signal<Page>) -> Element {
    let detail = use_signal(|| load_detail(&sha));
    let (prev_file, next_file) = detail
        .read()
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
    let mut commenting = use_signal(|| false);
    let mut comment_text = use_signal(|| String::new());

    // Reset scroll position when navigating to a different file
    let mut last_path = use_signal(|| String::new());
    let current_file = format!("{}:{}", sha, path);
    if *last_path.read() != current_file {
        scroll_offset.set(0);
        selected_line.set(None);
        commenting.set(false);
        comment_text.set("".to_string());
        last_path.set(current_file);
    }

    match &all_lines {
        Err(e) => rsx! {
            p { class: "error", "Error: {e}" }
        },
        Ok(lines) => {
            let comments: Vec<josh_changes::Comment> = detail
                .read()
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
                        {
                            let ln = selected_file_line(&items, *selected_line.read());
                            rsx! {
                                button {
                                    class: "nav-btn",
                                    disabled: ln.is_none() || *commenting.read(),
                                    onclick: move |_| {
                                        if ln.is_some() {
                                            commenting.set(true);
                                            comment_text.set("".to_string());
                                        }
                                    },
                                    if let Some(n) = ln {
                                        "Comment on line {n}"
                                    } else {
                                        "Select a line to comment"
                                    }
                                }
                            }
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
                    if *commenting.read() {
                        {
                            let ln = selected_file_line(&items, *selected_line.read());
                            let sha_for_save = sha.clone();
                            let path_for_save = path.clone();
                            rsx! {
                                div { class: "comment-form",
                                    div { class: "comment-form-header",
                                        if let Some(n) = ln {
                                            "Commenting on line {n} of {path_for_save}"
                                        }
                                    }
                                    textarea {
                                        class: "comment-input",
                                        placeholder: "Write your comment...",
                                        value: "{comment_text}",
                                        autofocus: true,
                                        oninput: move |e| comment_text.set(e.value()),
                                    }
                                    div { class: "comment-form-actions",
                                        button {
                                            class: "nav-btn",
                                            onclick: move |_| {
                                                commenting.set(false);
                                                comment_text.set("".to_string());
                                            },
                                            "Cancel"
                                        }
                                        button {
                                            class: "nav-btn",
                                            disabled: comment_text.read().trim().is_empty(),
                                            onclick: {
                                                let mut detail_sig = detail;
                                                let sha_save = sha_for_save.clone();
                                                let path_save = path_for_save.clone();
                                                move |_| {
                                                    let msg = comment_text.read().trim().to_string();
                                                    if msg.is_empty() {
                                                        return;
                                                    }
                                                    if let Some(line_num) = selected_file_line(
                                                        &items,
                                                        *selected_line.read(),
                                                    ) {
                                                        if save_comment(
                                                            &sha_save,
                                                            &path_save,
                                                            line_num,
                                                            &msg,
                                                        )
                                                        .is_ok()
                                                        {
                                                            detail_sig
                                                                .set(load_detail(&sha_save));
                                                            commenting.set(false);
                                                            comment_text.set("".to_string());
                                                        }
                                                    }
                                                }
                                            },
                                            "Save"
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

fn selected_file_line(items: &[DiffItem], sel: Option<usize>) -> Option<u32> {
    let idx = sel?;
    for i in (0..=idx.min(items.len().saturating_sub(1))).rev() {
        if let DiffItem::Line(line) = &items[i] {
            return line.new_ln.map(|n| n as u32);
        }
    }
    None
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
    review_decision: String,
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

fn save_comment(
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

#[derive(Clone)]
struct Row {
    change_id: String,
    sha: String,
    subject: String,
    author: String,
    series: String,
    review_decision: String,
}

struct ListData {
    rows: Vec<Row>,
    dependencies: HashMap<String, Vec<String>>,
    dependents: HashMap<String, Vec<String>>,
}

fn load_rows() -> anyhow::Result<ListData> {
    let repo = git2::Repository::discover(".")?;

    let changes = josh_changes::list_changes(&repo)?;

    let mut oid_to_change_id: HashMap<String, String> = HashMap::new();
    for change in &changes {
        oid_to_change_id.insert(
            change.commit().to_string(),
            change.id().unwrap_or("").to_string(),
        );
    }

    let mut rows = Vec::new();
    let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();
    let mut dep_counts: HashMap<String, usize> = HashMap::new();

    for change in &changes {
        let commit = repo.find_commit(change.commit())?;
        let subject = commit
            .message()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        let change_id = change.id().unwrap_or("").to_string();

        let review_decision = josh_changes::read_pr_data(&repo, &change_id)
            .ok()
            .flatten()
            .and_then(|json| {
                serde_json::from_str::<serde_json::Value>(&json)
                    .ok()
                    .and_then(|v| v["review_decision"].as_str().map(|s| s.to_string()))
            })
            .unwrap_or_default();

        rows.push(Row {
            change_id: change_id.clone(),
            sha: change.commit().to_string(),
            subject,
            author: change.author().to_string(),
            series: change.series().join(", "),
            review_decision,
        });

        let mut deps: Vec<String> = Vec::new();
        for oid in change.contributing(&repo)? {
            let oid_str = oid.to_string();
            if let Some(dep_id) = oid_to_change_id.get(&oid_str) {
                if dep_id != &change_id {
                    deps.push(dep_id.clone());
                }
            }
        }
        let count = deps.len();
        dependencies.insert(change_id.clone(), deps);
        dep_counts.insert(change_id, count);
    }

    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    for (cid, deps) in &dependencies {
        for dep_id in deps {
            dependents
                .entry(dep_id.clone())
                .or_default()
                .push(cid.clone());
        }
    }

    rows.sort_by_key(|r| dep_counts.get(&r.change_id).copied().unwrap_or(0));

    Ok(ListData {
        rows,
        dependencies,
        dependents,
    })
}
