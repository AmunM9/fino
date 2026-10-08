/**
 * The OS the app runs on, read once from the web view (WKWebView on macOS, WebView2 on
 * Windows). public/theme-boot.js sets the same value as `<html data-platform>` before first
 * paint, so CSS can adapt the window chrome without waiting for React.
 */
export type Platform = "macos" | "windows" | "linux";

export function detectPlatform(userAgent: string): Platform {
  if (/Windows/i.test(userAgent)) return "windows";
  if (/Mac OS X|Macintosh/i.test(userAgent)) return "macos";
  return "linux";
}

function isPlatform(value: string | null): value is Platform {
  return value === "macos" || value === "windows" || value === "linux";
}

/** Dev previews in the browser can pose as another OS: `?platform=windows`. */
const previewAs = import.meta.env.DEV ? new URLSearchParams(location.search).get("platform") : null;

export const platform: Platform = isPlatform(previewAs)
  ? previewAs
  : typeof navigator === "undefined"
    ? "macos"
    : detectPlatform(navigator.userAgent);

/** HEIC → JPEG needs the system's HEIC decoder, which only macOS provides (see fino-core). */
export const heicConversionAvailable = platform === "macos";
