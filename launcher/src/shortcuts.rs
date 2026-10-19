//! Shortcuts that Windows installers put on the Wine desktop.
use crate::log::LogSink;
use crate::wine::Wine;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Shortcut {
    pub name: String,
    pub lnk: PathBuf,
    /// Unix path of the target program, when the .lnk carries one.
    pub target: Option<PathBuf>,
}

/// Desktop folders of every Wine user in the prefix, Public included.
pub fn desktop_dirs(prefix: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(prefix.join("drive_c/users"))
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path().join("Desktop")).filter(|d| d.is_dir()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs
}

fn lnk_files(prefix: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for d in desktop_dirs(prefix) {
        if let Ok(rd) = std::fs::read_dir(&d) {
            out.extend(
                rd.filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk"))),
            );
        }
    }
    out.sort();
    out
}

pub fn scan(prefix: &Path) -> Vec<Shortcut> {
    let mut out: Vec<Shortcut> = Vec::new();
    for lnk in lnk_files(prefix) {
        let name = lnk.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        if out.iter().any(|s| s.name.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let target = lnk_target(&lnk).and_then(|t| windows_to_unix(prefix, &t));
        out.push(Shortcut { name, lnk, target });
    }
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// `C:\x\y` → `<prefix>/drive_c/x/y`.
pub fn windows_to_unix(prefix: &Path, win: &str) -> Option<PathBuf> {
    let rest = win.strip_prefix("C:").or_else(|| win.strip_prefix("c:"))?;
    let rel = rest.trim_start_matches('\\').replace('\\', "/");
    Some(prefix.join("drive_c").join(rel))
}

/// The Windows target path stored in a .lnk file.
pub fn lnk_target(lnk: &Path) -> Option<String> {
    let link = lnk::ShellLink::open(lnk, lnk::encoding::WINDOWS_1252).ok()?;
    link.link_target()
}

/// Starts the shortcut through Wine's `start`, which applies the link's arguments and
/// directory, on a thread whose output goes to `log` (debug channels included).
pub fn launch(wine: &Wine, s: &Shortcut, log: &LogSink) -> std::thread::JoinHandle<anyhow::Result<std::process::ExitStatus>> {
    let cmd = wine.command(["start".as_ref(), "/unix".as_ref(), s.lnk.as_os_str()]);
    let log = log.clone();
    std::thread::spawn(move || crate::wine::run_logged(cmd, &log))
}

/// Changes whenever a shortcut appears, disappears or is rewritten; the UI polls it.
pub fn dir_fingerprint(prefix: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in lnk_files(prefix) {
        f.hash(&mut h);
        std::fs::metadata(&f).and_then(|m| m.modified()).ok().hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(n: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(n)
    }

    #[test]
    fn scan_finds_public_and_user_desktop() {
        let d = tempfile::tempdir().unwrap();
        let users = d.path().join("drive_c/users");
        for (u, f) in [("Public", "Cubase 15.lnk"), ("xg", "Activation Manager.lnk"), ("xg", "readme.txt"), ("Public", "activation manager.lnk")] {
            std::fs::create_dir_all(users.join(u).join("Desktop")).unwrap();
            std::fs::write(users.join(u).join("Desktop").join(f), b"not a real link").unwrap();
        }
        let s = scan(d.path());
        let names: Vec<_> = s.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(names[0].eq_ignore_ascii_case("Activation Manager"));
        assert_eq!(names[1], "Cubase 15");
        assert!(s.iter().all(|x| x.target.is_none()));
    }

    #[test]
    fn windows_to_unix_maps_drive_c() {
        let p = Path::new("/pre");
        assert_eq!(windows_to_unix(p, r"C:\Program Files\Steinberg\Cubase 15\Cubase15.exe"), Some(PathBuf::from("/pre/drive_c/Program Files/Steinberg/Cubase 15/Cubase15.exe")));
        assert_eq!(windows_to_unix(p, r"c:\x"), Some(PathBuf::from("/pre/drive_c/x")));
        assert_eq!(windows_to_unix(p, r"D:\x"), None);
    }

    #[test]
    fn lnk_target_reads_real_link() {
        assert_eq!(
            lnk_target(&fixture("test.lnk")).as_deref(),
            Some(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        );
        assert_eq!(lnk_target(&fixture("README")), None);
    }

    #[test]
    fn launch_output_goes_to_the_log() {
        let d = tempfile::tempdir().unwrap();
        let p = crate::wine::testutil::fake_appdir(d.path(), r#"echo "launched: $*""#);
        let log = crate::log::LogSink::open(&p.logs, "t.log").unwrap();
        let s = Shortcut { name: "Cubase 15".into(), lnk: d.path().join("Cubase 15.lnk"), target: None };
        let st = launch(&Wine::new(p), &s, &log).join().unwrap().unwrap();
        assert!(st.success());
        let text = std::fs::read_to_string(log.path()).unwrap();
        assert!(text.contains("launched: start /unix") && text.contains("Cubase 15.lnk"), "{text}");
    }

    #[test]
    fn fingerprint_changes_when_shortcut_added() {
        let d = tempfile::tempdir().unwrap();
        let desk = d.path().join("drive_c/users/Public/Desktop");
        std::fs::create_dir_all(&desk).unwrap();
        let a = dir_fingerprint(d.path());
        std::fs::write(desk.join("New.lnk"), b"x").unwrap();
        assert_ne!(a, dir_fingerprint(d.path()));
    }
}
