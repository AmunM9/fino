import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { CSSProperties } from "react";
import { FolderSearch, Plus, SplitSquareHorizontal, Square, Undo2 } from "lucide-react";
import { comparableResults } from "../../lib/compare";
import { formatCount, formatPercent, savedFraction, sizeParts } from "../../lib/format";
import { errorMessage } from "../../lib/ipc";
import { t } from "../../lib/strings";
import type { FileResult } from "../../lib/types";
import { useApp } from "../../state/AppProvider";
import { liveStats } from "../../state/session";
import { DropStage } from "./DropStage";
import { PhotoStack } from "./PhotoStack";
import { ResultList } from "./ResultList";
import "./optimize.css";

function RunningStage() {
  const { session, cancel, queued } = useApp();
  const done = session.results.length;
  const progress = session.total > 0 ? done / session.total : 0;
  const latest = session.results[session.results.length - 1];

  return (
    <section className="running" aria-live="polite">
      <div className="running__copy">
        <p className="eyebrow">{t.session.working}</p>
        <p className="running__count num" style={{ "--glyphs": `${formatCount(done)} / ${formatCount(session.total)}`.length } as CSSProperties}>
          {formatCount(done)}
          <span> / {formatCount(session.total)}</span>
        </p>
        <p className="running__file">{latest?.name ?? "…"}</p>
        <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={session.total} aria-valuenow={done}>
          <span style={{ transform: `scaleX(${progress})` }} />
        </div>
        <div className="running__actions">
          <button type="button" className="btn btn--ghost" onClick={cancel}>
            <Square aria-hidden /> {t.session.cancel}
          </button>
          {queued > 0 && <span className="badge">{t.drop.busyQueued(queued)}</span>}
        </div>
      </div>
      <PhotoStack results={session.results} />
    </section>
  );
}

function DoneStage() {
  const { session, undo, undoing, resetSession, openCompare, notify } = useApp();
  const stats = liveStats(session.results);
  const saved = sizeParts(stats.savedBytes);
  const fraction = savedFraction(stats.originalBytes, stats.outputBytes);
  const summary = session.summary;
  const comparable = comparableResults(session.results);
  const skipped = session.results.filter((r) => r.status === "skipped").length;
  const failed = session.results.filter((r) => r.status === "failed").length;

  const open = (result: FileResult) => {
    const index = comparable.findIndex((r) => r.id === result.id);
    if (index >= 0) openCompare({ results: comparable, index });
  };
  const reveal = () => {
    const first = comparable[0] ?? session.results.find((r) => r.outputs.length > 0);
    const path = first?.outputs[0]?.path ?? first?.path;
    if (path) revealItemInDir(path).catch((e) => notify(errorMessage(e)));
  };

  return (
    <section className="done" aria-labelledby="done-heading">
      <header className="done__hero">
        {summary?.undone ? (
          <h1 id="done-heading" className="done__figure done__figure--muted">
            {t.session.undone}
          </h1>
        ) : stats.savedBytes > 0 ? (
          <h1 id="done-heading" className="done__figure num">
            −{saved.value}
            <small>{saved.unit}</small>
          </h1>
        ) : (
          <h1 id="done-heading" className="done__figure done__figure--muted">
            {t.session.nothingSaved}
          </h1>
        )}
        <p className="done__line">
          {stats.savedBytes > 0 && !summary?.undone ? (
            <>
              {t.session.photos(stats.optimized)} · <strong>{formatPercent(fraction)}</strong> {t.session.lighter}
            </>
          ) : (
            !summary?.undone && t.session.nothingSavedHint
          )}
          {skipped > 0 && <span className="done__meta"> · {t.session.skipped(skipped)}</span>}
          {failed > 0 && <span className="done__meta done__meta--danger"> · {t.session.failed(failed)}</span>}
        </p>
        <div className="done__actions">
          <button type="button" className="btn btn--signal" disabled={comparable.length === 0} onClick={() => open(comparable[0])}>
            <SplitSquareHorizontal aria-hidden /> {t.session.compare}
          </button>
          <button type="button" className="btn" onClick={reveal}>
            <FolderSearch aria-hidden /> {t.session.reveal}
          </button>
          {summary?.canUndo && (
            <button type="button" className="btn" disabled={undoing} onClick={() => undo(summary.id)}>
              <Undo2 aria-hidden /> {t.session.undo}
            </button>
          )}
          <button type="button" className="btn btn--ghost" onClick={resetSession}>
            <Plus aria-hidden /> {t.session.newSession}
          </button>
        </div>
      </header>
      <ResultList results={session.results} onOpen={open} />
    </section>
  );
}

export function OptimizeView() {
  const { session } = useApp();
  if (session.phase === "running") return <RunningStage />;
  if (session.phase === "done") return <DoneStage />;
  return <DropStage />;
}
