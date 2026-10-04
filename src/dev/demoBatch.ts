/**
 * A large, realistic batch for the dev preview (`?demo`): a wedding photographer's 24 MP
 * camera files saving ~69 %, the ratio measured on real shoots. Previews live in
 * dev-fixtures/camera (not in the repo).
 */
import type { FileResult } from "../lib/types";

export const DEMO_TOTAL = 1248;
/** Photos already finished when the preview opens mid-batch. */
export const DEMO_DONE_AT_START = 842;

const PREVIEWS = [
  "_MAN4943", "_MAN4949", "_MAN5080", "_MAN4971", "_MAN6168", "_MAN4947",
  "_MAN5013", "_MAN6175", "_MAN4963", "_MAN5050", "_MAN7452", "_MAN6164",
];
const FIRST_FRAME = 4936;
const SKIP_EVERY = 53;

/** Deterministic 0–1 noise so every run (and screenshot) looks the same. */
const noise = (i: number) => {
  const x = Math.sin(i * 12.9898) * 43758.5453;
  return x - Math.floor(x);
};

export function demoResult(i: number): FileResult {
  const name = `_MAN${FIRST_FRAME + i * 3}.JPG`;
  const originalBytes = Math.round(4_600_000 + noise(i) * 2_600_000);
  const preview = `/dev-fixtures/camera/${PREVIEWS[i % PREVIEWS.length]}.jpg`;
  const skipped = i % SKIP_EVERY === SKIP_EVERY - 1;
  const outputBytes = skipped ? originalBytes : Math.round(originalBytes * (0.27 + noise(i + 7) * 0.09));
  return {
    id: i,
    path: `/Volumes/Fotos/Boda/${name}`,
    name,
    status: skipped ? "skipped" : "done",
    originalBytes,
    outputBytes,
    outputs: skipped
      ? []
      : [{ path: preview, bytes: outputBytes, width: 6048, height: 4024, sizeLabel: "Original" }],
    originalPath: skipped ? null : preview,
    width: 6048,
    height: 4024,
    score: skipped ? null : 85 + noise(i + 3) * 1.8,
    quality: skipped ? null : 74 + Math.round(noise(i + 5) * 10),
    skipReason: skipped ? "alreadyOptimized" : null,
    error: null,
    millis: 560 + Math.round(noise(i + 11) * 300),
    previewPath: skipped ? null : preview,
    lossless: false,
    converted: false,
  };
}
