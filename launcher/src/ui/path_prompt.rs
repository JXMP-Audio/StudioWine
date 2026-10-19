//! Path entry used by "Run .exe…" and the font import. Works without an xdg desktop
//! portal: the path can be typed, pasted, dropped onto the window or picked with Browse….
use super::StudioWineApp;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    RunProgram,
    ImportFonts,
}

pub struct Prompt {
    pub purpose: Purpose,
    pub text: String,
    error: Option<String>,
    browse: Option<Receiver<Option<PathBuf>>>,
    focused: bool,
}

impl Prompt {
    pub fn new(purpose: Purpose, text: String) -> Prompt {
        Prompt { purpose, text, error: None, browse: None, focused: false }
    }
    fn kind(&self) -> PathKind {
        match self.purpose {
            Purpose::RunProgram => PathKind::Program,
            Purpose::ImportFonts => PathKind::Folder,
        }
    }
}

impl StudioWineApp {
    /// Files dropped onto the window: a program opens the Run prompt, a folder fills an open font prompt.
    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        let Some(p) = dropped.into_iter().next() else { return };
        let text = p.to_string_lossy().into_owned();
        match &mut self.prompt {
            Some(pr) if pr.purpose == Purpose::ImportFonts && p.is_dir() => pr.text = text,
            _ if p.is_file() => self.prompt = Some(Prompt::new(Purpose::RunProgram, text)),
            _ => {}
        }
    }

    pub(super) fn prompt_window(&mut self, ctx: &egui::Context) {
        let Some(pr) = &mut self.prompt else { return };
        if let Some(rx) = &pr.browse {
            if let Ok(r) = rx.try_recv() {
                if let Some(p) = r {
                    pr.text = p.to_string_lossy().into_owned();
                    pr.error = None;
                }
                pr.browse = None;
            }
        }
        let (title, hint, action) = match pr.purpose {
            Purpose::RunProgram => ("Run a Windows program", "Path to an installer or program (.exe, .msi) — or drop the file on this window", "Run"),
            Purpose::ImportFonts => ("Use fonts from folder", "Folder with Windows font files, e.g. a mounted C:\\Windows\\Fonts", "Import"),
        };
        let (mut open, mut go, mut browse) = (true, false, false);
        egui::Window::new(title).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(hint);
            if pr.purpose == Purpose::ImportFonts {
                // Registering fonts restarts the wineserver, which ends every Windows program.
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "Importing closes all running Windows programs (Cubase, plug-in hosts…). Save your work first.",
                );
            }
            let r = ui.add(egui::TextEdit::singleline(&mut pr.text).desired_width(460.0));
            if !pr.focused {
                r.request_focus();
                pr.focused = true;
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
            if let Some(e) = &pr.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(pr.browse.is_none(), egui::Button::new("Browse…")).clicked() {
                    browse = true;
                }
                if ui.button(action).clicked() {
                    go = true;
                }
            });
        });
        if browse {
            let (tx, rx) = channel();
            let purpose = pr.purpose;
            std::thread::spawn(move || {
                let d = rfd::FileDialog::new();
                let r = match purpose {
                    Purpose::RunProgram => d.add_filter("Windows programs", &["exe", "msi", "EXE", "MSI"]).pick_file(),
                    Purpose::ImportFonts => d.pick_folder(),
                };
                let _ = tx.send(r);
            });
            pr.browse = Some(rx);
        }
        if !open {
            self.prompt = None;
            return;
        }
        if go {
            let home = dirs::home_dir().unwrap_or_default();
            match parse_user_path(&pr.text, pr.kind(), &home) {
                Err(e) => pr.error = Some(e),
                Ok(path) => {
                    let purpose = pr.purpose;
                    self.prompt = None;
                    match purpose {
                        Purpose::RunProgram => self.run_program(path),
                        Purpose::ImportFonts => self.import_fonts(path),
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// A Windows program or installer (.exe, .msi).
    Program,
    /// A directory (fonts folder).
    Folder,
}

/// Turns what the user typed, pasted or dropped into an existing path of the right kind.
/// Accepts surrounding quotes, `~/…` and `file://` URIs (percent-encoded).
pub fn parse_user_path(text: &str, kind: PathKind, home: &Path) -> Result<PathBuf, String> {
    let t = text.trim().trim_matches(|c| c == '"' || c == '\'');
    if t.is_empty() {
        return Err("Enter a path first.".into());
    }
    let path = if let Some(uri) = t.strip_prefix("file://") {
        PathBuf::from(percent_decode(uri))
    } else if let Some(rest) = t.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(t)
    };
    if !path.exists() {
        return Err(format!("{} not found", path.display()));
    }
    match kind {
        PathKind::Folder if !path.is_dir() => Err(format!("{} is not a folder", path.display())),
        PathKind::Program
            if !path.is_file()
                || !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("msi")) =>
        {
            Err(format!("{} is not a Windows program (.exe or .msi)", path.display()))
        }
        _ => Ok(path),
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_quoted_tilde_and_uri() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("Setup Kontakt 8.exe");
        std::fs::write(&f, b"MZ").unwrap();
        let home = d.path();
        assert_eq!(parse_user_path(&f.to_string_lossy(), PathKind::Program, home), Ok(f.clone()));
        assert_eq!(parse_user_path(&format!("  '{}'\n", f.display()), PathKind::Program, home), Ok(f.clone()));
        assert_eq!(parse_user_path("~/Setup Kontakt 8.exe", PathKind::Program, home), Ok(f.clone()));
        let uri = format!("file://{}", f.display()).replace(' ', "%20");
        assert_eq!(parse_user_path(&uri, PathKind::Program, home), Ok(f));
    }

    #[test]
    fn rejects_missing_wrong_type_and_empty() {
        let d = tempfile::tempdir().unwrap();
        let txt = d.path().join("notes.txt");
        std::fs::write(&txt, b"x").unwrap();
        let msi = d.path().join("a.MSI");
        std::fs::write(&msi, b"x").unwrap();
        assert!(parse_user_path("", PathKind::Program, d.path()).is_err());
        assert!(parse_user_path("/nonexistent/x.exe", PathKind::Program, d.path()).unwrap_err().contains("not found"));
        assert!(parse_user_path(&txt.to_string_lossy(), PathKind::Program, d.path()).unwrap_err().contains(".exe"));
        assert_eq!(parse_user_path(&msi.to_string_lossy(), PathKind::Program, d.path()), Ok(msi));
        assert!(parse_user_path(&txt.to_string_lossy(), PathKind::Folder, d.path()).unwrap_err().contains("folder"));
        assert_eq!(parse_user_path(&d.path().to_string_lossy(), PathKind::Folder, d.path()), Ok(d.path().to_path_buf()));
    }
}
