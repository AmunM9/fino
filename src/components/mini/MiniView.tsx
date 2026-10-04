import { CircleCheck, Maximize2, Plus } from "lucide-react";
import { useState } from "react";
import { usePhotoPicker } from "../../hooks/usePhotoPicker";
import { formatCount, formatPercent, savedFraction } from "../../lib/format";
import { fileUrl } from "../../lib/ipc";
import { t } from "../../lib/strings";
import { useApp, type ReplacePrompt } from "../../state/AppProvider";
import { liveStats } from "../../state/session";
import { SavingsGauge } from "../panel/SavingsGauge";
import "./mini.css";

/** The two newest thumbnails: the top one fades in over the previous as photos finish. */
function PhotoBackdrop({ previews }: { previews: string[] }) {
  return (
    <div className="mini__backdrop" aria-hidden="true" data-tauri-drag-region>
      {previews.map((path) => (
        <img key={path} src={fileUrl(path)} alt="" draggable={false} />
      ))}
      <div className="mini__scrim" data-tauri-drag-region />
    </div>
  );
}

function ReplaceSheet({ prompt }: { prompt: ReplacePrompt }) {
  const [dontAsk, setDontAsk] = useState(false);
  return (
    <div className="mini__sheet" role="alertdialog" aria-labelledby="mini-sheet-title" aria-describedby="mini-sheet-body">
      <h2 id="mini-sheet-title">{t.mini.replaceTitle}</h2>
      <p id="mini-sheet-body">{t.mini.replaceBody(prompt.count, prompt.keepBackups)}</p>
      <div className="mini__sheet-actions">
        <button type="button" className="btn" onClick={() => prompt.answer(false, false)}>
          {t.session.confirmCancel}
        </button>
        <button type="button" className="btn btn--signal" autoFocus onClick={() => prompt.answer(true, dontAsk)}>
          {t.mini.continue}
        </button>
      </div>
      <label className="mini__check">
        <input type="checkbox" checked={dontAsk} onChange={(e) => setDontAsk(e.target.checked)} />
        {t.mini.dontAskAgain}
      </label>
    </div>
  );
}

/** The compact droplet window: drop photos on it and watch the savings dial fill. */
export function MiniView({ isOver }: { isOver: boolean }) {
  const { session, cancel, updateSettings, replacePrompt } = useApp();
  const pick = usePhotoPicker();
  const stats = liveStats(session.results);
  const running = session.phase === "running";
  const done = session.phase === "done";
  const previews = running
    ? session.results.flatMap((r) => (r.previewPath ? [r.previewPath] : [])).slice(-2)
    : [];
  const fraction = savedFraction(stats.originalBytes, stats.outputBytes);

  // The mini window has no result list, so HEIC left alone is said here instead.
  const heicSkipped = session.results.filter((r) => r.skipReason === "conversionOff").length;
  const footer = running
    ? t.mini.progress(formatCount(stats.processed), formatCount(session.total))
    : done && heicSkipped > 0
      ? t.convert.skippedMini(heicSkipped)
      : done && stats.savedBytes > 0
        ? t.mini.done(formatCount(stats.optimized), formatPercent(fraction))
        : t.mini.drop;

  return (
    <div className="mini" data-phase={session.phase} data-over={isOver} data-photo={previews.length > 0} data-tauri-drag-region>
      {previews.length > 0 && <PhotoBackdrop previews={previews} />}
      <button
        type="button"
        className="icon-btn mini__expand"
        aria-label={t.mini.expand}
        title={t.mini.expand}
        onClick={() => updateSettings({ compactWindow: false })}
      >
        <Maximize2 />
      </button>

      <div className="mini__dial" data-tauri-drag-region>
        <SavingsGauge
          savedBytes={stats.savedBytes}
          fraction={fraction}
          active={session.phase !== "idle"}
          badge={done && stats.savedBytes > 0 ? <CircleCheck aria-hidden /> : null}
        />
      </div>

      {running ? (
        <button type="button" className="btn mini__stop" onClick={cancel}>
          {t.session.cancel}
        </button>
      ) : (
        <button type="button" className="mini__add" aria-label={t.mini.add} title={t.mini.add} onClick={() => pick(false)}>
          <Plus aria-hidden />
        </button>
      )}
      <p className="mini__footer num" aria-live="polite" data-tauri-drag-region>
        {isOver ? t.drop.hovering : footer}
      </p>

      {replacePrompt && <ReplaceSheet prompt={replacePrompt} />}
    </div>
  );
}
