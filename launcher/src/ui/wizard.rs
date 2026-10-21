//! First-start setup wizard: where the Wine environment goes, then one row per step with
//! progress, Retry and Show log on failure.
use super::{Mode, StudioWineApp};
use crate::setup::{all_steps, run_steps, setup_steps, Progress, ProgressMsg, StepId};
use crate::state::State;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

#[derive(Clone, PartialEq)]
enum RowState {
    Pending,
    Running,
    Done,
    Failed(String),
}

struct Row {
    id: StepId,
    title: &'static str,
    state: RowState,
    fraction: f32,
    status: String,
}

pub struct Wizard {
    rows: Vec<Row>,
    rx: Option<Receiver<ProgressMsg>>,
    /// Where the Wine environment goes, as typed; applied when setup starts.
    location: String,
    location_error: Option<String>,
    browse: Option<Receiver<Option<PathBuf>>>,
    /// Unticked: setup leaves out the Steinberg Download Assistant; saved when setup starts.
    install_sda: bool,
}

impl Wizard {
    pub fn new(prefix: &Path, skip_sda: bool) -> Wizard {
        let rows = all_steps()
            .iter()
            .map(|s| Row { id: s.id(), title: s.title(), state: RowState::Pending, fraction: 0.0, status: String::new() })
            .collect();
        Wizard { rows, rx: None, location: prefix.to_string_lossy().into_owned(), location_error: None, browse: None, install_sda: !skip_sda }
    }

    /// Rows of steps whose check already passes start as done (resuming a setup).
    pub fn with_done(steps: &[(StepId, bool)], prefix: &Path, skip_sda: bool) -> Wizard {
        let mut w = Wizard::new(prefix, skip_sda);
        for r in &mut w.rows {
            if steps.iter().any(|(id, ok)| *id == r.id && *ok) {
                r.state = RowState::Done;
                r.fraction = 1.0;
                r.status = "Done".into();
            }
        }
        w
    }

    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    fn row(&mut self, id: StepId) -> Option<&mut Row> {
        self.rows.iter_mut().find(|r| r.id == id)
    }

    /// Applies progress messages; returns true when every step has finished.
    fn poll(&mut self) -> bool {
        let Some(rx) = self.rx.take() else { return false };
        let mut all_done = false;
        let mut stopped = false;
        while let Ok(m) = rx.try_recv() {
            match m {
                ProgressMsg::Started(id) => {
                    if let Some(r) = self.row(id) {
                        r.state = RowState::Running;
                        r.fraction = 0.05;
                    }
                }
                ProgressMsg::Status(id, s) => {
                    if let Some(r) = self.row(id) {
                        r.status = s;
                    }
                }
                ProgressMsg::Fraction(id, f) => {
                    if let Some(r) = self.row(id) {
                        r.fraction = f.clamp(0.0, 1.0);
                    }
                }
                ProgressMsg::Done(id) => {
                    if let Some(r) = self.row(id) {
                        r.state = RowState::Done;
                        r.fraction = 1.0;
                        r.status = "Done".into();
                    }
                }
                ProgressMsg::Failed(id, e) => {
                    if let Some(r) = self.row(id) {
                        r.state = RowState::Failed(e);
                    }
                    stopped = true;
                }
                ProgressMsg::AllDone => {
                    all_done = true;
                    stopped = true;
                }
            }
        }
        if !stopped {
            self.rx = Some(rx);
        }
        all_done
    }
}

/// Painted rather than taken from a font: egui's bundled fonts lack ○ ✔ ✖.
fn status_icon(ui: &mut egui::Ui, state: &RowState) {
    if *state == RowState::Running {
        ui.add(egui::Spinner::new().size(14.0));
        return;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    let (c, p) = (rect.center(), ui.painter());
    let green = egui::Color32::from_rgb(80, 190, 90);
    let white = egui::Stroke::new(2.0, egui::Color32::WHITE);
    match state {
        RowState::Pending | RowState::Running => {
            p.circle_stroke(c, 6.0, egui::Stroke::new(1.5, ui.visuals().weak_text_color()));
        }
        RowState::Done => {
            p.circle_filled(c, 7.0, green);
            p.line_segment([c + egui::vec2(-3.5, 0.0), c + egui::vec2(-1.0, 3.0)], white);
            p.line_segment([c + egui::vec2(-1.0, 3.0), c + egui::vec2(3.5, -3.0)], white);
        }
        RowState::Failed(_) => {
            p.circle_filled(c, 7.0, ui.visuals().error_fg_color);
            p.line_segment([c + egui::vec2(-3.0, -3.0), c + egui::vec2(3.0, 3.0)], white);
            p.line_segment([c + egui::vec2(3.0, -3.0), c + egui::vec2(-3.0, 3.0)], white);
        }
    }
}

impl StudioWineApp {
    fn start_setup(&mut self) {
        for r in &mut self.wizard.rows {
            if r.state != RowState::Done {
                r.state = RowState::Pending;
                r.status.clear();
                r.fraction = 0.0;
            }
        }
        let skip = !self.wizard.install_sda;
        let mut state = State::load(&self.paths.state_file);
        if state.skip_download_assistant != skip {
            state.skip_download_assistant = skip;
            if let Err(e) = state.save(&self.paths.state_file) {
                self.ctx.log.line(&format!("cannot save state: {e:#}"));
            }
            self.refresh_checks();
        }
        let (tx, rx) = channel();
        run_steps(setup_steps(skip), self.ctx.clone(), self.paths.state_file.clone(), Progress::new(tx));
        self.wizard.rx = Some(rx);
    }

    pub(super) fn restart_wizard(&mut self) {
        self.refresh_checks();
        self.wizard = Wizard::with_done(&self.steps_done, &self.paths.prefix, self.skip_download_assistant());
        self.mode = Mode::Wizard;
    }

    /// Switches to the location typed in the wizard, checked and created first. False, with
    /// the reason shown under the field, when it cannot be used.
    fn apply_location(&mut self) -> bool {
        let home = dirs::home_dir().unwrap_or_default();
        let chosen = match super::path_prompt::expand_user_path(&self.wizard.location, &home) {
            Ok(p) => p,
            Err(e) => {
                self.wizard.location_error = Some(e);
                return false;
            }
        };
        if chosen == self.paths.prefix {
            return true;
        }
        let checked = crate::setup::prefix::check_location(&chosen, &self.paths.default_prefix())
            .and_then(|()| crate::setup::prefix::prepare_location(&chosen));
        if let Err(e) = checked {
            self.wizard.location_error = Some(e);
            return false;
        }
        self.set_prefix(chosen);
        true
    }

    fn location_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(rx) = &self.wizard.browse {
            if let Ok(r) = rx.try_recv() {
                if let Some(p) = r {
                    self.wizard.location = p.to_string_lossy().into_owned();
                    self.wizard.location_error = None;
                }
                self.wizard.browse = None;
            }
        }
        let idle = !self.wizard.running() && self.job.is_none();
        let default = self.paths.default_prefix().to_string_lossy().into_owned();
        ui.label(egui::RichText::new("Wine environment location").strong());
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 150.0).max(200.0);
            let r = ui.add_enabled(idle, egui::TextEdit::singleline(&mut self.wizard.location).desired_width(w));
            if r.changed() {
                self.wizard.location_error = None;
            }
            if ui.add_enabled(idle && self.wizard.browse.is_none(), egui::Button::new("Browse…")).clicked() {
                let (tx, rx) = channel();
                std::thread::spawn(move || {
                    let _ = tx.send(rfd::FileDialog::new().pick_folder());
                });
                self.wizard.browse = Some(rx);
            }
            if ui.add_enabled(idle && self.wizard.location != default, egui::Button::new("Default")).clicked() {
                self.wizard.location = default.clone();
                self.wizard.location_error = None;
            }
        });
        match &self.wizard.location_error {
            Some(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            None => {
                ui.weak("A new or empty folder on a Linux file system with about 3 GB free (plus your Steinberg software). A folder with an earlier StudioWine environment is reused.");
            }
        }
    }

    pub(super) fn wizard_ui(&mut self, ui: &mut egui::Ui) {
        if self.wizard.poll() {
            self.refresh_checks();
            self.rewrite_handlers();
            self.claim_handlers();
            self.mode = super::mode_for(&State::load(&self.paths.state_file), &self.steps_done);
            return;
        }
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            ui.heading("Set up StudioWine");
            ui.label("StudioWine prepares a Windows environment for Steinberg software and plug-ins. This runs once and needs an internet connection.");
            ui.add_space(12.0);
            self.location_ui(ui);
            ui.add_space(12.0);
            let mut failed = false;
            let idle = !self.wizard.running() && self.job.is_none();
            // The rows scroll; the buttons below stay visible in a small window.
            let rows_height = (ui.available_height() - 64.0).max(80.0);
            egui::ScrollArea::vertical().max_height(rows_height).auto_shrink([false, true]).show(ui, |ui| {
                let install_sda = &mut self.wizard.install_sda;
                for r in &self.wizard.rows {
                    let optional = r.id == StepId::DownloadAssistant && r.state != RowState::Done;
                    let skipped = optional && !*install_sda;
                    ui.horizontal(|ui| {
                        status_icon(ui, &r.state);
                        let title = egui::RichText::new(r.title).strong();
                        ui.label(if skipped { title.weak().strikethrough() } else { title });
                    });
                    if optional {
                        ui.add_enabled(idle, egui::Checkbox::new(install_sda, "Install it (needed to download Cubase and other Steinberg software; you can add it later from Settings)"));
                    }
                    if !skipped {
                        ui.add(egui::ProgressBar::new(r.fraction).desired_height(6.0));
                    }
                    match &r.state {
                        RowState::Failed(e) => {
                            failed = true;
                            ui.colored_label(ui.visuals().error_fg_color, e);
                        }
                        _ if !r.status.is_empty() => {
                            ui.weak(&r.status);
                        }
                        _ => {}
                    }
                    ui.add_space(6.0);
                }
            });
            ui.add_space(8.0);
            if let Some((m, true)) = &self.message {
                ui.colored_label(ui.visuals().error_fg_color, m);
            }
            ui.horizontal(|ui| {
                if let Some(job) = &self.job {
                    // e.g. a reset still deleting the old environment
                    ui.spinner();
                    ui.label(if job.status.is_empty() { job.label.clone() } else { format!("{}: {}", job.label, job.status) });
                } else if self.wizard.running() {
                    ui.spinner();
                    ui.label("Working…");
                } else if failed {
                    if ui.button("Retry").clicked() && self.apply_location() {
                        self.start_setup();
                    }
                    if ui.button("Show log").clicked() {
                        let log = self.ctx.log.path().to_path_buf();
                        self.open_path(&log);
                    }
                } else if ui.button("Start setup").clicked() && self.apply_location() {
                    self.start_setup();
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_steps_start_as_done() {
        let w = Wizard::with_done(&[(StepId::Prefix, true), (StepId::Fonts, false)], Path::new("/d/prefix"), false);
        assert_eq!(w.location, "/d/prefix");
        assert!(w.install_sda);
        assert!(!Wizard::new(Path::new("/d/prefix"), true).install_sda);
        assert!(w.rows.iter().find(|r| r.id == StepId::Prefix).unwrap().state == RowState::Done);
        assert!(w.rows.iter().find(|r| r.id == StepId::Fonts).unwrap().state == RowState::Pending);
    }
}
