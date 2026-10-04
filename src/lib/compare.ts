import type { FileResult } from "./types";

/** Results that have both an original and an optimized file to show side by side. */
export function isComparable(r: FileResult): boolean {
  return r.status === "done" && r.originalPath !== null && r.outputs.length > 0;
}

export function comparableResults(results: FileResult[]): FileResult[] {
  return results.filter(isComparable).sort((a, b) => a.id - b.id);
}
