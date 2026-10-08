import { useEffect } from "react";
import type { Appearance } from "../lib/types";

/** Read by public/theme-boot.js before first paint. */
const CACHE_KEY = "fino.appearance";
const LIGHT_QUERY = "(prefers-color-scheme: light)";

export function resolveTheme(appearance: Appearance, systemIsLight: boolean): "light" | "dark" {
  if (appearance === "system") return systemIsLight ? "light" : "dark";
  return appearance;
}

/** Applies the chosen appearance to <html data-theme>, following the system live when "system". */
export function useAppearance(appearance: Appearance | undefined): void {
  useEffect(() => {
    if (!appearance) return;
    try {
      localStorage.setItem(CACHE_KEY, appearance);
    } catch {
      // Storage unavailable: the boot script falls back to the system theme.
    }
    const media = window.matchMedia(LIGHT_QUERY);
    const apply = () => {
      document.documentElement.dataset.theme = resolveTheme(appearance, media.matches);
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [appearance]);
}
