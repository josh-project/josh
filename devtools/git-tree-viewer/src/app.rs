use crate::ui::panels::show_panels;
use crate::{AppMode, GitDebugApp};

impl eframe::App for GitDebugApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if let AppMode::Trace { rx, traces } = &mut self.mode {
            while let Ok(trace) = rx.try_recv() {
                traces.push(trace);
            }
        }

        show_panels(ui, self);
    }
}
