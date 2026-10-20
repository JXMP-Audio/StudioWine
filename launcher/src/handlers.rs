//! Menu entries and URL handlers written by winemenubuilder call plain `wine`; point the
//! ones belonging to our prefix at the AppImage so sign-in links reach the right Wine.
use std::path::Path;

/// For an `Exec=` line of our prefix that calls `wine` (or an earlier AppImage with
/// `--wine`), the same line calling `"<appimage>" --wine`. None for any other line.
pub fn rewrite_exec(line: &str, prefix: &Path, appimage: &Path) -> Option<String> {
    if !line.starts_with("Exec=") {
        return None;
    }
    // winemenubuilder writes `env "WINEPREFIX=<path>"`; also accept `WINEPREFIX="<path>"`.
    let p = prefix.display();
    let head_end = [format!("\"WINEPREFIX={p}\""), format!("WINEPREFIX=\"{p}\"")]
        .iter()
        .find_map(|needle| line.find(needle.as_str()).map(|i| i + needle.len()))?;
    let rest = line[head_end..].trim_start();
    let tail = if let Some(t) = rest.strip_prefix("wine ") {
        t
    } else if let Some(quoted) = rest.strip_prefix('"') {
        let close = quoted.find('"')?;
        quoted[close + 1..].strip_prefix(" --wine ")?
    } else {
        return None;
    };
    Some(format!("{} \"{}\" --wine {}", &line[..head_end], appimage.display(), tail))
}

fn desktop_files(apps_dir: &Path) -> Vec<std::path::PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.filter_map(Result::ok) {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "desktop") {
                    out.push(p);
                }
            }
        }
    }
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(apps_dir) {
        out.extend(rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| {
            p.is_file()
                && p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    (n.starts_with("wine-protocol-") || n.starts_with("wine-extension-")) && n.ends_with(".desktop")
                })
        }));
    }
    walk(&apps_dir.join("wine"), &mut out);
    out.sort();
    out
}

/// Rewrites every matching .desktop file under `apps_dir`; returns how many changed.
pub fn rewrite_all_in(apps_dir: &Path, prefix: &Path, appimage: &Path) -> anyhow::Result<usize> {
    let mut changed = 0;
    for f in desktop_files(apps_dir) {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let new: String = text
            .split_inclusive('\n')
            .map(|l| {
                let (body, nl) = l.strip_suffix('\n').map(|b| (b, "\n")).unwrap_or((l, ""));
                rewrite_exec(body, prefix, appimage).map(|r| format!("{r}{nl}")).unwrap_or_else(|| l.to_string())
            })
            .collect();
        if new != text {
            std::fs::write(&f, new)?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// `~/.local/share/applications`, where winemenubuilder writes (it follows XDG_DATA_HOME too).
fn apps_dir() -> anyhow::Result<std::path::PathBuf> {
    Ok(dirs::data_dir().ok_or_else(|| anyhow::anyhow!("no XDG data directory"))?.join("applications"))
}

/// `rewrite_all_in` on `~/.local/share/applications` (wine-protocol-*, wine-extension-* and wine/**).
pub fn rewrite_all(prefix: &Path, appimage: &Path) -> anyhow::Result<usize> {
    rewrite_all_in(&apps_dir()?, prefix, appimage)
}

const ASSOCIATIONS_KEY: &str = r"HKEY_CURRENT_USER\Software\Wine\FileOpenAssociations";

/// The associations recorded in a `reg export` of ASSOCIATIONS_KEY (`.cpr`, `net-steinberg-sda`…).
pub fn recorded_associations(export: &str) -> Vec<String> {
    let head = format!("[{ASSOCIATIONS_KEY}\\");
    export
        .lines()
        .filter_map(|l| l.trim_end().strip_prefix(head.as_str())?.strip_suffix(']'))
        .filter(|name| !name.contains('\\'))
        .map(str::to_string)
        .collect()
}

/// Those of `recorded` (associations of `prefix`) whose handler file is gone or was since
/// written by another environment. Wine writes a handler only when the recorded association
/// changes, so it never takes these back by itself.
pub fn stale_associations(prefix: &Path, recorded: &[String], apps_dir: &Path) -> Vec<String> {
    let p = prefix.display();
    let ours = [format!("\"WINEPREFIX={p}\""), format!("WINEPREFIX=\"{p}\"")];
    recorded
        .iter()
        .filter(|name| {
            // The file names winemenubuilder.c gives protocol and extension handlers.
            let file = match name.strip_prefix('.') {
                Some(ext) => format!("wine-extension-{ext}.desktop"),
                None => format!("wine-protocol-{name}.desktop"),
            };
            let text = std::fs::read_to_string(apps_dir.join(file)).unwrap_or_default();
            !text.lines().any(|l| l.starts_with("Exec=") && ours.iter().any(|o| l.contains(o.as_str())))
        })
        .cloned()
        .collect()
}

/// `reg export` output: UTF-16LE with a BOM from Wine's reg.exe.
fn decode_reg(b: &[u8]) -> String {
    match b.strip_prefix(&[0xFF, 0xFE]) {
        Some(u) => String::from_utf16_lossy(&u.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()),
        None => String::from_utf8_lossy(b).into_owned(),
    }
}

/// Makes the file and link handlers of `ctx`'s environment the active ones: forgets the
/// associations another environment took over, then lets winemenubuilder write every handler
/// that is missing, including those no installer announced (the Activation Manager's link).
pub fn claim(ctx: &crate::setup::Ctx) -> anyhow::Result<()> {
    claim_in(ctx, &apps_dir()?)
}

fn claim_in(ctx: &crate::setup::Ctx, apps_dir: &Path) -> anyhow::Result<()> {
    let prefix = &ctx.paths.prefix;
    if !prefix.join("system.reg").is_file() {
        return Ok(());
    }
    // Ask Wine: user.reg on disk lags behind while a program of the environment runs.
    std::fs::create_dir_all(&ctx.paths.cache)?;
    let export = ctx.paths.cache.join("associations.reg");
    let _ = std::fs::remove_file(&export);
    let file = crate::wine::dos_path(&export);
    let st = ctx.wine.run_wait(["reg", "export", ASSOCIATIONS_KEY, file.as_str(), "/y"], &ctx.log)?;
    // Fails when nothing is recorded yet.
    let recorded = if st.success() { recorded_associations(&decode_reg(&std::fs::read(&export)?)) } else { Vec::new() };
    let _ = std::fs::remove_file(&export);
    let stale = stale_associations(prefix, &recorded, apps_dir);
    if !stale.is_empty() {
        ctx.log.line(&format!("handlers to take back for this environment: {}", stale.join(" ")));
    }
    // One subkey each: the key itself holds the user's Enable and Exclude settings.
    for name in &stale {
        let key = format!(r"HKCU\Software\Wine\FileOpenAssociations\{name}");
        let st = ctx.wine.run_wait(["reg", "delete", key.as_str(), "/f"], &ctx.log)?;
        anyhow::ensure!(st.success(), "cannot reset the recorded association {name} ({st})");
    }
    let st = ctx.wine.run_wait(["winemenubuilder", "-a"], &ctx.log)?;
    anyhow::ensure!(st.success(), "winemenubuilder failed ({st})");
    // Files written before winemenubuilder knew STUDIOWINE_MENU_EXEC still call plain `wine`.
    rewrite_all_in(apps_dir, prefix, &ctx.paths.appimage)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRE: &str = "/home/u/.local/share/studiowine/prefix";

    // Formats written by this Wine's winemenubuilder.c (lines 1276 and 2041).
    /// What Wine's `reg export` of ASSOCIATIONS_KEY writes: UTF-16LE with a BOM, CRLF.
    fn export(names: &[&str]) -> Vec<u8> {
        let mut t = format!("Windows Registry Editor Version 5.00\r\n\r\n[{ASSOCIATIONS_KEY}]\r\n\"Exclude\"=hex(7):00,00\r\n\r\n");
        for n in names {
            t += &format!("[{ASSOCIATIONS_KEY}\\{n}]\r\n\"AppName\"=\"x\"\r\n\r\n");
        }
        [0xFF, 0xFE].into_iter().chain(t.encode_utf16().flat_map(u16::to_le_bytes)).collect()
    }

    #[test]
    fn recorded_associations_from_reg_export() {
        let names = recorded_associations(&decode_reg(&export(&[".cpr", "net-steinberg-sda"])));
        assert_eq!(names, [".cpr", "net-steinberg-sda"]);
        assert!(recorded_associations("").is_empty());
    }

    #[test]
    fn stale_associations_are_missing_or_foreign_handlers() {
        let d = tempfile::tempdir().unwrap();
        let (pre, apps) = (d.path().join("Bottles/Cubase"), d.path().join("apps"));
        let recorded: Vec<String> =
            [".cpr", ".chm", "net-steinberg-sda", "net-steinberg-activation-manager"].map(String::from).into();
        std::fs::create_dir_all(&apps).unwrap();
        let exec = |p: &Path| format!("[Desktop Entry]\nExec=env \"WINEPREFIX={}\" \"/a.AppImage\" --wine start %u\n", p.display());
        std::fs::write(apps.join("wine-extension-cpr.desktop"), exec(&pre)).unwrap();
        std::fs::write(apps.join("wine-extension-chm.desktop"), exec(Path::new(PRE))).unwrap();
        std::fs::write(apps.join("wine-protocol-net-steinberg-sda.desktop"), exec(&pre)).unwrap();
        // A prefix that is a prefix of ours ("…/Cubase" vs "…/Cubase 2") must not count as ours.
        let longer = d.path().join("Bottles/Cubase 2");
        std::fs::write(apps.join("wine-protocol-net-steinberg-activation-manager.desktop"), exec(&longer)).unwrap();
        assert_eq!(stale_associations(&pre, &recorded, &apps), [".chm", "net-steinberg-activation-manager"]);
        std::fs::remove_file(apps.join("wine-extension-cpr.desktop")).unwrap();
        assert_eq!(stale_associations(&pre, &recorded, &apps), [".cpr", ".chm", "net-steinberg-activation-manager"]);
    }

    #[test]
    fn claim_forgets_taken_over_associations_then_runs_winemenubuilder() {
        let d = tempfile::tempdir().unwrap();
        // `reg export` copies the prepared export (if any) to the Z:\ path it is given.
        let fake = r#"echo "$*" >> "$WINEPREFIX/../calls.log"
if [ "$1 $2" = "reg export" ]; then
  [ -f "$WINEPREFIX/../export.reg" ] || exit 1
  cp "$WINEPREFIX/../export.reg" "$(echo "$4" | sed -e 's/^Z://' -e 's#\\#/#g')"
fi
"#;
        let paths = crate::wine::testutil::fake_appdir(d.path(), fake);
        let ctx = crate::setup::Ctx::new(paths.clone(), crate::log::LogSink::open(&paths.logs, "t.log").unwrap());
        let apps = d.path().join("apps");
        std::fs::create_dir_all(&apps).unwrap();
        let calls = || std::fs::read_to_string(paths.data.join("calls.log")).unwrap_or_default();
        claim_in(&ctx, &apps).unwrap();
        assert_eq!(calls(), "", "no environment yet: nothing runs");

        std::fs::create_dir_all(&paths.prefix).unwrap();
        std::fs::write(paths.prefix.join("system.reg"), "WINE REGISTRY Version 2\n").unwrap();
        claim_in(&ctx, &apps).unwrap();
        assert!(calls().ends_with("/y\nwinemenubuilder -a\n"), "nothing recorded yet: {}", calls());

        std::fs::write(paths.data.join("export.reg"), export(&["net-steinberg-sda"])).unwrap();
        let mine = format!("[Desktop Entry]\nExec=env \"WINEPREFIX={}\" wine start %u\n", paths.prefix.display());
        std::fs::write(apps.join("wine-protocol-net-steinberg-sda.desktop"), &mine).unwrap();
        std::fs::remove_file(paths.data.join("calls.log")).unwrap();
        claim_in(&ctx, &apps).unwrap();
        assert!(!calls().contains("reg delete"), "handlers are ours: only add missing ones: {}", calls());
        let t = std::fs::read_to_string(apps.join("wine-protocol-net-steinberg-sda.desktop")).unwrap();
        assert!(t.contains(&format!("\"{}\" --wine start %u", paths.appimage.display())), "{t}");

        std::fs::write(apps.join("wine-protocol-net-steinberg-sda.desktop"), mine.replace(&paths.prefix.display().to_string(), PRE)).unwrap();
        std::fs::remove_file(paths.data.join("calls.log")).unwrap();
        claim_in(&ctx, &apps).unwrap();
        let c = calls();
        assert!(c.starts_with(r"reg export HKEY_CURRENT_USER\Software\Wine\FileOpenAssociations Z:"), "{c}");
        assert!(c.ends_with("reg delete HKCU\\Software\\Wine\\FileOpenAssociations\\net-steinberg-sda /f\nwinemenubuilder -a\n"), "{c}");
        assert!(!paths.cache.join("associations.reg").exists(), "export removed");
    }

    #[test]
    fn rewrite_real_winemenubuilder_formats() {
        let ai = Path::new("/opt/StudioWine.AppImage");
        let menu = format!(r#"Exec=env "WINEPREFIX={PRE}" wine "C:\\users\\Public\\Desktop\\Cubase 15.lnk""#);
        assert_eq!(
            rewrite_exec(&menu, Path::new(PRE), ai).as_deref(),
            Some(format!(r#"Exec=env "WINEPREFIX={PRE}" "/opt/StudioWine.AppImage" --wine "C:\\users\\Public\\Desktop\\Cubase 15.lnk""#).as_str())
        );
        let proto = format!(r#"Exec=env "WINEPREFIX={PRE}" wine start %u"#);
        assert_eq!(
            rewrite_exec(&proto, Path::new(PRE), ai).as_deref(),
            Some(format!(r#"Exec=env "WINEPREFIX={PRE}" "/opt/StudioWine.AppImage" --wine start %u"#).as_str())
        );
        let spaced = r#"Exec=env "WINEPREFIX=/home/Jürgen Müller/.local/share/studiowine/prefix" wine start /ProgIDOpen "Cubase.Project" %f"#;
        let out = rewrite_exec(spaced, Path::new("/home/Jürgen Müller/.local/share/studiowine/prefix"), ai).unwrap();
        assert!(out.ends_with(r#" "/opt/StudioWine.AppImage" --wine start /ProgIDOpen "Cubase.Project" %f"#), "{out}");
        let again = rewrite_exec(&out, Path::new("/home/Jürgen Müller/.local/share/studiowine/prefix"), Path::new("/new/S.AppImage")).unwrap();
        assert!(again.contains(r#" "/new/S.AppImage" --wine start"#), "{again}");
        assert_eq!(rewrite_exec(r#"Exec=env "WINEPREFIX=/home/u/.wine" wine start %u"#, Path::new(PRE), ai), None);
    }

    #[test]
    fn rewrites_file_generated_by_our_wine() {
        let d = tempfile::tempdir().unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wine-extension-vbs.desktop");
        std::fs::copy(&src, d.path().join("wine-extension-vbs.desktop")).unwrap();
        let pre = Path::new("/tmp/h/a b/prefix");
        assert_eq!(rewrite_all_in(d.path(), pre, Path::new("/x y/StudioWine.AppImage")).unwrap(), 1);
        let t = std::fs::read_to_string(d.path().join("wine-extension-vbs.desktop")).unwrap();
        assert!(t.contains(r#"Exec=env "WINEPREFIX=/tmp/h/a b/prefix" "/x y/StudioWine.AppImage" --wine start /ProgIDOpen "VBSFile" %f"#), "{t}");
    }

    #[test]
    fn rewrite_all_includes_extension_handlers() {
        let d = tempfile::tempdir().unwrap();
        let ext = format!("[Desktop Entry]\nExec=env \"WINEPREFIX={PRE}\" wine start /ProgIDOpen \"x\" %f\n");
        std::fs::write(d.path().join("wine-extension-cpr.desktop"), &ext).unwrap();
        assert_eq!(rewrite_all_in(d.path(), Path::new(PRE), Path::new("/a.AppImage")).unwrap(), 1);
    }

    #[test]
    fn rewrite_exec_points_at_appimage() {
        let l = format!(r#"Exec=env WINEPREFIX="{PRE}" wine C:\\windows\\command\\start.exe /ProgIDOpen net-steinberg-sda %u"#);
        assert_eq!(
            rewrite_exec(&l, Path::new(PRE), Path::new("/opt/StudioWine.AppImage")).as_deref(),
            Some(format!(r#"Exec=env WINEPREFIX="{PRE}" "/opt/StudioWine.AppImage" --wine C:\\windows\\command\\start.exe /ProgIDOpen net-steinberg-sda %u"#).as_str())
        );
    }

    #[test]
    fn rewrite_quotes_appimage_path_with_spaces() {
        let l = format!(r#"Exec=env WINEPREFIX="{PRE}" wine start /unix "/x/Cubase 15.lnk""#);
        let out = rewrite_exec(&l, Path::new(PRE), Path::new("/home/a b/StudioWine.AppImage")).unwrap();
        assert!(out.contains(r#" "/home/a b/StudioWine.AppImage" --wine start /unix "/x/Cubase 15.lnk""#), "{out}");
    }

    #[test]
    fn rewrite_replaces_old_appimage_path() {
        let l = format!(r#"Exec=env WINEPREFIX="{PRE}" "/old/StudioWine-0.0.9.AppImage" --wine start x"#);
        let out = rewrite_exec(&l, Path::new(PRE), Path::new("/new/StudioWine.AppImage")).unwrap();
        assert_eq!(out, format!(r#"Exec=env WINEPREFIX="{PRE}" "/new/StudioWine.AppImage" --wine start x"#));
    }

    #[test]
    fn rewrite_ignores_other_prefix() {
        let l = r#"Exec=env WINEPREFIX="/home/u/.wine" wine start x"#;
        assert_eq!(rewrite_exec(l, Path::new(PRE), Path::new("/a.AppImage")), None);
        assert_eq!(rewrite_exec("Name=Cubase", Path::new(PRE), Path::new("/a.AppImage")), None);
    }

    #[test]
    fn rewrite_all_in_changes_only_our_files() {
        let d = tempfile::tempdir().unwrap();
        let apps = d.path();
        std::fs::create_dir_all(apps.join("wine/Programs/Steinberg")).unwrap();
        let ours = format!("[Desktop Entry]\nName=Cubase 15\nExec=env WINEPREFIX=\"{PRE}\" wine start /unix x.lnk\nType=Application\n");
        let other = "[Desktop Entry]\nExec=env WINEPREFIX=\"/home/u/.wine\" wine start y\n";
        std::fs::write(apps.join("wine/Programs/Steinberg/Cubase 15.desktop"), &ours).unwrap();
        std::fs::write(apps.join("wine-protocol-net-steinberg-sda.desktop"), &ours).unwrap();
        std::fs::write(apps.join("wine-protocol-other.desktop"), other).unwrap();
        std::fs::write(apps.join("firefox.desktop"), &ours).unwrap();
        assert_eq!(rewrite_all_in(apps, Path::new(PRE), Path::new("/a.AppImage")).unwrap(), 2);
        assert!(std::fs::read_to_string(apps.join("wine/Programs/Steinberg/Cubase 15.desktop")).unwrap().contains("\"/a.AppImage\" --wine start"));
        assert_eq!(std::fs::read_to_string(apps.join("wine-protocol-other.desktop")).unwrap(), other);
        assert_eq!(std::fs::read_to_string(apps.join("firefox.desktop")).unwrap(), ours);
        assert_eq!(rewrite_all_in(apps, Path::new(PRE), Path::new("/a.AppImage")).unwrap(), 0, "idempotent");
    }
}
