# StudioWine AppImage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One x86-64 AppImage that contains giang17's Wine fork, Wine Mono, the PowerShell shim, free substitute fonts and a Rust/egui launcher that sets up a prefix and gets a user to a running Steinberg Download Assistant / Cubase 15.

**Architecture:** A `build/` pipeline runs inside an Ubuntu 22.04 podman container (glibc 2.35 baseline) and produces `out/{wine,fonts,shim,launcher}`; an assembly stage turns these into `AppDir` and an AppImage. At runtime `AppRun` starts the `studiowine` binary, which owns the prefix at `$XDG_DATA_HOME/studiowine/prefix`, runs a 5-step setup wizard on worker threads and otherwise shows the Wine desktop shortcuts.

**Tech Stack:** bash + podman (via `host-spawn` from the dev container), Wine `giang17/wine@d2d1-dcomp-11.0`, llvm-mingw, Python 3 + fontTools, Mono `mcs`, Rust 1.95 + eframe/egui 0.37 (glow backend), ureq 3, lnk 0.6, pelite 0.10, ttf-parser, image 0.25, rfd 0.17, serde/serde_json, anyhow, appimagetool (continuous).

**Spec:** `docs/superpowers/specs/2026-10-19-studiowine-appimage-design.md` (background: `PLAN-wine-environment.md`).

## Global Constraints

- Product name `StudioWine`; binary `studiowine`; output `StudioWine-<version>-x86_64.AppImage`; version `0.1.0`. Never "Steinberg"/"Cubase" in name or icon.
- Wine: `https://github.com/giang17/wine`, branch `d2d1-dcomp-11.0`, pinned commit `5ecbcc049334effeba70a9db2987034d73313575`; `--enable-archs=i386,x86_64`; `make install` (keeps `share/wine/wine.inf`).
- Build container base `ubuntu:22.04`; every shipped ELF must need at most `GLIBC_2.35`.
- No DXVK, no vkd3d. The launcher never sets `WINEDLLOVERRIDES` for `d2d1,dcomp,dwrite,dxgi,d3d11,d3d10core,windowscodecs,gdiplus`.
- Prefix `$XDG_DATA_HOME/studiowine/prefix` (default `~/.local/share/studiowine/prefix`), `WINEARCH=win64`; cache/logs/state in `$XDG_DATA_HOME/studiowine/{cache,logs,state.json}`. Nothing written into the AppImage. Never touch `~/.wine`.
- Wine Mono `10.4.1` bundled at `AppDir/wine/share/wine/mono/wine-mono-10.4.1-x86.msi`; Gecko not installed (`WINEDLLOVERRIDES=mshtml=d` only during `wineboot`).
- Downloads (HTTPS only, only from wizard/install actions): Download Assistant via the `https://www.steinberg.net/sda-win` link scraped from `https://o.steinberg.net/en/support/downloads/steinberg_download_assistant.html` (fallback to that same pinned link if the page has no match; error if neither resolves to a `.exe`); WebView2 from `https://go.microsoft.com/fwlink/?linkid=2124701`, run with `/silent /install`.
- No proprietary file in the AppImage. Free fonts: Selawik 1.01 (`https://github.com/microsoft/Selawik/releases/download/1.01/Selawik_Release.zip`), Liberation 2 + DejaVu + Noto Sans Symbols2 from Ubuntu 22.04 packages `fonts-liberation2 fonts-dejavu-core fonts-noto-core`, Bravura 1.482 (`…/steinbergmedia/bravura/releases/download/bravura-1.482/{Bravura.otf,BravuraText.otf,Bravura.json}`), Petaluma 1.065 (tag archive `https://github.com/steinbergmedia/petaluma/archive/refs/tags/petaluma-1.065.tar.gz`). Licence texts shipped in `AppDir/fonts/licenses/`.
- Do not bundle: glibc family, libGL/libEGL/libGLX/libGLdispatch/Mesa/NVIDIA, libvulkan, libX*/libxcb*/libwayland*, libasound, libpulse*, libpipewire*, libdrm, libgbm, libgcc_s, libstdc++.
- Every setup failure stops the wizard with the step's error text visible; no step reports success without its verification check passing.

## Review Focus

1. **Paths with spaces and non-ASCII** (home dir `"/home/Jürgen Müller"`, `.lnk` names like `Cubase 15.lnk`, `Program Files (x86)`) — every command is built with `Command::arg`, never a shell string; `.desktop` rewriting quotes the AppImage path. Tests: `wine::tests::env_handles_spaces`, `handlers::tests::rewrite_quotes_appimage_path_with_spaces`.
2. **Offline or blocked network during the wizard** — the download step fails with a readable message naming the URL, Retry works, and a partially downloaded file is never used (`.part` then rename). Test: `download::tests::interrupted_download_leaves_no_final_file`.
3. **Steinberg page changes / Microsoft link redirects to HTML** — resolved URL must end in `.exe` and response must start with `MZ`; otherwise error. Tests: `sda::tests::page_without_link_uses_pinned`, `download::tests::rejects_non_pe_payload`.
4. **AppImage moved or renamed after setup** (handlers point at a dead path) — handlers are re-rewritten on every launcher start, not only after installs. Test: `handlers::tests::rewrite_replaces_old_appimage_path`.
5. **User font folder with mixed-case Windows names (`SEGOEUI.TTF`, `Arial.ttf`) and duplicates** — matched case-insensitively, stored lower-case, a pre-existing differently-cased copy is removed (the fork documents dwrite failing on two spellings). Test: `fonts::tests::import_is_case_insensitive_and_dedups`.

---

## File Structure

```
.gitignore                         reference/, out/, AppDir/, target/, .superpowers/, *.AppImage
build/versions.env                 every pinned commit, URL, version, sha256 (single source)
build/Containerfile                ubuntu:22.04 + Wine build deps + llvm-mingw + mono-mcs + python3-fonttools + rustup 1.95.0
build/build.sh                     host entry: builds image, runs stages in container, then appimage stage
build/podman.sh                    wrapper: `host-spawn podman …` when inside the dev container, else `podman …`
build/stages/10-wine.sh            fetch/configure/make/install Wine → out/wine
build/stages/20-fonts.sh           fetch + rename fonts → out/fonts (+ licenses/)
build/stages/30-shim.sh            build System.Management.Automation.dll + smahost.exe → out/shim
build/stages/40-launcher.sh        cargo build --release → out/launcher/studiowine
build/stages/50-appdir.sh          assemble AppDir, bundle libs, AppRun, desktop file, icon
build/stages/60-appimage.sh        appimagetool → out/StudioWine-0.1.0-x86_64.AppImage (runs on host)
build/tools/rename_font.py         fontTools: set family/full/postscript names
build/tools/bundle_libs.sh         ldd walk with exclude list
build/tests/check-wine.sh          asserts on out/wine
build/tests/check-fonts.sh         asserts on out/fonts
build/tests/check-glibc.sh         max GLIBC symbol version of every ELF in a tree ≤ 2.35
build/assets/studiowine.desktop, studiowine.svg, AppRun
launcher/Cargo.toml
launcher/src/main.rs               CLI dispatch: `--wine …` passthrough vs GUI
launcher/src/paths.rs              Paths (appdir/data/prefix/cache/logs/state)
launcher/src/wine.rs               Wine: env + Command construction, run/wait, wineserver -k
launcher/src/log.rs                LogSink: append-only log file + line callback
launcher/src/state.rs              State (state.json)
launcher/src/registry.rs           .reg writer + Wine .reg reader
launcher/src/download.rs           HTTPS download with progress, .part, MZ check, cache
launcher/src/fonts.rs              font catalog, install, registry set, status, user import
launcher/src/setup/mod.rs          StepId, Step trait, Ctx, Progress, run_steps
launcher/src/setup/prefix.rs       step 1
launcher/src/setup/fonts_step.rs   step 2 (calls fonts.rs)
launcher/src/setup/webview2.rs     step 3
launcher/src/setup/shim.rs         step 4
launcher/src/setup/sda.rs          step 5 (+ page scrape)
launcher/src/shortcuts.rs          scan Desktop .lnk → Shortcut list
launcher/src/icons.rs              PE icon → RGBA image
launcher/src/handlers.rs           rewrite winemenubuilder .desktop Exec lines
launcher/src/ui/mod.rs             StudioWineApp (eframe::App), mode switch
launcher/src/ui/wizard.rs          wizard page
launcher/src/ui/grid.rs            shortcut grid + bottom bar + status dot
launcher/src/ui/fonts_dialog.rs    Fonts… dialog
launcher/src/ui/settings.rs        ⚙ menu + reset confirmation
launcher/tests/fixtures/           sda_page.html, sda_page_nolink.html, user.reg, system.reg, *.desktop, test.lnk, icon.exe
README.md                          user guide + hand-off test checklist
```

---

### Task 1: Repository, pinned versions, build container

**Files:** Create `.gitignore`, `build/versions.env`, `build/Containerfile`, `build/podman.sh`, `build/build.sh` (skeleton with `--stage NAME` and `all`).

**Interfaces:**
- Produces: image tag `studiowine-build:22.04`; `build/podman.sh <podman args…>`; `build/build.sh [all|wine|fonts|shim|launcher|appdir|appimage]`; stages run as `podman run --rm -v $REPO:/src -w /src studiowine-build:22.04 bash build/stages/NN-x.sh`, with `out/` = `/src/out`.
- `versions.env` variables: `WINE_REPO`, `WINE_COMMIT=5ecbcc049334effeba70a9db2987034d73313575`, `WINE_MONO_VERSION=10.4.1`, `LLVM_MINGW_URL=https://github.com/mstorsjo/llvm-mingw/releases/download/20261006/llvm-mingw-20261006-ucrt-ubuntu-22.04-x86_64.tar.xz`, `RUST_VERSION=1.95.0`, `SELAWIK_URL`, `BRAVURA_BASE`, `PETALUMA_URL`, `APPIMAGETOOL_URL=https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage`, `STUDIOWINE_VERSION=0.1.0`, plus a `*_SHA256` for every fixed-version download (fill by downloading once and recording).

- [ ] **Step 1:** `git init` at the repo root; write `.gitignore` (must list `reference/` — it holds Cubase binaries that must never be committed).
- [ ] **Step 2:** Write `versions.env`, `podman.sh` (detect `/run/.containerenv` or `host-spawn` on PATH → prefix `host-spawn`), `Containerfile` (apt: Wine build deps for X11 driver without wayland — `gcc flex bison pkg-config libx11-dev libxext-dev libxcursor-dev libxi-dev libxrandr-dev libxrender-dev libxinerama-dev libxcomposite-dev libxfixes-dev libxkbcommon-dev libfreetype-dev libfontconfig-dev libgnutls28-dev libgl-dev libegl-dev libvulkan-dev libpulse-dev libasound2-dev libdbus-1-dev libudev-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev`, plus `mono-mcs python3-fonttools python3-brotli fonts-liberation2 fonts-dejavu-core fonts-noto-core curl xz-utils zstd unzip file binutils patchelf xvfb`; llvm-mingw unpacked to `/opt/llvm-mingw` on PATH; rustup with `$RUST_VERSION`; vendored `linux/ntsync.h` from Linux v6.14 uapi into `/usr/include/linux/`).
- [ ] **Step 3: Verify** `build/podman.sh build -t studiowine-build:22.04 -f build/Containerfile build/` succeeds and `build/podman.sh run --rm studiowine-build:22.04 sh -c 'ldd --version | head -1; x86_64-w64-mingw32-clang --version | head -1; i686-w64-mingw32-clang --version | head -1; mcs --version; cargo --version'` prints glibc 2.35, both clang targets, mcs, cargo 1.95.
- [ ] **Step 4:** Commit `build: container and pinned versions`.

### Task 2: Wine build stage

**Files:** Create `build/stages/10-wine.sh`, `build/tests/check-wine.sh`, `build/tests/check-glibc.sh`.

**Interfaces:**
- Consumes: Task 1 image + `versions.env`.
- Produces: `out/wine/` = `make install` tree with `--prefix=/` into `DESTDIR=out/wine` (relocatable; `bin/wine`, `lib/wine/{x86_64-unix,x86_64-windows,i386-windows}`, `share/wine/wine.inf`), plus `out/wine/share/wine/mono/wine-mono-10.4.1-x86.msi`. Build tree cached in `out/.cache/wine-src` / `out/.cache/wine-build` so reruns are incremental.

- [ ] **Step 1: Write `check-wine.sh`** asserting: `out/wine/bin/wine --version` (run in container) prints `wine-11.0`; `grep -q 'Cubase15.exe' out/wine/share/wine/wine.inf`; `grep -qi HideWineVersion` same file; `out/wine/lib/wine/x86_64-windows/{d2d1,dcomp,dxgi,d3d11,dwrite,uianimation,pwrshsip}.dll` and `out/wine/lib/wine/i386-windows/d2d1.dll` exist; `check-glibc.sh out/wine` passes; `objdump -p out/wine/lib/wine/x86_64-unix/ntdll.so | grep -q libunwind` is false.
- [ ] **Step 2:** Run `check-wine.sh` → fails (no `out/wine`).
- [ ] **Step 3: Implement `10-wine.sh`:** shallow-fetch `$WINE_COMMIT`, `configure --prefix=/ --enable-archs=i386,x86_64 --with-mingw=clang --without-wayland --without-unwind --disable-tests`; `make -j$(nproc)`; `make install DESTDIR=/src/out/wine`; fail if `config.h` lacks `HAVE_LINUX_NTSYNC_H` (print warning only); download the Wine Mono MSI into `share/wine/mono/` and verify sha256; strip unix `.so` and binaries.
- [ ] **Step 4:** `build/build.sh wine && build/tests/check-wine.sh` → all assertions pass (expect 30–60 min first run).
- [ ] **Step 5:** Commit `build: wine fork stage`.

### Task 3: Font stage

**Files:** Create `build/stages/20-fonts.sh`, `build/tools/rename_font.py`, `build/tests/check-fonts.sh`.

**Interfaces:**
- Produces `out/fonts/` with exactly these files (Windows file names, lower-case) and `out/fonts/manifest.json` = `[{ "file": "segoeui.ttf", "family": "Segoe UI", "substitute_for": "Segoe UI", "sha256": "…" }, …]` (`substitute_for` = null for genuinely free fonts):
  - Renamed Selawik → family `Segoe UI`: `segoeuil.ttf` (Light), `segoeuisl.ttf` (Semilight), `segoeui.ttf` (Regular), `seguisb.ttf` (Semibold), `segoeuib.ttf` (Bold).
  - Renamed Liberation Sans Regular → family `Microsoft Sans Serif`: `micross.ttf`.
  - File-name stand-ins (unrenamed, as in the fork's `MSCORE_FILE_SUBST`): `arial.ttf arialbd.ttf ariali.ttf arialbi.ttf` ← LiberationSans-*, `times.ttf timesbd.ttf timesi.ttf timesbi.ttf` ← LiberationSerif-*, `cour.ttf courbd.ttf couri.ttf courbi.ttf` ← LiberationMono-*.
  - Free as-is: `DejaVuSans.ttf DejaVuSans-Bold.ttf DejaVuSerif.ttf DejaVuSansMono.ttf NotoSansSymbols2-Regular.ttf Bravura.otf BravuraText.otf Petaluma.otf PetalumaText.otf PetalumaScript.otf`, plus `smufl/Bravura/bravura_metadata.json` (from `Bravura.json`) and `smufl/Petaluma/petaluma_metadata.json`.
  - `licenses/` with OFL texts of Selawik, Liberation, DejaVu, Noto, Bravura, Petaluma, and `licenses/RENAMED.txt` stating which files are modified versions of which OFL font.
- `rename_font.py IN OUT --family NAME [--subfamily STYLE]`: rewrites name IDs 1, 2, 4, 6, 16, 17 (Windows + Mac records), PostScript name = family without spaces + `-` + style; drops name IDs 16/17 when style is one of the four RIBBI styles.

- [ ] **Step 1: Write `check-fonts.sh`:** for every manifest entry the file exists and sha matches; `fc-scan --format '%{family[0]}\n' out/fonts/segoeui.ttf` prints `Segoe UI`, same for `segoeuib.ttf`; `micross.ttf` → `Microsoft Sans Serif`; `arial.ttf` → `Liberation Sans`; licences directory non-empty.
- [ ] **Step 2:** Run → fails.
- [ ] **Step 3:** Implement `rename_font.py` and `20-fonts.sh` (Selawik zip contents are `selawk{l,sl,,sb,b}.ttf` — map to the five Segoe file names above; verify every download sha256).
- [ ] **Step 4:** `build/build.sh fonts && build/tests/check-fonts.sh` → pass.
- [ ] **Step 5:** Commit `build: substitute font stage`.

### Task 4: PowerShell shim stage

**Files:** Create `build/stages/30-shim.sh`.

**Interfaces:**
- Consumes: Wine source checkout from Task 2 (`scripts/mono-sma-shim/`).
- Produces: `out/shim/System.Management.Automation.dll`, `out/shim/smahost.exe`, `out/shim/LICENSE` (copied from the Wine tree — LGPL).

- [ ] **Step 1:** Implement: run the fork's `scripts/mono-sma-shim/build.sh` (host `mcs` is present in the image, so no prefix is needed), copy outputs.
- [ ] **Step 2: Verify** `build/build.sh shim` then `test -s out/shim/System.Management.Automation.dll && monodis --assembly out/shim/System.Management.Automation.dll | grep -q 'Version:.*3.0.0.0'` (install `mono-utils` in the image if `monodis` is missing), and the fork's `test/run-tests.sh` passes inside the container using `out/wine/bin/wine` under `xvfb-run` (17 cases).
- [ ] **Step 3:** Commit `build: powershell shim stage`.

### Task 5: Launcher core — paths, wine command, log, state

**Files:** Create `launcher/Cargo.toml`, `launcher/src/{main.rs,paths.rs,wine.rs,log.rs,state.rs}`; `build/stages/40-launcher.sh`.

**Interfaces (Produces):**
```rust
// paths.rs
pub struct Paths { pub appdir: PathBuf, pub wine_bin: PathBuf, pub wineserver_bin: PathBuf,
                   pub lib_dir: PathBuf, pub fonts_src: PathBuf, pub shim_dir: PathBuf,
                   pub data: PathBuf, pub prefix: PathBuf, pub cache: PathBuf,
                   pub logs: PathBuf, pub state_file: PathBuf, pub appimage: PathBuf }
impl Paths {
    pub fn from_roots(appdir: &Path, data: &Path, appimage: &Path) -> Paths; // pure
    pub fn resolve() -> anyhow::Result<Paths>; // APPDIR env or exe_dir/.., XDG_DATA_HOME or ~/.local/share + "studiowine", APPIMAGE env or current_exe
}
// layout under appdir: wine/bin/wine, wine/bin/wineserver, usr/lib, fonts, shim
// layout under data: prefix, cache, logs, state.json

// wine.rs
pub struct Wine { paths: Paths }
impl Wine {
    pub fn new(paths: Paths) -> Wine;
    pub fn env(&self) -> Vec<(OsString, OsString)>; // WINEPREFIX, WINEARCH=win64, WINELOADER=wine_bin,
        // WINESERVER=wineserver_bin, PATH=<appdir>/wine/bin:$PATH, LD_LIBRARY_PATH=<lib_dir>[:old],
        // WINEDEBUG=-all unless STUDIOWINE_DEBUG is set (then its value)
    pub fn command<I, S>(&self, args: I) -> std::process::Command where I: IntoIterator<Item=S>, S: AsRef<OsStr>;
    pub fn run_wait<I, S>(&self, args: I, log: &LogSink) -> anyhow::Result<std::process::ExitStatus>; // stdout+stderr lines → log
    pub fn kill_server(&self) -> anyhow::Result<()>; // wineserver -k, then -w
}
// log.rs
pub struct LogSink { /* Arc<Mutex<File>> + Option<Box<dyn Fn(&str)+Send+Sync>> */ }
impl LogSink { pub fn open(dir: &Path, name: &str) -> anyhow::Result<LogSink>; pub fn line(&self, s: &str); pub fn with_callback(self, f: impl Fn(&str)+Send+Sync+'static) -> LogSink; pub fn path(&self) -> &Path }
// state.rs
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
pub struct State { pub completed: Vec<StepId>, pub user_fonts_imported: bool }
impl State { pub fn load(path: &Path) -> State /* missing or corrupt → Default */; pub fn save(&self, path: &Path) -> anyhow::Result<()> /* write tmp + rename */ }
// main.rs: `studiowine --wine <args…>` → exec wine with Wine::env (exit code passthrough); no args → GUI (Task 12)
```
`StepId` is defined in `setup/mod.rs` (Task 8) as `#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)] pub enum StepId { Prefix, Fonts, WebView2, Shim, DownloadAssistant }`; create it in this task so `state.rs` compiles.

- [ ] **Step 1: Write failing tests:** `paths::tests::from_roots_layout` (prefix == data/prefix, wine_bin == appdir/wine/bin/wine); `wine::tests::env_sets_prefix_and_path` (WINEPREFIX equals prefix, PATH starts with `<appdir>/wine/bin:`, WINEDEBUG == `-all`); `wine::tests::env_handles_spaces` (data dir `"/tmp/a b/Jürgen"` round-trips unchanged); `wine::tests::run_wait_logs_output` using a fake `wine` shell script in a temp appdir that echoes `$@` — log file contains the args; `state::tests::roundtrip` and `state::tests::corrupt_file_gives_default`.
- [ ] **Step 2:** `cd launcher && cargo test` → fails to compile/fails.
- [ ] **Step 3:** Implement. Dependencies: `anyhow`, `serde`, `serde_json`, `dirs`, `tempfile` (dev).
- [ ] **Step 4:** `cargo test` → pass.
- [ ] **Step 5:** Implement `40-launcher.sh` (`cargo build --release --locked` in container → `out/launcher/studiowine`); verify `build/build.sh launcher && build/tests/check-glibc.sh out/launcher`.
- [ ] **Step 6:** Commit `launcher: paths, wine env, state`.

### Task 6: Registry writer/reader

**Files:** Create `launcher/src/registry.rs`, fixtures `tests/fixtures/{user.reg,system.reg}` (small real-format Wine excerpts).

**Interfaces (Produces):**
```rust
pub enum RegValue { Sz(String), Dword(u32), MultiSz(Vec<String>), Binary(Vec<u8>), Delete }
pub struct RegFile { /* ordered map: key path ("HKEY_LOCAL_MACHINE\\Software\\…") → Vec<(name, RegValue)> */ }
impl RegFile {
    pub fn new() -> RegFile;
    pub fn set(&mut self, key: &str, name: &str, v: RegValue) -> &mut Self;
    pub fn to_regedit_bytes(&self) -> Vec<u8>; // "Windows Registry Editor Version 5.00", UTF-16LE with BOM, CRLF; MultiSz as hex(7), Binary as hex:
    pub fn import(&self, wine: &Wine, log: &LogSink, tmp_dir: &Path) -> anyhow::Result<()>; // write file, `wine regedit /S <file>`; error on non-zero exit
}
pub fn read_value(wine_reg_file: &Path, key: &str, name: &str) -> Option<String>; // raw right-hand side from Wine's user.reg/system.reg; key given with single backslashes, case-insensitive
```

- [ ] **Step 1: Failing tests:** `to_regedit_bytes` starts with BOM `FF FE` and decodes to text containing `[HKEY_CURRENT_USER\Software\Wine\Direct2D]` and `"text_linear_blend"=dword:00000001`; MultiSz `["a","b"]` encodes as `hex(7):61,00,00,00,62,00,00,00,00,00`; `read_value(fixture system.reg, r"Software\Microsoft\Windows NT\CurrentVersion\Fonts", "Arial (TrueType)")` == `Some("\"arial.ttf\"")`; missing key → None.
- [ ] **Step 2:** `cargo test registry` → fail. **Step 3:** implement. **Step 4:** pass.
- [ ] **Step 5:** Commit `launcher: registry writer and reader`.

### Task 7: Download module

**Files:** Create `launcher/src/download.rs`.

**Interfaces (Produces):**
```rust
pub struct Http { agent: ureq::Agent } // rustls, redirects ≤ 10, UA "StudioWine/0.1.0"
impl Http {
    pub fn new() -> Http;
    pub fn get_text(&self, url: &str) -> anyhow::Result<String>;
    pub fn resolve_final_url(&self, url: &str) -> anyhow::Result<String>; // follows redirects with HEAD, falls back to GET
    pub fn download_exe(&self, url: &str, dest_dir: &Path, progress: &dyn Fn(u64, Option<u64>)) -> anyhow::Result<PathBuf>;
    // file name = last path segment of final URL; writes "<name>.part", checks first two bytes == b"MZ",
    // renames to final; if final already exists with size == Content-Length, returns it without downloading
}
```
- [ ] **Step 1: Failing tests** with a local `std::net::TcpListener` HTTP stub thread: `downloads_and_renames` (body `MZ…` → file exists, no `.part`); `rejects_non_pe_payload` (body `<html>` → Err mentioning the URL, no final file); `interrupted_download_leaves_no_final_file` (stub closes after half of declared Content-Length → Err, no final file); `reuses_complete_cached_file` (second call makes no request).
- [ ] **Step 2–4:** fail → implement → pass (allow `http://127.0.0.1` only under `#[cfg(test)]`; production rejects non-HTTPS).
- [ ] **Step 5:** Commit `launcher: downloads`.

### Task 8: Setup framework + step 1 (prefix)

**Files:** Create `launcher/src/setup/{mod.rs,prefix.rs}`.

**Interfaces (Produces):**
```rust
pub struct Ctx { pub paths: Paths, pub wine: Wine, pub http: Http, pub log: LogSink }
pub enum ProgressMsg { Started(StepId), Status(StepId, String), Fraction(StepId, f32), Done(StepId), Failed(StepId, String), AllDone }
#[derive(Clone)] pub struct Progress { tx: std::sync::mpsc::Sender<ProgressMsg> }
pub trait Step: Send + Sync {
    fn id(&self) -> StepId;
    fn title(&self) -> &'static str;           // shown in wizard
    fn is_done(&self, ctx: &Ctx) -> bool;      // verification check on disk (also drives the status dot)
    fn run(&self, ctx: &Ctx, p: &Progress) -> anyhow::Result<()>;
}
pub fn all_steps() -> Vec<Box<dyn Step>>;     // order: Prefix, Fonts, WebView2, Shim, DownloadAssistant
pub fn run_steps(steps: Vec<Box<dyn Step>>, ctx: Arc<Ctx>, state_file: PathBuf, p: Progress) -> std::thread::JoinHandle<()>;
// for each step: if is_done → Done and record; else Started, run, then require is_done (else Failed "verification failed: <title>");
// on Err → Failed(id, format!("{e:#}")) and stop; records completed ids in State; AllDone at end
```
Step titles (exact copy): `"Create Wine environment"`, `"Install fonts"`, `"Install Microsoft WebView2 runtime"`, `"Install PowerShell support for installers"`, `"Install Steinberg Download Assistant"`.

Prefix step: `run` = create data dirs; `wine wineboot -u` with extra env `WINEDLLOVERRIDES=mshtml=d`; then RegFile `HKCU\Software\Wine` `Version`=`win10`. `is_done` = `prefix/system.reg` exists AND `prefix/drive_c/windows/mono` exists AND `read_value(user.reg, "Software\\Wine", "Version") == Some("\"win10\"")`.

- [ ] **Step 1: Failing tests** with the fake-wine appdir (fake `wine` script: on `wineboot` creates `system.reg`, `user.reg`, `drive_c/windows/mono/`; on `regedit /S f` appends `f`'s decoded text to `user.reg` in Wine format): `run_steps_stops_at_first_failure` (step 2 errors → messages are Started(Prefix), Done(Prefix), Started(Fonts), Failed(Fonts, msg) and no Started for later steps; state.completed == [Prefix]); `run_steps_skips_done_steps`; `failed_verification_is_failure` (run Ok but is_done false → Failed with "verification failed"); `prefix_step_runs_wineboot_with_mshtml_disabled` (fake wine logs env).
- [ ] **Step 2–4:** fail → implement → pass.
- [ ] **Step 5:** Commit `launcher: setup runner and prefix step`.

### Task 9: Fonts module + step 2

**Files:** Create `launcher/src/fonts.rs`, `launcher/src/setup/fonts_step.rs`. Add `ttf-parser` dependency.

**Interfaces (Produces):**
```rust
pub struct ManifestEntry { pub file: String, pub family: String, pub substitute_for: Option<String>, pub sha256: String } // serde from AppDir/fonts/manifest.json
pub enum FontStatus { Real, Substitute, Missing }
pub struct FamilyStatus { pub family: &'static str, pub status: FontStatus }
pub const REQUIRED_FAMILIES: &[&str] = &["Segoe UI", "Microsoft Sans Serif", "Arial", "Courier New", "Times New Roman", "Bravura", "Petaluma"];
pub const USER_IMPORT_PATTERNS: &[&str] = &["segoeui*.ttf", "segui*.ttf", "micross.ttf", "arial*.ttf", "cour*.ttf", "times*.ttf", "tahoma*.ttf", "seguisym.ttf", "seguiemj.ttf", "segmdl2.ttf", "segoeicons.ttf"]; // case-insensitive
pub fn reg_name(font_file: &Path) -> anyhow::Result<String>;     // "<typographic or legacy family>[ <subfamily unless Regular>] (TrueType|OpenType)"
pub fn install_bundled(paths: &Paths) -> anyhow::Result<Vec<PathBuf>>; // copy every manifest file into prefix/drive_c/windows/Fonts (skip a target whose sha differs from the bundled sha — i.e. a user-imported real font), copy smufl/* to "drive_c/Program Files/Common Files/SMuFL/Fonts/"
pub fn registry_set(paths: &Paths, dpi: u32) -> anyhow::Result<RegFile>;  // see values below
pub fn family_status(paths: &Paths) -> anyhow::Result<Vec<FamilyStatus>>;
pub struct ImportReport { pub copied: Vec<String>, pub skipped: Vec<String> }
pub fn import_user_fonts(src: &Path, paths: &Paths) -> anyhow::Result<ImportReport>; // copy matches as lower-case names, delete other-cased duplicates in Fonts dir, return report
pub fn prefix_dpi(paths: &Paths) -> u32; // user.reg "Control Panel\Desktop" LogPixels dword, default 96
```
`registry_set` values (copied from the fork's `wine-font-setup.sh`):
- `HKLM\Software\Microsoft\Windows NT\CurrentVersion\Fonts`: one entry per file in `drive_c/windows/Fonts` → `reg_name(file)` = file name.
- `HKLM\…\FontSubstitutes` (only for families whose genuine file is absent): `Arial→Liberation Sans`, `Times New Roman→Liberation Serif`, `Courier New→Liberation Mono`, `Verdana→DejaVu Sans`, `Georgia→DejaVu Serif`, `Trebuchet MS→DejaVu Sans`, `Comic Sans MS→DejaVu Sans`, `Andale Mono→DejaVu Sans Mono`, `Impact→DejaVu Sans`; always (only if absent) `Malgun Gothic, Meiryo, Meiryo UI, Microsoft JhengHei UI, Microsoft YaHei UI, Yu Gothic UI → Segoe UI`.
- `HKLM\…\FontLink\SystemLink`: `Tahoma` = MultiSz `["DejaVuSans.ttf,DejaVu Sans","NotoSansSymbols2-Regular.ttf,Noto Sans Symbols2","MSGOTHIC.TTC,MS UI Gothic","MINGLIU.TTC,PMingLiU","SIMSUN.TTC,SimSun","GULIM.TTC,Gulim","YUGOTHM.TTC,Yu Gothic UI","MSJH.TTC,Microsoft JhengHei UI","MSYH.TTC,Microsoft YaHei UI","MALGUN.TTF,Malgun Gothic","SEGUISYM.TTF,Segoe UI Symbol"]`; `DejaVu Sans` = `["NotoSansSymbols2-Regular.ttf,Noto Sans Symbols2"]`.
- `HKCU\Software\Wine\Direct2D`: `text_enhanced_contrast`=50 (only if absent), `text_linear_blend`=1, `text_grayscale_blend`=1, `text_cleartype_blend`=1. `HKCU\Software\Wine\DirectWrite`: `outline_in_natural_modes`=1.
- `HKCU\Control Panel\Desktop`: `FontSmoothing`=Sz `"2"`, `FontSmoothingType`=Dword 2, `FontSmoothingOrientation`=Dword 1.
- `HKCU\Control Panel\Desktop\WindowMetrics`: `CaptionFont, IconFont, MenuFont, MessageFont, SmCaptionFont, StatusFont` = Binary LOGFONTW (92 bytes): `lfHeight = -((9*dpi+36)/72)` (i32 LE), width/escapement/orientation 0, weight 400, italic/underline/strikeout 0, charset 1 (DEFAULT_CHARSET), precision/clip/quality/pitch 0, face `"Segoe UI"` UTF-16LE zero-padded to 64 bytes.

Font step: `run` = `install_bundled`, `registry_set(..).import`, `kill_server`. `is_done` = `family_status` has no `Missing` AND `read_value(user.reg, "Software\\Wine\\Direct2D", "text_linear_blend")` is Some.

- [ ] **Step 1: Failing tests** (temp appdir with a fake manifest and two tiny real TTFs copied from `/usr/share/fonts` at test time, skipped if absent): `reg_name_uses_family_and_style` (DejaVuSans-Bold → `"DejaVu Sans Bold (TrueType)"`); `logfont_is_92_bytes_with_segoe_face` (dpi 96 → first 4 bytes = `F4 FF FF FF` (-12), bytes 28..44 decode to "Segoe UI"); `substitutes_only_when_genuine_absent`; `install_keeps_user_real_font` (Fonts/segoeui.ttf with different sha survives install_bundled); `status_reports_real_vs_substitute`; `import_is_case_insensitive_and_dedups` (src has `SEGOEUI.TTF`, prefix has `Segoeui.ttf` → result only `segoeui.ttf`, report.copied == ["segoeui.ttf"]); `import_ignores_unlisted_files` (`notes.txt`, `wingding.ttf` → skipped).
- [ ] **Step 2–4:** fail → implement → pass.
- [ ] **Step 5:** Commit `launcher: fonts and font step`.

### Task 10: Steps 3–5 (WebView2, shim, Download Assistant)

**Files:** Create `launcher/src/setup/{webview2.rs,shim.rs,sda.rs}`, fixtures `sda_page.html` (copy of the live page, trimmed), `sda_page_nolink.html`.

**Interfaces (Produces):**
```rust
// webview2.rs
pub const WEBVIEW2_URL: &str = "https://go.microsoft.com/fwlink/?linkid=2124701";
pub fn find_webview2(prefix: &Path) -> Option<PathBuf>; // drive_c/Program Files (x86)/Microsoft/EdgeWebView/Application/<ver>/msedgewebview2.exe
// run: download_exe → wine <exe> /silent /install → kill_server; is_done: find_webview2 is Some
// shim.rs
pub fn gac_target(prefix: &Path) -> Option<PathBuf>; // first dir named "gac" ≤ 4 levels under drive_c/windows/mono, + System.Management.Automation/3.0.0.0__31bf3856ad364e35/System.Management.Automation.dll
// run: copy shim dll there; copy smahost.exe to temp dir; wine smahost.exe -c 'Write-Host "gac-probe"; EXIT 42' (cwd temp, timeout 120 s);
//      output must contain "INFO:   gac-probe" and "EXIT:   42" else Err with the output; is_done: target exists and bytes equal bundled dll
// sda.rs
pub const SDA_PAGE: &str = "https://o.steinberg.net/en/support/downloads/steinberg_download_assistant.html";
pub const SDA_PINNED: &str = "https://www.steinberg.net/sda-win";
pub fn find_sda_link(html: &str) -> Option<String>; // first href matching (?i)https://www\.steinberg\.net/sda-win\b or an absolute *.exe containing "Download_Assistant" and "win"
pub fn find_sda_exe(prefix: &Path) -> Option<PathBuf>; // any "Steinberg Download Assistant.exe" under drive_c/Program Files*/Steinberg (case-insensitive, depth ≤ 4)
// run: get_text(SDA_PAGE) (on error use pinned) → link or SDA_PINNED → resolve_final_url must end with ".exe" → download_exe → wine <exe> (wait)
//      → wine winemenubuilder -a -r → handlers::rewrite_all (Task 11); is_done: find_sda_exe is Some
```
- [ ] **Step 1: Failing tests:** `sda::tests::finds_link_on_live_page_fixture` (== `https://www.steinberg.net/sda-win`); `page_without_link_uses_pinned` (None → caller uses SDA_PINNED; test the caller fn `choose_sda_url(Option<String>) -> &str`); `find_sda_exe_case_insensitive`; `webview2::tests::finds_versioned_exe`; `shim::tests::gac_target_found` and `probe_output_parsing` (`fn probe_ok(out: &str) -> bool`).
- [ ] **Step 2–4:** fail → implement → pass.
- [ ] **Step 5:** Commit `launcher: webview2, shim and download assistant steps`.

### Task 11: Shortcuts, icons, URL handlers

**Files:** Create `launcher/src/{shortcuts.rs,icons.rs,handlers.rs}`, fixtures `test.lnk` (made with the `lnk` crate's writer in a test helper or a real Wine-made `.lnk`), `icon.exe` (any small PE with an icon, e.g. Wine's `notepad.exe` from `out/wine`, copied at test time and skipped if absent), `wine-protocol-net-steinberg-sda.desktop`, `wine-app.desktop`.

**Interfaces (Produces):**
```rust
// shortcuts.rs
pub struct Shortcut { pub name: String /* file stem */, pub lnk: PathBuf, pub target: Option<PathBuf> /* unix path */ }
pub fn desktop_dirs(prefix: &Path) -> Vec<PathBuf>; // drive_c/users/<every user dir except Public>/Desktop + drive_c/users/Public/Desktop
pub fn scan(prefix: &Path) -> Vec<Shortcut>;        // *.lnk, sorted by name case-insensitively, deduped by name
pub fn windows_to_unix(prefix: &Path, win: &str) -> Option<PathBuf>; // "C:\\x\\y" → prefix/drive_c/x/y (case-insensitive drive letter)
pub fn launch(wine: &Wine, s: &Shortcut) -> anyhow::Result<std::process::Child>; // wine start /unix <lnk>
pub fn dir_fingerprint(prefix: &Path) -> u64;        // hash of (name, mtime) of every .lnk — UI polls every 2 s
// icons.rs
pub fn icon_rgba(exe: &Path, size: u32) -> Option<(u32, u32, Vec<u8>)>; // largest RT_GROUP_ICON image via pelite, decoded with `image` (ico/png), resized to size
// handlers.rs
pub fn rewrite_exec(line: &str, prefix: &Path, appimage: &Path) -> Option<String>;
// for an "Exec=" line containing `WINEPREFIX="<prefix>"` and an unquoted `wine` word (or a previous `"<old>" --wine`), return
// `Exec=env WINEPREFIX="<prefix>" "<appimage>" --wine <rest>`; None if the line belongs to another prefix
pub fn rewrite_all(prefix: &Path, appimage: &Path) -> anyhow::Result<usize>; // ~/.local/share/applications/wine-protocol-*.desktop and ~/.local/share/applications/wine/**/*.desktop
```
- [ ] **Step 1: Failing tests:** `scan_finds_public_and_user_desktop`; `windows_to_unix_maps_drive_c`; `rewrite_exec_points_at_appimage`; `rewrite_quotes_appimage_path_with_spaces` (appimage `/home/a b/StudioWine.AppImage` appears quoted); `rewrite_replaces_old_appimage_path`; `rewrite_ignores_other_prefix`; `icon_rgba_reads_pe_icon` (skipped when fixture missing).
- [ ] **Step 2–4:** fail → implement → pass.
- [ ] **Step 5:** Commit `launcher: shortcuts, icons, url handlers`.

### Task 12: GUI (layout C)

**Files:** Create `launcher/src/ui/{mod.rs,wizard.rs,grid.rs,fonts_dialog.rs,settings.rs}`; wire GUI into `main.rs`. Dependencies: `eframe` 0.37 (`default-features = false, features = ["glow", "default_fonts", "x11", "wayland"]`), `egui` 0.37, `rfd` 0.17.

**Interfaces:**
- Consumes: everything above.
- Produces: `pub fn run_gui(paths: Paths) -> anyhow::Result<()>`; pure helpers with tests:
  - `ui::mode_for(state: &State, steps_done: &[(StepId, bool)]) -> Mode` — `Mode::Wizard` unless every step is completed in `state` (wizard shown once; afterwards Main even if a check later fails).
  - `grid::status_dot(steps_done: &[(StepId, bool)]) -> (Dot, String)` — `Dot::Green, "Ready"` when all true; otherwise `Dot::Amber` and `"Missing: <titles joined with ', '>"`.
  - `grid::needs_sda_card(shortcuts: &[Shortcut], sda_installed: bool) -> bool` — true when `!sda_installed`.

Behaviour (approved layout C):
- Window title `StudioWine`, initial size 760×520.
- **Wizard:** full-window page, five rows (title, progress bar, status line), **Start setup** button; on `Failed` show error text in red + **Retry** (re-runs from the failed step) + **Show log** (opens log via `xdg-open`). On `AllDone` switch to Main.
- **Main:** 4-column grid of tiles (64 px icon + name); click → `shortcuts::launch`; poll `dir_fingerprint` every 2 s and on change rescan; `needs_sda_card` → card "Steinberg Download Assistant is not installed" + **Install** (runs step 5 only). Bottom bar: **▶ Run .exe…** (rfd picker, filters `exe`, `msi`; `.msi` → `wine msiexec /i <file>`; runs on a thread; afterwards `handlers::rewrite_all`), **Fonts…**, status dot with tooltip. ⚙ top-right.
- **Fonts… dialog:** table of `family_status` (Real/Substitute/Missing), button **Use fonts from folder…** → rfd folder picker → `import_user_fonts` → re-run `registry_set().import` + `kill_server` → show report ("Copied: …").
- **⚙ menu:** Open prefix folder (`xdg-open`), Show log, Re-run setup (sets Mode::Wizard), Reset prefix… (modal: "Deactivate your Steinberg and other licences first — a new Wine environment counts as a new computer." + **Delete and start over** / Cancel → `kill_server`, delete prefix + state, Mode::Wizard).
- On every GUI start: `handlers::rewrite_all` (Review Focus 4).
- Long operations never block the UI thread (threads + `mpsc`, `ctx.request_repaint()`).

- [ ] **Step 1: Failing tests** for `mode_for`, `status_dot`, `needs_sda_card`.
- [ ] **Step 2–4:** fail → implement helpers and UI → pass; `cargo clippy -- -D warnings` clean.
- [ ] **Step 5: Manual check** on the host with a dev appdir (`APPDIR=$PWD/AppDir cargo run`) after Task 13 exists — window opens, wizard rows render.
- [ ] **Step 6:** Commit `launcher: egui ui`.

### Task 13: AppDir and AppImage

**Files:** Create `build/stages/50-appdir.sh`, `build/stages/60-appimage.sh`, `build/tools/bundle_libs.sh`, `build/assets/{AppRun,studiowine.desktop,studiowine.svg}`.

**Interfaces:**
- Produces `AppDir/` layout: `AppRun`, `studiowine.desktop`, `studiowine.svg`, `.DirIcon`, `usr/bin/studiowine`, `usr/lib/*.so*` (bundled), `wine/` (= `out/wine`), `fonts/` (= `out/fonts`), `shim/` (= `out/shim`); and `out/StudioWine-0.1.0-x86_64.AppImage`.
- `AppRun`: `HERE=$(dirname "$(readlink -f "$0")")`; `export APPDIR=$HERE`; exec `$HERE/usr/bin/studiowine "$@"` (the launcher sets `LD_LIBRARY_PATH` only for Wine children, not for itself).
- `bundle_libs.sh <tree> <dest>`: `ldd` every ELF under `wine/lib/wine/x86_64-unix` and `wine/bin`, plus explicitly the dlopen'd sonames `libfreetype.so.6 libfontconfig.so.1 libgnutls.so.30 libgstreamer-1.0.so.0` and their `ldd` closure; copy into `usr/lib` except the Global Constraints exclude list.
- Desktop file: `Name=StudioWine`, `Exec=studiowine`, `Icon=studiowine`, `Categories=AudioVideo;Audio;`.

- [ ] **Step 1:** Implement stages and tools.
- [ ] **Step 2: Verify** `build/build.sh all` produces the AppImage; `build/tests/check-glibc.sh AppDir` passes; `./out/StudioWine-0.1.0-x86_64.AppImage --wine --version` prints `wine-11.0`; `--appimage-extract-and-run` variant works when FUSE is absent.
- [ ] **Step 3:** Commit `build: appdir and appimage`.

### Task 14: End-to-end on this machine, distro spot-checks, hand-off

**Files:** Create `README.md` (what it is, requirements: x86-64, X11 or XWayland, glibc ≥ 2.35; first-start walkthrough; Fonts… explanation incl. licence note; troubleshooting `STUDIOWINE_DEBUG=+d2d1,+dcomp,+dwrite`; hand-off checklist = spec §9 criteria 2–5).

- [ ] **Step 1:** Run the AppImage on the host (Debian 13) with an empty `XDG_DATA_HOME=$(mktemp -d)`; wizard steps 1–4 complete, step 5 runs the Download Assistant installer; Download Assistant window opens and renders text in the substitute "Segoe UI". Record log paths and screenshots in the README's "Verified" section.
- [ ] **Step 2:** Verify `~/.wine` mtime unchanged and no files outside `$XDG_DATA_HOME/studiowine` and `~/.local/share/applications` changed (`find ~ -newer <stamp> -not -path …`).
- [ ] **Step 3:** Spot-check start-up in `ubuntu:22.04`, `ubuntu:24.04`, `fedora:latest` containers with the X socket mounted (`-e DISPLAY -v /tmp/.X11-unix:/tmp/.X11-unix`, host GL via `--device /dev/dri`): `--wine --version` works and the launcher window opens (wizard step 1 completes). Note results in README.
- [ ] **Step 4:** Commit `docs: readme and verification notes`; hand off to the requester for criteria 2–5 (Steinberg sign-in, Cubase 15 install, project playback, Native Access, real fonts import).
