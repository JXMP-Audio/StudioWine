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
- **[WineASIO](https://github.com/wineasio/wineasio)** 1.3.0 (64-bit), an ASIO driver that
  plays through JACK — PipeWire's JACK or a JACK server — for low audio latency.
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
- For WineASIO: PipeWire's JACK support (`pipewire-jack` on Debian/Ubuntu/Arch,
  `pipewire-jack-audio-connection-kit` on Fedora) or a running JACK server

## Using it

```sh
chmod +x StudioWine-0.2.0-x86_64.AppImage
./StudioWine-0.2.0-x86_64.AppImage
```

**First start** — choose where the Wine environment goes (default
`~/.local/share/studiowine/prefix`; any new or empty folder on a Linux file system, e.g. a
separate audio disk), then click *Start setup*. The wizard runs six steps (create the Wine
environment, install fonts, install WebView2, install PowerShell support, register WineASIO,
install the Steinberg Download Assistant). The last step opens Steinberg's installer: follow
its window. If you do not use Steinberg software, untick *Install it* under the last step
before starting: setup then ends after WineASIO, and *Settings → Install Steinberg Download
Assistant* adds it later. If a step fails it stops with the reason; *Retry* continues from there, *Show log*
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
- **Settings** → open the Wine environment folder, show the log, see whether WineASIO finds
  JACK, set the WineASIO buffer size, install the Download Assistant if setup skipped it, re-run setup (also to switch to another location), or
  reset the environment. **Deactivate your licences before resetting** — a new environment counts as
  a new computer for Steinberg and other vendors.

Everything lives in `~/.local/share/studiowine/` (`prefix/` unless you chose another
location, `cache/`, `logs/`, `state.json`, which also records the chosen location). Inside Windows programs, *Documents*, *Music* and *Downloads* are your
normal home folders, so projects and downloads end up where you expect; the Windows
*Desktop* is private to StudioWine. Sign-in links (`net-steinberg-sda:`, `net-steinberg-activation-manager:`) and menu entries
that Wine creates in `~/.local/share/applications` call the AppImage directly from the
moment they are written — the bundled `winemenubuilder` is patched for that — so they work
even when the StudioWine window is closed. If you move the AppImage, start it once from the
new place. These handlers are shared by all Wine environments; whenever StudioWine starts,
switches location or finishes a task, it makes those of the current location the active
ones and adds any an installer registered without telling Wine (Wine otherwise leaves a
handler pointing at whichever environment wrote it last).

**Audio:** select **WineASIO** in Cubase (Studio → Studio Setup → Audio System) for the
lowest latency. It connects to PipeWire through its JACK interface (StudioWine finds
Debian/Ubuntu's `pipewire-0.3/jack` library by itself, like `pw-jack`) or to a running JACK
server; *Settings* says which one it found. Cubase sets the buffer size, starting from the
size picked under *Settings → WineASIO buffer size* (default 256 samples ≈ 5 ms at 48 kHz);
WineASIO's own control panel is not included. Without JACK, use the *Steinberg built-in
ASIO Driver*.

### Troubleshooting

```sh
STUDIOWINE_DEBUG=+d2d1,+dcomp,+dwrite ./StudioWine-0.2.0-x86_64.AppImage
```

turns on Wine debug channels; output goes to `~/.local/share/studiowine/logs/studiowine.log`.
`./StudioWine-…AppImage --wine <args>` runs the bundled Wine on the environment directly
(e.g. `--wine winecfg`). With `WINEPREFIX` set it runs on that environment instead, which
is how menu entries and sign-in links name theirs.

## Building

Requires podman (used through `host-spawn` from a toolbox/distrobox container).

```sh
build/build.sh            # image, wine (30–60 min first time), fonts, shim, launcher, appdir, appimage
build/build.sh launcher appdir appimage   # after launcher changes
cd launcher && cargo test # launcher unit tests
build/tests/check-wine.sh && build/tests/check-fonts.sh
build/tests/smoke-distro.sh ubuntu:22.04  # start-up check on another distribution
```

Every pinned input (Wine commit, WineASIO commit, Wine Mono, llvm-mingw, fonts, Rust) is in
`build/versions.env`, downloads with their SHA-256. The `wine` stage applies
`build/patches/wine/*.patch` to the pinned commit and builds WineASIO against the result,
with `build/patches/wineasio/*.patch` applied. Upstream WineASIO calls `mlockall(MCL_FUTURE)`
when it opens; under the usual 8 MB locked-memory limit every later mapping then fails, so
PipeWire's JACK client cannot start its thread and the host (Cubase) freezes. The patch locks
memory only when that limit is unlimited.
`0002-msvcrt-fread-null-stream.patch` makes `fread` on a NULL stream return 0, as Windows'
`msvcrt.dll` does: Cardinal (and other VCV Rack based plug-ins) open every entry of the patch
folder they archive when the host saves their state, the folder itself included, and pass the
failed open straight to `fread`; without the patch Cubase crashes as soon as it saves the
plug-in's state, for instance right after loading it.

## Verified (2026-10-19, 2026-10-20)

| Check | Result |
|---|---|
| `--wine --version` on Debian 13 (host), Ubuntu 22.04, Debian 12, Fedora (glibc 2.42) | `wine-11.0` |
| Launcher window opens on Ubuntu 22.04, Debian 12, Fedora, Ubuntu 24.04 (Xvfb) | yes |
| Wizard steps 1–4 in a clean environment (Ubuntu 24.04, Xvfb) | passed — WebView2 runtime 155.0.4283.62 installed, PowerShell probe `EXIT 42` |
| Step 5: Download Assistant 1.40.1 downloaded, installer opens and renders | yes — not clicked through: accepting Steinberg's licence is the user's decision |
| Failure path (installer cancelled) | step marked failed, Retry / Show log offered |
| Main view, Fonts… dialog, Run .exe… (Wine notepad started via typed path) | yes |
| `~/.wine` untouched | yes |
| Handlers written by this Wine (`Exec=env "WINEPREFIX=…" wine …`) are re-pointed at the AppImage | yes — 5 real `wine-extension-*.desktop` files |
| Activation Manager sign-in (2026-10-19 log): link reached the running Activation Manager every time; the first attempt failed only because the handler still called plain `wine` while the window sat idle, and the retry came after the code had expired (`invalid_grant`) | fixed: `winemenubuilder` writes the AppImage itself |
| `net-steinberg-activation-manager:/login/oauth2/code?…` run through the generated handler, launcher closed (Ubuntu 24.04) | URL received unchanged |
| Wizard: custom location `/tmp/Audio Disk/Wine ü`; `/etc` refused; steps 1–5 there; relaunch reopens it | yes |
| WineASIO via COM (test ASIO host): no server → init fails cleanly; `jackd` → 16 in / 16 out, 48 kHz, buffer 16–8192 set by the host | yes |
| Settings → WineASIO buffer size writes `Preferred buffersize` | yes |
| Switching location takes the handlers back: a link registered without a handler is added and a handler another environment overwrote is rewritten (only that association is reset; the user's `Exclude` setting is kept) | yes |
| Cardinal 26.02 (VST2 build, same Rack code as the VST3) asked for its state through a minimal host: crashed in `fread` before the patch, returns its 2227-byte state after | yes |
| WineASIO streaming on the host's PipeWire 1.6.9 (`pipewire-jack`, 8 MB memlock): createBuffers → start → ~375 callbacks/2 s at 256 frames on Wine threads, PipeWire quantum restored; unpatched driver hangs in init | yes |
| Wine's Desktop folder is private (installer shortcuts stay out of `~/Desktop`); Documents stays linked to `~/Documents` | yes |

## Hand-off checklist (needs a Steinberg account)

1. Start the AppImage, run the setup; in the Download Assistant installer accept the
   licence and finish.
2. Sign in to the Download Assistant through the browser; install **Cubase 15**.
3. Open the **Activation Manager**, sign in, activate; close and reopen it — the sign-in
   must survive.
4. Start **Cubase 15** from its tile: the Hub appears (WebView2), an empty project opens,
   labels and numbers are readable (substitute Segoe UI / Microsoft Sans Serif), audio plays
   through WineASIO (with `pipewire-jack` installed) and through the Steinberg built-in
   ASIO Driver.
5. **Run .exe…** → Native Access 3 installer; Native Access signs in and renders.
6. **Fonts…** → *Use fonts from folder…* with your Windows `Fonts` folder: the families
   switch to *genuine*.

Report anything that differs, with `~/.local/share/studiowine/logs/studiowine.log`.
