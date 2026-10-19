//! First-start setup wizard: five rows with progress, Retry and Show log on failure.
use super::{Mode, StudioWineApp};
use crate::setup::{all_steps, run_steps, Progress, ProgressMsg, StepId};
use crate::state::State;
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
}

impl Wizard {
    pub fn new() -> Wizard {
        let rows = all_steps()
            .iter()
            .map(|s| Row { id: s.id(), title: s.title(), state: RowState::Pending, fraction: 0.0, status: String::new() })
            .collect();
        Wizard { rows, rx: None }
    }

    /// Rows of steps whose check already passes start as done (resuming a setup).
    pub fn with_done(steps: &[(StepId, bool)]) -> Wizard {
        let mut w = Wizard::new();
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
        let (tx, rx) = channel();
        run_steps(all_steps(), self.ctx.clone(), self.paths.state_file.clone(), Progress::new(tx));
        self.wizard.rx = Some(rx);
    }

    pub(super) fn restart_wizard(&mut self) {
        self.refresh_checks();
        self.wizard = Wizard::with_done(&self.steps_done);
        self.mode = Mode::Wizard;
    }

    pub(super) fn wizard_ui(&mut self, ui: &mut egui::Ui) {
        if self.wizard.poll() {
            self.refresh_checks();
            self.rewrite_handlers();
            self.mode = super::mode_for(&State::load(&self.paths.state_file), &self.steps_done);
            return;
        }
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            ui.heading("Set up StudioWine");
            ui.label("StudioWine prepares a Windows environment for Steinberg software and plug-ins. This runs once and needs an internet connection.");
            ui.add_space(12.0);
            let mut failed = false;
            for r in &self.wizard.rows {
                ui.horizontal(|ui| {
                    status_icon(ui, &r.state);
                    ui.label(egui::RichText::new(r.title).strong());
                });
                ui.add(egui::ProgressBar::new(r.fraction).desired_height(6.0));
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
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if self.wizard.running() {
                    ui.spinner();
                    ui.label("Working…");
                } else if failed {
                    if ui.button("Retry").clicked() {
                        self.start_setup();
                    }
                    if ui.button("Show log").clicked() {
                        let log = self.ctx.log.path().to_path_buf();
                        self.open_path(&log);
                    }
                } else if ui.button("Start setup").clicked() {
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
        let w = Wizard::with_done(&[(StepId::Prefix, true), (StepId::Fonts, false)]);
        assert!(w.rows.iter().find(|r| r.id == StepId::Prefix).unwrap().state == RowState::Done);
        assert!(w.rows.iter().find(|r| r.id == StepId::Fonts).unwrap().state == RowState::Pending);
    }
}
