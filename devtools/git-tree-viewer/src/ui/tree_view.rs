use crate::constants::{FONT_SIZE, SHA_SHORT_LEN};
use crate::git::TreeItem;
use egui::Color32;

pub fn tree_entry_label(
    icon: &str,
    name: &str,
    oid: git2::Oid,
    text_color: Color32,
    sha_color: Color32,
) -> egui::text::LayoutJob {
    let full_id = oid.to_string();
    let short_id = &full_id[..full_id.len().min(SHA_SHORT_LEN)];
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &format!("{} {} (", icon, name),
        0.0,
        egui::TextFormat::simple(egui::FontId::proportional(FONT_SIZE), text_color),
    );
    job.append(
        short_id,
        0.0,
        egui::TextFormat::simple(egui::FontId::monospace(FONT_SIZE), sha_color),
    );
    job.append(
        ")",
        0.0,
        egui::TextFormat::simple(egui::FontId::proportional(FONT_SIZE), text_color),
    );
    job
}

pub fn show_tree_item(
    ui: &mut egui::Ui,
    item: &TreeItem,
    selected_oid: Option<git2::Oid>,
    on_file_clicked: &mut dyn FnMut(String, git2::Oid),
) {
    let text_color = ui.visuals().text_color();
    let sha_color = ui.visuals().strong_text_color();

    match item {
        TreeItem::Directory {
            name,
            oid,
            children,
            ..
        } => {
            let label = tree_entry_label("📁", name, *oid, text_color, sha_color);
            ui.collapsing(label, |ui| {
                for child in children {
                    show_tree_item(ui, child, selected_oid, on_file_clicked);
                }
            });
        }
        TreeItem::File {
            name,
            full_path,
            oid,
        } => {
            let is_selected = selected_oid.map(|s| s == *oid).unwrap_or(false);

            let label = tree_entry_label("📄", name, *oid, text_color, sha_color);
            if ui.selectable_label(is_selected, label).clicked() {
                on_file_clicked(full_path.clone(), *oid);
            }
        }
        TreeItem::Other { name, oid, .. } => {
            let label = tree_entry_label("❓", name, *oid, text_color, sha_color);
            ui.label(label);
        }
    }
}
