# StudioWine — Cubase 15 & Windows audio software AppImage — design

Date: 2026-10-19 · Status: draft for approval
Background analysis: `PLAN-wine-environment.md` (REA dependency analysis of Cubase 15).

## 1. Intent

Linux musicians should be able to download **one AppImage**, start it, and end up with a
working Steinberg Cubase 15 installation (and other Windows audio software — VST
instruments, vendor launchers like Native Access) without knowing anything about Wine.
It is for the requester and for other Linux musicians, so it must work on the first try on
common distributions, not only on the requester's machine.

Why this is hard today: Cubase 15 draws its whole UI through Direct2D + DirectWrite +
DirectComposition, which stock Wine does not implement well. giang17's Wine fork
(`github.com/giang17/wine`, branch `d2d1-dcomp-11.0`) implements it and documents a
verified Cubase 15 / Download Assistant / Activation Manager setup
(`documentation/steinberg-setup-guide.md`). StudioWine packages that fork plus every
prefix-setup step the guide requires, behind a small GUI.

## 2. Goals

1. A single x86-64 AppImage containing: the fork's Wine build (built from source),
   Wine Mono, the PowerShell shim, the bundled substitute fonts, and the launcher.
2. A Rust + egui launcher (layout "C · Minimal", approved):
   - first start: a one-page **setup wizard** with per-step progress, ending by
     downloading and running the Steinberg Download Assistant installer;
   - afterwards: a **grid of the shortcuts** found in the Wine desktop folder, click to launch;
   - bottom bar: **Run .exe…**, **Fonts…**, ready-status dot; a ⚙ menu.
3. WebView2 and Electron/Chromium apps render (Steinberg Hub, Native Access, UVI Portal,
   plug-in launchers) — required by most VSTs and their launchers.
4. Correct Cubase fonts out of the box via free substitutes, with an easy path for users to
   supply their own licensed Microsoft fonts.

## 3. Non-goals

- No yabridge / bridging Windows plug-ins into native Linux DAWs.
- No DXVK, no vkd3d in the prefix (see §6.2).
- No 32-bit-only hosts as a target (the Wine build still contains the i386 PE half; see §6.1).
- No redistribution of any proprietary file: no Segoe UI / Microsoft Sans Serif, no
  WebView2 runtime, no Steinberg software inside the AppImage.
- No support for non-x86-64 machines, no Wayland-native driver work (runs under X11 /
  XWayland as the fork does).
- No licence/activation workarounds of any kind.

## 4. Anti-goals (would be a failure even if it "works")

- Touching the user's system Wine or `~/.wine`. Everything lives in StudioWine's own data dir.
- Shipping or downloading proprietary fonts or Microsoft binaries from unofficial sources.
- Silent failure: a setup step that fails must stop the wizard and say so; no "success" with
  a half-installed prefix (the fork documents exactly that failure mode for the shim).
- Requiring a terminal for any normal task.
- Using "Steinberg" or "Cubase" in the product name or icon (trademark).

## 5. Users & flows

### 5.1 First start (no prefix)
Wizard steps, each with a progress bar, a plain-language status line, **Retry** on failure
and a **Show log** link:

1. **Create Wine prefix** at `$XDG_DATA_HOME/studiowine/prefix` (default
   `~/.local/share/studiowine/prefix`), `WINEARCH=win64`, `wineboot -u` with the bundled
   Wine. The fork's `wine.inf` provides its registry defaults (HideWineVersion for
   `Cubase{12..15}.exe`, pwrshsip registration, `ThemeActive=0`). Wine Mono installed from
   the bundled MSI (no download prompt). Wine Gecko not installed (not needed — guide §2).
   Windows version: Windows 10.
2. **Fonts** — install the bundled font set and registry keys (§7).
3. **WebView2 runtime** — download Microsoft's Evergreen Standalone x64 installer from
   Microsoft's official URL at runtime and run it silently in the prefix. Verify by
   checking the installed `msedgewebview2.exe` exists.
4. **PowerShell shim** — install the prebuilt `System.Management.Automation.dll` shim into
   the prefix's Wine-Mono GAC (fork `scripts/mono-sma-shim/install.sh` logic) and run its
   probe. Must happen before the Download Assistant's first start (guide §4).
5. **Steinberg Download Assistant** — download the Windows installer linked from
   `https://o.steinberg.net/en/support/downloads/steinberg_download_assistant.html` and
   run it in the prefix; wait for exit; then run `winemenubuilder -a -r` so the
   `net-steinberg-*://` URL handlers are written (guide §5/§6).

When all steps are done the wizard is never shown again (state stored in the data dir).

### 5.2 Normal start
Grid of shortcuts found in the prefix's desktop folders
(`drive_c/users/<user>/Desktop` and `drive_c/users/Public/Desktop`), `.lnk` files. Each tile:
icon extracted from the `.lnk` target's PE icon resources (fallback: generic icon) and the
shortcut name. Click → launch through the bundled Wine with the shortcut's target,
arguments and working directory. The grid refreshes automatically when a launched process
exits (so newly installed apps appear). If no Download Assistant shortcut/installation is
found, a card offers **Install Download Assistant** (step 5 again).

### 5.3 Bottom bar and ⚙ menu
- **Run .exe…** — file picker for `.exe`/`.msi`; runs it in the same prefix
  (`.msi` via `msiexec /i`). For VST instruments and vendor installers. Grid refreshes after
  exit.
- **Fonts…** — dialog listing each required font family as *real* / *substitute* / *missing*,
  and a **Use fonts from folder…** button: user picks a folder (e.g. a mounted Windows
  `C:\Windows\Fonts`); the launcher copies only the known Microsoft font files it finds
  (Segoe UI family `segoeui*.ttf`/`segui*.ttf`, `micross.ttf`, `arial*.ttf`, `cour*.ttf`,
  `tahoma*.ttf`, `seguisym.ttf`, `seguiemj.ttf`, `segmdl2.ttf`, `SegoeIcons.ttf`),
  replaces the substitutes' registry entries with them, and restarts wineserver.
- **Status dot** — green "ready" when all wizard steps are verified; amber with a tooltip
  naming what is missing otherwise.
- **⚙ menu** — Open prefix folder · Show log · Re-run setup · Reset prefix (confirmation
  dialog warning to deactivate Steinberg/other licences first, because a new prefix counts
  as a new computer).

### 5.4 URL handlers (sign-in)
Wine's generated `~/.local/share/applications/wine-protocol-*.desktop` files call plain
`wine`. After every Wine session the launcher started ends (and after step 5), it rewrites
the `Exec=` lines of the handlers belonging to its prefix to call the AppImage
(`"$APPIMAGE" --wine …`) so browser sign-in links (`net-steinberg-sda://`,
`net-steinberg-activation-manager://`, and other vendors') land in the right prefix with the
right Wine. The same applies to the app menu entries winemenubuilder writes.

### 5.5 CLI entry points (for the handlers, not for users)
- `studiowine` — GUI.
- `studiowine --wine <args…>` — run the bundled `wine` with StudioWine's environment
  (prefix, PATH, library paths).

## 6. Decided constraints

### 6.1 Wine runner
- Built **from source**: `giang17/wine`, branch `d2d1-dcomp-11.0`, pinned to a specific
  commit recorded in the build script. Not the mklnln v11.0 prebuilt (published 2026-07-03:
  lacks pwrshsip and HideWineVersion defaults needed by the Cubase installer; needs glibc
  2.38 + libunwind, so not portable).
- `--enable-archs=i386,x86_64` (new WoW64) as the fork requires; `make install` output
  including `share/wine/wine.inf`.
- Built in an **Ubuntu 22.04 container** (glibc 2.35 baseline) via host podman, so the
  AppImage runs on Ubuntu 22.04+, Debian 12+, Fedora 38+ and their derivatives.
- ntsync support compiled in if the headers are available (used only when `/dev/ntsync` exists).

### 6.2 Graphics stack
- **No DXVK/vkd3d.** The fork's composition swapchain and DComp popup handling live in its
  own `dxgi`/`d3d11`/wined3d; DXVK would replace them (fork README "Leave DXVK out of the
  prefix"). Keep `d2d1`, `dcomp`, `dwrite`, `dxgi`, `d3d11`, `d3d10core`, `windowscodecs`,
  `gdiplus` builtin. The launcher must not set DLL overrides for these.
- Leave the fork's GL present path on (default).

### 6.3 Fonts
See §7. Free substitutes by default; user-supplied licensed fonts replace them.

### 6.4 Launcher
- Rust (stable) + egui/eframe. Single binary, runs as the AppImage's `AppRun` target.
- No network access other than: the Steinberg Download Assistant page/installer and the
  Microsoft WebView2 installer, both over HTTPS, only when the user is in the wizard or
  clicks an install action.

### 6.5 Data locations
- Prefix: `$XDG_DATA_HOME/studiowine/prefix`
- Downloads cache, logs, state: `$XDG_DATA_HOME/studiowine/{cache,logs,state.json}`
- Nothing written inside the AppImage; nothing outside the data dir except the rewritten
  `.desktop` handlers/menu entries in `~/.local/share/applications`.

## 7. Font plan (from the REA analysis + fork font guide)

Cubase requires the exact DirectWrite family names (graphics2d falls back to "Segoe UI" and
then drops the font; Cubase 15's Hub crashes in `hubservice.dll` without a "Segoe UI" family).

| Family Cubase asks for | Bundled free substitute | Treatment |
|---|---|---|
| Segoe UI (Regular/Bold/Semibold/Light/Italic) | Selawik (Microsoft, OFL, metric-compatible) | family renamed to "Segoe UI" in a modified copy, registered under `HKLM\…\Fonts` |
| Microsoft Sans Serif | Liberation Sans (or another OFL/metric-close sans) | family renamed to "Microsoft Sans Serif", registered |
| Arial, Courier New, Times New Roman, Verdana… | Liberation Sans/Mono/Serif, DejaVu | registered under their own names + `FontSubstitutes`, plus files named as the core fonts where plugins check for files (fork font guide) |
| Bravura, Bravura Text, Petaluma (+Script, Text) | upstream OFL fonts | installed + `C:\Program Files\Common Files\SMuFL\Fonts\<Name>\` with metadata JSON |
| Symbol fallback | DejaVu Sans + Noto Sans Symbols2 | FontLink `SystemLink` entries as in the fork's `wine-font-setup.sh` |

Plus the fork's text-rendering switches (`HKCU\Software\Wine\Direct2D` `text_*`,
`HKCU\Software\Wine\DirectWrite\outline_in_natural_modes`), font smoothing
(`FontSmoothing=2`, `FontSmoothingType=2`) and the system UI font = Segoe UI 9pt
(`WindowMetrics`), mirroring `wine-font-setup.sh`.

User-supplied fonts (Fonts… dialog) overwrite the substitute registrations; the dialog shows
the result. Licence texts of all bundled fonts ship in the AppImage.

## 8. Build & distribution

- One build script (`build/build.sh`) on the requester's machine: runs the Wine build in
  podman (Ubuntu 22.04), builds the shim, builds the launcher, assembles `AppDir`, bundles
  required shared libraries, runs `appimagetool`.
- Output: `StudioWine-<version>-x86_64.AppImage`.
- Bundle the Unix-side libraries Wine needs that a target distro may lack (FreeType,
  fontconfig, GnuTLS stack, SDL2, etc.); **do not** bundle glibc, libGL/Mesa/NVIDIA,
  Vulkan loader/drivers, X11/Wayland client libs, ALSA/PulseAudio/PipeWire client libs.
- Audio: the Steinberg built-in ASIO Driver installed by the Download Assistant works under
  this Wine (guide §7). wineasio is not bundled in v1.

## 9. Success criteria

On a clean Ubuntu 24.04 and a clean Fedora (current) VM/host with X11 or XWayland:
1. AppImage starts, wizard completes all 5 steps without a terminal.
2. Download Assistant signs in through the browser, installs Cubase 15; Activation Manager
   signs in; sign-in survives restarting it.
3. Cubase 15 starts from its tile, Hub shows (WebView2), an empty project opens, UI text is
   in the "Segoe UI"/"Microsoft Sans Serif" substitutes (no blank/garbled labels), audio plays
   through the Steinberg built-in ASIO driver.
4. Native Access 3 installs via Run .exe…, signs in and renders (Electron).
5. Fonts… with a folder of genuine Windows fonts switches the families to *real*.
6. The user's `~/.wine` and system Wine are unchanged.

### 9.1 Verification split (decided)
- The builder verifies on the requester's machine (Debian 13 host, podman available)
  everything up to and including: AppImage starts, wizard steps 1–4 pass, the Download
  Assistant installer runs and the Download Assistant window opens and renders.
- Then a **hand-off**: the requester signs in with their Steinberg account, installs Cubase 15
  and runs criteria 2–5; the builder fixes whatever the requester reports.
- Criterion 1 on other distros is spot-checked by running the AppImage inside an
  Ubuntu 22.04/24.04 and a Fedora container with the host's X11/XWayland socket, as far as
  that is possible without a VM.

## 10. Left to the builder

- egui crate versions and layout details within approved layout C; icon extraction method
  for `.lnk`/PE icons; how the shortcut grid detects changes.
- How the Steinberg Download Assistant installer URL is discovered from the download page
  (scrape vs. pinned URL with page-scrape fallback) — must fail loudly if the page changes.
- The MinGW toolchain used for the PE side (distro mingw vs llvm-mingw) as long as the
  build is reproducible inside the container.
- Exact free substitute for Microsoft Sans Serif and the font-renaming tool (fonttools).
- Whether to reuse the fork's `wine-font-setup.sh` and `mono-sma-shim` scripts directly
  (both LGPL, in the same repo) or reimplement their effect in Rust.
- Library bundling tool (linuxdeploy, manual `ldd` walk, …).
- Logging format.

## 11. Calls made by the designer (not explicitly decided by the requester)

- Product name **StudioWine** (avoids Steinberg trademarks).
- Prefix in `~/.local/share/studiowine/prefix`; x86-64 only.
- Ubuntu 22.04 build baseline.
- WebView2 Evergreen runtime downloaded from Microsoft at first start (not redistributable).
- Rewriting Wine's URL-handler `.desktop` files to point at the AppImage.
- ⚙ menu contents (Open prefix folder, Show log, Re-run setup, Reset prefix).
- wineasio not bundled in v1.
- No self-update in v1; the build runs locally (no CI pipeline in v1).
- Selawik has no italic; Segoe UI Italic requests fall back to synthetic italic.
