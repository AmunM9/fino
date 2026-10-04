import type { FileResult, SessionEvent, SessionSummary } from "../lib/types";

export type SessionPhase = "idle" | "running" | "done";

export interface SessionState {
  phase: SessionPhase;
  sessionId: string | null;
  total: number;
  results: FileResult[];
  summary: SessionSummary | null;
  error: string | null;
}

export const initialSession: SessionState = {
  phase: "idle",
  sessionId: null,
  total: 0,
  results: [],
  summary: null,
  error: null,
};

export type SessionAction =
  | { type: "begin" }
  | { type: "event"; event: SessionEvent }
  | { type: "fail"; error: string }
  | { type: "undone" }
  | { type: "backupsGone" }
  | { type: "reset" };

export function sessionReducer(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case "begin":
      return { ...initialSession, phase: "running" };
    case "event":
      return applyEvent(state, action.event);
    case "fail":
      return { ...state, phase: state.results.length > 0 ? "done" : "idle", error: action.error };
    case "undone":
      return state.summary
        ? { ...state, summary: { ...state.summary, canUndo: false, undone: true, savedBytes: 0 } }
        : state;
    case "backupsGone":
      return state.summary?.canUndo ? { ...state, summary: { ...state.summary, canUndo: false } } : state;
    case "reset":
      return initialSession;
  }
}

function applyEvent(state: SessionState, event: SessionEvent): SessionState {
  switch (event.kind) {
    case "started":
      return { ...state, sessionId: event.sessionId, total: event.total };
    case "file":
      return { ...state, results: [...state.results, event.result] };
    case "finished":
      return { ...state, phase: "done", summary: event.summary };
  }
}

export interface LiveStats {
  processed: number;
  optimized: number;
  originalBytes: number;
  outputBytes: number;
  savedBytes: number;
}

/** Running totals derived from results — the right panel updates as each file lands. */
export function liveStats(results: FileResult[]): LiveStats {
  return results.reduce<LiveStats>(
    (acc, r) => ({
      processed: acc.processed + 1,
      optimized: acc.optimized + (r.status === "done" ? 1 : 0),
      originalBytes: acc.originalBytes + r.originalBytes,
      outputBytes: acc.outputBytes + r.outputBytes,
      savedBytes: acc.savedBytes + Math.max(0, r.originalBytes - r.outputBytes),
    }),
    { processed: 0, optimized: 0, originalBytes: 0, outputBytes: 0, savedBytes: 0 },
  );
}
