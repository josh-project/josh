use dioxus::prelude::*;

fn main() {
    dioxus::launch(app);
}

fn app() -> Element {
    let rows = use_signal(load_rows);

    rsx! {
        style { {include_str!("style.css")} }
        div { class: "app",
            h1 { "josh-gui" }
            match &*rows.read() {
                Ok(rows) if rows.is_empty() => rsx! {
                    p { "No outgoing changes found." }
                },
                Ok(rows) => rsx! {
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
                                    Row::Change { change_id, sha, subject, author, series } => rsx! {
                                        tr {
                                            td { code { "{change_id}" } }
                                            td { code { "{sha}" } }
                                            td { "{subject}" }
                                            td { "{author}" }
                                            td { "{series}" }
                                        }
                                    },
                                    Row::Contributing { change_id, sha, subject, author, series } => rsx! {
                                        tr { class: "contributing",
                                            td { code { class: "muted", "{change_id}" } }
                                            td { code { "{sha}" } }
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
    }
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
            sha: change.commit.to_string()[..8].to_string(),
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
                    sha: oid.to_string()[..8].to_string(),
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
