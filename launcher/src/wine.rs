//! Running the bundled Wine against StudioWine's prefix.
use crate::log::LogSink;
use crate::paths::Paths;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, BufReader};
use std::process::{Command, ExitStatus, Stdio};

/// Spawns `cmd` with piped output and copies every line into `log` until it exits.
pub fn run_logged(mut cmd: Command, log: &LogSink) -> anyhow::Result<ExitStatus> {
    log.line(&format!("$ {cmd:?}"));
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let pump = |r: Box<dyn std::io::Read + Send>, log: LogSink, done: std::sync::mpsc::Sender<()>| {
        std::thread::spawn(move || {
            // Byte lines decoded lossily: Windows programs may print in an ANSI codepage,
            // and stopping at the first invalid line would leave the pipe undrained.
            let mut r = BufReader::new(r);
            let mut buf = Vec::new();
            while matches!(r.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
                log.line(String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']));
                buf.clear();
            }
            let _ = done.send(());
        });
    };
    pump(Box::new(child.stdout.take().unwrap()), log.clone(), done_tx.clone());
    pump(Box::new(child.stderr.take().unwrap()), log.clone(), done_tx);
    let status = child.wait()?;
    // Installers often leave a service running (e.g. MicrosoftEdgeUpdate.exe) that inherited
    // our pipes, so EOF may never come: give the readers a moment, then leave them behind.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    for _ in 0..2 {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if done_rx.recv_timeout(left).is_err() {
            break;
        }
    }
    log.line(&format!("exit: {status}"));
    Ok(status)
}

/// A Unix path as Wine sees it through its `Z:` drive (`/a b/c` → `Z:\\a b\\c`).
pub fn dos_path(p: &std::path::Path) -> String {
    format!("Z:{}", p.to_string_lossy().replace('/', "\\"))
}

#[derive(Clone)]
pub struct Wine {
    paths: Paths,
}

impl Wine {
    pub fn new(paths: Paths) -> Wine {
        Wine { paths }
    }

    pub fn env(&self) -> Vec<(OsString, OsString)> {
        let p = &self.paths;
        let join = |first: &std::path::Path, var: &str| {
            let mut v = OsString::from(first);
            if let Some(old) = std::env::var_os(var).filter(|o| !o.is_empty()) {
                v.push(":");
                v.push(old);
            }
            v
        };
        let winedebug = std::env::var_os("STUDIOWINE_DEBUG").unwrap_or_else(|| "-all".into());
        // Bundled libraries first; then PipeWire's libjack for WineASIO, as `pw-jack` does.
        let mut lib_path = OsString::from(&p.lib_dir);
        if let Some(jack) = crate::audio::Jack::detect().extra_lib_dir() {
            lib_path.push(":");
            lib_path.push(jack);
        }
        // Our winemenubuilder writes this in place of `wine` into menu entries and URL
        // handlers, so they work the moment they are created (no `wine` on the host PATH).
        let menu_exec = format!("\"{}\" --wine", p.appimage.display());
        vec![
            ("WINEPREFIX".into(), p.prefix.clone().into()),
            ("WINEARCH".into(), "win64".into()),
            ("WINELOADER".into(), p.wine_bin.clone().into()),
            ("WINESERVER".into(), p.wineserver_bin.clone().into()),
            ("PATH".into(), join(&p.appdir.join("wine/bin"), "PATH")),
            ("LD_LIBRARY_PATH".into(), join(std::path::Path::new(&lib_path), "LD_LIBRARY_PATH")),
            ("WINEDEBUG".into(), winedebug),
            ("STUDIOWINE_MENU_EXEC".into(), menu_exec.into()),
        ]
    }

    pub fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut c = Command::new(&self.paths.wine_bin);
        c.args(args).envs(self.env());
        c
    }

    /// Runs wine to completion, writing every stdout/stderr line to `log`.
    pub fn run_wait<I, S>(&self, args: I, log: &LogSink) -> anyhow::Result<ExitStatus>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run_logged(self.command(args), log)
    }

    /// `wineserver -k` then `-w`: stops every process of the prefix so registry and
    /// font changes are re-read on the next start.
    pub fn kill_server(&self) -> anyhow::Result<()> {
        for flag in ["-k", "-w"] {
            Command::new(&self.paths.wineserver_bin).arg(flag).envs(self.env()).status()?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use crate::paths::Paths;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    /// An appdir whose wine/wineserver are shell scripts. `wine_script` is the body of the fake `wine`.
    pub fn fake_appdir(root: &Path, wine_script: &str) -> Paths {
        let appdir = root.join("app");
        let bin = appdir.join("wine/bin");
        std::fs::create_dir_all(&bin).unwrap();
        for (name, body) in [("wine", wine_script), ("wineserver", "exit 0")] {
            let f = bin.join(name);
            std::fs::write(&f, format!("#!/bin/bash\n{body}\n")).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Paths::from_roots(&appdir, &root.join("data/studiowine"), &root.join("StudioWine.AppImage"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn get(env: &[(OsString, OsString)], k: &str) -> Option<String> {
        env.iter().find(|(n, _)| n == k).map(|(_, v)| v.to_string_lossy().into_owned())
    }

    #[test]
    fn env_sets_prefix_and_path() {
        let p = Paths::from_roots(Path::new("/app"), Path::new("/d/studiowine"), Path::new("/a.AppImage"));
        let env = Wine::new(p).env();
        assert_eq!(get(&env, "WINEPREFIX").as_deref(), Some("/d/studiowine/prefix"));
        assert_eq!(get(&env, "WINEARCH").as_deref(), Some("win64"));
        assert_eq!(get(&env, "WINELOADER").as_deref(), Some("/app/wine/bin/wine"));
        assert_eq!(get(&env, "WINESERVER").as_deref(), Some("/app/wine/bin/wineserver"));
        assert!(get(&env, "PATH").unwrap().starts_with("/app/wine/bin:"));
        assert!(get(&env, "LD_LIBRARY_PATH").unwrap().starts_with("/app/usr/lib"));
        assert_eq!(get(&env, "STUDIOWINE_MENU_EXEC").as_deref(), Some(r#""/a.AppImage" --wine"#));
        if std::env::var_os("STUDIOWINE_DEBUG").is_none() {
            assert_eq!(get(&env, "WINEDEBUG").as_deref(), Some("-all"));
        }
        for (k, _) in &env {
            assert_ne!(k, "WINEDLLOVERRIDES", "no DLL overrides for the graphics stack");
        }
    }

    #[test]
    fn run_wait_returns_when_a_background_child_keeps_the_pipe_open() {
        let d = tempfile::tempdir().unwrap();
        // Like an installer that leaves an updater service running with our stdout inherited.
        let p = testutil::fake_appdir(d.path(), "echo installed; sleep 30 & exit 0");
        let log = LogSink::open(&p.logs, "t.log").unwrap();
        let t = std::time::Instant::now();
        assert!(Wine::new(p).run_wait(["setup.exe"], &log).unwrap().success());
        assert!(t.elapsed() < std::time::Duration::from_secs(10), "took {:?}", t.elapsed());
        assert!(std::fs::read_to_string(log.path()).unwrap().contains("installed"));
    }

    #[test]
    fn non_utf8_output_does_not_stop_logging() {
        let d = tempfile::tempdir().unwrap();
        let p = testutil::fake_appdir(d.path(), r"printf 'Gr\xfc\xdfe\n'; echo after-the-bad-line");
        let log = LogSink::open(&p.logs, "t.log").unwrap();
        Wine::new(p).run_wait(["x.exe"], &log).unwrap();
        let text = String::from_utf8_lossy(&std::fs::read(log.path()).unwrap()).into_owned();
        assert!(text.contains("after-the-bad-line"), "{text}");
    }

    #[test]
    fn dos_path_maps_through_z_drive() {
        assert_eq!(dos_path(Path::new("/tmp/a b/x.reg")), r"Z:\tmp\a b\x.reg");
    }

    #[test]
    fn env_handles_spaces() {
        let p = Paths::from_roots(Path::new("/app"), Path::new("/tmp/a b/Jürgen"), Path::new("/a.AppImage"));
        let env = Wine::new(p).env();
        assert_eq!(get(&env, "WINEPREFIX").as_deref(), Some("/tmp/a b/Jürgen/prefix"));
    }

    #[test]
    fn run_wait_logs_output() {
        let d = tempfile::tempdir().unwrap();
        let p = testutil::fake_appdir(d.path(), r#"echo "args:$*"; echo "prefix:$WINEPREFIX" >&2"#);
        let log = LogSink::open(&p.logs, "t.log").unwrap();
        let st = Wine::new(p.clone()).run_wait(["msiexec", "/i", "a b.msi"], &log).unwrap();
        assert!(st.success());
        let text = std::fs::read_to_string(log.path()).unwrap();
        assert!(text.contains("args:msiexec /i a b.msi"), "{text}");
        assert!(text.contains(&format!("prefix:{}", p.prefix.display())), "{text}");
    }
}
