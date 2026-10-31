//! Locations inside the AppImage (read-only) and in the user's data directory.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Paths {
    pub appdir: PathBuf,
    pub wine_bin: PathBuf,
    pub wineserver_bin: PathBuf,
    pub fonts_src: PathBuf,
    pub shim_dir: PathBuf,
    pub data: PathBuf,
    pub prefix: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub state_file: PathBuf,
    pub appimage: PathBuf,
}

impl Paths {
    pub fn from_roots(appdir: &Path, data: &Path, appimage: &Path) -> Paths {
        Paths {
            appdir: appdir.to_path_buf(),
            wine_bin: appdir.join("wine/bin/wine"),
            wineserver_bin: appdir.join("wine/bin/wineserver"),
            fonts_src: appdir.join("fonts"),
            shim_dir: appdir.join("shim"),
            data: data.to_path_buf(),
            prefix: data.join("prefix"),
            cache: data.join("cache"),
            logs: data.join("logs"),
            state_file: data.join("state.json"),
            appimage: appimage.to_path_buf(),
        }
    }

    /// The default `<data>/prefix` replaced by a location chosen in the wizard.
    pub fn with_prefix(mut self, prefix: Option<&Path>) -> Paths {
        if let Some(p) = prefix {
            self.prefix = p.to_path_buf();
        }
        self
    }

    /// The location the wizard offers first.
    pub fn default_prefix(&self) -> PathBuf {
        self.data.join("prefix")
    }

    /// APPDIR (set by AppRun) or the directory two levels above the executable
    /// (`AppDir/usr/bin/studiowine`); `$XDG_DATA_HOME/studiowine`; `$APPIMAGE` or the executable;
    /// the prefix recorded in state.json.
    pub fn resolve() -> anyhow::Result<Paths> {
        let exe = std::env::current_exe()?;
        let appdir = match std::env::var_os("APPDIR") {
            Some(d) => PathBuf::from(d),
            None => exe
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .ok_or_else(|| anyhow::anyhow!("cannot locate AppDir from {}", exe.display()))?
                .to_path_buf(),
        };
        let data = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("no XDG data directory (is HOME set?)"))?
            .join("studiowine");
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from).unwrap_or(exe);
        let chosen = crate::state::State::load(&data.join("state.json")).prefix;
        Ok(Paths::from_roots(&appdir, &data, &appimage).with_prefix(chosen.as_deref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_roots_layout() {
        let p = Paths::from_roots(Path::new("/app"), Path::new("/data/studiowine"), Path::new("/x/S.AppImage"));
        assert_eq!(p.wine_bin, Path::new("/app/wine/bin/wine"));
        assert_eq!(p.wineserver_bin, Path::new("/app/wine/bin/wineserver"));
        assert_eq!(p.fonts_src, Path::new("/app/fonts"));
        assert_eq!(p.shim_dir, Path::new("/app/shim"));
        assert_eq!(p.prefix, Path::new("/data/studiowine/prefix"));
        assert_eq!(p.cache, Path::new("/data/studiowine/cache"));
        assert_eq!(p.logs, Path::new("/data/studiowine/logs"));
        assert_eq!(p.state_file, Path::new("/data/studiowine/state.json"));
        assert_eq!(p.appimage, Path::new("/x/S.AppImage"));
        let moved = p.with_prefix(Some(Path::new("/mnt/audio/wine")));
        assert_eq!(moved.prefix, Path::new("/mnt/audio/wine"));
        assert_eq!(moved.default_prefix(), Path::new("/data/studiowine/prefix"));
        assert_eq!(moved.state_file, Path::new("/data/studiowine/state.json"), "state stays in the data directory");
    }
}
