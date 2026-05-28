use crate::constants::{
    BUBBLE_CORNER_RADIUS, BUBBLE_HORIZONTAL_PADDING, COMMIT_BUBBLE_HEIGHT, FONT_SIZE,
    REVWALK_LIMIT, SHA_SHORT_LEN,
};
use git2::{Oid, Repository};

pub fn show_commit_bubble(
    ui: &mut egui::Ui,
    selected: bool,
    short_id: &str,
    message: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), COMMIT_BUBBLE_HEIGHT),
        egui::Sense::click(),
    );

    let visuals = ui.visuals().clone();
    let bg = if selected {
        visuals.selection.bg_fill
    } else if response.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };

    if bg != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(BUBBLE_CORNER_RADIUS), bg);
    }

    let mut job = egui::text::LayoutJob::default();
    job.append(
        &format!("{}:", short_id),
        0.0,
        egui::TextFormat::simple(
            egui::FontId::monospace(FONT_SIZE),
            visuals.strong_text_color(),
        ),
    );
    job.append(
        &format!(" {}", message),
        0.0,
        egui::TextFormat::simple(egui::FontId::proportional(FONT_SIZE), visuals.text_color()),
    );

    let galley = ui.painter().layout_job(job);
    let text_rect = rect.shrink2(egui::vec2(BUBBLE_HORIZONTAL_PADDING, 0.0));
    let text_pos = text_rect.left_center() - egui::vec2(0.0, galley.size().y / 2.0);
    ui.painter()
        .with_clip_rect(text_rect)
        .galley(text_pos, galley, egui::Color32::WHITE);

    response
}

pub fn show_commits(
    ui: &mut egui::Ui,
    repo: &Repository,
    history_start: Option<Oid>,
    selected_commit: &mut Option<Oid>,
    selected_file: &mut Option<(String, Oid)>,
    file_content: &mut Option<String>,
) {
    let history_start = match history_start {
        None => return,
        Some(oid) => oid,
    };

    let mut revwalk = repo.revwalk().expect("Failed to get revwalk");
    revwalk
        .push(history_start)
        .expect("Failed to push history start");
    revwalk
        .set_sorting(git2::Sort::NONE)
        .expect("Failed to set sorting");

    let commits: Vec<_> = revwalk
        .take(REVWALK_LIMIT)
        .filter_map(|oid| oid.ok())
        .filter_map(|oid| repo.find_commit(oid).ok())
        .map(|commit| {
            let message = commit
                .message()
                .unwrap_or("<no message>")
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            let short_id = commit.id().to_string()[..SHA_SHORT_LEN].to_string();
            let commit_id = commit.id();
            (commit_id, short_id, message)
        })
        .collect();

    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
        commits
            .into_iter()
            .for_each(|(commit_id, short_id, message)| {
                let commit_selected = *selected_commit == Some(commit_id);
                if show_commit_bubble(ui, commit_selected, &short_id, &message).clicked() {
                    *selected_commit = Some(commit_id);
                    *selected_file = None;
                    *file_content = None;
                }
            });
    });
}
