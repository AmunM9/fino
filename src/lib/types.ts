/** IPC contract with the Rust side (src-tauri/src/model.rs). Keep in sync. */

export type Strength = "pristine" | "identical" | "compact";

export type ResizeMode = "longEdge" | "maxWidth" | "maxHeight";

export interface SizePreset {
  id: string;
  label: string;
  /** null = keep original dimensions */
  mode: ResizeMode | null;
  pixels: number | null;
}

export type OutputMode = "replace" | "export";

/** "system" follows macOS live. */
export type Appearance = "system" | "light" | "dark";

export interface Settings {
  outputMode: OutputMode;
  warnBeforeReplace: boolean;
  keepBackups: boolean;
  backupRetentionDays: number;
  /** null = a "Fino" folder next to each original */
  exportDir: string | null;
  sizes: SizePreset[];
  strength: Strength;
  stripLocation: boolean;
  skipOptimized: boolean;
  appearance: Appearance;
}

export type FileStatus = "done" | "skipped" | "failed";

export type SkipReason =
  | "alreadyOptimized"
  | "noGain"
  | "unsupported"
  | "cmyk"
  | "exoticJpeg"
  | "tooLarge"
  | "hdrGainMap"
  | "embeddedMedia";

export interface OutputFile {
  path: string;
  bytes: number;
  width: number;
  height: number;
  sizeLabel: string;
}

export interface FileResult {
  id: number;
  path: string;
  name: string;
  status: FileStatus;
  originalBytes: number;
  outputBytes: number;
  outputs: OutputFile[];
  /** Where the untouched original can be viewed (the file itself or its backup). */
  originalPath: string | null;
  width: number;
  height: number;
  /** Perceptual similarity to the source, SSIMULACRA 2 scale (100 = identical). */
  score: number | null;
  quality: number | null;
  skipReason: SkipReason | null;
  error: string | null;
  millis: number;
  /** Small upright JPEG for thumbnails (session cache). */
  previewPath: string | null;
  /** Output decodes to exactly the original pixels. */
  lossless: boolean;
}

export interface SessionSummary {
  id: string;
  startedAt: number;
  finishedAt: number;
  photos: number;
  optimized: number;
  skipped: number;
  failed: number;
  originalBytes: number;
  outputBytes: number;
  savedBytes: number;
  outputMode: OutputMode;
  strength: Strength;
  canUndo: boolean;
  undone: boolean;
}

export type SessionEvent =
  | { kind: "started"; sessionId: string; total: number }
  | { kind: "file"; result: FileResult }
  | { kind: "finished"; summary: SessionSummary };

export interface Totals {
  savedBytes: number;
  originalBytes: number;
  photos: number;
  sessions: number;
  since: number | null;
}

/** A session as History shows it: stored summary + what is on disk right now. */
export interface SessionEntry extends SessionSummary {
  /** At least one photo still has its original and optimized file in place. */
  comparable: boolean;
  /** Space this session's undo backups take now (0 = none left). */
  backupBytes: number;
}

export interface History {
  totals: Totals;
  sessions: SessionEntry[];
  /** All undo backups on disk. */
  backupBytes: number;
}
