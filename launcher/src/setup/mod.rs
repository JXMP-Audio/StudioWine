//! Setup wizard steps and the runner that executes them.
pub mod fonts_step;
pub mod prefix;
pub mod sda;
pub mod shim;
pub mod webview2;

use crate::download::Http;
use crate::log::LogSink;
use crate::paths::Paths;
use crate::state::State;
use crate::wine::Wine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum StepId {
    Prefix,
    Fonts,
    WebView2,
    Shim,
    DownloadAssistant,
}

pub struct Ctx {
    pub paths: Paths,
    pub wine: Wine,
    pub http: Http,
    pub log: LogSink,
}

impl Ctx {
    pub fn new(paths: Paths, log: LogSink) -> Ctx {
        Ctx { wine: Wine::new(paths.clone()), paths, http: Http::new(), log }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProgressMsg {
    Started(StepId),
    Status(StepId, String),
    Fraction(StepId, f32),
    Done(StepId),
    Failed(StepId, String),
    AllDone,
}

#[derive(Clone)]
pub struct Progress {
    tx: Sender<ProgressMsg>,
}

impl Progress {
    pub fn new(tx: Sender<ProgressMsg>) -> Progress {
        Progress { tx }
    }
    pub fn send(&self, m: ProgressMsg) {
        let _ = self.tx.send(m);
    }
    pub fn status(&self, id: StepId, s: impl Into<String>) {
        self.send(ProgressMsg::Status(id, s.into()));
    }
    pub fn fraction(&self, id: StepId, f: f32) {
        self.send(ProgressMsg::Fraction(id, f));
    }
}

pub trait Step: Send + Sync {
    fn id(&self) -> StepId;
    fn title(&self) -> &'static str;
    /// Verification on disk; also drives the status dot.
    fn is_done(&self, ctx: &Ctx) -> bool;
    fn run(&self, ctx: &Ctx, p: &Progress) -> anyhow::Result<()>;
}

pub fn all_steps() -> Vec<Box<dyn Step>> {
    vec![
        Box::new(prefix::PrefixStep),
        Box::new(fonts_step::FontsStep),
        Box::new(webview2::WebView2Step),
        Box::new(shim::ShimStep),
        Box::new(sda::SdaStep),
    ]
}

pub fn step_title(id: StepId) -> &'static str {
    all_steps().into_iter().find(|s| s.id() == id).map(|s| s.title()).unwrap_or("Unknown step")
}

/// Runs `steps` in order on a worker thread. Stops at the first failure; a step whose
/// run succeeds but whose verification fails is a failure too.
pub fn run_steps(steps: Vec<Box<dyn Step>>, ctx: Arc<Ctx>, state_file: PathBuf, p: Progress) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut state = State::load(&state_file);
        let record = |state: &mut State, id: StepId| {
            if !state.completed.contains(&id) {
                state.completed.push(id);
            }
            if let Err(e) = state.save(&state_file) {
                ctx.log.line(&format!("cannot save state: {e:#}"));
            }
        };
        for step in steps {
            let id = step.id();
            if step.is_done(&ctx) {
                record(&mut state, id);
                p.send(ProgressMsg::Done(id));
                continue;
            }
            p.send(ProgressMsg::Started(id));
            ctx.log.line(&format!("== {}", step.title()));
            let outcome = step.run(&ctx, &p).and_then(|()| {
                anyhow::ensure!(step.is_done(&ctx), "verification failed: {}", step.title());
                Ok(())
            });
            match outcome {
                Ok(()) => {
                    record(&mut state, id);
                    p.send(ProgressMsg::Done(id));
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    ctx.log.line(&format!("FAILED: {msg}"));
                    state.completed.retain(|s| *s != id);
                    let _ = state.save(&state_file);
                    p.send(ProgressMsg::Failed(id, msg));
                    return;
                }
            }
        }
        p.send(ProgressMsg::AllDone);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wine::testutil::fake_appdir;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;

    struct Fake {
        id: StepId,
        fail: bool,
        verifies: bool,
        done: Arc<AtomicBool>,
        ran: Arc<AtomicBool>,
    }
    impl Fake {
        fn new(id: StepId) -> Fake {
            Fake { id, fail: false, verifies: true, done: Arc::default(), ran: Arc::default() }
        }
    }
    impl Step for Fake {
        fn id(&self) -> StepId {
            self.id
        }
        fn title(&self) -> &'static str {
            "fake step"
        }
        fn is_done(&self, _: &Ctx) -> bool {
            self.done.load(Ordering::SeqCst)
        }
        fn run(&self, _: &Ctx, _: &Progress) -> anyhow::Result<()> {
            self.ran.store(true, Ordering::SeqCst);
            if self.fail {
                anyhow::bail!("boom");
            }
            self.done.store(self.verifies, Ordering::SeqCst);
            Ok(())
        }
    }

    fn ctx(d: &std::path::Path) -> Arc<Ctx> {
        let p = fake_appdir(d, "exit 0");
        let log = LogSink::open(&p.logs, "t.log").unwrap();
        Arc::new(Ctx::new(p, log))
    }

    fn collect(steps: Vec<Box<dyn Step>>, c: Arc<Ctx>) -> Vec<ProgressMsg> {
        let (tx, rx) = channel();
        let sf = c.paths.state_file.clone();
        run_steps(steps, c, sf, Progress::new(tx)).join().unwrap();
        rx.into_iter().collect()
    }

    #[test]
    fn run_steps_stops_at_first_failure() {
        let d = tempfile::tempdir().unwrap();
        let c = ctx(d.path());
        let mut bad = Fake::new(StepId::Fonts);
        bad.fail = true;
        let later = Fake::new(StepId::Shim);
        let later_ran = later.ran.clone();
        let msgs = collect(vec![Box::new(Fake::new(StepId::Prefix)), Box::new(bad), Box::new(later)], c.clone());
        assert_eq!(
            msgs,
            vec![
                ProgressMsg::Started(StepId::Prefix),
                ProgressMsg::Done(StepId::Prefix),
                ProgressMsg::Started(StepId::Fonts),
                ProgressMsg::Failed(StepId::Fonts, "boom".into()),
            ]
        );
        assert!(!later_ran.load(Ordering::SeqCst));
        assert_eq!(State::load(&c.paths.state_file).completed, vec![StepId::Prefix]);
    }

    #[test]
    fn run_steps_skips_done_steps() {
        let d = tempfile::tempdir().unwrap();
        let s = Fake::new(StepId::Prefix);
        s.done.store(true, Ordering::SeqCst);
        let ran = s.ran.clone();
        let msgs = collect(vec![Box::new(s)], ctx(d.path()));
        assert_eq!(msgs, vec![ProgressMsg::Done(StepId::Prefix), ProgressMsg::AllDone]);
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[test]
    fn failed_verification_is_failure() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Fake::new(StepId::Prefix);
        s.verifies = false;
        let msgs = collect(vec![Box::new(s)], ctx(d.path()));
        assert_eq!(msgs.last(), Some(&ProgressMsg::Failed(StepId::Prefix, "verification failed: fake step".into())));
    }
}
