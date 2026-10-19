//! Append-only log file, optionally mirrored to a callback (the wizard's status line).
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type Callback = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub struct LogSink {
    file: Arc<Mutex<File>>,
    path: PathBuf,
    callback: Option<Callback>,
}

impl LogSink {
    pub fn open(dir: &Path, name: &str) -> anyhow::Result<LogSink> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(name);
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(LogSink { file: Arc::new(Mutex::new(file)), path, callback: None })
    }

    #[cfg(test)]
    pub fn with_callback(self, f: impl Fn(&str) + Send + Sync + 'static) -> LogSink {
        LogSink { callback: Some(Arc::new(f)), ..self }
    }

    pub fn line(&self, s: &str) {
        if let Ok(mut f) = self.file.lock() {
            let _ = writeln!(f, "{s}");
        }
        if let Some(cb) = &self.callback {
            cb(s);
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_go_to_file_and_callback() {
        let d = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let s2 = seen.clone();
        let log = LogSink::open(d.path(), "t.log").unwrap().with_callback(move |l| s2.lock().unwrap().push(l.to_string()));
        log.line("hello");
        log.line("world");
        let text = std::fs::read_to_string(log.path()).unwrap();
        assert!(text.contains("hello\n") && text.contains("world\n"));
        assert_eq!(*seen.lock().unwrap(), vec!["hello", "world"]);
    }
}
