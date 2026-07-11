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

struct ModeDialog {
    tx: std::sync::mpsc::Sender<Option<Mode>>,
}

const BUTTON_SIZE: [f32; 2] = [120.0, 40.0];
const WINDOW_SIZE: [f32; 2] = [265.0, 120.0];

impl eframe::App for ModeDialog {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Git Tree Viewer");
                ui.separator();
                ui.label("Select a mode:");

                ui.horizontal(|ui| {
                    if ui
                        .add_sized(BUTTON_SIZE, egui::Button::new("Browse"))
                        .clicked()
                    {
                        self.tx.send(Some(Mode::Browse)).ok();
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }

                    if ui
                        .add_sized(BUTTON_SIZE, egui::Button::new("Trace"))
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

/// Opens a small window with "Browse" and "Trace" buttons and returns the
/// chosen mode, or `None` if the window is closed without a selection.
pub fn select_mode() -> Option<Mode> {
    let (tx, rx) = std::sync::mpsc::channel();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(WINDOW_SIZE)
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
