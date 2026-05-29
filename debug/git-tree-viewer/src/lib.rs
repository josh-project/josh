mod app;
pub mod constants;
pub mod git;
pub mod server;
pub mod ui;

use std::path::{Path, PathBuf};

pub use ui::commit_list::show_commit_bubble;

pub enum AppMode {
    Browse {
        rev_spec: Option<String>,
    },
    Trace {
        traces: Vec<Trace>,
        rx: std::sync::mpsc::Receiver<Trace>,
    },
}

pub enum RepoSource {
    TempDir(tempfile::TempDir),
    Path(PathBuf),
}

impl RepoSource {
    pub fn new_temp() -> anyhow::Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("git-tree-viewer")
            .tempdir()?;

        let repo = git2::Repository::init_bare(dir.path())?;
        repo.config()?.set_str("http.receivepack", "true")?;

        Ok(Self::TempDir(dir))
    }
}

impl AsRef<Path> for RepoSource {
    fn as_ref(&self) -> &Path {
        match self {
            RepoSource::TempDir(d) => d.path(),
            RepoSource::Path(p) => p.as_ref(),
        }
    }
}

pub struct Trace {
    pub session: String,
    pub commit: String,
    pub label: String,
}

pub struct UiState {
    history_start: Option<git2::Oid>,
    selected_commit: Option<git2::Oid>,
    selected_file: Option<(String, git2::Oid)>,
    file_content: Option<String>,
    selected_session: Option<String>,
    error: Option<String>,
}

pub struct GitDebugApp {
    mode: AppMode,
    repo: git2::Repository,
    ui_state: UiState,
    _repo_source: RepoSource,
}

impl GitDebugApp {
    pub fn new(mode: AppMode, repo_source: RepoSource) -> Result<Self, git2::Error> {
        let repo = git::open_repo(repo_source.as_ref())?;

        let resolved_commit = match &mode {
            AppMode::Browse {
                rev_spec: commit_spec,
            } => {
                let oid = git::resolve_commit(&repo, commit_spec.as_deref())?;
                Some(oid)
            }
            AppMode::Trace { .. } => None,
        };

        Ok(Self {
            repo,
            mode,
            ui_state: UiState {
                history_start: resolved_commit,
                selected_commit: resolved_commit,
                selected_file: None,
                file_content: None,
                selected_session: None,
                error: None,
            },
            _repo_source: repo_source,
        })
    }
}

pub fn show_repo_viewer(mode: AppMode, repo_source: RepoSource) -> anyhow::Result<()> {
    let app = GitDebugApp::new(mode, repo_source)?;

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
