mod common;
mod detail;
mod diff;
mod list;

use std::collections::HashMap;

use clap::Parser;
use dioxus::prelude::*;

use common::breadcrumb;
use detail::DetailView;
use diff::FileDiffView;
use list::{ListView, RowMetadata, load_rows};

#[derive(Debug, Parser)]
#[command(name = "josh-gui")]
struct Cli {
    /// Target branch (default: HEAD's branch).
    #[arg(short = 'b', long = "branch")]
    branch: Option<String>,

    /// View the changes ref for this remote instead of the Local one.
    #[arg(long = "remote")]
    remote: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    let scope = resolve_initial_scope(&cli);

    dioxus::LaunchBuilder::new()
        .with_cfg(
            dioxus::desktop::Config::new()
                .with_window(dioxus::desktop::WindowBuilder::new().with_title("Josh")),
        )
        .with_context(scope)
        .launch(app);
}

fn resolve_initial_scope(cli: &Cli) -> josh_changes::ChangesRef {
    let branch = cli.branch.clone().unwrap_or_else(|| {
        git2::Repository::discover(".")
            .ok()
            .and_then(|r| josh_changes::head_branch(&r).ok())
            .unwrap_or_default()
    });
    match &cli.remote {
        Some(name) => josh_changes::ChangesRef::Remote {
            remote: name.clone(),
            branch,
        },
        None => josh_changes::ChangesRef::Local { branch },
    }
}

pub fn scope_label(scope: &josh_changes::ChangesRef) -> String {
    match scope {
        josh_changes::ChangesRef::Local { branch } => format!("Local · {}", branch),
        josh_changes::ChangesRef::Remote { remote, branch } => {
            format!("remote: {} · {}", remote, branch)
        }
    }
}

/// Open a fresh josh-core transaction and run `josh_github_changes::sync::sync`
/// against `scope`. Mirrors the cache+transaction wiring in
/// `josh-cli/src/bin/josh.rs:248` (sled load, layered backends, open per call)
/// so the GUI gets behavior identical to running `josh changes sync` from the
/// terminal. Transactions are intentionally short-lived: built per invocation
/// and dropped when the sync finishes.
async fn run_sync(scope: josh_changes::ChangesRef) -> anyhow::Result<()> {
    let repo = git2::Repository::discover(".")?;
    let repo_path = repo.path().to_path_buf();
    let git_common_dir = repo.commondir().to_path_buf();
    josh_core::cache::sled_load(&git_common_dir)?;
    let cache = std::sync::Arc::new(
        josh_core::cache::CacheStack::new()
            .with_backend(josh_core::cache::SledCacheBackend::default())
            .with_backend(josh_core::cache::DistributedCacheBackend::new(
                &git_common_dir,
            )?),
    );
    let txn = josh_core::cache::TransactionContext::new(&repo_path, cache).open()?;
    josh_github_changes::sync::sync(
        &repo,
        &txn,
        &scope,
        josh_github_changes::sync::SyncOptions::default(),
    )
    .await?;
    Ok(())
}

#[derive(Clone, PartialEq)]
pub enum Page {
    List,
    Detail { sha: String },
    FileDiff { sha: String, path: String },
}

fn app() -> Element {
    let initial_scope = use_context::<josh_changes::ChangesRef>();
    let current_scope = use_signal(|| initial_scope.clone());

    // Single source of truth for "what state is the changes ref in right now".
    // Bumped by local mutations (save_comment / save_vote) and by a background
    // poll that detects external changes (fetches, other processes). Loaders
    // across the app subscribe to this signal so a bump triggers a refresh
    // everywhere derived data is shown.
    let mut changes_ref_oid: Signal<Option<git2::Oid>> = use_signal(|| {
        git2::Repository::discover(".")
            .ok()
            .and_then(|r| josh_changes::read_ref_oid(&r, &current_scope.read()))
    });
    use_context_provider(|| changes_ref_oid);

    let mut list_data: Signal<anyhow::Result<list::ListData>> =
        use_signal(|| load_rows(&current_scope.read()));
    let metadata_cache: Signal<HashMap<String, RowMetadata>> = use_signal(HashMap::new);
    let scroll_offset = use_signal(|| 0usize);
    let page = use_signal(|| Page::List);
    let mut selected_change = use_signal(|| None::<String>);

    // Reload the list whenever the ref OID or scope changes.
    use_effect(move || {
        let _ = changes_ref_oid.read();
        let scope = current_scope.read().clone();
        list_data.set(load_rows(&scope));
    });

    // When the scope changes, re-read the OID immediately so we don't have to
    // wait for the next poll tick.
    use_effect(move || {
        let scope = current_scope.read().clone();
        let new_oid = git2::Repository::discover(".")
            .ok()
            .and_then(|r| josh_changes::read_ref_oid(&r, &scope));
        if new_oid != *changes_ref_oid.peek() {
            changes_ref_oid.set(new_oid);
        }
    });

    // Poll the ref's OID once a second to pick up external changes. Reading a
    // ref is a tiny disk read; we only write the signal (and trigger reloads)
    // when the OID actually changes, so an idle GUI stays idle.
    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let scope = current_scope.peek().clone();
            let new_oid = git2::Repository::discover(".")
                .ok()
                .and_then(|r| josh_changes::read_ref_oid(&r, &scope));
            if new_oid != *changes_ref_oid.peek() {
                changes_ref_oid.set(new_oid);
            }
        }
    });

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

    let on_keydown = move |evt: dioxus::events::KeyboardEvent| {
        use dioxus::prelude::Key;
        if !matches!(&*page.read(), Page::List) {
            return;
        }
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

    let scope_text = scope_label(&current_scope.read());
    let mut sync_in_progress: Signal<bool> = use_signal(|| false);
    let scope_class = if *sync_in_progress.read() {
        "header-scope syncing"
    } else {
        "header-scope"
    };
    let on_sync_click = move |_| {
        // peek() avoids re-subscribing this handler to a signal it writes.
        if *sync_in_progress.peek() {
            return;
        }
        sync_in_progress.set(true);
        let scope = current_scope.read().clone();
        spawn(async move {
            if let Err(e) = run_sync(scope).await {
                eprintln!("sync failed: {e:#}");
            }
            sync_in_progress.set(false);
            // No manual oid bump; the 1s poll picks up new ref tips.
        });
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
                span { class: "{scope_class}", onclick: on_sync_click, "{scope_text}" }
            }
            match &*page.read() {
                Page::List => rsx! {
                    ListView {
                        list_data,
                        metadata_cache,
                        scroll_offset,
                        scope: current_scope.read().clone(),
                        page,
                        selected_change,
                    }
                },
                Page::Detail { sha } => rsx! {
                    DetailView {
                        sha: sha.clone(),
                        scope: current_scope.read().clone(),
                        page,
                    }
                },
                Page::FileDiff { sha, path } => rsx! {
                    FileDiffView {
                        sha: sha.clone(),
                        path: path.clone(),
                        scope: current_scope.read().clone(),
                        page,
                    }
                },
            }
        }
    }
}
