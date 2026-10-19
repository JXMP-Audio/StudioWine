//! The StudioWine window (layout C): a one-time setup wizard, then the shortcut grid.
pub mod fonts_dialog;
pub mod grid;
pub mod path_prompt;
pub mod settings;
pub mod wizard;

use crate::log::LogSink;
use crate::paths::Paths;
use crate::setup::{all_steps, Ctx, StepId};
use crate::state::State;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Wizard,
    Main,
}

/// The wizard is shown until every step has completed once; afterwards the main view,
/// even if a later check fails (the status dot reports that).
pub fn mode_for(state: &State, steps: &[(StepId, bool)]) -> Mode {
    if steps.iter().all(|(id, _)| state.completed.contains(id)) {
        Mode::Main
    } else {
        Mode::Wizard
    }
}

pub fn run_gui(paths: Paths) -> anyhow::Result<()> {
    let log = LogSink::open(&paths.logs, "studiowine.log")?;
    let app = StudioWineApp::new(paths, log);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("StudioWine")
            .with_app_id("studiowine")
            .with_inner_size([760.0, 520.0])
            .with_min_inner_size([520.0, 380.0]),
        ..Default::default()
    };
    eframe::run_native("StudioWine", options, Box::new(|_cc| Ok(Box::new(app))))
        .map_err(|e| anyhow::anyhow!("cannot open the window: {e}"))
}

pub(super) enum JobMsg {
    Status(String),
    Done(Result<String, String>),
}

/// One background operation at a time (installer, font import, reset…).
pub(super) struct Job {
    pub label: String,
    pub status: String,
    pub rx: Receiver<JobMsg>,
}

pub struct StudioWineApp {
    pub(super) paths: Paths,
    pub(super) ctx: Arc<Ctx>,
    pub(super) mode: Mode,
    pub(super) steps_done: Vec<(StepId, bool)>,
    pub(super) wizard: wizard::Wizard,
    pub(super) grid: grid::Grid,
    pub(super) fonts: fonts_dialog::FontsDialog,
    pub(super) settings: settings::Settings,
    pub(super) job: Option<Job>,
    pub(super) prompt: Option<path_prompt::Prompt>,
    /// Last result line shown in the bottom bar: (text, is_error).
    pub(super) message: Option<(String, bool)>,
}

impl StudioWineApp {
    fn new(paths: Paths, log: LogSink) -> StudioWineApp {
        let ctx = Arc::new(Ctx::new(paths.clone(), log));
        let mut app = StudioWineApp {
            paths,
            ctx,
            mode: Mode::Wizard,
            steps_done: Vec::new(),
            wizard: wizard::Wizard::new(),
            grid: grid::Grid::new(),
            fonts: fonts_dialog::FontsDialog::default(),
            settings: settings::Settings::default(),
            job: None,
            prompt: None,
            message: None,
        };
        app.rewrite_handlers();
        app.refresh_checks();
        app.wizard = wizard::Wizard::with_done(&app.steps_done);
        app.mode = mode_for(&State::load(&app.paths.state_file), &app.steps_done);
        app
    }

    pub(super) fn refresh_checks(&mut self) {
        self.steps_done = all_steps().iter().map(|s| (s.id(), s.is_done(&self.ctx))).collect();
    }

    /// Re-point Wine's menu entries and URL handlers at this AppImage (it may have moved).
    pub(super) fn rewrite_handlers(&self) {
        match crate::handlers::rewrite_all(&self.paths.prefix, &self.paths.appimage) {
            Ok(n) if n > 0 => self.ctx.log.line(&format!("{n} handler files rewritten")),
            Ok(_) => {}
            Err(e) => self.ctx.log.line(&format!("cannot rewrite handlers: {e:#}")),
        }
    }

    /// Runs `f` on a worker thread; `f` reports progress through its argument.
    pub(super) fn spawn_job(
        &mut self,
        label: &str,
        f: impl FnOnce(&dyn Fn(String)) -> anyhow::Result<String> + Send + 'static,
    ) {
        let (tx, rx) = channel();
        let status_tx = tx.clone();
        std::thread::spawn(move || {
            let report = move |s: String| {
                let _ = status_tx.send(JobMsg::Status(s));
            };
            let r = f(&report).map_err(|e| format!("{e:#}"));
            let _ = tx.send(JobMsg::Done(r));
        });
        self.job = Some(Job { label: label.to_string(), status: String::new(), rx });
        self.message = None;
    }

    fn poll_job(&mut self) {
        let Some(job) = &mut self.job else { return };
        let mut finished = None;
        while let Ok(m) = job.rx.try_recv() {
            match m {
                JobMsg::Status(s) => job.status = s,
                JobMsg::Done(r) => finished = Some(r),
            }
        }
        if let Some(r) = finished {
            let label = job.label.clone();
            self.job = None;
            self.message = match r {
                Ok(s) if s.is_empty() => None,
                Ok(s) => Some((s, false)),
                Err(e) => {
                    self.ctx.log.line(&format!("{label} failed: {e}"));
                    Some((format!("{label} failed: {e}"), true))
                }
            };
            self.rewrite_handlers();
            self.refresh_checks();
            self.grid.invalidate();
            self.fonts.invalidate();
        }
    }

    pub(super) fn open_path(&mut self, p: &std::path::Path) {
        if let Err(e) = std::process::Command::new("xdg-open").arg(p).spawn() {
            self.message = Some((format!("cannot open {}: {e}", p.display()), true));
        }
    }
}

impl eframe::App for StudioWineApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_job();
        if self.job.is_some() || self.wizard.running() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
        match self.mode {
            Mode::Wizard => self.wizard_ui(ui),
            Mode::Main => self.main_ui(ui),
        }
        self.fonts_window(ui.ctx());
        if self.mode == Mode::Main && self.job.is_none() {
            self.handle_dropped_files(ui.ctx());
            self.prompt_window(ui.ctx());
        }
        self.reset_modal(ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [StepId; 5] = [StepId::Prefix, StepId::Fonts, StepId::WebView2, StepId::Shim, StepId::DownloadAssistant];

    #[test]
    fn wizard_until_all_steps_completed_once() {
        let steps: Vec<_> = ALL.iter().map(|s| (*s, true)).collect();
        assert_eq!(mode_for(&State::default(), &steps), Mode::Wizard);
        let mut st = State { completed: ALL[..4].to_vec(), ..Default::default() };
        assert_eq!(mode_for(&st, &steps), Mode::Wizard);
        st.completed = ALL.to_vec();
        assert_eq!(mode_for(&st, &steps), Mode::Main);
        let broken: Vec<_> = ALL.iter().map(|s| (*s, *s != StepId::WebView2)).collect();
        assert_eq!(mode_for(&st, &broken), Mode::Main, "a later failing check does not bring the wizard back");
    }
}
