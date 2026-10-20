//! Which JACK library WineASIO will load (it `dlopen`s `libjack.so.0` from the host).
use std::path::{Path, PathBuf};

/// Where distributions put PipeWire's JACK library outside the default linker path
/// (Debian/Ubuntu `pipewire-jack`, Fedora's `pipewire-jack-audio-connection-kit-libs`).
const PIPEWIRE_JACK_DIRS: [&str; 3] =
    ["usr/lib/x86_64-linux-gnu/pipewire-0.3/jack", "usr/lib64/pipewire-0.3/jack", "usr/lib/pipewire-0.3/jack"];
const SYSTEM_LIB_DIRS: [&str; 4] = ["usr/lib/x86_64-linux-gnu", "usr/lib64", "usr/lib", "lib/x86_64-linux-gnu"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Jack {
    /// PipeWire's libjack; `extra_dir` must be put on LD_LIBRARY_PATH (what `pw-jack` does).
    PipeWire { extra_dir: Option<PathBuf> },
    /// Only JACK2's libjack: works while a `jackd` server runs.
    Jack2 { pipewire_running: bool },
    Missing { pipewire_running: bool },
}

impl Jack {
    /// `root` is `/` outside tests; `runtime_dir` is `$XDG_RUNTIME_DIR`.
    pub fn detect_in(root: &Path, runtime_dir: Option<&Path>) -> Jack {
        let pipewire_running = runtime_dir.is_some_and(|d| d.join("pipewire-0").exists());
        if pipewire_running {
            if let Some(d) = PIPEWIRE_JACK_DIRS.iter().map(|d| root.join(d)).find(|d| d.join("libjack.so.0").exists()) {
                return Jack::PipeWire { extra_dir: Some(d) };
            }
        }
        let Some(lib) = SYSTEM_LIB_DIRS.iter().map(|d| root.join(d).join("libjack.so.0")).find(|l| l.exists()) else {
            return Jack::Missing { pipewire_running };
        };
        // JACK2 installs libjack.so.0.1.0; PipeWire's carries its own version (libjack.so.0.3.x).
        let real = std::fs::canonicalize(&lib).unwrap_or(lib);
        let jack2 = real.file_name().is_some_and(|n| n.to_string_lossy().starts_with("libjack.so.0.1."));
        if jack2 {
            Jack::Jack2 { pipewire_running }
        } else {
            Jack::PipeWire { extra_dir: None }
        }
    }

    pub fn detect() -> Jack {
        Jack::detect_in(Path::new("/"), std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).as_deref())
    }

    pub fn extra_lib_dir(&self) -> Option<&Path> {
        match self {
            Jack::PipeWire { extra_dir } => extra_dir.as_deref(),
            _ => None,
        }
    }

    /// One line for the user: whether WineASIO can work, and what to install otherwise.
    pub fn summary(&self) -> (&'static str, bool) {
        match self {
            Jack::PipeWire { .. } => ("WineASIO: PipeWire JACK found", true),
            Jack::Jack2 { pipewire_running: true } => {
                ("WineASIO: install pipewire-jack (only JACK2 is installed, and no jackd runs under PipeWire)", false)
            }
            Jack::Jack2 { pipewire_running: false } => ("WineASIO: start the JACK server (jackd) before Cubase", true),
            Jack::Missing { pipewire_running: true } => ("WineASIO: install pipewire-jack", false),
            Jack::Missing { pipewire_running: false } => ("WineASIO: install pipewire-jack or JACK", false),
        }
    }
}

/// ASIO buffer sizes offered in Settings (frames; 256 ≈ 5 ms at 48 kHz).
pub const BUFFER_SIZES: [u32; 6] = [64, 128, 256, 512, 1024, 2048];
pub const DEFAULT_BUFFER_SIZE: u32 = 256;

/// WineASIO's settings: let the ASIO host (Cubase) set JACK's buffer size, starting at `frames`.
pub fn buffer_settings(frames: u32) -> crate::registry::RegFile {
    use crate::registry::{RegFile, RegValue};
    let key = r"HKEY_CURRENT_USER\Software\Wine\WineASIO";
    let mut reg = RegFile::new();
    reg.set(key, "Fixed buffersize", RegValue::Dword(0));
    reg.set(key, "Preferred buffersize", RegValue::Dword(frames));
    reg
}

/// The preferred buffer size stored in the prefix, if WineASIO has been set up.
pub fn current_buffer_size(prefix: &Path) -> Option<u32> {
    let v = crate::registry::read_value(&prefix.join("user.reg"), r"Software\Wine\WineASIO", "Preferred buffersize")?;
    u32::from_str_radix(v.strip_prefix("dword:")?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn debian_pipewire_jack_needs_extra_dir() {
        let d = tempfile::tempdir().unwrap();
        let (root, run) = (d.path().join("root"), d.path().join("run"));
        touch(&run.join("pipewire-0"));
        touch(&root.join("usr/lib/x86_64-linux-gnu/libjack.so.0.1.0"));
        std::os::unix::fs::symlink("libjack.so.0.1.0", root.join("usr/lib/x86_64-linux-gnu/libjack.so.0")).unwrap();
        assert_eq!(Jack::detect_in(&root, Some(&run)), Jack::Jack2 { pipewire_running: true }, "JACK2 only (this host today)");
        touch(&root.join("usr/lib/x86_64-linux-gnu/pipewire-0.3/jack/libjack.so.0"));
        let j = Jack::detect_in(&root, Some(&run));
        assert_eq!(j.extra_lib_dir(), Some(root.join("usr/lib/x86_64-linux-gnu/pipewire-0.3/jack").as_path()));
        assert!(j.summary().1);
    }

    #[test]
    fn system_pipewire_jack_and_missing() {
        let d = tempfile::tempdir().unwrap();
        let (root, run) = (d.path().join("root"), d.path().join("run"));
        std::fs::create_dir_all(&run).unwrap();
        assert_eq!(Jack::detect_in(&root, Some(&run)), Jack::Missing { pipewire_running: false });
        touch(&root.join("usr/lib/libjack.so.0.3.1400"));
        std::os::unix::fs::symlink("libjack.so.0.3.1400", root.join("usr/lib/libjack.so.0")).unwrap();
        assert_eq!(Jack::detect_in(&root, Some(&run)), Jack::PipeWire { extra_dir: None }, "Arch: pipewire-jack is the system libjack");
    }

    #[test]
    fn buffer_size_roundtrip_through_wine_registry_format() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(current_buffer_size(d.path()), None);
        std::fs::write(
            d.path().join("user.reg"),
            "WINE REGISTRY Version 2\n\n[Software\\\\Wine\\\\WineASIO] 1792441375\n\"Fixed buffersize\"=dword:00000000\n\"Preferred buffersize\"=dword:00000100\n",
        )
        .unwrap();
        assert_eq!(current_buffer_size(d.path()), Some(256));
        let text = String::from_utf16_lossy(
            &buffer_settings(128).to_regedit_bytes()[2..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>(),
        );
        assert!(text.contains("\"Preferred buffersize\"=dword:00000080"), "{text}");
        assert!(text.contains("\"Fixed buffersize\"=dword:00000000"), "{text}");
    }
}
