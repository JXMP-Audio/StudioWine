# Cubase 15 under Wine — dependency analysis & environment plan

Target: `reference/Cubase 15/` (Cubase15.exe, 135 MB, x86-64 PE, MSVC 2022 build).
Method: import tables of all 60 PE binaries (`analysis/imports.tsv`), REA/Ghidra
decompilation of `graphics2d.dll` (sha256 `72527318…31780c8`), extraction of
`Skins/skin.srf` (`analysis/srf_extract.py`), string scans of Cubase15.exe and
ScoringEngine.dll.

Legend: **[observed]** = seen directly in the binary/skin, **[inferred]** = reasoning
from observations + known Wine behaviour, **[verify]** = must be checked in your prefix.

---

## 1. Why the fonts are wrong — root cause chain

### 1.1 How Cubase draws text  [observed]
All UI drawing goes through `graphics2d.dll`, which contains three back-ends:
`D2DFactory/D2DDrawDevice` (Direct2D + DirectWrite + DirectComposition),
`GdiPlusFactoryImpl` and `GdiFactoryImpl`.

D2D backend initialisation (`FUN_1800378f0`, ev_468d0162…):
```
D2D1CreateFactory(MULTI_THREADED, IID_ID2D1Factory1)          // bb12d362-…
DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, IID_IDWriteFactory) // b859ee5a-…
CoCreateInstance(CLSID_WICImagingFactory, IID_IWICImagingFactory)
+ two more sub-inits; all 5 objects must be non-null, otherwise returns 0
```
So text is rendered by **DirectWrite** (Wine's `dwrite.dll`), not by GDI, as long
as D2D comes up.

### 1.2 Font-name resolution has a hard-coded fallback  [observed]
`FUN_1800561e0` (ev_4dc0f82f…): face name `"System"` → replaced by `"Segoe UI"`,
then looked up in the font collection.
`FUN_180036860` (ev_9054a02d…): for every font the skin requests, if the family
lookup returns NULL it retries with **`"Segoe UI"`**; if that also fails, the font
is silently dropped.

### 1.3 What the skin asks for  [observed — skin.srf, `<variant condition="WINDOWS">`]
| Skin font role | Face | Sizes / style |
|---|---|---|
| message, label, largelabel, smalllabel, tinylabel, list, tab, column | **Segoe UI** | 10–13 |
| data, dataXL, smalldata, largedata, (alt) smalllabel | **Microsoft Sans Serif** | 9–15, mostly **bold** |
| monospaced, smallmonospaced | **Courier New** | 10–12, mono |
| misc. dialogs | **Arial** | 11, 20 bold |

(Helvetica Neue / Monaco / Geneva entries are the macOS variant — irrelevant.)

### 1.4 Bundled fonts are loaded privately  [observed]
`FUN_180070120` (ev_15d56598…): enumerates `Cubase 15/Fonts/*.ttf|*.otf` and
calls `AddFontResourceExW(path, FR_PRIVATE /*0x10*/, 0)`. Plus a custom
`DWFontCollectionLoader`/`DWFontFileIterator` exists for DirectWrite.
Bundled: Steinberg Icons, Steinberg Score Icons, Steinberg Chord Symbols (+Jazz,
Sans Serif), Steinberg Notat Legacy. **Not bundled:** Bravura / Petaluma (only
`*.metrics` files are here — the real fonts come from the installer).

### 1.5 Score editor (ScoringEngine.dll = Dorico engine)  [observed]
* Tries `IDWriteFactory6 → 5 → 3 → 2`; message *"Can't initialize IDWriteFactory6.
  Use GDI font engine instead."*
* Reads `HKLM\Software\Microsoft\Windows NT\CurrentVersion\FontSubstitutes`.
* Needs **Bravura, Bravura Text, Petaluma, Petaluma Script, Petaluma Text**,
  metadata in `C:\Program Files\Common Files\SMuFL\Fonts\Bravura\` (bravura_metadata.json).
* Also names Segoe UI Symbol, Segoe UI Emoji, Segoe MDL2 Assets, Segoe Fluent
  Icons, Tahoma, MS Sans Serif.

### 1.6 Resulting failure mode  [inferred]
A default Wine prefix has none of Segoe UI / Microsoft Sans Serif (and Arial/
Courier New only after `corefonts`). Wine's DirectWrite system collection is built
from fonts *registered in the prefix* (`HKLM\…\Fonts` + `C:\windows\Fonts`), and
does **not** apply GDI `FontSubstitutes` or host fontconfig aliases to
`FindFamilyName`. So every `Microsoft Sans Serif` request misses → falls back to
`Segoe UI` → misses again → text falls through to whatever dwrite's internal
default is (or disappears). That matches "starts, but wrong fonts".
Substitutes configured with winetricks/`FontSubstitutes` help GDI-drawn parts
(menus, native dialogs) but not the D2D-drawn main UI — which is why registry
tweaks found online usually only partially work.

---

## 2. Full dependency inventory

### 2.1 Runtime libraries
| Dependency | Used by | Wine status | Action |
|---|---|---|---|
| MSVCP140, VCRUNTIME140(_1), CONCRT140, MSVCP140_1/_2, MSVCP140_CODECVT_IDS, api-ms-win-crt-* | almost all | builtin msvcp140 is incomplete; codecvt_ids/concrt need native | `winetricks -q vcrun2022` |
| gdiplus | graphics2d, Sampler Track, Drum Machine, Modulation FX | builtin OK | none (native `gdiplus` only if images break) |
| d2d1 (ID2D1Factory1) | graphics2d + 7 plug-ins | builtin, good | keep builtin |
| dwrite (IDWriteFactory…6) | graphics2d, ScoringEngine, plug-ins | builtin, good on Wine ≥ 9 | keep builtin, **fix fonts (§3)** |
| dcomp (DirectComposition) | graphics2d, Sampler Track, Drum Machine, Mod FX | builtin is mostly stubs **[verify]** | watch `WINEDEBUG=+dcomp`; Cubase has non-DComp paths (`createDrawDeviceHDC`) |
| d3d11 / dxgi / d3d9 / d3d12 | graphics2d, videoengine, ScoringEngine, VST Connect | wined3d or DXVK/VKD3D | **DXVK recommended** (D2D on wined3d is slow) |
| opengl32 / glu32 | plug-ins, VST Connect | host GL | none |
| windowscodecs (WIC) | graphics2d via CoCreateInstance | builtin OK | none |
| WebView2Loader / webview.dll | Hub, help, sign-in | **no working WebView2 in Wine** | expect blank Hub; disable Hub at startup |
| mfplat, mfreadwrite, WMVCore | videoengine, VST Connect, WMA codec | partial | only for video/WMA; optional `mf` |
| UIAutomationCore | ScoringEngine | builtin stubs OK | none |
| avrt, winmm, pdh, setupapi | audio engine | builtin OK | audio via wineasio (§4) |
| crypt32, wintrust, bcrypt, secur32, wininet, ws2_32 | licensing / network | builtin OK | Steinberg Activation Manager is a separate install |
| dbghelp, dbgeng | crash reporter | stubs | none |
| prntvpt, winspool | score printing | builtin | none |

### 2.2 Fonts (the main gap)
| Font | Needed by | Source |
|---|---|---|
| **Segoe UI** (Regular, Bold; ideally Semibold/Light/Italic) | skin (all labels) + hard-coded fallback | genuine: from a licensed Windows install (`C:\Windows\Fonts\segoeui*.ttf`) |
| **Microsoft Sans Serif** (`micross.ttf`) | skin data fields | genuine: from Windows |
| Arial, Courier New | skin | `winetricks corefonts` (or from Windows) |
| Tahoma, MS Sans Serif | ScoringEngine | Wine ships Tahoma; `winetricks tahoma` optional |
| Segoe UI Symbol / Emoji / MDL2 Assets / Fluent Icons | ScoringEngine (icons) | from Windows (optional, score editor glyph icons) |
| Bravura, Bravura Text, Petaluma, Petaluma Script, Petaluma Text + SMuFL metadata | Score editor | free (OFL), github.com/steinbergmedia/bravura and /petaluma, or the Cubase installer |
| Steinberg Icons / Score Icons / Chord Symbols* / Notat Legacy, DoricoIcons | Cubase / ScoringEngine | already bundled, loaded privately — also install system-wide as a safeguard |

Free fallback if Windows fonts are not available: **Selawik** (Microsoft's OFL,
metric-compatible Segoe UI stand-in) and Liberation Sans for Microsoft Sans Serif —
but because Cubase looks up the *exact family name*, those only work if registered
under the expected name (FontSubstitutes are not honoured by dwrite → see §3 step 4).

---

## 3. Plan — font fix (do this first)

Assume `WINEPREFIX` = the prefix where Cubase runs (Bottles: `~/.var/app/com.usebottles.bottles/data/bottles/bottles/<name>`,
Lutris: the game's prefix). Wine is not on the host `PATH` here (only Bottles and
Lutris Flatpaks), so run the commands below via Bottles' "Run command"/terminal
or the Lutris Wine runner.

1. **Baseline log** — reproduce and capture which lookups fail:
   `WINEDEBUG=+dwrite,+font,+d2d,+dcomp wine Cubase15.exe 2> fonts.log`
   then `grep -iE "FindFamilyName|not found|E_NOTIMPL" fonts.log`.
   Confirms §1.6 (expect misses on `Microsoft Sans Serif` / `Segoe UI`).
2. **Install runtimes**: `winetricks -q vcrun2022 corefonts`.
3. **Install fonts into the prefix (not the host)** — copy into
   `$WINEPREFIX/drive_c/windows/Fonts/`:
   segoeui.ttf, segoeuib.ttf, segoeuisb.ttf, segoeuil.ttf, segoeuii.ttf, segoeuiz.ttf,
   micross.ttf, seguisym.ttf, seguiemj.ttf, segmdl2.ttf, (SegoeIcons.ttf),
   Bravura.otf, BravuraText.otf, Petaluma*.otf, and the 6 Steinberg TTFs from
   `Cubase 15/Fonts/` plus `Components/ScoringEngine/fonts/DoricoIcons-Regular.ttf`.
4. **Register every font** under
   `HKLM\Software\Microsoft\Windows NT\CurrentVersion\Fonts`
   (e.g. `"Segoe UI (TrueType)"="segoeui.ttf"`, `"Microsoft Sans Serif (TrueType)"="micross.ttf"` …)
   — this is what Wine's dwrite system collection enumerates. Generate the .reg
   with a small script reading each font's `name` table (fc-scan/fonttools) to get
   the exact family string.
   *Fallback-only variant:* if you must use Selawik/Liberation, rename their family
   name to "Segoe UI"/"Microsoft Sans Serif" with fonttools for private use; a
   `FontSubstitutes` entry alone will **not** fix the D2D UI.
5. **SMuFL layout for the score editor**: create
   `drive_c/Program Files/Common Files/SMuFL/Fonts/Bravura/` with `Bravura.otf`
   and `bravura_metadata.json` (copy from `Components/ScoringEngine/fonts/metadata/`),
   same for Petaluma.
6. **Rendering quality** (HKCU\Control Panel\Desktop):
   `FontSmoothing="2"`, `FontSmoothingType=dword:2`, `FontSmoothingGamma=dword:578`,
   `FontSmoothingOrientation=dword:1`. Set `LogPixels` in `HKCU\Control Panel\Desktop`
   (96/120/144) to match your scaling; Cubase is per-monitor DPI-aware
   (`SetProcessDpiAwarenessContext`, `--dpiawareness` switch) **[observed]**.
7. **Restart the prefix** (`wineserver -k`) — dwrite caches the system collection.
8. **Verify**: rerun step 1 — no FindFamilyName misses; visually check Project
   window track names (Segoe UI), transport/time display (Microsoft Sans Serif bold),
   Key editor, Score editor (Bravura), chord track (Steinberg Chord Symbols).

## 4. Plan — rest of the environment

| Step | Action | Why |
|---|---|---|
| A | Use a recent Wine (≥ 9.x, preferably 10/11 staging or Wine-TkG/GE) 64-bit prefix, Windows version = Windows 10 | IDWriteFactory6/7, ID2D1Factory1, better d2d |
| B | DXVK (`winetricks dxvk` or Bottles toggle) | d3d11 for D2D, video engine, plug-in GUIs |
| C | wineasio (`regsvr32 wineasio64.dll`) + JACK/PipeWire-JACK | Cubase audio driver; no WASAPI-exclusive needed |
| D | Install Steinberg Library Manager / Activation Manager in the same prefix | licensing (not in this folder) |
| E | Keep `d2d1`, `dwrite`, `gdiplus`, `windowscodecs` **builtin**; do not override with native Windows copies | native dwrite/d2d1 do not run on Wine |
| F | Disable Hub / sign-in pages at startup; accept that WebView2 content is blank | no WebView2 runtime in Wine |
| G | If the UI flickers or is black: test `WINEDEBUG=+dcomp`; if DComp creation fails and Cubase does not fall back, file it; try `WINEDLLOVERRIDES=dcomp=d` to force the HDC path **[verify]** | DirectComposition stubs |

## 5. Unknowns / to verify
* How graphics2d chooses D2D vs GDI+ vs GDI at runtime (no config string found
  in graphics2d; selection logic is likely in Cubase15.exe — not yet traced).
* Whether the `DWFontCollectionLoader` puts the bundled `Fonts/` into the same
  collection that `FUN_180036860` searches (if it does, Segoe UI still has to
  come from the system collection).
* Wine dcomp coverage in your exact Wine build.
* Exact Wine version/prefix in use — none was found on `PATH`; Bottles and
  Lutris Flatpaks are installed.

## Evidence index (REA, graphics2d.dll)
| Claim | Evidence ID |
|---|---|
| Font/GDI/DWrite strings, "Segoe UI" | ev_560fc696… |
| "System"→"Segoe UI" mapping | ev_4dc0f82f… |
| Missing family → retry "Segoe UI" | ev_9054a02d… |
| AddFontResourceExW(FR_PRIVATE) for ttf/otf | ev_15d56598…, ev_584c2732… |
| D2D/DWrite/WIC factory init | ev_468d0162…, ev_201fcd04…, ev_f3f5e35a… |
