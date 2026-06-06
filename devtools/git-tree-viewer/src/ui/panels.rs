use crate::constants::{PANEL_DEFAULT_WIDTH, SHA_SHORT_LEN};
use crate::ui::commit_list::{show_commit_bubble, show_commits};
use crate::ui::file_preview::show_file_preview;
use crate::ui::tree_view::show_tree_item;
use crate::GitDebugApp;
use crate::Trace;
use crate::{git, AppMode};

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
                app.ui_state.history_start,
                &mut app.ui_state.selected_commit,
                &mut app.ui_state.selected_file,
                &mut app.ui_state.file_content,
            );
        });
}

fn show_sessions_section(ui: &mut egui::Ui, app: &mut GitDebugApp) {
    let traces = match &app.mode {
        AppMode::Trace { traces, .. } => traces,
        _ => return,
    };

    let mut sessions: Vec<&String> = traces.iter().map(|t| &t.session).collect();
    sessions.sort();
    sessions.dedup();

    egui::ComboBox::from_label("Session")
        .selected_text(app.ui_state.selected_session.as_deref().unwrap_or("(all)"))
        .show_ui(ui, |ui| {
            for session in &sessions {
                ui.selectable_value(
                    &mut app.ui_state.selected_session,
                    Some(session.to_string()),
                    *session,
                );
            }
            ui.separator();
            if ui
                .selectable_label(app.ui_state.selected_session.is_none(), "(all)")
                .clicked()
            {
                app.ui_state.selected_session = None;
            }
        });

    let filtered: Vec<&Trace> = traces
        .iter()
        .filter(|t| {
            app.ui_state
                .selected_session
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
                    let selected = app.ui_state.selected_commit == Some(oid);
                    if show_commit_bubble(ui, selected, short_id, &trace.label).clicked() {
                        app.ui_state.history_start = Some(oid);
                        app.ui_state.selected_commit = Some(oid);
                        app.ui_state.selected_file = None;
                        app.ui_state.file_content = None;
                    }
                }
            }
        });
}

fn show_left_panel(ui: &mut egui::Ui, app: &mut GitDebugApp) {
    match app.mode {
        AppMode::Trace { .. } => {
            ui.heading("Sessions");
            show_sessions_section(ui, app);

            egui::Panel::bottom("left_bottom_pane")
                .resizable(true)
                .default_size(PANEL_DEFAULT_WIDTH)
                .show_inside(ui, |ui| {
                    show_commits_section(ui, app);
                });
        }
        AppMode::Browse { .. } => {
            show_commits_section(ui, app);
        }
    }
}

fn show_central_panel(ui: &mut egui::Ui, app: &mut GitDebugApp) {
    ui.heading("Tree contents");

    let selected_commit = match app.ui_state.selected_commit {
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
            let selected_oid = app.ui_state.selected_file.as_ref().map(|(_, oid)| *oid);
            for item in &tree_items {
                show_tree_item(ui, item, selected_oid, &mut |path, oid| {
                    app.ui_state.selected_file = Some((path, oid));
                    app.ui_state.file_content = Some(git::load_blob_content(&app.repo, oid));
                });
            }
        });
}

pub fn show_panels(ui: &mut egui::Ui, app: &mut GitDebugApp) {
    egui::Panel::top("top_panel").show_inside(ui, |ui| {
        show_top_panel(ui, &app.ui_state.error);
    });

    egui::Panel::left("left_panel")
        .default_size(PANEL_DEFAULT_WIDTH)
        .show_inside(ui, |ui| {
            show_left_panel(ui, app);
        });

    egui::Panel::right("right_panel")
        .default_size(PANEL_DEFAULT_WIDTH)
        .show_inside(ui, |ui| {
            show_file_preview(ui, &app.ui_state.selected_file, &app.ui_state.file_content);
        });

    egui::CentralPanel::default().show_inside(ui, |ui| {
        show_central_panel(ui, app);
    });
}
