//! Setup wizard steps and the runner that executes them.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum StepId {
    Prefix,
    Fonts,
    WebView2,
    Shim,
    DownloadAssistant,
}
