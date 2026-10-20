//! Step 1: create the Wine prefix (Wine Mono from the bundle, no Gecko, Windows 10).
use super::{Ctx, Progress, Step, StepId};
use crate::registry::{read_value, RegFile, RegValue};
use crate::wine::run_logged;
use std::path::Path;

/// Wine links each user's Desktop to the host's `~/Desktop`; installers would then put
/// their shortcuts on the real desktop and the grid would show them from there. Replace
/// the link by a private folder (Wine only creates the link when the folder is missing).
/// Documents, Music and Downloads stay linked on purpose: projects and downloads belong
/// in the user's home.
pub fn make_desktops_private(prefix: &std::path::Path) -> anyhow::Result<()> {
    for desk in user_desktops(prefix) {
        if desk.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
            std::fs::remove_file(&desk)?; // removes the link only
            std::fs::create_dir(&desk)?;
        }
    }
    Ok(())
}

fn desktops_private(prefix: &std::path::Path) -> bool {
    user_desktops(prefix).iter().all(|d| !d.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()))
}

fn user_desktops(prefix: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(prefix.join("drive_c/users"))
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path().join("Desktop")).collect())
        .unwrap_or_default()
}

/// Written into every environment StudioWine creates; a non-empty folder without it is
/// never accepted as a location (Reset deletes the whole folder).
const MARKER: &str = ".studiowine";

pub fn mark(prefix: &Path) {
    let _ = std::fs::write(prefix.join(MARKER), b"StudioWine Wine environment\n");
}

/// Whether `p` may hold the Wine environment: an absolute path that is missing, an empty
/// folder, or a folder with an environment StudioWine created. `default` (the usual
/// `<data>/prefix`) is accepted as it is, since environments from before the marker lack it.
pub fn check_location(p: &Path, default: &Path) -> Result<(), String> {
    if !p.is_absolute() {
        return Err("Enter a full path, e.g. /mnt/audio/StudioWine.".into());
    }
    // Desktop-entry Exec lines carry the path in double quotes; these would need escaping.
    if let Some(c) = p.to_string_lossy().chars().find(|c| matches!(c, '"' | '`' | '$' | '\\' | '%') || c.is_control()) {
        return Err(format!("The path must not contain {c:?}."));
    }
    if p == default || !p.exists() {
        return Ok(());
    }
    if !p.is_dir() {
        return Err(format!("{} is a file, not a folder.", p.display()));
    }
    let empty = std::fs::read_dir(p).map_err(|e| format!("{}: {e}", p.display()))?.next().is_none();
    if empty || p.join(MARKER).is_file() {
        Ok(())
    } else {
        Err(format!(
            "{} is not empty. Choose a new or empty folder, or one that already holds a StudioWine environment.",
            p.display()
        ))
    }
}

/// Creates the folder and makes sure the file system can hold a Wine environment
/// (symbolic links: FAT, exFAT and NTFS mounts cannot).
pub fn prepare_location(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("Cannot create {}: {e}", p.display()))?;
    let probe = p.join(".studiowine-symlink-test");
    let _ = std::fs::remove_file(&probe);
    let r = std::os::unix::fs::symlink("drive_c", &probe);
    let _ = std::fs::remove_file(&probe);
    r.map_err(|e| format!("{} cannot hold a Wine environment (no symbolic links: {e}). Choose a folder on a Linux file system.", p.display()))
}

pub struct PrefixStep;

impl Step for PrefixStep {
    fn id(&self) -> StepId {
        StepId::Prefix
    }
    fn title(&self) -> &'static str {
        "Create Wine environment"
    }
    fn is_done(&self, ctx: &Ctx) -> bool {
        let pre = &ctx.paths.prefix;
        pre.join("system.reg").is_file()
            && pre.join("drive_c/windows/mono").is_dir()
            && read_value(&pre.join("user.reg"), r"Software\Wine", "Version").as_deref() == Some("\"win10\"")
            && desktops_private(pre)
    }
    fn run(&self, ctx: &Ctx, p: &Progress) -> anyhow::Result<()> {
        for d in [&ctx.paths.data, &ctx.paths.cache, &ctx.paths.logs] {
            std::fs::create_dir_all(d)?;
        }
        p.status(self.id(), "Creating the Wine environment (this takes a minute)…");
        let mut cmd = ctx.wine.command(["wineboot", "-u"]);
        cmd.env("WINEDLLOVERRIDES", "mshtml=d"); // no Gecko download prompt; nothing here needs it
        let st = run_logged(cmd, &ctx.log)?;
        anyhow::ensure!(st.success(), "wineboot failed ({st})");
        make_desktops_private(&ctx.paths.prefix)?;
        mark(&ctx.paths.prefix);
        p.fraction(self.id(), 0.8);
        p.status(self.id(), "Setting Windows 10 mode…");
        let mut reg = RegFile::new();
        reg.set(r"HKEY_CURRENT_USER\Software\Wine", "Version", RegValue::Sz("win10".into()));
        reg.import(&ctx.wine, &ctx.log, &ctx.paths.cache)?;
        ctx.wine.kill_server()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::LogSink;
    use crate::wine::testutil::fake_appdir;

    /// wineboot creates the prefix files; regedit records the imported file's text.
    const FAKE: &str = r#"
calls="$WINEPREFIX/../calls.log"; mkdir -p "$WINEPREFIX"
echo "$1 DLLOVERRIDES=$WINEDLLOVERRIDES" >> "$calls"
case "$1" in
  wineboot) mkdir -p "$WINEPREFIX/drive_c/windows/mono"; echo 'WINE REGISTRY Version 2' > "$WINEPREFIX/system.reg"; echo 'WINE REGISTRY Version 2' > "$WINEPREFIX/user.reg" ;;
  regedit) f=$(echo "$3" | sed -e 's/^Z://' -e 's#\\#/#g'); if iconv -f UTF-16 -t UTF-8 "$f" | grep -q '"Version"="win10"'; then printf '[Software\\\\Wine] 1\n"Version"="win10"\n' >> "$WINEPREFIX/user.reg"; fi ;;
esac
"#;

    #[test]
    fn desktop_symlinks_become_private_folders() {
        let d = tempfile::tempdir().unwrap();
        let home_desktop = d.path().join("home/Desktop");
        std::fs::create_dir_all(&home_desktop).unwrap();
        std::fs::write(home_desktop.join("mine.txt"), b"keep").unwrap();
        let users = d.path().join("prefix/drive_c/users");
        std::fs::create_dir_all(users.join("xg")).unwrap();
        std::fs::create_dir_all(users.join("Public/Desktop")).unwrap();
        std::os::unix::fs::symlink(&home_desktop, users.join("xg/Desktop")).unwrap();
        std::os::unix::fs::symlink(d.path().join("home/Documents"), users.join("xg/Documents")).unwrap();
        let prefix = d.path().join("prefix");
        assert!(!desktops_private(&prefix));
        make_desktops_private(&prefix).unwrap();
        let desk = users.join("xg/Desktop");
        assert!(desk.is_dir() && !desk.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(users.join("xg/Documents").symlink_metadata().unwrap().file_type().is_symlink(), "Documents stays linked");
        assert_eq!(std::fs::read(home_desktop.join("mine.txt")).unwrap(), b"keep", "the host desktop is untouched");
        assert!(desktops_private(&prefix));
    }

    #[test]
    fn location_rules() {
        let d = tempfile::tempdir().unwrap();
        let def = d.path().join("data/prefix");
        assert!(check_location(Path::new("rel/wine"), &def).is_err());
        assert!(check_location(&d.path().join("new/wine"), &def).is_ok(), "missing folder");
        assert!(check_location(&d.path().join("a$b"), &def).unwrap_err().contains('$'));
        let empty = d.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        assert!(check_location(&empty, &def).is_ok());
        let music = d.path().join("Music");
        std::fs::create_dir(&music).unwrap();
        std::fs::write(music.join("song.flac"), b"x").unwrap();
        assert!(check_location(&music, &def).unwrap_err().contains("not empty"), "never a folder Reset could wipe");
        let foreign = d.path().join(".wine");
        std::fs::create_dir(&foreign).unwrap();
        std::fs::write(foreign.join("system.reg"), b"WINE REGISTRY").unwrap();
        assert!(check_location(&foreign, &def).is_err(), "another Wine prefix is not taken over");
        mark(&foreign);
        assert!(check_location(&foreign, &def).is_ok(), "an environment StudioWine created");
        std::fs::create_dir_all(&def).unwrap();
        std::fs::write(def.join("system.reg"), b"x").unwrap();
        assert!(check_location(&def, &def).is_ok(), "the default location from before the marker");
        let f = d.path().join("file");
        std::fs::write(&f, b"x").unwrap();
        assert!(check_location(&f, &def).unwrap_err().contains("not a folder"));
    }

    #[test]
    fn prepare_creates_folder_on_a_linux_file_system() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x/y/prefix");
        prepare_location(&p).unwrap();
        assert!(p.is_dir());
        assert_eq!(std::fs::read_dir(&p).unwrap().count(), 0, "the probe link is removed");
    }

    #[test]
    fn prefix_step_runs_wineboot_with_mshtml_disabled_and_sets_win10() {
        let d = tempfile::tempdir().unwrap();
        let paths = fake_appdir(d.path(), FAKE);
        let log = LogSink::open(&paths.logs, "t.log").unwrap();
        let ctx = Ctx::new(paths.clone(), log);
        let (tx, _rx) = std::sync::mpsc::channel();
        assert!(!PrefixStep.is_done(&ctx));
        PrefixStep.run(&ctx, &Progress::new(tx)).unwrap();
        let calls = std::fs::read_to_string(paths.data.join("calls.log")).unwrap();
        assert!(calls.contains("wineboot DLLOVERRIDES=mshtml=d"), "{calls}");
        assert!(calls.lines().any(|l| l.starts_with("regedit DLLOVERRIDES=") && !l.contains("mshtml")), "{calls}");
        assert!(PrefixStep.is_done(&ctx));
        assert!(paths.prefix.join(MARKER).is_file());
    }
}
