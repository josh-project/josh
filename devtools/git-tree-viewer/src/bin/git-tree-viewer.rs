use git_tree_viewer::ui::mode_dialog::{select_mode, Mode};
use git_tree_viewer::{show_repo_viewer, AppMode, RepoSource, Trace};

use clap::Parser;
use std::env;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    rev_spec: Option<String>,

    #[arg(long)]
    mode: Option<Mode>,
}

fn main() {
    let args = Args::parse();
    let current_dir = env::current_dir().expect("Failed to get current directory");

    let (mode, repo_source) = match args.mode.or_else(select_mode) {
        Some(Mode::Browse) => (
            AppMode::Browse {
                rev_spec: args.rev_spec,
            },
            RepoSource::Path(current_dir),
        ),
        Some(Mode::Trace) => {
            let repo_source = RepoSource::new_temp().expect("Failed to create temp repo");
            let (tx, rx) = std::sync::mpsc::channel::<Trace>();

            git_tree_viewer::server::start(tx, repo_source.as_ref());

            (
                AppMode::Trace {
                    traces: Default::default(),
                    rx,
                },
                repo_source,
            )
        }
        None => {
            eprintln!("No mode selected, exiting");
            return;
        }
    };

    if let Err(e) = show_repo_viewer(mode, repo_source) {
        eprintln!("Error running viewer: {}", e);
        std::process::exit(1);
    }
}
