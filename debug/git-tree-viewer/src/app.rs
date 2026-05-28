use crate::ui::panels::show_panels;
use crate::{AppMode, GitDebugApp};

impl eframe::App for GitDebugApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if self.mode == AppMode::Trace {
            if let Some(rx) = &self.rx {
                while let Ok(trace) = rx.try_recv() {
                    self.traces.push(trace);
                }
            }
        }

        show_panels(ui, self);
    }
}
