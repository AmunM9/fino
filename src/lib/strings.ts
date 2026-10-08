import { en } from "./i18n/en";
import { es } from "./i18n/es";
import type { Dictionary, Language } from "./i18n/types";

export type { Language } from "./i18n/types";

const dictionaries: Record<Language, Dictionary> = { en, es };

/** Each language named in itself, so anyone can find their own in the list. */
export const languageNames: Record<Language, string> = { en: "English", es: "Español" };

/**
 * UI copy in the current language. These are live bindings: `setLanguage` swaps them and
 * the next render reads the new copy, so read them while rendering, not at module load.
 */
export let t = es.t;
export let strengthCopy = es.strengths;
export let skipCopy = es.skips;
export let language: Language = "es";

export function setLanguage(next: Language): void {
  const dictionary = dictionaries[next];
  language = next;
  t = dictionary.t;
  strengthCopy = dictionary.strengths;
  skipCopy = dictionary.skips;
  if (typeof document !== "undefined") document.documentElement.lang = next;
}
