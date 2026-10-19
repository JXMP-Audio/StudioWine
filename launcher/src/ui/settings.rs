//! ⚙ menu: open prefix folder, show log, re-run setup, reset prefix (with confirmation).
use super::StudioWineApp;

#[derive(Default)]
pub struct Settings {
    confirm_reset: bool,
}

impl StudioWineApp {
    pub(super) fn settings_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Settings", |ui| {
            if ui.button("Open Wine environment folder").clicked() {
                let p = self.paths.prefix.clone();
                self.open_path(&p);
            }
            if ui.button("Show log").clicked() {
                let p = self.ctx.log.path().to_path_buf();
                self.open_path(&p);
            }
            if ui.add_enabled(self.job.is_none(), egui::Button::new("Re-run setup")).clicked() {
                self.restart_wizard();
            }
            ui.separator();
            if ui.add_enabled(self.job.is_none(), egui::Button::new("Reset Wine environment…")).clicked() {
                self.settings.confirm_reset = true;
            }
        });
    }

    pub(super) fn reset_modal(&mut self, ctx: &egui::Context) {
        if !self.settings.confirm_reset {
            return;
        }
        let mut close = false;
        let mut reset = false;
        egui::Modal::new(egui::Id::unique("studiowine-reset-modal")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading("Delete the Wine environment?");
            ui.label("All Windows programs installed in StudioWine and their settings are deleted.");
            ui.label(egui::RichText::new("Deactivate your Steinberg and other licences first — a new Wine environment counts as a new computer.").strong());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Delete and start over").clicked() {
                    reset = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if reset {
            let ctx2 = self.ctx.clone();
            let state_file = self.paths.state_file.clone();
            self.spawn_job("Resetting", move |_| {
                ctx2.wine.kill_server()?;
                if ctx2.paths.prefix.exists() {
                    std::fs::remove_dir_all(&ctx2.paths.prefix)?;
                }
                let _ = std::fs::remove_file(&state_file);
                Ok("The Wine environment was deleted.".into())
            });
            self.restart_wizard();
        }
        if reset || close {
            self.settings.confirm_reset = false;
        }
    }
}
