use crate::ui::panels::show_panels;
use crate::{AppMode, GitDebugApp};
use crate::server;

impl eframe::App for GitDebugApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        if self.mode == AppMode::Trace {
            if !self.server_started {
                if let Some(tx) = self.tx.take() {
                    server::start(tx);
                    self.server_started = true;
                }
            }
            if let Some(rx) = &self.rx {
                while let Ok(trace) = rx.try_recv() {
                    self.traces.push(trace);
                }
            }
        }

        show_panels(ui, self);
    }
}
