<sub>A JPEGmini alternative</sub>

# Fino

**Lighter photos that look exactly the same.**

English · [Español](README.md)

[![CI](https://github.com/AmunM9/fino/actions/workflows/ci.yml/badge.svg)](https://github.com/AmunM9/fino/actions/workflows/ci.yml)

Fino is a macOS and Windows app that recompresses JPEGs *perceptually*. It tries smaller and smaller versions of each photo, compares every candidate with the original region by region, and keeps the smallest one that shows no visible artifact. Built from open components, running offline on your computer.

```
89 camera photos (24 MP)   511 MB → 156 MB   (−69.5%)   ~0.25 s per photo
same resolution · same metadata · SSIMULACRA 2 ≈ 85
```

![Fino optimizing a batch of 1,248 camera photos](docs/screenshots/running-dark.png)

<table>
  <tr>
    <td><img src="docs/screenshots/compare.jpg" alt="Before/after viewer with a split divider"></td>
    <td><img src="docs/screenshots/done-light.png" alt="Finished session: savings per photo"></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/history-light.png" alt="Session history"></td>
    <td><img src="docs/screenshots/settings-light.png" alt="Settings"></td>
  </tr>
</table>

<sub>Screenshots use sample data: real camera photos, figures simulated at the measured savings (~67%).</sub>

> The interface is in Spanish for now.

## Features

| | |
|---|---|
| **Optimize originals** | Replaces each photo, keeping a backup first so you can **undo the whole session** (kept 1, 7 or 30 days). |
| **Backups** | Stored in `~/Library/Application Support/app.fino.desktop/backups/` (Windows: `%APPDATA%\app.fino.desktop\backups\`). They expire on their own (checked when Fino opens and whenever you return to it). Settings shows how much space they take and **frees** them; History discards a single session's backup. Backups deleted by hand are detected. |
| **HEIC → JPEG (optional, macOS)** | Off by default: HEIC is already the lighter format and Fino leaves it alone. Turned on, each HEIC becomes a JPEG at the *Compact* strength — about 25% lighter than the HEIC on iPhone photos — with EXIF, XMP, colour profile and orientation intact; HDR, depth and portrait data are dropped. Live Photos stay paired with their video. |
| **Mini window** | A small dial that floats over other apps: drop photos or folders on it and watch each photo go by as it is optimized. |
| **Export a copy** | Leaves originals untouched. Copies go to a `Fino` folder next to each photo or to a fixed destination, mirroring the folder structure. |
| **Several sizes** | Up to 4 sizes per export (long edge, max width or max height). Honors EXIF orientation and never upscales. |
| **Strength** | *Flawless* (no difference even under a loupe), *Identical* (recommended: looks the same as the original) and *Compact* (a little lighter, for the web and social media). |
| **Compare** | Before/after view with a draggable divider and a **100% loupe** that follows the cursor. Only offered while both the original and the optimized file are still in place. |
| **History** | Total savings, unlimited sessions (local SQLite database), undo, and a CSV log with the reason each file was skipped. |
| **Light and dark** | Follows the system or is fixed from Settings — title bar and dialogs included. |
| **Privacy** | Optionally strips GPS location from EXIF and XMP; all other metadata stays. |
| **Respects your files** | Keeps EXIF, XMP, IPTC and ICC profiles byte for byte, plus creation/modification dates, permissions and, on macOS, Finder tags. Writes atomically. |
| **Lossless when it pays** | When recompressing isn't worth it (already-compressed photos), it rewrites only the entropy coding: −3 to −7% with **identical** pixels. |
| **Never worse** | If it can't save at least 3%, the file is left as is. Photos Fino already processed and HDR photos with a gain map are skipped. |
| **Open with Fino** | macOS: drop photos or folders on the Dock icon, or use "Open With → Fino". Windows: "Open with → Fino" in Explorer; if Fino is already open, the photos go to that window. |
| **Apple Silicon, Intel and Windows** | Universal binary on macOS; on Windows 10/11 (x64) it installs per user into `%LOCALAPPDATA%\Programs\Fino`, no admin rights needed. |
| **CLI** | `fino` runs the same engine from the terminal. |

## Install

Download the installer from the [latest release](https://github.com/AmunM9/fino/releases/latest):

- **macOS**: open the `.dmg` and drag Fino to Applications.
- **Windows**: run the `Fino_x.y.z_x64-setup.exe` installer.

The installers are not yet signed by Apple or with a Windows certificate, so each system warns the first time:
on macOS, open System Settings → Privacy & Security → "Open Anyway"; on Windows, under "Windows protected your PC" click "More info" → "Run anyway".

## How the engine works

```
JPEG ─► inspect ─► decode + DCT coefficients ─► [Lanczos3 resize]
     ─► quality search (batch hint → jump to predicted crossing → interpolation)
           each probe: re-quantization in the DCT domain
                       hardest tiles at full res → global score at ½ res
           the global score's error map picks the tiles to watch
     ─► winner written progressive + optimal Huffman (same pixels)
     ─► little gain? → lossless pass (DCT coefficients untouched)
     ─► original metadata + "Fino/" marker ─► atomic write
```

- **Encoder**: re-quantizes the JPEG's own DCT coefficients with N. Robidoux's table — no
  second color conversion, DCT or chroma resampling — so each probe costs half as much and
  the only added loss is the new quantization. Progressive output with optimal Huffman
  tables; keeps the original chroma subsampling. HEIC sources and resized outputs are
  encoded from pixels with libjpeg-turbo (through mozjpeg).
- **Speed**: 89 photos of 24 MP in ~22 s on an M1 Pro (about 2 probes per photo).
- **Metric**: [zensim](https://crates.io/crates/zensim), a fast SSIMULACRA 2 approximation in
  XYB: global score (banding, color) + **worst tile** at full resolution (blocking, ringing),
  because the eye goes straight to the worst part of a photo.
- **Targets** (calibrated against true SSIMULACRA 2 on 89 camera photos):

| Strength | Global (½ res) | Worst tile | True SSIMULACRA 2 |
|---|---|---|---|
| Flawless | ≥ 94.0 | ≥ 91.0 | ≈ 89 |
| Identical | ≥ 91.5 | ≥ 86.5 | ≈ 85.5 |
| Compact | ≥ 89.0 | ≥ 82.5 | ≈ 82 |

## Layout

```
crates/fino-core   engine: inspection, codec, metric, search, metadata, files
crates/fino-cli    the `fino` binary
src-tauri          desktop app: sessions, history, backups, IPC commands
src                user interface (React + TypeScript)
docs               design notes and screenshots
```

## Development

Requirements: stable Rust, Node 18+ and `nasm` (libjpeg's Intel/AMD SIMD). On macOS, the Xcode Command Line Tools; on Windows, Visual Studio Build Tools (C++) and WebView2 (built into Windows 10/11).

```bash
npm install
```

```bash
npm run tauri dev
```

```bash
cargo test --workspace
```

```bash
npm run tauri build
```

On Windows, `npm run tauri build` writes the installer to `target/release/bundle/nsis/`. Every push to `main` and every PR also builds it on GitHub Actions (artifact `fino-windows-installer`), alongside the tests on macOS and Windows.

There is one codebase for both platforms: what is specific to each system is chosen at compile time (`#[cfg(target_os = …)]` in Rust, `src-tauri/tauri.windows.conf.json` for the window and the installer, `data-platform` in the UI). Windows has no HEIC conversion (it needs macOS's system decoder) and no Finder tags.

macOS universal binary (Apple Silicon + Intel; needs the `x86_64-apple-darwin` target):

```bash
npm run build:universal
```

To browse the UI with sample data, run `npm run dev` and open `http://localhost:1420/?state=done` (params: `state=idle|running|done`, `view=history|settings|compare`, `theme=light|dark`).

CLI:

```bash
cargo run --release -p fino-cli -- ~/Pictures/Trip
```

```bash
cargo run --release -p fino-cli -- --in-place --long-edge 2048 --strip-location photos/
```

To calibrate the targets against true SSIMULACRA 2:

```bash
cargo run --release -p fino-core --example calibrate -- folder/with/jpegs
```

## Third-party licenses

mozjpeg (IJG/BSD), zune-jpeg (MIT/Apache-2.0/Zlib), zensim (MIT/Apache-2.0), fast_image_resize (MIT/Apache-2.0) and Tauri (MIT/Apache-2.0). Fonts: Bricolage Grotesque and Geist (SIL OFL 1.1). The comparison photo comes from the Kodak Lossless True Color Image Suite; the others are the author's.
