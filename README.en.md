<sub>A JPEGmini alternative</sub>

# Fino

**Lighter photos that look exactly the same.**

English · [Español](README.md)

Fino is a macOS app that recompresses JPEGs *perceptually*. It tries smaller and smaller versions of each photo, compares every candidate with the original region by region, and keeps the smallest one that shows no visible artifact. Built from open components, running offline on your Mac.

```
89 camera photos (24 MP)   511 MB → 156 MB   (−69.4%)   ~0.6 s per photo
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
| **Backups** | Stored in `~/Library/Application Support/app.fino.desktop/backups/`. They expire on their own (checked when Fino opens and whenever you return to it). Settings shows how much space they take and **frees** them; History discards a single session's backup. Backups deleted by hand are detected. |
| **HEIC → JPEG (optional)** | Off by default: HEIC is already the lighter format and Fino leaves it alone. Turned on, each HEIC becomes a JPEG at the *Compact* strength — about 25% lighter than the HEIC on iPhone photos — with EXIF, XMP, colour profile and orientation intact; HDR, depth and portrait data are dropped. Live Photos stay paired with their video. |
| **Mini window** | A small dial that floats over other apps: drop photos or folders on it and watch each photo go by as it is optimized. |
| **Export a copy** | Leaves originals untouched. Copies go to a `Fino` folder next to each photo or to a fixed destination, mirroring the folder structure. |
| **Several sizes** | Up to 4 sizes per export (long edge, max width or max height). Honors EXIF orientation and never upscales. |
| **Strength** | *Flawless* (no difference even under a loupe), *Identical* (recommended: looks the same as the original) and *Compact* (a little lighter, for the web and social media). |
| **Compare** | Before/after view with a draggable divider and a **100% loupe** that follows the cursor. Only offered while both the original and the optimized file are still in place. |
| **History** | Total savings, unlimited sessions (local SQLite database), undo, and a CSV log with the reason each file was skipped. |
| **Light and dark** | Follows macOS or is fixed from Settings — title bar and dialogs included. |
| **Privacy** | Optionally strips GPS location from EXIF and XMP; all other metadata stays. |
| **Respects your files** | Keeps EXIF, XMP, IPTC and ICC profiles byte for byte, plus creation/modification dates, Finder tags and permissions. Writes atomically. |
| **Lossless when it pays** | When recompressing isn't worth it (already-compressed photos), it rewrites only the entropy coding: −3 to −7% with **identical** pixels. |
| **Never worse** | If it can't save at least 3%, the file is left as is. Photos Fino already processed and HDR photos with a gain map are skipped. |
| **Finder** | Drop photos or folders on the Dock icon, or use "Open With → Fino". |
| **Apple Silicon and Intel** | Universal binary. |
| **CLI** | `fino` runs the same engine from the terminal. |

## How the engine works

```
JPEG ─► inspect ─► decode ─► [Lanczos3 resize]
     ─► quality search with the fast encoder (batch hint → gallop → bisection)
           each probe: global score at ½ res → hardest tiles at full res
           winner: verified on EVERY tile
     ─► winner written progressive + optimal Huffman (same pixels)
     ─► little gain? → lossless pass (DCT coefficients untouched)
     ─► original metadata + "Fino/" marker ─► atomic write
```

- **Encoder**: libjpeg-turbo (through mozjpeg in fast mode) with N. Robidoux's quantization
  table; progressive output with optimal Huffman tables. Keeps the original chroma subsampling.
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

Requirements: stable Rust, Node 18+ and the Xcode Command Line Tools.

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

Universal binary (Apple Silicon + Intel; needs `nasm` for Intel SIMD and the `x86_64-apple-darwin` target):

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
