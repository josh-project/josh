use git_tree_viewer::{show_repo_viewer, AppMode};

use clap::Parser;
use std::env;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    commit: Option<String>,
    #[arg(long, value_enum)]
    mode: Option<AppMode>,
}

fn select_mode() -> AppMode {
    let (tx, rx) = std::sync::mpsc::channel();

    struct ModeDialog {
        tx: std::sync::mpsc::Sender<AppMode>,
    }

    impl eframe::App for ModeDialog {
        fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
            egui::CentralPanel::default().show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("Git Tree Viewer");
                    ui.add_space(12.0);
                    ui.label("Select mode:");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized([120.0, 40.0], egui::Button::new("Browse"))
                            .clicked()
                        {
                            self.tx.send(AppMode::Browse).ok();
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        ui.add_space(12.0);
                        if ui
                            .add_sized([120.0, 40.0], egui::Button::new("Trace"))
                            .clicked()
                        {
                            self.tx.send(AppMode::Trace).ok();
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
            });
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([320.0, 150.0])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "Git Tree Viewer",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(ModeDialog { tx }))
        }),
    )
    .ok();

    rx.recv().unwrap_or(AppMode::Browse)
}

fn main() {
    let args = Args::parse();
    let current_dir = env::current_dir().expect("Failed to get current directory");

    let mode = args.mode.unwrap_or_else(select_mode);

    if let Err(e) = show_repo_viewer(current_dir, args.commit.as_deref(), mode) {
        eprintln!("Error running viewer: {}", e);
        std::process::exit(1);
    }
}
