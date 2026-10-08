import type { es } from "./es";

export type Language = "en" | "es";

export interface StrengthCopy {
  name: string;
  promise: string;
  typical: string;
}

/** Every language has exactly the Spanish dictionary's keys: a missing one fails the build. */
export type Dictionary = typeof es;
