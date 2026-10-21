//! ⚙ menu: open prefix folder, show log, WineASIO status and buffer size, install a skipped
//! Download Assistant, re-run setup, reset prefix (with confirmation).
use super::StudioWineApp;
use crate::audio::{buffer_settings, current_buffer_size, Jack, BUFFER_SIZES};
use crate::setup::StepId;

#[derive(Default)]
pub struct Settings {
    confirm_reset: bool,
    /// JACK status line and whether WineASIO can work with it.
    pub jack: (&'static str, bool),
    buffer_size: Option<u32>,
    /// Last size set from the menu; user.reg shows it only once wineserver saves the
    /// registry, and stopping wineserver here would close Cubase.
    buffer_size_set: Option<u32>,
}

impl Settings {
    /// Re-read after every job; the menu itself is drawn every frame while open.
    pub fn refresh(&mut self, prefix: &std::path::Path) {
        self.jack = Jack::detect().summary();
        self.buffer_size = current_buffer_size(prefix);
    }
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
            ui.separator();
            let (jack, ok) = self.settings.jack;
            if ok {
                ui.weak(jack);
            } else {
                ui.colored_label(ui.visuals().warn_fg_color, jack);
            }
            let asio = self.steps_done.contains(&(StepId::WineAsio, true));
            ui.add_enabled_ui(asio && self.job.is_none(), |ui| {
                ui.menu_button("WineASIO buffer size", |ui| {
                    for n in BUFFER_SIZES {
                        let label = format!("{n} samples ({:.1} ms at 48 kHz)", n as f32 / 48.0);
                        let current = self.settings.buffer_size_set.or(self.settings.buffer_size);
                        if ui.radio(current == Some(n), label).clicked() {
                            self.set_buffer_size(n);
                        }
                    }
                    ui.weak("Applies when Cubase restarts its audio engine.");
                });
            });
            ui.separator();
            if self.grid.sda_skipped && ui.add_enabled(self.job.is_none(), egui::Button::new("Install Steinberg Download Assistant")).clicked() {
                self.install_sda();
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

    fn set_buffer_size(&mut self, frames: u32) {
        self.settings.buffer_size_set = Some(frames);
        let ctx = self.ctx.clone();
        self.spawn_job("Setting the WineASIO buffer size", move |_| {
            buffer_settings(frames).import(&ctx.wine, &ctx.log, &ctx.paths.cache)?;
            Ok(format!("WineASIO buffer size: {frames} samples"))
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
                // Nothing is completed any more; the chosen location and the Download Assistant choice stay.
                let old = crate::state::State::load(&state_file);
                let keep = crate::state::State { prefix: old.prefix, skip_download_assistant: old.skip_download_assistant, ..Default::default() };
                keep.save(&state_file)?;
                Ok("The Wine environment was deleted.".into())
            });
            // Fresh rows: the deletion is still running, so the checks would see the old prefix.
            self.wizard = super::wizard::Wizard::new(&self.paths.prefix, self.skip_download_assistant());
            self.mode = super::Mode::Wizard;
        }
        if reset || close {
            self.settings.confirm_reset = false;
        }
    }
}
