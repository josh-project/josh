/// Mode chosen in the startup dialog (or via the `--mode` CLI flag).
#[derive(Clone, Copy)]
pub enum Mode {
    Browse,
    Trace,
}

impl std::str::FromStr for Mode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "browse" => Ok(Mode::Browse),
            "trace" => Ok(Mode::Trace),
            other => Err(format!(
                "invalid mode '{other}' (expected 'browse' or 'trace')"
            )),
        }
    }
}

/// Opens a small window with "Browse" and "Trace" buttons and returns the
/// chosen mode, or `None` if the window is closed without a selection.
pub fn select_mode() -> Option<Mode> {
    let (tx, rx) = std::sync::mpsc::channel();

    struct ModeDialog {
        tx: std::sync::mpsc::Sender<Option<Mode>>,
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
                            self.tx.send(Some(Mode::Browse)).ok();
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        ui.add_space(12.0);
                        if ui
                            .add_sized([120.0, 40.0], egui::Button::new("Trace"))
                            .clicked()
                        {
                            self.tx.send(Some(Mode::Trace)).ok();
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
        Box::new(move |_cc| Ok(Box::new(ModeDialog { tx }))),
    )
    .ok();

    rx.recv().unwrap_or_default()
}
