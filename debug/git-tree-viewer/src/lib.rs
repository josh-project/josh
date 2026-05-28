mod app;
pub mod constants;
pub mod git;
mod server;
pub mod ui;

use std::path::Path;

pub use ui::commit_list::show_commit_bubble;

#[derive(Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum AppMode {
    Browse,
    Trace,
}

pub struct Trace {
    pub session: String,
    pub commit: String,
    pub label: String,
}

pub struct GitDebugApp {
    pub repo: git2::Repository,
    pub resolved_commit: git2::Oid,
    pub history_start: git2::Oid,
    pub selected_commit: git2::Oid,
    pub selected_file: Option<(String, git2::Oid)>,
    pub file_content: Option<String>,
    pub error: Option<String>,
    pub traces: Vec<Trace>,
    pub selected_session: Option<String>,
    pub mode: AppMode,
    rx: Option<std::sync::mpsc::Receiver<Trace>>,
    tx: Option<std::sync::mpsc::Sender<Trace>>,
    server_started: bool,
}

impl GitDebugApp {
    pub fn new(
        repo_path: impl AsRef<Path>,
        commit_spec: Option<&str>,
        mode: AppMode,
    ) -> Result<Self, git2::Error> {
        let repo = git::open_repo(repo_path)?;
        let resolved_commit = git::resolve_commit(&repo, commit_spec)?;
        Ok(Self {
            repo,
            resolved_commit,
            history_start: resolved_commit,
            selected_commit: resolved_commit,
            selected_file: None,
            file_content: None,
            error: None,
            traces: Vec::new(),
            selected_session: None,
            mode,
            rx: None,
            tx: None,
            server_started: false,
        })
    }
}

pub fn show_repo_viewer(
    repo_path: impl AsRef<Path>,
    commit_spec: Option<&str>,
    mode: AppMode,
) -> anyhow::Result<()> {
    let repo_path = repo_path.as_ref().to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel::<Trace>();

    let mut app = GitDebugApp {
        rx: Some(rx),
        tx: Some(tx.clone()),
        ..GitDebugApp::new(repo_path, commit_spec, mode)?
    };

    if mode == AppMode::Trace {
        server::start(tx);
        app.server_started = true;
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_min_inner_size([constants::WINDOW_MIN_WIDTH, constants::WINDOW_MIN_HEIGHT]),
        ..Default::default()
    };

    eframe::run_native(
        "Git Tree Debugger",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    )?;

    Ok(())
}
