//! Persistent launcher state (state.json in the data directory).
use crate::setup::StepId;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
pub struct State {
    pub completed: Vec<StepId>,
    pub user_fonts_imported: bool,
    /// Wine environment chosen in the wizard; None is the default `<data>/prefix`.
    #[serde(default)]
    pub prefix: Option<PathBuf>,
}

impl State {
    /// A missing or unreadable file is a fresh start.
    pub fn load(path: &Path) -> State {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Written to a temporary file and renamed, so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("sub/state.json");
        let s = State { completed: vec![StepId::Prefix, StepId::Fonts], user_fonts_imported: true, prefix: Some("/mnt/audio/wine".into()) };
        s.save(&f).unwrap();
        assert_eq!(State::load(&f), s);
    }

    #[test]
    fn file_without_prefix_loads() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("state.json");
        std::fs::write(&f, r#"{"completed":["Prefix"],"user_fonts_imported":false}"#).unwrap();
        assert_eq!(State::load(&f), State { completed: vec![StepId::Prefix], ..Default::default() });
    }

    #[test]
    fn corrupt_file_gives_default() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("state.json");
        std::fs::write(&f, "{not json").unwrap();
        assert_eq!(State::load(&f), State::default());
        assert_eq!(State::load(&d.path().join("missing.json")), State::default());
    }
}
