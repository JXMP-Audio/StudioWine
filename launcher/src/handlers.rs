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

/// `rewrite_all_in` on `~/.local/share/applications` (wine-protocol-*, wine-extension-* and wine/**).
pub fn rewrite_all(prefix: &Path, appimage: &Path) -> anyhow::Result<usize> {
    let apps = dirs::data_dir().ok_or_else(|| anyhow::anyhow!("no XDG data directory"))?.join("applications");
    rewrite_all_in(&apps, prefix, appimage)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRE: &str = "/home/u/.local/share/studiowine/prefix";

    // Formats written by this Wine's winemenubuilder.c (lines 1276 and 2041).
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
