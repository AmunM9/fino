import { platform } from "./platform";
import { language, type Language } from "./strings";

// Sizes match the system's file manager for the same files: Finder counts in thousands,
// Windows Explorer in 1024s (and still calls them KB / MB).
const KB = platform === "windows" ? 1024 : 1000;
const MB = KB * KB;
const GB = MB * KB;

interface Formats {
  oneDecimal: Intl.NumberFormat;
  twoDecimals: Intl.NumberFormat;
  integer: Intl.NumberFormat;
  percent: Intl.NumberFormat;
  dateTime: Intl.DateTimeFormat;
  monthYear: Intl.DateTimeFormat;
}

function makeFormats(locale: Language): Formats {
  return {
    oneDecimal: new Intl.NumberFormat(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
    twoDecimals: new Intl.NumberFormat(locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
    integer: new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }),
    // "69 %" in Spanish, "69%" in English.
    percent: new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 0 }),
    dateTime: new Intl.DateTimeFormat(locale, { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }),
    monthYear: new Intl.DateTimeFormat(locale, { month: "short", year: "numeric" }),
  };
}

const cache = new Map<Language, Formats>();

/** Number and date formats for the current UI language, built once per language. */
function formats(): Formats {
  let found = cache.get(language);
  if (!found) {
    found = makeFormats(language);
    cache.set(language, found);
  }
  return found;
}

export interface SizeParts {
  value: string;
  unit: "KB" | "MB" | "GB";
}

/** Splits a byte count into a display number and unit, e.g. 13 000 000 → { "13,0", "MB" }. */
export function sizeParts(bytes: number): SizeParts {
  const { oneDecimal, twoDecimals, integer } = formats();
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
  return formats().percent.format(Math.round(fraction * 100) / 100);
}

export function formatCount(n: number): string {
  return formats().integer.format(n);
}

export function formatScore(score: number): string {
  return formats().oneDecimal.format(score);
}

export const formatDateTime = (ms: number): string => formats().dateTime.format(new Date(ms));
export const formatMonthYear = (ms: number): string => formats().monthYear.format(new Date(ms));

export function formatDuration(ms: number): string {
  const { oneDecimal, integer } = formats();
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
