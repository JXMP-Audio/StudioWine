//! Menu entries and URL handlers written by winemenubuilder call plain `wine`; point the
//! ones belonging to our prefix at the AppImage so sign-in links reach the right Wine.
use std::path::Path;

/// For an `Exec=` line of our prefix that calls `wine` (or an earlier AppImage with
/// `--wine`), the same line calling `"<appimage>" --wine`. None for any other line.
pub fn rewrite_exec(line: &str, prefix: &Path, appimage: &Path) -> Option<String> {
    if !line.starts_with("Exec=") {
        return None;
    }
    let needle = format!("WINEPREFIX=\"{}\"", prefix.display());
    let head_end = line.find(&needle)? + needle.len();
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
                    n.starts_with("wine-protocol-") && n.ends_with(".desktop")
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

/// `rewrite_all_in` on `~/.local/share/applications` (wine-protocol-*.desktop and wine/**).
pub fn rewrite_all(prefix: &Path, appimage: &Path) -> anyhow::Result<usize> {
    let apps = dirs::data_dir().ok_or_else(|| anyhow::anyhow!("no XDG data directory"))?.join("applications");
    rewrite_all_in(&apps, prefix, appimage)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRE: &str = "/home/u/.local/share/studiowine/prefix";

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
