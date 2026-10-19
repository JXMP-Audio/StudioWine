//! Step 2: bundled substitute fonts + registry (FontLink, substitutes, text rendering).
use super::{Ctx, Progress, Step, StepId};
use crate::fonts;
use crate::registry::read_value;

pub struct FontsStep;

impl Step for FontsStep {
    fn id(&self) -> StepId {
        StepId::Fonts
    }
    fn title(&self) -> &'static str {
        "Install fonts"
    }
    fn is_done(&self, ctx: &Ctx) -> bool {
        let no_missing = fonts::family_status(&ctx.paths)
            .is_ok_and(|s| s.iter().all(|f| f.status != fonts::FontStatus::Missing));
        no_missing
            && read_value(&ctx.paths.prefix.join("user.reg"), r"Software\Wine\Direct2D", "text_linear_blend").is_some()
    }
    fn run(&self, ctx: &Ctx, p: &Progress) -> anyhow::Result<()> {
        p.status(self.id(), "Copying fonts…");
        let n = fonts::install_bundled(&ctx.paths)?.len();
        ctx.log.line(&format!("{n} font files copied"));
        p.fraction(self.id(), 0.5);
        p.status(self.id(), "Registering fonts and text rendering settings…");
        fonts::registry_set(&ctx.paths, fonts::prefix_dpi(&ctx.paths))?.import(&ctx.wine, &ctx.log, &ctx.paths.cache)?;
        ctx.wine.kill_server()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::LogSink;
    use crate::wine::testutil::fake_appdir;
    use std::path::Path;

    const FAKE: &str = r#"
case "$1" in
  regedit) f=$(echo "$3" | sed -e 's/^Z://' -e 's#\\#/#g'); if iconv -f UTF-16 -t UTF-8 "$f" | grep -q text_linear_blend; then printf '[Software\\\\Wine\\\\Direct2D] 1\n"text_linear_blend"=dword:00000001\n' >> "$WINEPREFIX/user.reg"; fi ;;
esac
"#;

    #[test]
    fn fonts_step_installs_all_and_verifies() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../out/fonts");
        if !src.join("manifest.json").is_file() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let p = fake_appdir(d.path(), FAKE);
        std::os::unix::fs::symlink(&src, &p.fonts_src).unwrap();
        std::fs::create_dir_all(&p.prefix).unwrap();
        let ctx = Ctx::new(p.clone(), LogSink::open(&p.logs, "t.log").unwrap());
        assert!(!FontsStep.is_done(&ctx));
        let (tx, _rx) = std::sync::mpsc::channel();
        FontsStep.run(&ctx, &Progress::new(tx)).unwrap();
        assert!(p.prefix.join("drive_c/windows/Fonts/segoeui.ttf").is_file());
        assert!(FontsStep.is_done(&ctx));
    }
}
