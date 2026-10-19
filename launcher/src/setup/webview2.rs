//! Step 3: Microsoft Edge WebView2 runtime (downloaded from Microsoft; not redistributable).
use super::{Ctx, Progress, Step, StepId};
use std::path::{Path, PathBuf};

pub const WEBVIEW2_URL: &str = "https://go.microsoft.com/fwlink/?linkid=2124701";

/// `drive_c/Program Files (x86)/Microsoft/EdgeWebView/Application/<version>/msedgewebview2.exe`
pub fn find_webview2(prefix: &Path) -> Option<PathBuf> {
    let app = prefix.join("drive_c/Program Files (x86)/Microsoft/EdgeWebView/Application");
    let mut found: Vec<PathBuf> = std::fs::read_dir(app)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path().join("msedgewebview2.exe"))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.pop()
}

pub struct WebView2Step;

impl Step for WebView2Step {
    fn id(&self) -> StepId {
        StepId::WebView2
    }
    fn title(&self) -> &'static str {
        "Install Microsoft WebView2 runtime"
    }
    fn is_done(&self, ctx: &Ctx) -> bool {
        find_webview2(&ctx.paths.prefix).is_some()
    }
    fn run(&self, ctx: &Ctx, p: &Progress) -> anyhow::Result<()> {
        let id = self.id();
        p.status(id, "Downloading the WebView2 runtime from Microsoft…");
        let exe = ctx.http.download_exe(WEBVIEW2_URL, &ctx.paths.cache, &|done, total| {
            if let Some(t) = total.filter(|t| *t > 0) {
                p.fraction(id, 0.7 * done as f32 / t as f32);
            }
        })?;
        p.status(id, "Installing the WebView2 runtime…");
        let st = ctx.wine.run_wait([exe.as_os_str(), "/silent".as_ref(), "/install".as_ref()], &ctx.log)?;
        ctx.log.line(&format!("WebView2 installer finished: {st}"));
        ctx.wine.kill_server()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_versioned_exe() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(find_webview2(d.path()), None);
        let app = d.path().join("drive_c/Program Files (x86)/Microsoft/EdgeWebView/Application");
        std::fs::create_dir_all(app.join("SetupMetrics")).unwrap();
        std::fs::create_dir_all(app.join("154.0.3400.12")).unwrap();
        std::fs::write(app.join("154.0.3400.12/msedgewebview2.exe"), b"MZ").unwrap();
        assert_eq!(find_webview2(d.path()), Some(app.join("154.0.3400.12/msedgewebview2.exe")));
    }
}
