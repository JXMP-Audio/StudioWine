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
    let pump = |r: Box<dyn std::io::Read + Send>, log: LogSink| {
        std::thread::spawn(move || {
            for l in BufReader::new(r).lines().map_while(Result::ok) {
                log.line(l.trim_end_matches('\r'));
            }
        })
    };
    let a = pump(Box::new(child.stdout.take().unwrap()), log.clone());
    let b = pump(Box::new(child.stderr.take().unwrap()), log.clone());
    let status = child.wait()?;
    let _ = (a.join(), b.join());
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

    pub fn paths(&self) -> &Paths {
        &self.paths
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
        vec![
            ("WINEPREFIX".into(), p.prefix.clone().into()),
            ("WINEARCH".into(), "win64".into()),
            ("WINELOADER".into(), p.wine_bin.clone().into()),
            ("WINESERVER".into(), p.wineserver_bin.clone().into()),
            ("PATH".into(), join(&p.appdir.join("wine/bin"), "PATH")),
            ("LD_LIBRARY_PATH".into(), join(&p.lib_dir, "LD_LIBRARY_PATH")),
            ("WINEDEBUG".into(), winedebug),
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
        if std::env::var_os("STUDIOWINE_DEBUG").is_none() {
            assert_eq!(get(&env, "WINEDEBUG").as_deref(), Some("-all"));
        }
        for (k, _) in &env {
            assert_ne!(k, "WINEDLLOVERRIDES", "no DLL overrides for the graphics stack");
        }
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
