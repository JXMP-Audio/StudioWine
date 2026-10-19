//! Icons for the shortcut grid, taken from the target program's PE resources.
use std::path::Path;

/// The program's largest icon, scaled to `size`×`size` RGBA.
pub fn icon_rgba(exe: &Path, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    let data = std::fs::read(exe).ok()?;
    let pe = pelite::PeFile::from_bytes(&data).ok()?;
    let (_, group) = pe.resources().ok()?.icons().find_map(Result::ok)?;
    let mut ico = Vec::new();
    group.write(&mut ico).ok()?;
    let img = image::load_from_memory_with_format(&ico, image::ImageFormat::Ico).ok()?;
    let rgba = image::imageops::resize(&img.to_rgba8(), size, size, image::imageops::FilterType::Lanczos3);
    Some((size, size, rgba.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_rgba_reads_pe_icon() {
        let exe = Path::new(env!("CARGO_MANIFEST_DIR")).join("../out/wine/lib/wine/x86_64-windows/notepad.exe");
        if !exe.is_file() {
            return;
        }
        let (w, h, px) = icon_rgba(&exe, 64).expect("notepad has an icon");
        assert_eq!((w, h), (64, 64));
        assert_eq!(px.len(), 64 * 64 * 4);
        assert!(px.chunks(4).any(|p| p[3] > 0), "not fully transparent");
    }

    #[test]
    fn non_pe_gives_none() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("x.exe");
        std::fs::write(&f, b"MZ nonsense").unwrap();
        assert_eq!(icon_rgba(&f, 64), None);
        assert_eq!(icon_rgba(&d.path().join("missing.exe"), 64), None);
    }
}
