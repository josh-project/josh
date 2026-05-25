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
                                th { "Subject" }
                                th { "Author" }
                                th { "Series" }
                            }
                        }
                        tbody {
                            for row in rows.iter() {
                                match row {
                                    Row::Change { change_id, subject, author, series } => rsx! {
                                        tr {
                                            td { code { "{change_id}" } }
                                            td { "{subject}" }
                                            td { "{author}" }
                                            td { "{series}" }
                                        }
                                    },
                                    Row::Contributing { oid, subject } => rsx! {
                                        tr { class: "contributing",
                                            td { code { "{oid}" } }
                                            td { "{subject}" }
                                            td {}
                                            td {}
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
        subject: String,
        author: String,
        series: String,
    },
    Contributing {
        oid: String,
        subject: String,
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

    let mut rows = Vec::new();
    for change in &changes {
        let commit = repo.find_commit(change.commit)?;
        let subject = commit
            .message()
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        rows.push(Row::Change {
            change_id: change.id.clone().unwrap_or_default(),
            subject,
            author: change.author.clone(),
            series: change.series.join(", "),
        });

        for oid in change.contributing(&repo)? {
            if let Ok(c) = repo.find_commit(oid) {
                let c_subject = c
                    .message()
                    .unwrap_or("")
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                rows.push(Row::Contributing {
                    oid: oid.to_string()[..8].to_string(),
                    subject: c_subject,
                });
            }
        }
    }
    Ok(rows)
}
