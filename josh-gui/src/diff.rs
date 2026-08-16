use dioxus::prelude::*;

use crate::Page;
use crate::common::{FlatComment, parse_hunk_header, render_comment_card};
use crate::detail::{self, DetailData, FileStat};

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

fn load_file_diff(sha: &str, path: &str, context_lines: u32) -> anyhow::Result<Vec<DiffLine>> {
    let repo = git2::Repository::discover(".")?;
    let oid = git2::Oid::from_str(sha)?;
    let commit = repo.find_commit(oid)?;

    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    let mut opts = git2::DiffOptions::new();
    opts.context_lines(context_lines);
    let diff =
        repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit.tree()?), Some(&mut opts))?;

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
        crate::common::flatten_thread(comments, &[root], target, 0, &mut flat);
    }

    let mut by_line: std::collections::HashMap<u32, Vec<usize>> = std::collections::HashMap::new();
    for (fi, (target, _)) in flat.iter().enumerate() {
        by_line.entry(*target).or_default().push(fi);
    }

    let mut inserted = vec![false; flat.len()];
    let mut items: Vec<DiffItem> = Vec::new();

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

    for (fi, _) in flat.iter().enumerate() {
        if !inserted[fi] {
            items.push(DiffItem::Comment(flat[fi].1.clone()));
        }
    }

    items
}

#[component]
pub fn FileDiffView(
    sha: String,
    path: String,
    scope: josh_changes::ChangesRef,
    mut page: Signal<Page>,
) -> Element {
    let mut detail = use_signal(|| detail::load_detail(&sha, &scope));
    let mut prev_sha = use_signal(|| sha.clone());
    if *prev_sha.read() != sha {
        detail.set(detail::load_detail(&sha, &scope));
        prev_sha.set(sha.clone());
    }
    let (prev_file, next_file) = detail
        .read()
        .as_ref()
        .ok()
        .and_then(|d: &DetailData| {
            let i = d.files.iter().position(|f: &FileStat| f.path == path)?;
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
                            let scope_for_save = scope.clone();
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
                                                let scope_save = scope_for_save.clone();
                                                move |_| {
                                                    let msg = comment_text.read().trim().to_string();
                                                    if msg.is_empty() {
                                                        return;
                                                    }
                                                    if let Some(line_num) = selected_file_line(
                                                        &items,
                                                        *selected_line.read(),
                                                    ) {
                                                        if detail::save_comment(
                                                            &sha_save,
                                                            &path_save,
                                                            line_num,
                                                            &msg,
                                                            &scope_save,
                                                        )
                                                        .is_ok()
                                                        {
                                                            detail_sig.set(detail::load_detail(
                                                                &sha_save,
                                                                &scope_save,
                                                            ));
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
