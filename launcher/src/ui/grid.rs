//! Main view: shortcut grid, Download Assistant card, bottom bar with the status dot.
use super::StudioWineApp;
use crate::setup::{step_title, StepId};
use crate::shortcuts::{self, Shortcut};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

const ICON: u32 = 64;
type Icon = Option<(u32, u32, Vec<u8>)>;

pub struct Grid {
    shortcuts: Vec<Shortcut>,
    fingerprint: Option<u64>,
    last_poll: Instant,
    sda_installed: bool,
    textures: HashMap<PathBuf, Option<egui::TextureHandle>>,
    icon_tx: Sender<(PathBuf, Icon)>,
    icon_rx: Receiver<(PathBuf, Icon)>,
}

impl Grid {
    pub fn new() -> Grid {
        let (icon_tx, icon_rx) = channel();
        Grid {
            shortcuts: Vec::new(),
            fingerprint: None,
            last_poll: Instant::now() - Duration::from_secs(60),
            sda_installed: false,
            textures: HashMap::new(),
            icon_tx,
            icon_rx,
        }
    }

    /// Forces a rescan on the next frame.
    pub fn invalidate(&mut self) {
        self.fingerprint = None;
        self.last_poll = Instant::now() - Duration::from_secs(60);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dot {
    Green,
    Amber,
}

pub fn status_dot(steps: &[(StepId, bool)]) -> (Dot, String) {
    let missing: Vec<&str> = steps.iter().filter(|(_, ok)| !ok).map(|(id, _)| step_title(*id)).collect();
    if missing.is_empty() {
        (Dot::Green, "Ready".into())
    } else {
        (Dot::Amber, format!("Missing: {}", missing.join(", ")))
    }
}

pub fn needs_sda_card(_shortcuts: &[Shortcut], sda_installed: bool) -> bool {
    !sda_installed
}

impl StudioWineApp {
    fn poll_shortcuts(&mut self) {
        if self.grid.last_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.grid.last_poll = Instant::now();
        let fp = shortcuts::dir_fingerprint(&self.paths.prefix);
        if self.grid.fingerprint != Some(fp) {
            self.grid.fingerprint = Some(fp);
            self.grid.shortcuts = shortcuts::scan(&self.paths.prefix);
            self.grid.sda_installed = crate::setup::sda::find_sda_exe(&self.paths.prefix).is_some();
        }
        // Wine writes URL handlers whenever a program registers one (e.g. the Activation
        // Manager while the Download Assistant runs); point them at the AppImage right away.
        self.rewrite_handlers();
    }

    fn texture_for(&mut self, ctx: &egui::Context, s: &Shortcut) -> Option<egui::TextureHandle> {
        while let Ok((lnk, icon)) = self.grid.icon_rx.try_recv() {
            let tex = icon.map(|(w, h, px)| {
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
                ctx.load_texture(lnk.to_string_lossy(), img, egui::TextureOptions::LINEAR)
            });
            self.grid.textures.insert(lnk, tex);
        }
        match self.grid.textures.get(&s.lnk) {
            Some(t) => t.clone(),
            None => {
                self.grid.textures.insert(s.lnk.clone(), None);
                if let Some(target) = s.target.clone() {
                    let (tx, lnk, ctx) = (self.grid.icon_tx.clone(), s.lnk.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let _ = tx.send((lnk, crate::icons::icon_rgba(&target, ICON)));
                        ctx.request_repaint();
                    });
                }
                None
            }
        }
    }

    pub(super) fn main_ui(&mut self, ui: &mut egui::Ui) {
        self.poll_shortcuts();
        let busy = self.job.is_some();
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("StudioWine");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.settings_menu(ui));
            });
        });
        egui::Panel::bottom("bottom").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Run .exe…")).on_hover_text("Run a Windows installer or program (.exe, .msi) in StudioWine").clicked() {
                    self.prompt = Some(super::path_prompt::Prompt::new(super::path_prompt::Purpose::RunProgram, String::new()));
                }
                if ui.button("Fonts…").clicked() {
                    self.fonts.open = true;
                }
                if let Some(job) = &self.job {
                    ui.spinner();
                    ui.label(if job.status.is_empty() { job.label.clone() } else { format!("{}: {}", job.label, job.status) });
                } else if let Some((m, err)) = &self.message {
                    if *err {
                        ui.colored_label(ui.visuals().error_fg_color, m);
                    } else {
                        ui.label(m);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (dot, tip) = status_dot(&self.steps_done);
                    let color = match dot {
                        Dot::Green => egui::Color32::from_rgb(80, 190, 90),
                        Dot::Amber => egui::Color32::from_rgb(230, 170, 50),
                    };
                    ui.label(if dot == Dot::Green { "ready" } else { "check setup" }).on_hover_text(&tip);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 6.0, color);
                    resp.on_hover_text(tip);
                });
            });
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if needs_sda_card(&self.grid.shortcuts, self.grid.sda_installed) {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Steinberg Download Assistant is not installed.");
                            if ui.add_enabled(!busy, egui::Button::new("Install")).clicked() {
                                self.install_sda();
                            }
                        });
                    });
                    ui.add_space(8.0);
                }
                if self.grid.shortcuts.is_empty() {
                    ui.weak("No shortcuts yet. Programs you install appear here.");
                }
                let shortcuts = self.grid.shortcuts.clone();
                egui::Grid::new("shortcuts").spacing([12.0, 12.0]).show(ui, |ui| {
                    for (i, s) in shortcuts.iter().enumerate() {
                        let tex = self.texture_for(ui.ctx(), s);
                        let clicked = ui
                            .allocate_ui(egui::vec2(160.0, 110.0), |ui| {
                                ui.vertical_centered(|ui| {
                                    let r = match &tex {
                                        Some(t) => ui.add(egui::Button::image(egui::Image::new(t).fit_to_exact_size(egui::vec2(64.0, 64.0)))),
                                        None => {
                                            // Placeholder until the icon thread delivers (or for targets without one).
                                            let r = ui.add(egui::Button::new("").min_size(egui::vec2(72.0, 72.0)));
                                            let c = ui.visuals().weak_text_color();
                                            ui.painter().rect_stroke(r.rect.shrink(16.0), 6.0, egui::Stroke::new(2.0, c), egui::StrokeKind::Inside);
                                            r
                                        }
                                    };
                                    ui.add(egui::Label::new(&s.name).wrap());
                                    r.clicked()
                                })
                                .inner
                            })
                            .inner;
                        if clicked {
                            self.launch(s);
                        }
                        if i % 4 == 3 {
                            ui.end_row();
                        }
                    }
                });
            });
        });
    }

    fn launch(&mut self, s: &Shortcut) {
        let _ = shortcuts::launch(&self.ctx.wine, s, &self.ctx.log);
        self.message = Some((format!("Starting {}…", s.name), false));
    }

    pub(super) fn run_program(&mut self, file: PathBuf) {
        let ctx = self.ctx.clone();
        self.spawn_job("Running program", move |report| {
            let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
            report(name.clone());
            let is_msi = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("msi"));
            let st = if is_msi {
                ctx.wine.run_wait(["msiexec".as_ref(), "/i".as_ref(), file.as_os_str()], &ctx.log)?
            } else {
                ctx.wine.run_wait([file.as_os_str()], &ctx.log)?
            };
            Ok(format!("{name} finished ({st})"))
        });
    }

    fn install_sda(&mut self) {
        let ctx = self.ctx.clone();
        let state_file = self.paths.state_file.clone();
        self.spawn_job("Installing Steinberg Download Assistant", move |report| {
            let (tx, rx) = channel();
            let h = crate::setup::run_steps(vec![Box::new(crate::setup::sda::SdaStep)], ctx, state_file, crate::setup::Progress::new(tx));
            let mut result = Ok("Steinberg Download Assistant installed".to_string());
            for m in rx {
                match m {
                    crate::setup::ProgressMsg::Status(_, s) => report(s),
                    crate::setup::ProgressMsg::Failed(_, e) => result = Err(anyhow::anyhow!(e)),
                    _ => {}
                }
            }
            let _ = h.join();
            result
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn green_when_everything_verified() {
        assert_eq!(status_dot(&[(StepId::Prefix, true), (StepId::Fonts, true)]), (Dot::Green, "Ready".to_string()));
    }

    #[test]
    fn amber_names_missing_steps() {
        let (dot, tip) = status_dot(&[(StepId::Prefix, true), (StepId::Fonts, false), (StepId::WebView2, false)]);
        assert_eq!(dot, Dot::Amber);
        assert_eq!(tip, "Missing: Install fonts, Install Microsoft WebView2 runtime");
    }

    #[test]
    fn sda_card_only_without_download_assistant() {
        assert!(needs_sda_card(&[], false));
        assert!(!needs_sda_card(&[], true));
    }
}
