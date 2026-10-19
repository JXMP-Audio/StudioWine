//! Fonts… dialog: which families are real, substitutes or missing; import licensed fonts.
use super::StudioWineApp;
use crate::fonts::{self, FamilyStatus, FontStatus};

#[derive(Default)]
pub struct FontsDialog {
    pub open: bool,
    status: Option<Vec<FamilyStatus>>,
}

impl FontsDialog {
    pub fn invalidate(&mut self) {
        self.status = None;
    }
}

impl StudioWineApp {
    pub(super) fn fonts_window(&mut self, ctx: &egui::Context) {
        if !self.fonts.open {
            return;
        }
        if self.fonts.status.is_none() {
            self.fonts.status = fonts::family_status(&self.paths).ok();
        }
        let mut open = self.fonts.open;
        let mut import = false;
        egui::Window::new("Fonts").open(&mut open).resizable(false).collapsible(false).show(ctx, |ui| {
            ui.label("Windows programs ask for these fonts by name. StudioWine ships free look-alikes; you can replace them with the genuine fonts.");
            ui.add_space(6.0);
            egui::Grid::new("font_status").striped(true).show(ui, |ui| {
                for f in self.fonts.status.iter().flatten() {
                    ui.label(f.family);
                    match f.status {
                        FontStatus::Real => ui.colored_label(egui::Color32::from_rgb(80, 190, 90), "genuine"),
                        FontStatus::Substitute => ui.label("free substitute"),
                        FontStatus::Missing => ui.colored_label(ui.visuals().error_fg_color, "missing"),
                    };
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.weak("Only import fonts you hold a licence for, e.g. from the Fonts folder of your own Windows installation (C:\\Windows\\Fonts).");
            if ui.add_enabled(self.job.is_none(), egui::Button::new("Use fonts from folder…")).clicked() {
                import = true;
            }
        });
        self.fonts.open = open;
        if import {
            self.import_fonts();
        }
    }

    fn import_fonts(&mut self) {
        let ctx = self.ctx.clone();
        self.spawn_job("Importing fonts", move |report| {
            let Some(dir) = rfd::FileDialog::new().set_title("Folder with Windows fonts").pick_folder() else {
                return Ok(String::new());
            };
            let r = fonts::import_user_fonts(&dir, &ctx.paths)?;
            if r.copied.is_empty() {
                anyhow::bail!("no Windows font files (segoeui.ttf, micross.ttf, arial.ttf, …) found in {}", dir.display());
            }
            report("registering fonts".into());
            fonts::registry_set(&ctx.paths, fonts::prefix_dpi(&ctx.paths))?.import(&ctx.wine, &ctx.log, &ctx.paths.cache)?;
            ctx.wine.kill_server()?;
            Ok(format!("Copied: {}", r.copied.join(", ")))
        });
    }
}
