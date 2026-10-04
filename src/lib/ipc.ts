import { Channel, convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { FileResult, History, SessionEvent, SessionSummary, Settings } from "./types";

export const ipc = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  getHistory: () => invoke<History>("get_history"),
  sessionResults: (id: string) => invoke<FileResult[]>("session_results", { id }),
  cancelSession: () => invoke<void>("cancel_session"),
  undoSession: (id: string) => invoke<History>("undo_session", { id }),
  discardBackup: (id: string) => invoke<History>("discard_backup", { id }),
  freeBackups: () => invoke<History>("free_backups"),
  exportLog: (id: string, destination: string) => invoke<void>("export_log", { id, destination }),
  takeOpenedPaths: () => invoke<string[]>("take_opened_paths"),

  optimize(paths: string[], onEvent: (event: SessionEvent) => void): Promise<SessionSummary> {
    const channel = new Channel<SessionEvent>();
    channel.onmessage = onEvent;
    return invoke<SessionSummary>("optimize", { paths, onEvent: channel });
  },
};

/** URL the webview can load for a local photo (scope is granted by the backend). */
export const fileUrl = (path: string) => convertFileSrc(path);

export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
