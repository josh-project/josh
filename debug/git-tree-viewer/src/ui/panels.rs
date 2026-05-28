use crate::constants::{PANEL_DEFAULT_WIDTH, SHA_SHORT_LEN};
use crate::git;
use crate::ui::commit_list::{show_commit_bubble, show_commits};
use crate::ui::file_preview::show_file_preview;
use crate::ui::tree_view::show_tree_item;
use crate::GitDebugApp;
use crate::Trace;

fn show_top_panel(ui: &mut egui::Ui, error: &Option<String>) {
    ui.heading("Git Tree Viewer");

    if let Some(err) = error {
        ui.colored_label(egui::Color32::RED, err);
    }
}

fn show_commits_section(ui: &mut egui::Ui, app: &mut GitDebugApp) {
    ui.heading("Commits");
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            show_commits(
                ui,
                &app.repo,
                app.history_start,
                &mut app.selected_commit,
                &mut app.selected_file,
                &mut app.file_content,
            );
        });
}

fn show_sessions_section(ui: &mut egui::Ui, app: &mut GitDebugApp) {
    let mut sessions: Vec<&String> = app.traces.iter().map(|t| &t.session).collect();
    sessions.sort();
    sessions.dedup();

    egui::ComboBox::from_label("Session")
        .selected_text(app.selected_session.as_deref().unwrap_or("(all)"))
        .show_ui(ui, |ui| {
            for session in &sessions {
                ui.selectable_value(
                    &mut app.selected_session,
                    Some(session.to_string()),
                    *session,
                );
            }
            ui.separator();
            if ui
                .selectable_label(app.selected_session.is_none(), "(all)")
                .clicked()
            {
                app.selected_session = None;
            }
        });

    let filtered: Vec<&Trace> = app
        .traces
        .iter()
        .filter(|t| {
            app.selected_session
                .as_ref()
                .map(|s| &t.session == s)
                .unwrap_or(true)
        })
        .collect();

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(100.0)
        .show(ui, |ui| {
            for trace in &filtered {
                if let Ok(oid) = git2::Oid::from_str(&trace.commit) {
                    let short_id = &trace.commit[..SHA_SHORT_LEN.min(trace.commit.len())];
                    let selected = app.selected_commit == Some(oid);
                    if show_commit_bubble(ui, selected, short_id, &trace.label).clicked() {
                        app.history_start = Some(oid);
                        app.selected_commit = Some(oid);
                        app.selected_file = None;
                        app.file_content = None;
                    }
                }
            }
        });
}

fn show_left_panel(ui: &mut egui::Ui, app: &mut GitDebugApp) {
    match app.mode {
        crate::AppMode::Trace => {
            ui.heading("Sessions");
            show_sessions_section(ui, app);

            egui::Panel::bottom("left_bottom_pane")
                .resizable(true)
                .default_size(PANEL_DEFAULT_WIDTH)
                .show_inside(ui, |ui| {
                    show_commits_section(ui, app);
                });
        }
        crate::AppMode::Browse { .. } => {
            show_commits_section(ui, app);
        }
    }
}

fn show_central_panel(ui: &mut egui::Ui, app: &mut GitDebugApp) {
    ui.heading("Tree contents");

    let selected_commit = match app.selected_commit {
        None => return,
        Some(oid) => oid,
    };

    let tree_id = app
        .repo
        .find_commit(selected_commit)
        .expect("Failed to find commit")
        .tree()
        .expect("Failed to get tree")
        .id();

    let tree_items = git::build_tree(&app.repo, tree_id, "");

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let selected_oid = app.selected_file.as_ref().map(|(_, oid)| *oid);
            for item in &tree_items {
                show_tree_item(ui, item, selected_oid, &mut |path, oid| {
                    app.selected_file = Some((path, oid));
                    app.file_content = Some(git::load_blob_content(&app.repo, oid));
                });
            }
        });
}

pub fn show_panels(ui: &mut egui::Ui, app: &mut GitDebugApp) {
    egui::Panel::top("top_panel").show_inside(ui, |ui| {
        show_top_panel(ui, &app.error);
    });

    egui::Panel::left("left_panel")
        .default_size(PANEL_DEFAULT_WIDTH)
        .show_inside(ui, |ui| {
            show_left_panel(ui, app);
        });

    egui::Panel::right("right_panel")
        .default_size(PANEL_DEFAULT_WIDTH)
        .show_inside(ui, |ui| {
            show_file_preview(ui, &app.selected_file, &app.file_content);
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        show_central_panel(ui, app);
    });
}
