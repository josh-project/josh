use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;

use crate::Page;
use crate::common::{
    check_status_display, check_status_label, review_decision_display, review_decision_label,
    vote_state_display, vote_state_label,
};

const ROW_HEIGHT: u32 = 32;
const VISIBLE_ROWS: u32 = 40;
const OVERSCAN_ROWS: u32 = 20;

#[derive(Clone)]
pub struct Row {
    pub change_id: String,
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub series: String,
    pub contributing_count: usize,
}

#[derive(Clone, Default)]
pub struct RowMetadata {
    pub review_decision: String,
    pub check_status: String,
    pub local_vote: Option<String>,
    pub comments_count: usize,
}

pub struct ListData {
    pub rows: Vec<Row>,
    pub dependencies: HashMap<String, Vec<String>>,
    pub dependents: HashMap<String, Vec<String>>,
}

#[component]
pub fn ListView(
    list_data: Signal<anyhow::Result<ListData>>,
    metadata_cache: Signal<HashMap<String, RowMetadata>>,
    mut scroll_offset: Signal<usize>,
    scope: josh_changes::ChangesRef,
    mut page: Signal<Page>,
    mut selected_change: Signal<Option<String>>,
) -> Element {
    {
        let scope_for_meta = scope.clone();
        use_effect(move || {
            let offset = *scroll_offset.read() as u32;
            let viewport_px = VISIBLE_ROWS * ROW_HEIGHT;
            let overscan_px = OVERSCAN_ROWS * ROW_HEIGHT;
            let data_ref = list_data.peek();
            let Ok(data) = &*data_ref else {
                return;
            };
            let total = data.rows.len();
            let start = (offset.saturating_sub(overscan_px) / ROW_HEIGHT) as usize;
            let start = start.min(total);
            let end =
                ((offset + viewport_px + overscan_px).div_ceil(ROW_HEIGHT) as usize).min(total);
            let needed: Vec<String> = {
                let cache = metadata_cache.peek();
                data.rows[start..end]
                    .iter()
                    .filter(|r| !cache.contains_key(&r.change_id))
                    .map(|r| r.change_id.clone())
                    .collect()
            };
            drop(data_ref);
            if needed.is_empty() {
                return;
            }
            let fetched = load_metadata_batch(&scope_for_meta, &needed);
            if !fetched.is_empty() {
                let mut cache = metadata_cache.write();
                for (cid, meta) in fetched {
                    cache.insert(cid, meta);
                }
            }
        });
    }

    use_effect(move || {
        let offset = *scroll_offset.peek();
        if offset > 0 {
            let js = format!(
                "var el=document.getElementById('changes-scroll');if(el)el.scrollTop={};",
                offset
            );
            let _ = dioxus::document::eval(&js);
        }
    });

    let mut sel_effect_ready = use_signal(|| false);
    use_effect(move || {
        let sel = selected_change.read();
        if !*sel_effect_ready.peek() {
            sel_effect_ready.set(true);
            return;
        }
        let Some(cid) = sel.as_deref() else {
            return;
        };
        let data_ref = list_data.peek();
        let Ok(data) = &*data_ref else {
            return;
        };
        let Some(idx) = data.rows.iter().position(|r| r.change_id == cid) else {
            return;
        };
        let viewport_px = VISIBLE_ROWS * ROW_HEIGHT;
        let top = (idx as u32) * ROW_HEIGHT;
        let offset = *scroll_offset.peek() as u32;
        let target = if top < offset {
            Some(top)
        } else if top + ROW_HEIGHT > offset + viewport_px {
            Some((top + ROW_HEIGHT).saturating_sub(viewport_px))
        } else {
            None
        };
        if let Some(t) = target {
            let js = format!(
                "var el=document.getElementById('changes-scroll');if(el)el.scrollTop={};",
                t
            );
            let _ = dioxus::document::eval(&js);
        }
    });

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

            let total = data.rows.len();
            let offset = *scroll_offset.read() as u32;
            let viewport_px = VISIBLE_ROWS * ROW_HEIGHT;
            let overscan_px = OVERSCAN_ROWS * ROW_HEIGHT;
            let start = (offset.saturating_sub(overscan_px) / ROW_HEIGHT) as usize;
            let start = start.min(total);
            let end =
                ((offset + viewport_px + overscan_px).div_ceil(ROW_HEIGHT) as usize).min(total);

            let top_h = (start as u32) * ROW_HEIGHT;
            let bot_h = ((total - end) as u32) * ROW_HEIGHT;

            let cache = metadata_cache.read();

            rsx! {
                div {
                    class: "scroll-table",
                    id: "changes-scroll",
                    onscroll: move |e| {
                        scroll_offset.set(e.data.scroll_top() as usize);
                    },
                    table { class: "changes",
                        colgroup {
                            col {}
                            col { style: "width: 18ch" }
                            col { style: "width: 10ch" }
                            col { style: "width: 12ch" }
                            col { style: "width: 8ch" }
                            col { style: "width: 5ch" }
                            col { style: "width: 10ch" }
                            col { style: "width: 9ch" }
                        }
                        thead {
                            tr {
                                th { "Subject" }
                                th { "Author" }
                                th { "Series" }
                                th { "Review" }
                                th { "Vote" }
                                th { "Deps" }
                                th { "Comments" }
                                th { "Checks" }
                            }
                        }
                        tbody {
                            if top_h > 0 {
                                tr { style: "height: {top_h}px",
                                    td { colspan: "8" }
                                }
                            }
                            for i in start..end {
                                {
                                    let row = &data.rows[i];
                                    let is_sel = sel_cid.as_deref() == Some(&row.change_id);
                                    let class = if is_sel {
                                        "selected"
                                    } else if related.contains(row.change_id.as_str()) {
                                        "related"
                                    } else {
                                        ""
                                    };
                                    let sha = row.sha.clone();
                                    let cid = row.change_id.clone();
                                    let meta = cache.get(&row.change_id).cloned();
                                    let loading = meta.is_none();
                                    let m = meta.unwrap_or_default();
                                    rsx! {
                                        tr {
                                            id: "change-{row.sha}",
                                            class: "{class}",
                                            onclick: {
                                                let c = cid.clone();
                                                move |_| selected_change.set(Some(c.clone()))
                                            },
                                            td {
                                                onclick: {
                                                    let s = sha.clone();
                                                    move |evt| {
                                                        evt.stop_propagation();
                                                        page.set(Page::Detail { sha: s.clone() });
                                                    }
                                                },
                                                "{row.subject}"
                                            }
                                            td { "{row.author}" }
                                            td { "{row.series}" }
                                            td {
                                                class: "review-{review_decision_label(&m.review_decision)}",
                                                class: if loading { "loading" },
                                                "{review_decision_display(&m.review_decision)}"
                                            }
                                            td {
                                                class: if let Some(ref v) = m.local_vote {
                                                    "vote-{vote_state_label(v)}"
                                                },
                                                class: if loading { "loading" },
                                                if let Some(ref v) = m.local_vote {
                                                    "{vote_state_display(v)}"
                                                }
                                            }
                                            td { class: "num", "{row.contributing_count}" }
                                            td {
                                                class: "num",
                                                class: if loading { "loading" },
                                                if !loading && m.comments_count > 0 {
                                                    "{m.comments_count}"
                                                }
                                            }
                                            td {
                                                class: "check-{check_status_label(&m.check_status)}",
                                                class: if loading { "loading" },
                                                "{check_status_display(&m.check_status)}"
                                            }
                                        }
                                    }
                                }
                            }
                            if bot_h > 0 {
                                tr { style: "height: {bot_h}px",
                                    td { colspan: "8" }
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

pub fn load_rows(scope: &josh_changes::ChangesRef) -> anyhow::Result<ListData> {
    let repo = git2::Repository::discover(".")?;

    let changes = josh_changes::list_changes(&repo, scope)?;

    let mut oid_to_change_id: HashMap<String, String> = HashMap::new();
    for change in &changes {
        oid_to_change_id.insert(
            change.commit().to_string(),
            change.id().unwrap_or("").to_string(),
        );
    }

    let mut rows = Vec::new();
    let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();

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

        let mut deps: Vec<String> = Vec::new();
        for oid in change.contributing(&repo)? {
            let oid_str = oid.to_string();
            if let Some(dep_id) = oid_to_change_id.get(&oid_str) {
                if dep_id != &change_id {
                    deps.push(dep_id.clone());
                }
            }
        }

        rows.push(Row {
            change_id: change_id.clone(),
            sha: change.commit().to_string(),
            subject,
            author: change.author().to_string(),
            series: change.series().join(", "),
            contributing_count: deps.len(),
        });

        dependencies.insert(change_id, deps);
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

    rows.sort_by_key(|r| r.contributing_count);

    Ok(ListData {
        rows,
        dependencies,
        dependents,
    })
}

fn load_metadata(
    repo: &git2::Repository,
    scope: &josh_changes::ChangesRef,
    change_id: &str,
) -> RowMetadata {
    let (review_decision, check_status) = josh_changes::read_pr_data(repo, change_id, scope)
        .ok()
        .flatten()
        .and_then(|json| {
            serde_json::from_str::<serde_json::Value>(&json)
                .ok()
                .map(|v| {
                    let rd = v["review_decision"]
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    let cs = v["check_status"]
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    (rd, cs)
                })
        })
        .unwrap_or_default();

    let local_vote = josh_changes::read_vote(repo, change_id, None, scope)
        .ok()
        .flatten()
        .map(|v| v.state);

    let comments_count = josh_changes::read_comments(repo, change_id, scope)
        .map(|c| c.len())
        .unwrap_or(0);

    RowMetadata {
        review_decision,
        check_status,
        local_vote,
        comments_count,
    }
}

pub fn load_metadata_batch(
    scope: &josh_changes::ChangesRef,
    change_ids: &[String],
) -> Vec<(String, RowMetadata)> {
    if change_ids.is_empty() {
        return Vec::new();
    }
    let Ok(repo) = git2::Repository::discover(".") else {
        return Vec::new();
    };
    change_ids
        .iter()
        .map(|cid| (cid.clone(), load_metadata(&repo, scope, cid)))
        .collect()
}
