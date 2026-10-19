//! Step 1: create the Wine prefix (Wine Mono from the bundle, no Gecko, Windows 10).
use super::{Ctx, Progress, Step, StepId};
use crate::registry::{read_value, RegFile, RegValue};
use crate::wine::run_logged;

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
    }
}
