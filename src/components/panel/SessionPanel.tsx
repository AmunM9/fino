import { formatBytes, formatCount, formatMonthYear, formatPercent, savedFraction, sizeParts } from "../../lib/format";
import { t } from "../../lib/strings";
import { useApp } from "../../state/AppProvider";
import { liveStats } from "../../state/session";
import { SavingsGauge } from "./SavingsGauge";
import "./panel.css";

export function SessionPanel() {
  const { session, history } = useApp();
  const live = liveStats(session.results);
  const fraction = savedFraction(live.originalBytes, live.outputBytes);
  const totals = history?.totals;
  const allTime = sizeParts(totals?.savedBytes ?? 0);

  return (
    <aside className="panel" aria-label="Resumen de la sesión">
      <SavingsGauge savedBytes={live.savedBytes} fraction={fraction} active={session.phase !== "idle"} />

      <dl className="panel__stats">
        <div className="panel__stat">
          <dt>{t.panel.photos}</dt>
          <dd className="num" data-muted={live.processed === 0}>
            {formatCount(live.optimized)}
            <span className="panel__of"> / {formatCount(session.total || live.processed)}</span>
          </dd>
        </div>
        <div className="panel__stat">
          <dt>{t.panel.savings}</dt>
          <dd className="num" data-muted={live.savedBytes === 0} data-signal={live.savedBytes > 0}>
            {formatPercent(fraction)}
          </dd>
          <p className="panel__hint">{live.originalBytes > 0 ? `${formatBytes(live.originalBytes)} → ${formatBytes(live.outputBytes)}` : t.panel.savingsHint}</p>
        </div>
      </dl>

      <div className="panel__alltime">
        <span className="panel__label">{t.panel.allTime}</span>
        <span className="panel__alltime-value num">
          {allTime.value}
          <small>{allTime.unit}</small>
        </span>
        {totals?.since != null && <span className="panel__hint">{t.panel.since(formatMonthYear(totals.since))}</span>}
      </div>
    </aside>
  );
}
