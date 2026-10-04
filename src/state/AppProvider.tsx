import { ask } from "@tauri-apps/plugin-dialog";
import { createContext, useCallback, useContext, useEffect, useMemo, useReducer, useRef, useState, type ReactNode } from "react";
import { useAppearance } from "../hooks/useAppearance";
import { errorMessage, ipc } from "../lib/ipc";
import { t } from "../lib/strings";
import type { FileResult, History, Settings } from "../lib/types";
import { initialSession, sessionReducer, type SessionState } from "./session";

export type View = "optimize" | "history" | "settings";

/** Sessions per History page. */
const HISTORY_PAGE = 50;

export interface CompareTarget {
  results: FileResult[];
  index: number;
}

interface AppContextValue {
  view: View;
  setView: (view: View) => void;
  settings: Settings | null;
  updateSettings: (patch: Partial<Settings>) => void;
  history: History | null;
  session: SessionState;
  queued: number;
  start: (paths: string[]) => void;
  cancel: () => void;
  undo: (sessionId: string) => Promise<void>;
  undoing: boolean;
  /** Re-reads History and the disk (expired or hand-deleted backups, moved files). */
  refreshHistory: () => void;
  /** Shows the next page of older sessions. */
  loadMoreHistory: () => void;
  discardBackup: (sessionId: string) => Promise<void>;
  freeBackups: () => Promise<void>;
  /** A discard or free-all is in flight. */
  clearing: boolean;
  resetSession: () => void;
  compare: CompareTarget | null;
  openCompare: (target: CompareTarget | null) => void;
  notice: string | null;
  notify: (message: string) => void;
  dismissNotice: () => void;
}

const AppContext = createContext<AppContextValue | null>(null);

export function useApp(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp must be used inside <AppProvider>");
  return ctx;
}

/** Replacing originals asks first — for every batch, including ones queued mid-session. */
async function confirmReplace(settings: Settings | null, count: number): Promise<boolean> {
  const mustAsk = settings === null || (settings.outputMode === "replace" && settings.warnBeforeReplace);
  if (!mustAsk) return true;
  return ask(t.session.confirmReplace(count, settings?.keepBackups ?? true), {
    title: t.session.confirmReplaceTitle,
    kind: "warning",
    okLabel: t.session.confirmOk,
    cancelLabel: t.session.confirmCancel,
  });
}

export function AppProvider({ children }: { children: ReactNode }) {
  const [view, setView] = useState<View>("optimize");
  const [settings, setSettings] = useState<Settings | null>(null);
  const [history, setHistory] = useState<History | null>(null);
  const [session, dispatch] = useReducer(sessionReducer, initialSession);
  const [compare, openCompare] = useState<CompareTarget | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [queued, setQueued] = useState(0);
  const [undoing, setUndoing] = useState(false);
  const [clearing, setClearing] = useState(false);

  useAppearance(settings?.appearance);

  const settingsRef = useRef<Settings | null>(null);
  const saveChain = useRef<Promise<unknown>>(Promise.resolve());
  const queue = useRef<string[]>([]);
  /** True from the moment a batch is accepted (including its confirmation) until it ends. */
  const busy = useRef(false);
  /** Bumped by every History request; only the latest one may land (no stale overwrite). */
  const historySeq = useRef(0);
  /** An undo or a discard is changing backups; background refreshes wait for its result. */
  const mutating = useRef(false);
  /** How many sessions the History list shows; grows a page at a time. */
  const historyLimit = useRef(HISTORY_PAGE);

  useEffect(() => {
    ipc
      .getSettings()
      .then((s) => {
        settingsRef.current = s;
        setSettings(s);
      })
      .catch((e) => setNotice(errorMessage(e)));
  }, []);

  const loadHistory = useCallback(async (request: () => Promise<History>): Promise<void> => {
    const seq = ++historySeq.current;
    const next = await request();
    if (seq === historySeq.current) setHistory(next);
  }, []);

  const refreshHistory = useCallback(() => {
    if (mutating.current) return;
    loadHistory(() => ipc.getHistory(historyLimit.current)).catch((e) => setNotice(errorMessage(e)));
  }, [loadHistory]);

  const loadMoreHistory = useCallback(() => {
    historyLimit.current += HISTORY_PAGE;
    refreshHistory();
  }, [refreshHistory]);

  useEffect(() => refreshHistory(), [refreshHistory]);

  // Files can change behind Fino's back (Finder, other apps): re-check when the user comes back.
  useEffect(() => {
    const onVisible = () => {
      if (document.visibilityState === "visible") refreshHistory();
    };
    window.addEventListener("focus", refreshHistory);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.removeEventListener("focus", refreshHistory);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [refreshHistory]);

  useEffect(() => {
    if (view !== "optimize") refreshHistory();
  }, [view, refreshHistory]);

  // The session on screen loses its Undo button once its backup is gone (freed, expired, deleted).
  useEffect(() => {
    const entry = history?.sessions.find((s) => s.id === session.sessionId);
    if (entry && !entry.canUndo && !entry.undone) dispatch({ type: "backupsGone" });
  }, [history, session.sessionId]);

  /** Optimistic update; saves are serialized so responses can never land out of order. */
  const updateSettings = useCallback((patch: Partial<Settings>) => {
    const current = settingsRef.current;
    if (!current) return;
    const next = { ...current, ...patch };
    settingsRef.current = next;
    setSettings(next);
    saveChain.current = saveChain.current
      .then(() => ipc.saveSettings(next))
      .then((saved) => {
        if (settingsRef.current === next) {
          settingsRef.current = saved;
          setSettings(saved);
        }
      })
      .catch(async (e) => {
        setNotice(errorMessage(e));
        const stored = await ipc.getSettings().catch(() => current);
        settingsRef.current = stored;
        setSettings(stored);
      });
  }, []);

  const launch = useCallback(async (paths: string[]): Promise<void> => {
    busy.current = true;
    try {
      if (!(await confirmReplace(settingsRef.current, paths.length))) return;
      dispatch({ type: "begin" });
      await ipc.optimize(paths, (event) => dispatch({ type: "event", event }));
      await loadHistory(() => ipc.getHistory(historyLimit.current));
    } catch (e) {
      dispatch({ type: "fail", error: errorMessage(e) });
      setNotice(errorMessage(e));
    } finally {
      busy.current = false;
      const next = queue.current.splice(0);
      setQueued(0);
      if (next.length > 0) void launch(next);
    }
  }, [loadHistory]);

  const start = useCallback(
    (paths: string[]) => {
      if (paths.length === 0) return;
      setView("optimize");
      if (busy.current) {
        queue.current.push(...paths);
        setQueued(queue.current.length);
        return;
      }
      void launch(paths);
    },
    [launch],
  );

  const cancel = useCallback(() => {
    queue.current = [];
    setQueued(0);
    ipc.cancelSession().catch((e) => setNotice(errorMessage(e)));
  }, []);

  const undo = useCallback(
    async (sessionId: string) => {
      if (undoing) return;
      setUndoing(true);
      mutating.current = true;
      try {
        await loadHistory(() => ipc.undoSession(sessionId, historyLimit.current));
        if (session.sessionId === sessionId) dispatch({ type: "undone" });
      } catch (e) {
        setNotice(errorMessage(e));
        await loadHistory(() => ipc.getHistory(historyLimit.current)).catch(() => undefined);
      } finally {
        mutating.current = false;
        setUndoing(false);
      }
    },
    [session.sessionId, undoing, loadHistory],
  );

  const clearBackups = useCallback(async (job: () => Promise<History>) => {
    setClearing(true);
    mutating.current = true;
    try {
      await loadHistory(job);
    } catch (e) {
      setNotice(errorMessage(e));
      await loadHistory(() => ipc.getHistory(historyLimit.current)).catch(() => undefined);
    } finally {
      mutating.current = false;
      setClearing(false);
    }
  }, [loadHistory]);
  const discardBackup = useCallback(
    (sessionId: string) => clearBackups(() => ipc.discardBackup(sessionId, historyLimit.current)),
    [clearBackups],
  );
  const freeBackups = useCallback(() => clearBackups(() => ipc.freeBackups(historyLimit.current)), [clearBackups]);

  const value = useMemo<AppContextValue>(
    () => ({
      view,
      setView,
      settings,
      updateSettings,
      history,
      session,
      queued,
      start,
      cancel,
      undo,
      undoing,
      refreshHistory,
      loadMoreHistory,
      discardBackup,
      freeBackups,
      clearing,
      resetSession: () => dispatch({ type: "reset" }),
      compare,
      openCompare,
      notice,
      notify: setNotice,
      dismissNotice: () => setNotice(null),
    }),
    [
      view,
      settings,
      updateSettings,
      history,
      session,
      queued,
      start,
      cancel,
      undo,
      undoing,
      refreshHistory,
      loadMoreHistory,
      discardBackup,
      freeBackups,
      clearing,
      compare,
      notice,
    ],
  );

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}
