use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;

use crate::Page;
use crate::common::{
    check_status_display, check_status_label, review_decision_display, review_decision_label,
};

#[derive(Clone)]
pub struct Row {
    pub change_id: String,
    pub sha: String,
    pub subject: String,
    pub author: String,
    pub series: String,
    pub review_decision: String,
    pub check_status: String,
}

pub struct ListData {
    pub rows: Vec<Row>,
    pub dependencies: HashMap<String, Vec<String>>,
    pub dependents: HashMap<String, Vec<String>>,
}

pub fn list_view(
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
                                th { "Checks" }
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
                                            td {
                                                class: "check-{check_status_label(&row.check_status)}",
                                                "{check_status_display(&row.check_status)}"
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

pub fn load_rows() -> anyhow::Result<ListData> {
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

        let (review_decision, check_status) = josh_changes::read_pr_data(&repo, &change_id)
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

        rows.push(Row {
            change_id: change_id.clone(),
            sha: change.commit().to_string(),
            subject,
            author: change.author().to_string(),
            series: change.series().join(", "),
            review_decision,
            check_status,
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
