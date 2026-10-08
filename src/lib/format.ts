import { platform } from "./platform";

const LOCALE = "es";

// Sizes match the system's file manager for the same files: Finder counts in thousands,
// Windows Explorer in 1024s (and still calls them KB / MB).
const KB = platform === "windows" ? 1024 : 1000;
const MB = KB * KB;
const GB = MB * KB;

const oneDecimal = new Intl.NumberFormat(LOCALE, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
const twoDecimals = new Intl.NumberFormat(LOCALE, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
const integer = new Intl.NumberFormat(LOCALE, { maximumFractionDigits: 0 });

export interface SizeParts {
  value: string;
  unit: "KB" | "MB" | "GB";
}

/** Splits a byte count into a display number and unit, e.g. 13 000 000 → { "13,0", "MB" }. */
export function sizeParts(bytes: number): SizeParts {
  const b = Math.max(0, bytes);
  if (b >= GB) return { value: twoDecimals.format(b / GB), unit: "GB" };
  if (b >= MB) return { value: oneDecimal.format(b / MB), unit: "MB" };
  return { value: integer.format(b / KB), unit: "KB" };
}

export function formatBytes(bytes: number): string {
  const { value, unit } = sizeParts(bytes);
  return `${value} ${unit}`;
}

/** Fraction of bytes removed, 0–1. */
export function savedFraction(original: number, output: number): number {
  if (original <= 0) return 0;
  return Math.max(0, 1 - output / original);
}

export function formatPercent(fraction: number): string {
  return `${integer.format(Math.round(fraction * 100))} %`;
}

export function formatCount(n: number): string {
  return integer.format(n);
}

export function formatScore(score: number): string {
  return oneDecimal.format(score);
}

const dateTime = new Intl.DateTimeFormat(LOCALE, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
const monthYear = new Intl.DateTimeFormat(LOCALE, { month: "short", year: "numeric" });

export const formatDateTime = (ms: number) => dateTime.format(new Date(ms));
export const formatMonthYear = (ms: number) => monthYear.format(new Date(ms));

export function formatDuration(ms: number): string {
  if (ms < 1000) return `${integer.format(ms)} ms`;
  const s = ms / 1000;
  if (s < 60) return `${oneDecimal.format(s)} s`;
  return `${integer.format(Math.floor(s / 60))} min ${integer.format(Math.round(s % 60))} s`;
}

/** Last component of a macOS (`/`) or Windows (`\`) path. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

/** Shortens /Users/me/Pictures/Trip → ~/Pictures/Trip on macOS; Windows paths stay whole. */
export function prettyPath(path: string): string {
  return path.replace(/^\/Users\/[^/]+/, "~");
}
