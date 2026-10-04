/**
 * Browser-only preview of the UI with fake data (`npm run dev`, open :1420).
 * Never bundled into the app: main.tsx imports it only in dev builds outside Tauri.
 *
 * URL params: ?state=idle|running|done  &view=optimize|history|settings|compare
 *             &theme=system|light|dark  &demo (a 1 248-photo camera batch, see demoBatch.ts)
 *             &window=mini (compact window)  &ask=0 (no "replace originals?" question)
 * Used to capture docs/screenshots.
 */
import { emit } from "@tauri-apps/api/event";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type { FileResult, History, SessionEntry, SessionSummary, Settings } from "../lib/types";
import { DEMO_DONE_AT_START, DEMO_TOTAL, demoResult } from "./demoBatch";

const ROOT = `${location.pathname.startsWith("/") ? "" : "/"}`;
const FIXTURES = "/dev-fixtures";
const fixture = (folder: string, name: string) => `${FIXTURES}/${folder}/${name}`;

const FILES: Array<[string, number, number, number, number, number]> = [
  // name, original, output, width, height, score — Kodak test images only
  ["kodim05.jpg", 243_900, 142_341, 768, 512, 86.2],
  ["kodim23.jpg", 132_584, 63_875, 768, 512, 86.5],
  ["kodim03.jpg", 129_301, 69_839, 768, 512, 86.3],
];

function result(i: number): FileResult {
  const [name, originalBytes, outputBytes, width, height, score] = FILES[i];
  return {
    id: i,
    path: fixture("originals", name),
    name,
    status: "done",
    originalBytes,
    outputBytes,
    outputs: [{ path: fixture("Fino", name), bytes: outputBytes, width, height, sizeLabel: "Original" }],
    originalPath: fixture("originals", name),
    width,
    height,
    score,
    quality: 88,
    skipReason: null,
    error: null,
    millis: 1400,
    previewPath: fixture("Fino", name),
    lossless: false,
    converted: false,
  };
}

const sampleResults: FileResult[] = [
  ...FILES.map((_, i) => result(i)),
  { ...result(0), id: 3, name: "IMG_2041.jpg", status: "skipped", skipReason: "alreadyOptimized", outputBytes: 243_900, outputs: [] },
];

const summary = (id: string, startedAt: number, extra: Partial<SessionSummary> = {}): SessionSummary => ({
  id,
  startedAt,
  finishedAt: startedAt + 9000,
  photos: 6,
  optimized: 4,
  skipped: 2,
  failed: 0,
  originalBytes: 6_499_972,
  outputBytes: 1_816_981,
  savedBytes: 4_682_991,
  outputMode: "replace",
  strength: "identical",
  canUndo: true,
  undone: false,
  ...extra,
});

const entry = (s: SessionSummary, comparable: boolean, backupBytes: number): SessionEntry => ({ ...s, comparable, backupBytes });

const now = Date.now();
let history: History = {
  totals: { savedBytes: 51_024_000_000, originalBytes: 93_000_000_000, photos: 26_065, sessions: 41, since: now - 86400000 * 210 },
  sessions: [
    entry(summary("s4", now - 3600_000), true, 511_400_000),
    entry(summary("s3", now - 86400000, { outputMode: "export", canUndo: false, photos: 489, savedBytes: 976_000_000, originalBytes: 1_537_000_000 }), false, 0),
    entry(summary("s2", now - 86400000 * 3, { undone: true, canUndo: false, savedBytes: 0 }), false, 0),
    entry(summary("s1", now - 86400000 * 9, { strength: "compact", canUndo: false, photos: 639, savedBytes: 1_288_000_000, originalBytes: 2_346_000_000 }), true, 0),
  ],
  backupBytes: 511_400_000,
};

function dropBackups(ids: (id: string) => boolean): History {
  const sessions = history.sessions.map((s) => (ids(s.id) ? { ...s, canUndo: false, backupBytes: 0 } : s));
  history = { ...history, sessions, backupBytes: sessions.reduce((sum, s) => sum + s.backupBytes, 0) };
  return history;
}

let settings: Settings = {
  outputMode: "replace",
  warnBeforeReplace: true,
  keepBackups: true,
  backupRetentionDays: 7,
  exportDir: null,
  sizes: [{ id: "original", label: "Original", mode: null, pixels: null }],
  strength: "identical",
  stripLocation: false,
  skipOptimized: true,
  appearance: "system",
  compactWindow: false,
  convertHeic: true,
};

/** Clicks a button by its accessible name once the UI has rendered it. */
function clickLater(name: string, delay: number): void {
  setTimeout(() => {
    const buttons = [...document.querySelectorAll<HTMLButtonElement>("button")];
    buttons.find((b) => b.getAttribute("aria-label") === name || b.textContent?.trim() === name)?.click();
  }, delay);
}

type Emit = (event: unknown) => void;

/** Pace of the demo batch: one photo lands on the pile per tick. */
const DEMO_TICK_MS = 900;

let pendingOpen: string[] = [];

function channelEmitter(payload: Record<string, unknown> | undefined): Emit {
  const channel = payload?.onEvent as { id: number } | undefined;
  const callbacks = (window as unknown as { __TAURI_INTERNALS__: { callbacks?: Map<number, (m: unknown) => void> } }).__TAURI_INTERNALS__;
  let index = 0;
  return (event) => {
    const cb = callbacks.callbacks?.get(channel?.id ?? -1);
    cb?.({ message: event, index: index++ });
  };
}

export function installMocks(): void {
  const params = new URLSearchParams(location.search);
  const view = params.get("view");
  const state = view === "compare" ? "done" : (params.get("state") ?? "idle");
  const theme = params.get("theme");
  const demo = params.has("demo");
  const results = demo ? Array.from({ length: DEMO_TOTAL }, (_, i) => demoResult(i)) : sampleResults;
  if (theme === "light" || theme === "dark") settings = { ...settings, appearance: theme };
  if (params.get("window") === "mini") settings = { ...settings, compactWindow: true };
  if (params.get("ask") === "0") settings = { ...settings, warnBeforeReplace: false };
  mockWindows("main");
  mockIPC((cmd, payload) => {
    switch (cmd) {
      case "get_settings":
        return settings;
      case "save_settings":
        settings = (payload as { settings: Settings }).settings;
        return settings;
      case "get_history":
        return history;
      case "discard_backup":
        return dropBackups((id) => id === (payload as { id: string }).id);
      case "free_backups":
        return dropBackups(() => true);
      case "take_opened_paths": {
        const paths = pendingOpen;
        pendingOpen = [];
        return paths;
      }
      case "session_results":
        return results;
      case "optimize": {
        const emit = channelEmitter(payload as Record<string, unknown>);
        emit({ kind: "started", sessionId: "s5", total: demo ? DEMO_TOTAL : results.length });
        if (state === "running" && demo) {
          results.slice(0, DEMO_DONE_AT_START).forEach((r) => emit({ kind: "file", result: r }));
          let next = DEMO_DONE_AT_START;
          setInterval(() => emit({ kind: "file", result: demoResult(next++) }), DEMO_TICK_MS);
          return new Promise(() => undefined);
        }
        const shown = state === "running" ? results.slice(0, 2) : results;
        shown.forEach((r) => emit({ kind: "file", result: r }));
        if (state === "running") return new Promise(() => undefined);
        const s = summary("s5", now);
        emit({ kind: "finished", summary: s });
        return s;
      }
      case "plugin:dialog|message": {
        // ask() resolves true when the reply equals its OK label.
        const buttons = (payload as { buttons?: { OkCancelCustom?: [string, string] } }).buttons;
        return buttons?.OkCancelCustom?.[0] ?? "Yes";
      }
      default:
        return null;
    }
  }, { shouldMockEvents: true });
  const internals = (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__;
  internals.convertFileSrc = (path: string) => `${ROOT}${path}`;
  if (state !== "idle") {
    pendingOpen = results.map((r) => r.path);
    setTimeout(() => void emit("open-paths"), 600);
  }
  if (view === "history") clickLater("Historial", 300);
  if (view === "settings") clickLater("Ajustes", 300);
  if (view === "compare") clickLater("Comparar", 1500);
}
