//! Fonts: bundled free substitutes, their registry entries, status and user import.
//! Registry values mirror giang17/wine `scripts/wine-font-setup.sh`.
use crate::paths::Paths;
use crate::registry::{read_value, RegFile, RegValue};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Clone, Debug)]
pub struct ManifestEntry {
    pub file: String,
    pub substitute_for: Option<String>,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontStatus {
    Real,
    Substitute,
    Missing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FamilyStatus {
    pub family: &'static str,
    pub status: FontStatus,
}

pub const REQUIRED_FAMILIES: &[&str] =
    &["Segoe UI", "Microsoft Sans Serif", "Arial", "Courier New", "Times New Roman", "Bravura", "Petaluma"];

/// Windows font files the Fonts… dialog copies from a user-supplied folder (case-insensitive).
pub const USER_IMPORT_PATTERNS: &[&str] = &[
    "segoeui*.ttf", "segui*.ttf", "micross.ttf", "arial*.ttf", "cour*.ttf", "times*.ttf", "tahoma*.ttf",
    "seguisym.ttf", "seguiemj.ttf", "segmdl2.ttf", "segoeicons.ttf",
];

pub struct ImportReport {
    pub copied: Vec<String>,
    pub skipped: Vec<String>,
}

pub fn fonts_dir(paths: &Paths) -> PathBuf {
    paths.prefix.join("drive_c/windows/Fonts")
}

pub fn load_manifest(paths: &Paths) -> anyhow::Result<Vec<ManifestEntry>> {
    let f = paths.fonts_src.join("manifest.json");
    let bytes = std::fs::read(&f).map_err(|e| anyhow::anyhow!("cannot read {}: {e}", f.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn sha256(path: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// The file in `dir` whose name equals `name` ignoring case.
fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

fn name_string(face: &ttf_parser::Face, id: u16) -> Option<String> {
    let names = face.names();
    let pick = |want_windows: bool| {
        names.into_iter().filter(|n| n.name_id == id && n.is_unicode()).find(|n| {
            (n.platform_id == ttf_parser::PlatformId::Windows) == want_windows
        })
    };
    pick(true).or_else(|| pick(false)).and_then(|n| n.to_string())
}

/// `"<family>[ <style>] (TrueType|OpenType)"`, the value name Windows uses in the Fonts key.
pub fn reg_name(font_file: &Path) -> anyhow::Result<String> {
    let data = std::fs::read(font_file)?;
    let face = ttf_parser::Face::parse(&data, 0).map_err(|e| anyhow::anyhow!("{}: {e}", font_file.display()))?;
    let family = name_string(&face, 16)
        .or_else(|| name_string(&face, 1))
        .ok_or_else(|| anyhow::anyhow!("{}: no family name", font_file.display()))?;
    let style = name_string(&face, 17).or_else(|| name_string(&face, 2)).unwrap_or_default();
    let kind = match font_file.extension().map(|e| e.to_ascii_lowercase()) {
        Some(e) if e == "otf" => "OpenType",
        _ => "TrueType",
    };
    Ok(if style.is_empty() || style == "Regular" {
        format!("{family} ({kind})")
    } else {
        format!("{family} {style} ({kind})")
    })
}

/// True when `file` in the prefix holds the bundled substitute bytes for its name.
fn is_bundled_substitute(manifest: &[ManifestEntry], file: &Path) -> bool {
    let name = file.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    manifest
        .iter()
        .find(|e| e.file.to_lowercase() == name)
        .is_some_and(|e| e.substitute_for.is_some() && sha256(file).as_deref() == Some(e.sha256.as_str()))
}

/// Copies the bundled fonts into the prefix. A file the user replaced with a genuine
/// font (different bytes) is kept.
pub fn install_bundled(paths: &Paths) -> anyhow::Result<Vec<PathBuf>> {
    let manifest = load_manifest(paths)?;
    let dir = fonts_dir(paths);
    std::fs::create_dir_all(&dir)?;
    let mut copied = Vec::new();
    for e in &manifest {
        if let Some(existing) = find_ci(&dir, &e.file) {
            if sha256(&existing).as_deref() != Some(e.sha256.as_str()) {
                continue; // user-supplied genuine font
            }
        }
        let dst = dir.join(&e.file);
        std::fs::copy(paths.fonts_src.join(&e.file), &dst)?;
        copied.push(dst);
    }
    let smufl = paths.fonts_src.join("smufl");
    if let Ok(rd) = std::fs::read_dir(&smufl) {
        let dest_root = paths.prefix.join("drive_c/Program Files/Common Files/SMuFL/Fonts");
        for font in rd.filter_map(Result::ok) {
            let dest = dest_root.join(font.file_name());
            std::fs::create_dir_all(&dest)?;
            for f in std::fs::read_dir(font.path())?.filter_map(Result::ok) {
                std::fs::copy(f.path(), dest.join(f.file_name()))?;
            }
        }
    }
    Ok(copied)
}

/// LOGFONTW for Segoe UI 9pt at `dpi` (92 bytes), as Windows 10 reports for the system fonts.
pub fn logfont_segoe_9pt(dpi: u32) -> Vec<u8> {
    let height = -(((9 * dpi + 36) / 72) as i32);
    let mut lf = Vec::with_capacity(92);
    lf.extend(height.to_le_bytes());
    lf.extend([0u8; 12]); // width, escapement, orientation
    lf.extend(400u32.to_le_bytes()); // weight
    lf.extend([0, 0, 0, 1]); // italic, underline, strikeout, DEFAULT_CHARSET
    lf.extend([0u8; 4]); // precision, clip precision, quality, pitch and family
    let mut face: Vec<u8> = "Segoe UI".encode_utf16().flat_map(u16::to_le_bytes).collect();
    face.resize(64, 0);
    lf.extend(face);
    lf
}

pub fn prefix_dpi(paths: &Paths) -> u32 {
    read_value(&paths.prefix.join("user.reg"), r"Control Panel\Desktop", "LogPixels")
        .and_then(|v| u32::from_str_radix(v.strip_prefix("dword:")?, 16).ok())
        .filter(|d| (48..=480).contains(d))
        .unwrap_or(96)
}

const HKLM_NT: &str = r"HKEY_LOCAL_MACHINE\Software\Microsoft\Windows NT\CurrentVersion";

/// Families the fork's script points at free stand-ins, with the genuine file that makes
/// the entry unnecessary.
const CORE_SUBSTITUTES: &[(&str, &str, &str)] = &[
    ("Arial", "Liberation Sans", "arial.ttf"),
    ("Times New Roman", "Liberation Serif", "times.ttf"),
    ("Courier New", "Liberation Mono", "cour.ttf"),
    ("Verdana", "DejaVu Sans", "verdana.ttf"),
    ("Georgia", "DejaVu Serif", "georgia.ttf"),
    ("Trebuchet MS", "DejaVu Sans", "trebuc.ttf"),
    ("Comic Sans MS", "DejaVu Sans", "comic.ttf"),
    ("Andale Mono", "DejaVu Sans Mono", "andalemo.ttf"),
    ("Impact", "DejaVu Sans", "impact.ttf"),
];

/// CJK UI families that JUCE asks for; this fork's dwrite resolves a miss through FontSubstitutes.
const CJK_TO_SEGOE: &[&str] = &["Malgun Gothic", "Meiryo", "Meiryo UI", "Microsoft JhengHei UI", "Microsoft YaHei UI", "Yu Gothic UI"];

const TAHOMA_LINK: &[&str] = &[
    "DejaVuSans.ttf,DejaVu Sans",
    "NotoSansSymbols2-Regular.ttf,Noto Sans Symbols2",
    "MSGOTHIC.TTC,MS UI Gothic",
    "MINGLIU.TTC,PMingLiU",
    "SIMSUN.TTC,SimSun",
    "GULIM.TTC,Gulim",
    "YUGOTHM.TTC,Yu Gothic UI",
    "MSJH.TTC,Microsoft JhengHei UI",
    "MSYH.TTC,Microsoft YaHei UI",
    "MALGUN.TTF,Malgun Gothic",
    "SEGUISYM.TTF,Segoe UI Symbol",
];

fn genuine_present(manifest: &[ManifestEntry], dir: &Path, file: &str) -> bool {
    find_ci(dir, file).is_some_and(|f| !is_bundled_substitute(manifest, &f))
}

pub fn registry_set(paths: &Paths, dpi: u32) -> anyhow::Result<RegFile> {
    let manifest = load_manifest(paths)?;
    let dir = fonts_dir(paths);
    let system_reg = paths.prefix.join("system.reg");
    let user_reg = paths.prefix.join("user.reg");
    let mut r = RegFile::new();

    let fonts_key = format!(r"{HKLM_NT}\Fonts");
    let mut files: Vec<_> = std::fs::read_dir(&dir)?.filter_map(Result::ok).map(|e| e.path()).collect();
    files.sort();
    for f in files {
        let ext = f.extension().map(|e| e.to_ascii_lowercase());
        if !matches!(ext.as_deref().and_then(|e| e.to_str()), Some("ttf" | "otf" | "ttc")) {
            continue;
        }
        if let Ok(name) = reg_name(&f) {
            r.set(&fonts_key, &name, RegValue::Sz(f.file_name().unwrap().to_string_lossy().into_owned()));
        }
    }

    let subst_key = format!(r"{HKLM_NT}\FontSubstitutes");
    for (family, standin, file) in CORE_SUBSTITUTES {
        let v = if genuine_present(&manifest, &dir, file) { RegValue::Delete } else { RegValue::Sz((*standin).into()) };
        r.set(&subst_key, family, v);
    }
    for family in CJK_TO_SEGOE {
        if read_value(&system_reg, r"Software\Microsoft\Windows NT\CurrentVersion\FontSubstitutes", family).is_none() {
            r.set(&subst_key, family, RegValue::Sz("Segoe UI".into()));
        }
    }

    let link_key = format!(r"{HKLM_NT}\FontLink\SystemLink");
    r.set(&link_key, "Tahoma", RegValue::MultiSz(TAHOMA_LINK.iter().map(|s| s.to_string()).collect()));
    r.set(&link_key, "DejaVu Sans", RegValue::MultiSz(vec!["NotoSansSymbols2-Regular.ttf,Noto Sans Symbols2".into()]));

    let d2d = r"HKEY_CURRENT_USER\Software\Wine\Direct2D";
    if read_value(&user_reg, r"Software\Wine\Direct2D", "text_enhanced_contrast").is_none() {
        r.set(d2d, "text_enhanced_contrast", RegValue::Dword(50));
    }
    for v in ["text_linear_blend", "text_grayscale_blend", "text_cleartype_blend"] {
        r.set(d2d, v, RegValue::Dword(1));
    }
    r.set(r"HKEY_CURRENT_USER\Software\Wine\DirectWrite", "outline_in_natural_modes", RegValue::Dword(1));

    let desktop = r"HKEY_CURRENT_USER\Control Panel\Desktop";
    r.set(desktop, "FontSmoothing", RegValue::Sz("2".into()))
        .set(desktop, "FontSmoothingType", RegValue::Dword(2))
        .set(desktop, "FontSmoothingOrientation", RegValue::Dword(1));
    let lf = logfont_segoe_9pt(dpi);
    for v in ["CaptionFont", "IconFont", "MenuFont", "MessageFont", "SmCaptionFont", "StatusFont"] {
        r.set(r"HKEY_CURRENT_USER\Control Panel\Desktop\WindowMetrics", v, RegValue::Binary(lf.clone()));
    }
    Ok(r)
}

const STATUS_FILES: &[(&str, &str)] = &[
    ("Segoe UI", "segoeui.ttf"),
    ("Microsoft Sans Serif", "micross.ttf"),
    ("Arial", "arial.ttf"),
    ("Courier New", "cour.ttf"),
    ("Times New Roman", "times.ttf"),
    ("Bravura", "bravura.otf"),
    ("Petaluma", "petaluma.otf"),
];

pub fn family_status(paths: &Paths) -> anyhow::Result<Vec<FamilyStatus>> {
    let manifest = load_manifest(paths).unwrap_or_default();
    let dir = fonts_dir(paths);
    Ok(REQUIRED_FAMILIES
        .iter()
        .map(|family| {
            let file = STATUS_FILES.iter().find(|(f, _)| f == family).map(|(_, n)| *n).unwrap_or("");
            let status = match find_ci(&dir, file) {
                None => FontStatus::Missing,
                Some(f) if is_bundled_substitute(&manifest, &f) => FontStatus::Substitute,
                Some(_) => FontStatus::Real,
            };
            FamilyStatus { family, status }
        })
        .collect())
}

fn matches_pattern(name: &str, pattern: &str) -> bool {
    let name = name.to_lowercase();
    match pattern.split_once('*') {
        Some((pre, post)) => name.len() >= pre.len() + post.len() && name.starts_with(pre) && name.ends_with(post),
        None => name == pattern,
    }
}

/// Copies the known Microsoft font files from `src` into the prefix under lower-case
/// names, removing differently-cased duplicates (dwrite fails on two spellings).
pub fn import_user_fonts(src: &Path, paths: &Paths) -> anyhow::Result<ImportReport> {
    let entries = std::fs::read_dir(src).map_err(|e| anyhow::anyhow!("cannot open {}: {e}", src.display()))?;
    let dir = fonts_dir(paths);
    std::fs::create_dir_all(&dir)?;
    let mut report = ImportReport { copied: Vec::new(), skipped: Vec::new() };
    for e in entries.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        if !e.path().is_file() || !USER_IMPORT_PATTERNS.iter().any(|p| matches_pattern(&name, p)) {
            report.skipped.push(name);
            continue;
        }
        let lower = name.to_lowercase();
        for old in std::fs::read_dir(&dir)?.filter_map(Result::ok) {
            let n = old.file_name().to_string_lossy().into_owned();
            if n.to_lowercase() == lower && n != lower {
                std::fs::remove_file(old.path())?;
            }
        }
        std::fs::copy(e.path(), dir.join(&lower))?;
        report.copied.push(lower);
    }
    report.copied.sort();
    report.skipped.sort();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wine::testutil::fake_appdir;

    fn built_fonts() -> Option<PathBuf> {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../out/fonts");
        d.join("manifest.json").is_file().then_some(d)
    }

    /// A fake appdir whose fonts/ holds a subset of the built font set (manifest filtered to it).
    fn setup(root: &Path, files: &[&str]) -> Option<Paths> {
        let src = built_fonts()?;
        let p = fake_appdir(root, "exit 0");
        std::fs::create_dir_all(&p.fonts_src).unwrap();
        let all: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(src.join("manifest.json")).unwrap()).unwrap();
        let keep: Vec<_> = all.into_iter().filter(|e| files.contains(&e["file"].as_str().unwrap())).collect();
        for e in &keep {
            let f = e["file"].as_str().unwrap();
            std::fs::copy(src.join(f), p.fonts_src.join(f)).unwrap();
        }
        std::fs::create_dir_all(p.fonts_src.join("smufl/Bravura")).unwrap();
        std::fs::write(p.fonts_src.join("smufl/Bravura/bravura_metadata.json"), "{}").unwrap();
        std::fs::write(p.fonts_src.join("manifest.json"), serde_json::to_vec(&keep).unwrap()).unwrap();
        std::fs::create_dir_all(&p.prefix).unwrap();
        Some(p)
    }

    fn reg_text(r: &RegFile) -> String {
        let b = r.to_regedit_bytes();
        String::from_utf16(&b[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn reg_name_uses_family_and_style() {
        let Some(src) = built_fonts() else { return };
        assert_eq!(reg_name(&src.join("DejaVuSans-Bold.ttf")).unwrap(), "DejaVu Sans Bold (TrueType)");
        assert_eq!(reg_name(&src.join("segoeui.ttf")).unwrap(), "Segoe UI (TrueType)");
        assert_eq!(reg_name(&src.join("seguisb.ttf")).unwrap(), "Segoe UI Semibold (TrueType)");
        assert_eq!(reg_name(&src.join("Bravura.otf")).unwrap(), "Bravura (OpenType)");
    }

    #[test]
    fn logfont_is_92_bytes_with_segoe_face() {
        let lf = logfont_segoe_9pt(96);
        assert_eq!(lf.len(), 92);
        assert_eq!(&lf[0..4], &[0xF4, 0xFF, 0xFF, 0xFF]);
        assert_eq!(&lf[16..20], &400u32.to_le_bytes());
        assert_eq!(lf[23], 1);
        let face: Vec<u16> = lf[28..44].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(String::from_utf16(&face).unwrap(), "Segoe UI");
        assert!(lf[44..].iter().all(|b| *b == 0));
        assert_eq!(&logfont_segoe_9pt(144)[0..4], &(-18i32).to_le_bytes());
    }

    #[test]
    fn prefix_dpi_reads_logpixels() {
        let d = tempfile::tempdir().unwrap();
        let p = fake_appdir(d.path(), "exit 0");
        assert_eq!(prefix_dpi(&p), 96);
        std::fs::create_dir_all(&p.prefix).unwrap();
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/user.reg"), p.prefix.join("user.reg")).unwrap();
        assert_eq!(prefix_dpi(&p), 120);
    }

    #[test]
    fn registry_set_has_fork_values() {
        let d = tempfile::tempdir().unwrap();
        let Some(p) = setup(d.path(), &["segoeui.ttf", "arial.ttf", "DejaVuSans.ttf"]) else { return };
        install_bundled(&p).unwrap();
        let t = reg_text(&registry_set(&p, 96).unwrap());
        for want in [
            "\"Segoe UI (TrueType)\"=\"segoeui.ttf\"",
            "\"Liberation Sans (TrueType)\"=\"arial.ttf\"",
            "\"Arial\"=\"Liberation Sans\"",
            "\"Courier New\"=\"Liberation Mono\"",
            "\"Yu Gothic UI\"=\"Segoe UI\"",
            "\"text_enhanced_contrast\"=dword:00000032",
            "\"text_linear_blend\"=dword:00000001",
            "\"text_grayscale_blend\"=dword:00000001",
            "\"text_cleartype_blend\"=dword:00000001",
            "\"outline_in_natural_modes\"=dword:00000001",
            "\"FontSmoothing\"=\"2\"",
            "\"FontSmoothingType\"=dword:00000002",
            "[HKEY_CURRENT_USER\\Control Panel\\Desktop\\WindowMetrics]",
            "\"MenuFont\"=hex:f4,ff,ff,ff",
            "[HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows NT\\CurrentVersion\\FontLink\\SystemLink]",
        ] {
            assert!(t.contains(want), "missing {want}\n{t}");
        }
        // Tahoma chain starts with DejaVu Sans, then Noto Sans Symbols2 (UTF-16LE hex of "DejaVuSans.ttf,DejaVu Sans").
        assert!(t.contains("\"Tahoma\"=hex(7):44,00,65,00,6a,00,61,00,56,00,75,00,53,00"), "{t}");
    }

    #[test]
    fn substitutes_only_when_genuine_absent() {
        let d = tempfile::tempdir().unwrap();
        let Some(p) = setup(d.path(), &["arial.ttf"]) else { return };
        install_bundled(&p).unwrap();
        // A "genuine" arial.ttf: any different font file under that name.
        std::fs::copy(built_fonts().unwrap().join("DejaVuSans.ttf"), fonts_dir(&p).join("arial.ttf")).unwrap();
        let t = reg_text(&registry_set(&p, 96).unwrap());
        assert!(t.contains("\"Arial\"=-"), "genuine Arial present → substitute removed\n{t}");
        assert!(t.contains("\"Times New Roman\"=\"Liberation Serif\""), "{t}");
    }

    #[test]
    fn install_keeps_user_real_font() {
        let d = tempfile::tempdir().unwrap();
        let Some(p) = setup(d.path(), &["segoeui.ttf"]) else { return };
        std::fs::create_dir_all(fonts_dir(&p)).unwrap();
        std::fs::write(fonts_dir(&p).join("segoeui.ttf"), b"genuine bytes").unwrap();
        install_bundled(&p).unwrap();
        assert_eq!(std::fs::read(fonts_dir(&p).join("segoeui.ttf")).unwrap(), b"genuine bytes");
        assert!(p.prefix.join("drive_c/Program Files/Common Files/SMuFL/Fonts/Bravura/bravura_metadata.json").is_file());
    }

    #[test]
    fn status_reports_real_vs_substitute() {
        let d = tempfile::tempdir().unwrap();
        let Some(p) = setup(d.path(), &["segoeui.ttf", "micross.ttf", "arial.ttf"]) else { return };
        install_bundled(&p).unwrap();
        std::fs::write(fonts_dir(&p).join("micross.ttf"), b"genuine").unwrap();
        let s = family_status(&p).unwrap();
        let get = |f: &str| s.iter().find(|x| x.family == f).unwrap().status;
        assert_eq!(get("Segoe UI"), FontStatus::Substitute);
        assert_eq!(get("Microsoft Sans Serif"), FontStatus::Real);
        assert_eq!(get("Arial"), FontStatus::Substitute);
        assert_eq!(get("Courier New"), FontStatus::Missing);
        assert_eq!(s.len(), REQUIRED_FAMILIES.len());
    }

    #[test]
    fn import_is_case_insensitive_and_dedups() {
        let d = tempfile::tempdir().unwrap();
        let p = fake_appdir(d.path(), "exit 0");
        let src = d.path().join("winfonts");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("SEGOEUI.TTF"), b"real segoe").unwrap();
        std::fs::create_dir_all(fonts_dir(&p)).unwrap();
        std::fs::write(fonts_dir(&p).join("Segoeui.ttf"), b"old").unwrap();
        let r = import_user_fonts(&src, &p).unwrap();
        assert_eq!(r.copied, vec!["segoeui.ttf"]);
        let names: Vec<_> = std::fs::read_dir(fonts_dir(&p)).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names, vec!["segoeui.ttf"]);
        assert_eq!(std::fs::read(fonts_dir(&p).join("segoeui.ttf")).unwrap(), b"real segoe");
    }

    #[test]
    fn import_ignores_unlisted_files() {
        let d = tempfile::tempdir().unwrap();
        let p = fake_appdir(d.path(), "exit 0");
        let src = d.path().join("winfonts");
        std::fs::create_dir_all(&src).unwrap();
        for f in ["notes.txt", "wingding.ttf", "micross.ttf"] {
            std::fs::write(src.join(f), b"x").unwrap();
        }
        let r = import_user_fonts(&src, &p).unwrap();
        assert_eq!(r.copied, vec!["micross.ttf"]);
        let mut sk = r.skipped.clone();
        sk.sort();
        assert_eq!(sk, vec!["notes.txt", "wingding.ttf"]);
        assert!(import_user_fonts(&d.path().join("nope"), &p).is_err());
    }
}
