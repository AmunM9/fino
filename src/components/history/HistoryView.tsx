import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useConfirm } from "../ui/ConfirmDialog";
import { ArchiveX, Download, SplitSquareHorizontal, Undo2 } from "lucide-react";
import { comparableResults } from "../../lib/compare";
import { formatBytes, formatCount, formatDateTime, formatMonthYear, formatPercent, sizeParts } from "../../lib/format";
import { errorMessage, ipc } from "../../lib/ipc";
import { strengthCopy, t } from "../../lib/strings";
import type { Origin, SessionEntry } from "../../lib/types";
import { useApp } from "../../state/AppProvider";
import "./history.css";

function Tile({ label, value, unit, wide }: { label: string; value: string; unit?: string; wide?: boolean }) {
  return (
    <div className="tile" data-wide={wide}>
      <span className="tile__label">{label}</span>
      <span className="tile__value num">
        {value}
        {unit && <small>{unit}</small>}
      </span>
    </div>
  );
}

function OriginName({ origin }: { origin: Origin }) {
  switch (origin.kind) {
    case "files":
      return (
        <>
          <strong>{origin.first}</strong>
          {origin.more > 0 && <span className="origin__dim"> {t.history.originMore(origin.more)}</span>}
        </>
      );
    case "folder":
      return (
        <>
          {origin.parent && <span className="origin__dim">{origin.parent} › </span>}
          <strong>{origin.name}</strong>
        </>
      );
    case "folders":
      return (
        <>
          <strong>{origin.name}</strong>
          <span className="origin__dim"> › {t.history.originFolders(origin.count)}</span>
        </>
      );
    case "scattered":
      return <strong>{t.history.originScattered}</strong>;
  }
}

/** Where the photos came from, over how they were handled. Clicking shows it in Finder / Explorer. */
function OriginCell({ session }: { session: SessionEntry }) {
  const { notify } = useApp();
  const { origin } = session;
  const mode = `${session.outputMode === "replace" ? t.history.replaced : t.history.exported} · ${strengthCopy[session.strength].name}`;
  const reveal = origin?.reveal;
  const name = origin ? <OriginName origin={origin} /> : <strong>—</strong>;
  return (
    <td className="origin">
      {reveal ? (
        <button
          type="button"
          className="origin__name"
          title={t.history.originReveal(origin.path ?? reveal)}
          onClick={() => revealItemInDir(reveal).catch((e) => notify(errorMessage(e)))}
        >
          {name}
        </button>
      ) : (
        <span className="origin__name" title={origin?.path ?? undefined}>
          {name}
        </span>
      )}
      <span className="origin__mode">{mode}</span>
    </td>
  );
}

function SessionRow({ session }: { session: SessionEntry }) {
  const { undo, undoing, discardBackup, clearing, openCompare, notify, refreshHistory } = useApp();
  const confirm = useConfirm();
  const fraction = session.originalBytes > 0 ? session.savedBytes / session.originalBytes : 0;

  const exportLog = async () => {
    try {
      const date = new Date(session.startedAt).toISOString().slice(0, 10);
      const destination = await save({ defaultPath: `Fino ${date}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
      if (destination) await ipc.exportLog(session.id, destination);
    } catch (e) {
      notify(errorMessage(e));
    }
  };
  const compare = async () => {
    try {
      const results = comparableResults(await ipc.sessionResults(session.id));
      if (results.length > 0) {
        openCompare({ results, index: 0 });
      } else {
        notify(t.compare.unavailable);
        refreshHistory();
      }
    } catch (e) {
      notify(errorMessage(e));
    }
  };
  const confirmUndo = async () => {
    if ((await confirm({ title: t.history.undoConfirm, okLabel: t.session.undo })).ok) {
      await undo(session.id);
    }
  };
  const backupSize = formatBytes(session.backupBytes);
  const confirmDiscard = async () => {
    const { ok } = await confirm({
      title: t.history.discardTitle,
      body: t.history.discardConfirm(backupSize),
      okLabel: t.history.discardOk,
      danger: true,
    });
    if (ok) await discardBackup(session.id);
  };
  const compareLabel = session.comparable ? t.session.compare : t.history.nothingToCompare;

  return (
    <tr data-undone={session.undone}>
      <td>{formatDateTime(session.startedAt)}</td>
      <td className="num">{formatCount(session.photos)}</td>
      <OriginCell session={session} />
      <td className="num">{session.undone ? "—" : formatBytes(session.savedBytes)}</td>
      <td>
        {!session.undone && session.savedBytes > 0 && <span className="badge badge--signal num">−{formatPercent(fraction)}</span>}
      </td>
      <td className="history__actions">
        <button type="button" className="icon-btn" aria-label={compareLabel} title={compareLabel} onClick={compare} disabled={!session.comparable}>
          <SplitSquareHorizontal />
        </button>
        <button type="button" className="icon-btn" aria-label={t.history.exportLog} title={t.history.exportLog} onClick={exportLog}>
          <Download />
        </button>
        {session.canUndo && (
          <button type="button" className="icon-btn" aria-label={t.session.undo} title={t.session.undo} onClick={confirmUndo} disabled={undoing || clearing}>
            <Undo2 />
          </button>
        )}
        {session.canUndo && (
          <button
            type="button"
            className="icon-btn icon-btn--danger"
            aria-label={t.history.discardWithSize(backupSize)}
            title={t.history.discardWithSize(backupSize)}
            onClick={confirmDiscard}
            disabled={undoing || clearing}
          >
            <ArchiveX />
          </button>
        )}
      </td>
    </tr>
  );
}

export function HistoryView() {
  const { history, loadMoreHistory } = useApp();
  const hiddenSessions = history ? history.totals.sessions - history.sessions.length : 0;
  const totals = history?.totals;
  const saved = sizeParts(totals?.savedBytes ?? 0);
  const avg = totals && totals.originalBytes > 0 ? totals.savedBytes / totals.originalBytes : 0;

  return (
    <section className="page" aria-labelledby="history-heading">
      <header className="page__header">
        <h1 id="history-heading">{t.history.title}</h1>
        <p>{t.history.subtitle}</p>
      </header>

      <div className="tiles">
        <div className="tile tile--hero">
          <span className="tile__label">{t.history.totalSaved}</span>
          <span className="tile__hero num">
            {saved.value}
            <small>{saved.unit}</small>
          </span>
          {totals?.since != null && <span className="tile__foot">{t.panel.since(formatMonthYear(totals.since))}</span>}
        </div>
        <Tile label={t.history.photos} value={formatCount(totals?.photos ?? 0)} />
        <Tile label={t.history.avgReduction} value={formatPercent(avg)} />
        <Tile label={t.history.sessions} value={formatCount(totals?.sessions ?? 0)} wide />
      </div>

      {history && history.sessions.length > 0 ? (
        <div className="history__table-wrap">
          <table className="history__table">
            <thead>
              <tr>
                <th>{t.history.date}</th>
                <th>{t.history.files}</th>
                <th>{t.history.origin}</th>
                <th>{t.history.saved}</th>
                <th />
                <th aria-label="Acciones" />
              </tr>
            </thead>
            <tbody>
              {history.sessions.map((s) => (
                <SessionRow key={s.id} session={s} />
              ))}
            </tbody>
          </table>
          {hiddenSessions > 0 && (
            <button type="button" className="history__more" onClick={loadMoreHistory}>
              {t.history.loadMore(hiddenSessions)}
            </button>
          )}
        </div>
      ) : (
        <p className="page__empty">{t.history.empty}</p>
      )}
    </section>
  );
}
