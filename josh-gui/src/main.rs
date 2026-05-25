use dioxus::prelude::*;
use josh_changes::PushMode;

fn main() {
    dioxus::launch(app);
}

fn app() -> Element {
    let changes = use_signal(load_changes);
    let mode = use_signal(|| format!("{:?}", PushMode::Normal));

    rsx! {
        style { {include_str!("style.css")} }
        div { class: "app",
            h1 { "josh-gui" }
            p { class: "mode", "PushMode: {mode}" }
            match &*changes.read() {
                Ok(entries) if entries.is_empty() => rsx! {
                    p { "No Change: trailers found on HEAD's first-parent history." }
                },
                Ok(entries) => rsx! {
                    ul {
                        for entry in entries.iter() {
                            li {
                                code { "{entry.oid}" }
                                " — "
                                span { "{entry.change_id}" }
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
struct Entry {
    oid: String,
    change_id: String,
}

fn load_changes() -> anyhow::Result<Vec<Entry>> {
    let repo = git2::Repository::discover(".")?;
    let head = repo.head()?.peel_to_commit()?;

    let mut walk = repo.revwalk()?;
    walk.simplify_first_parent()?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(head.id())?;

    let mut out = Vec::new();
    for (i, oid) in walk.enumerate() {
        if i >= 50 {
            break;
        }
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        if let Some(id) = trailer(commit.message().unwrap_or(""), "Change") {
            out.push(Entry {
                oid: oid.to_string(),
                change_id: id,
            });
        }
    }
    Ok(out)
}

fn trailer(message: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}: ");
    let alt = format!("{key}-Id: ");
    message
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix(&prefix).or_else(|| l.strip_prefix(&alt)))
        .map(|s| s.to_string())
}
