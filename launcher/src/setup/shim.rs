//! Step 4: System.Management.Automation shim in Wine Mono's GAC, so Steinberg's
//! Setup.exe can run its PowerShell pre-install scripts (giang17/wine scripts/mono-sma-shim).
use super::{Ctx, Progress, Step, StepId};
use std::path::{Path, PathBuf};

const GAC_SUBDIR: &str = "System.Management.Automation/3.0.0.0__31bf3856ad364e35/System.Management.Automation.dll";

/// Where the shim goes: the first `gac` directory ≤ 4 levels below drive_c/windows/mono.
pub fn gac_target(prefix: &Path) -> Option<PathBuf> {
    fn find(dir: &Path, depth: u32) -> Option<PathBuf> {
        let mut subdirs: Vec<PathBuf> =
            std::fs::read_dir(dir).ok()?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subdirs.sort();
        if let Some(g) = subdirs.iter().find(|p| p.file_name().is_some_and(|n| n == "gac")) {
            return Some(g.clone());
        }
        if depth == 0 {
            return None;
        }
        subdirs.iter().find_map(|d| find(d, depth - 1))
    }
    find(&prefix.join("drive_c/windows/mono"), 3).map(|g| g.join(GAC_SUBDIR))
}

/// The probe script prints `Write-Host "gac-probe"; EXIT 42` through smahost.exe.
pub fn probe_ok(out: &str) -> bool {
    out.contains("INFO:   gac-probe") && out.contains("EXIT:   42")
}

/// Runs `cmd`, killing it after `limit`; returns stdout.
fn output_with_timeout(mut cmd: std::process::Command, limit: std::time::Duration) -> anyhow::Result<String> {
    use std::io::Read;
    let mut child = cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn()?;
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let start = std::time::Instant::now();
    while child.try_wait()?.is_none() {
        if start.elapsed() > limit {
            let _ = child.kill();
            anyhow::bail!("the PowerShell probe did not finish within {} s", limit.as_secs());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Ok(reader.join().unwrap_or_default())
}

pub struct ShimStep;

impl Step for ShimStep {
    fn id(&self) -> StepId {
        StepId::Shim
    }
    fn title(&self) -> &'static str {
        "Install PowerShell support for installers"
    }
    fn is_done(&self, ctx: &Ctx) -> bool {
        let bundled = std::fs::read(ctx.paths.shim_dir.join("System.Management.Automation.dll")).ok();
        let installed = gac_target(&ctx.paths.prefix).and_then(|t| std::fs::read(t).ok());
        bundled.is_some() && bundled == installed
    }
    fn run(&self, ctx: &Ctx, p: &Progress) -> anyhow::Result<()> {
        p.status(self.id(), "Installing PowerShell support…");
        let target = gac_target(&ctx.paths.prefix)
            .ok_or_else(|| anyhow::anyhow!("Wine Mono is not installed in the Wine environment (no GAC found)"))?;
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::copy(ctx.paths.shim_dir.join("System.Management.Automation.dll"), &target)?;
        p.status(self.id(), "Checking PowerShell support…");
        let probe_dir = ctx.paths.cache.join("sma-probe");
        std::fs::create_dir_all(&probe_dir)?;
        std::fs::copy(ctx.paths.shim_dir.join("smahost.exe"), probe_dir.join("smahost.exe"))?;
        let mut cmd = ctx.wine.command(["smahost.exe", "-c", r#"Write-Host "gac-probe"; EXIT 42"#]);
        cmd.current_dir(&probe_dir);
        let out = output_with_timeout(cmd, std::time::Duration::from_secs(120))?;
        ctx.log.line(&out);
        anyhow::ensure!(probe_ok(&out), "the PowerShell support check failed; output:\n{out}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gac_target_found() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(gac_target(d.path()), None);
        let gac = d.path().join("drive_c/windows/mono/mono-2.0/lib/mono/gac");
        std::fs::create_dir_all(&gac).unwrap();
        assert_eq!(gac_target(d.path()), Some(gac.join(GAC_SUBDIR)));
    }

    #[test]
    fn probe_output_parsing() {
        assert!(probe_ok("INFO:   gac-probe\r\nEXIT:   42\r\n"));
        assert!(!probe_ok("INFO:   gac-probe\nEXIT:   0\n"));
        assert!(!probe_ok("Unhandled Exception: System.IO.FileNotFoundException"));
    }
}
