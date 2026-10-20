//! Step: register the bundled WineASIO driver (ASIO → JACK/PipeWire, low latency).
use super::{Ctx, Progress, Step, StepId};
use crate::registry::read_value;

const DLL: &str = "wineasio64.dll";

pub struct WineAsioStep;

impl Step for WineAsioStep {
    fn id(&self) -> StepId {
        StepId::WineAsio
    }
    fn title(&self) -> &'static str {
        "Install WineASIO driver (low-latency audio)"
    }
    fn is_done(&self, ctx: &Ctx) -> bool {
        let pre = &ctx.paths.prefix;
        pre.join("drive_c/windows/system32").join(DLL).is_file()
            && read_value(&pre.join("system.reg"), r"Software\ASIO\WineASIO", "CLSID").is_some()
    }
    fn run(&self, ctx: &Ctx, p: &Progress) -> anyhow::Result<()> {
        // The PE file is Wine's placeholder for the ELF driver in x86_64-unix; programs that
        // look for the DLL on disk (as ASIO hosts do) find it in system32, like on Windows.
        let src = ctx.paths.appdir.join("wine/lib/wine/x86_64-windows").join(DLL);
        anyhow::ensure!(src.is_file(), "this StudioWine build has no WineASIO ({} missing)", src.display());
        p.status(self.id(), "Registering the WineASIO driver…");
        std::fs::copy(&src, ctx.paths.prefix.join("drive_c/windows/system32").join(DLL))?;
        let st = ctx.wine.run_wait(["regsvr32", "/s", DLL], &ctx.log)?;
        anyhow::ensure!(st.success(), "regsvr32 {DLL} failed ({st})");
        p.fraction(self.id(), 0.7);
        crate::audio::buffer_settings(crate::audio::DEFAULT_BUFFER_SIZE).import(&ctx.wine, &ctx.log, &ctx.paths.cache)?;
        let (jack, _) = crate::audio::Jack::detect().summary();
        ctx.log.line(jack);
        // Writes the registry to disk, where the check reads it.
        ctx.wine.kill_server()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::LogSink;
    use crate::wine::testutil::fake_appdir;

    /// regsvr32 writes the driver key like WineASIO's DllRegisterServer; regedit is accepted.
    const FAKE: &str = r#"
echo "$*" >> "$WINEPREFIX/../calls.log"
case "$1" in
  regsvr32) printf '[Software\\\\ASIO\\\\WineASIO] 1\n"CLSID"="{48D0C522-BFCC-45CC-8B84-17F25F33E6E8}"\n' >> "$WINEPREFIX/system.reg" ;;
esac
"#;

    #[test]
    fn copies_dll_registers_and_sets_buffer() {
        let d = tempfile::tempdir().unwrap();
        let paths = fake_appdir(d.path(), FAKE);
        let ctx = Ctx::new(paths.clone(), LogSink::open(&paths.logs, "t.log").unwrap());
        std::fs::create_dir_all(paths.prefix.join("drive_c/windows/system32")).unwrap();
        std::fs::write(paths.prefix.join("system.reg"), "WINE REGISTRY Version 2\n").unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        assert!(WineAsioStep.run(&ctx, &Progress::new(tx.clone())).unwrap_err().to_string().contains("no WineASIO"));

        let dlls = paths.appdir.join("wine/lib/wine/x86_64-windows");
        std::fs::create_dir_all(&dlls).unwrap();
        std::fs::write(dlls.join(DLL), b"MZ placeholder").unwrap();
        assert!(!WineAsioStep.is_done(&ctx));
        WineAsioStep.run(&ctx, &Progress::new(tx)).unwrap();
        assert!(WineAsioStep.is_done(&ctx));
        let calls = std::fs::read_to_string(paths.data.join("calls.log")).unwrap();
        assert!(calls.lines().any(|l| l == "regsvr32 /s wineasio64.dll"), "{calls}");
        assert!(calls.lines().any(|l| l.starts_with("regedit /S ")), "{calls}");
    }
}
