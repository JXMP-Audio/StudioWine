# StudioWine

One AppImage that installs and runs Steinberg Cubase 15 and other Windows music software
(VST instruments, vendor launchers such as Native Access) on Linux — without setting up
Wine yourself.

It bundles:

- **Wine** built from [giang17/wine](https://github.com/giang17/wine) (`d2d1-dcomp-11.0`), the
  fork that implements the Direct2D / DirectComposition / DirectWrite stack Cubase 15,
  WebView2 and JUCE 8 / VSTGUI plug-ins draw with. No DXVK: the fork's composition
  swapchain lives in its own `dxgi`/`d3d11`.
- **Wine Mono** and the fork's **PowerShell shim**, which Steinberg's installers need.
- **Free substitute fonts** registered under the names Windows programs ask for
  (Selawik as "Segoe UI", Liberation Sans as "Microsoft Sans Serif", Liberation/DejaVu for
  the core fonts, Bravura and Petaluma for the score editor). See `fonts/licenses/` in the
  AppImage.
- A small launcher (Rust + egui).

Nothing proprietary is shipped. The Microsoft WebView2 runtime and the Steinberg Download
Assistant are downloaded from Microsoft and Steinberg during setup.

## Requirements

- x86-64 Linux with glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+, Arch, …)
- X11 or XWayland, working OpenGL drivers
- About 3 GB free space for the Wine environment, plus your Steinberg software

## Using it

```sh
chmod +x StudioWine-0.1.0-x86_64.AppImage
./StudioWine-0.1.0-x86_64.AppImage
```

**First start** — click *Start setup*. The wizard runs five steps (create the Wine
environment, install fonts, install WebView2, install PowerShell support, install the
Steinberg Download Assistant). The last step opens Steinberg's installer: follow its
window. If a step fails it stops with the reason; *Retry* continues from there, *Show log*
opens the log.

**Afterwards** — the window shows the shortcuts Windows installers put on the desktop
(Cubase 15, Download Assistant, Activation Manager, Native Access, …). Click one to start it.

- **Run .exe…** runs any Windows installer or program (`.exe`, `.msi`) in the same
  environment, e.g. a VST instrument installer. Type or paste a path, drop the file on the
  window, or use *Browse…*.
- **Fonts…** shows whether each font is genuine or a substitute. If you hold a licence for
  the Microsoft fonts (for example on your own Windows partition), *Use fonts from folder…*
  copies `segoeui*.ttf`, `micross.ttf`, `arial*.ttf`, … from that folder and replaces the
  substitutes.
- **Settings** → open the Wine environment folder, show the log, re-run setup, or reset the
  environment. **Deactivate your licences before resetting** — a new environment counts as
  a new computer for Steinberg and other vendors.

Everything lives in `~/.local/share/studiowine/` (`prefix/`, `cache/`, `logs/`,
`state.json`). Sign-in links (`net-steinberg-*://`) and menu entries that Wine creates in
`~/.local/share/applications` are pointed at the AppImage automatically. If you move the
AppImage, start it once from the new place.

**Audio:** select the *Steinberg built-in ASIO Driver* in Cubase (Studio → Studio Setup →
Audio System).

### Troubleshooting

```sh
STUDIOWINE_DEBUG=+d2d1,+dcomp,+dwrite ./StudioWine-0.1.0-x86_64.AppImage
```

turns on Wine debug channels; output goes to `~/.local/share/studiowine/logs/studiowine.log`.
`./StudioWine-…AppImage --wine <args>` runs the bundled Wine on the environment directly
(e.g. `--wine winecfg`).

## Building

Requires podman (used through `host-spawn` from a toolbox/distrobox container).

```sh
build/build.sh            # image, wine (30–60 min first time), fonts, shim, launcher, appdir, appimage
build/build.sh launcher appdir appimage   # after launcher changes
cd launcher && cargo test # launcher unit tests
build/tests/check-wine.sh && build/tests/check-fonts.sh
build/tests/smoke-distro.sh ubuntu:22.04  # start-up check on another distribution
```

Every pinned input (Wine commit, Wine Mono, llvm-mingw, fonts, Rust) is in
`build/versions.env` with its SHA-256.

## Verified (2026-10-19)

| Check | Result |
|---|---|
| `--wine --version` on Debian 13 (host), Ubuntu 22.04, Debian 12, Fedora (glibc 2.42) | `wine-11.0` |
| Launcher window opens on Ubuntu 22.04, Debian 12, Fedora, Ubuntu 24.04 (Xvfb) | yes |
| Wizard steps 1–4 in a clean environment (Ubuntu 24.04, Xvfb) | passed — WebView2 runtime 155.0.4283.62 installed, PowerShell probe `EXIT 42` |
| Step 5: Download Assistant 1.40.1 downloaded, installer opens and renders | yes — not clicked through: accepting Steinberg's licence is the user's decision |
| Failure path (installer cancelled) | step marked failed, Retry / Show log offered |
| Main view, Fonts… dialog, Run .exe… (Wine notepad started via typed path) | yes |
| `~/.wine` untouched | yes |

## Hand-off checklist (needs a Steinberg account)

1. Start the AppImage, run the setup; in the Download Assistant installer accept the
   licence and finish.
2. Sign in to the Download Assistant through the browser; install **Cubase 15**.
3. Open the **Activation Manager**, sign in, activate; close and reopen it — the sign-in
   must survive.
4. Start **Cubase 15** from its tile: the Hub appears (WebView2), an empty project opens,
   labels and numbers are readable (substitute Segoe UI / Microsoft Sans Serif), audio plays
   through the Steinberg built-in ASIO Driver.
5. **Run .exe…** → Native Access 3 installer; Native Access signs in and renders.
6. **Fonts…** → *Use fonts from folder…* with your Windows `Fonts` folder: the families
   switch to *genuine*.

Report anything that differs, with `~/.local/share/studiowine/logs/studiowine.log`.
