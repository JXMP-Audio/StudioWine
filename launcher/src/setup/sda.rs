//! Step 5: Steinberg Download Assistant, from the link on Steinberg's download page.
use super::{Ctx, Progress, Step, StepId};
use std::path::{Path, PathBuf};

pub const SDA_PAGE: &str = "https://o.steinberg.net/en/support/downloads/steinberg_download_assistant.html";
pub const SDA_PINNED: &str = "https://www.steinberg.net/sda-win";

/// The Windows download link on the page: `…/sda-win`, or an absolute .exe link with
/// "Download_Assistant" and "win" in it.
pub fn find_sda_link(html: &str) -> Option<String> {
    let hrefs = html.split("href=\"").skip(1).filter_map(|s| s.split('"').next());
    hrefs.into_iter().find(|h| {
        let l = h.to_lowercase();
        let pinned = l.starts_with("https://www.steinberg.net/sda-win") && l[33..].chars().next().is_none_or(|c| !c.is_alphanumeric() && c != '-');
        let direct = l.starts_with("https://") && l.ends_with(".exe") && l.contains("download_assistant") && l.contains("win");
        pinned || direct
    }).map(str::to_string)
}

pub fn choose_sda_url(found: Option<String>) -> String {
    found.unwrap_or_else(|| SDA_PINNED.to_string())
}

/// `Steinberg Download Assistant.exe` under drive_c/Program Files*/Steinberg (case-insensitive, depth ≤ 4).
pub fn find_sda_exe(prefix: &Path) -> Option<PathBuf> {
    fn walk(dir: &Path, depth: u32) -> Option<PathBuf> {
        let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.filter_map(Result::ok).map(|e| e.path()).collect();
        entries.sort();
        for e in &entries {
            if e.is_file() && e.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("Steinberg Download Assistant.exe")) {
                return Some(e.clone());
            }
        }
        if depth == 0 {
            return None;
        }
        entries.iter().filter(|e| e.is_dir()).find_map(|e| walk(e, depth - 1))
    }
    ["Program Files", "Program Files (x86)"]
        .iter()
        .find_map(|pf| walk(&prefix.join("drive_c").join(pf).join("Steinberg"), 3))
}

pub struct SdaStep;

impl Step for SdaStep {
    fn id(&self) -> StepId {
        StepId::DownloadAssistant
    }
    fn title(&self) -> &'static str {
        "Install Steinberg Download Assistant"
    }
    fn is_done(&self, ctx: &Ctx) -> bool {
        find_sda_exe(&ctx.paths.prefix).is_some()
    }
    fn run(&self, ctx: &Ctx, p: &Progress) -> anyhow::Result<()> {
        let id = self.id();
        p.status(id, "Looking up the Steinberg Download Assistant…");
        let found = match ctx.http.get_text(SDA_PAGE) {
            Ok(html) => find_sda_link(&html),
            Err(e) => {
                ctx.log.line(&format!("download page not readable, using the pinned link: {e:#}"));
                None
            }
        };
        let url = choose_sda_url(found);
        let final_url = ctx.http.resolve_final_url(&url)?;
        let path_part = final_url.split(['?', '#']).next().unwrap_or(&final_url).to_lowercase();
        anyhow::ensure!(path_part.ends_with(".exe"), "Steinberg's download link no longer points to a Windows installer: {final_url}");
        p.status(id, "Downloading the Steinberg Download Assistant…");
        let exe = ctx.http.download_exe(&url, &ctx.paths.cache, &|done, total| {
            if let Some(t) = total.filter(|t| *t > 0) {
                p.fraction(id, 0.6 * done as f32 / t as f32);
            }
        })?;
        p.status(id, "Running the installer — follow its window…");
        let st = ctx.wine.run_wait([exe.as_os_str()], &ctx.log)?;
        ctx.log.line(&format!("Download Assistant installer finished: {st}"));
        p.status(id, "Registering sign-in links…");
        ctx.wine.run_wait(["winemenubuilder", "-a", "-r"], &ctx.log)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(n: &str) -> String {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(n)).unwrap()
    }

    #[test]
    fn finds_link_on_live_page_fixture() {
        assert_eq!(find_sda_link(&fixture("sda_page.html")).as_deref(), Some("https://www.steinberg.net/sda-win"));
    }

    #[test]
    fn finds_direct_exe_link() {
        let html = r#"<a href="https://download.steinberg.net/x/Steinberg_Download_Assistant_1.41.0_Installer_win.exe">x</a>"#;
        assert_eq!(
            find_sda_link(html).as_deref(),
            Some("https://download.steinberg.net/x/Steinberg_Download_Assistant_1.41.0_Installer_win.exe")
        );
    }

    #[test]
    fn page_without_link_uses_pinned() {
        assert_eq!(find_sda_link(&fixture("sda_page_nolink.html")), None);
        assert_eq!(choose_sda_url(None), SDA_PINNED);
        assert_eq!(choose_sda_url(Some("https://a/b.exe".into())), "https://a/b.exe");
    }

    #[test]
    fn find_sda_exe_case_insensitive() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(find_sda_exe(d.path()), None);
        let dir = d.path().join("drive_c/Program Files/Steinberg/Download Assistant");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("steinberg download assistant.EXE"), b"MZ").unwrap();
        assert_eq!(find_sda_exe(d.path()), Some(dir.join("steinberg download assistant.EXE")));
    }
}
