pub fn show_file_preview(
    ui: &mut egui::Ui,
    selected_file: &Option<(String, git2::Oid)>,
    file_content: &Option<String>,
) {
    ui.heading("File Preview");

    egui::ScrollArea::both()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            if let Some((filename, _)) = selected_file {
                ui.separator();
                ui.label(format!("File: {}", filename));
                ui.separator();

                if let Some(content) = file_content {
                    ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(content).monospace())
                                .wrap_mode(egui::TextWrapMode::Extend),
                        );
                    });
                }
            } else {
                ui.label("Select a file to preview");
            }
        });
}
