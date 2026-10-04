import { AlertTriangle, ArrowRight, Check, Minus } from "lucide-react";
import { isComparable } from "../../lib/compare";
import { formatBytes, formatPercent, formatScore, savedFraction } from "../../lib/format";
import { skipCopy, t } from "../../lib/strings";
import type { FileResult } from "../../lib/types";

interface Props {
  results: FileResult[];
  onOpen: (result: FileResult) => void;
}

function StatusIcon({ result }: { result: FileResult }) {
  if (result.status === "done") return <Check className="row__icon row__icon--done" aria-label="Optimizada" />;
  if (result.status === "failed") return <AlertTriangle className="row__icon row__icon--failed" aria-label="Error" />;
  return <Minus className="row__icon" aria-label="Omitida" />;
}

function Detail({ result }: { result: FileResult }) {
  if (result.status === "failed") return <span className="row__reason">{result.error}</span>;
  if (result.status === "skipped") return <span className="row__reason">{result.skipReason ? skipCopy[result.skipReason] : ""}</span>;
  return (
    <span className="row__sizes num">
      {formatBytes(result.originalBytes)}
      <ArrowRight aria-hidden />
      {formatBytes(result.outputBytes)}
    </span>
  );
}

/** A HEIC turned into a JPEG: the size change is shown signed — it can grow. */
function ConversionBadge({ result }: { result: FileResult }) {
  const change = result.originalBytes > 0 ? result.outputBytes / result.originalBytes - 1 : 0;
  const sign = change > 0 ? "+" : change < 0 ? "−" : "";
  return (
    <span className="badge badge--convert num" title={t.convert.hint}>
      {t.convert.badge} {sign}
      {formatPercent(Math.abs(change))}
    </span>
  );
}

export function ResultList({ results, onOpen }: Props) {
  const ordered = [...results].sort((a, b) => a.id - b.id);
  return (
    <ul className="results" aria-label="Resultados">
      {ordered.map((r) => {
        const canCompare = isComparable(r);
        const saved = savedFraction(r.originalBytes, r.outputBytes);
        return (
          <li key={r.id}>
            <button type="button" className="row" disabled={!canCompare} onClick={() => onOpen(r)} title={r.path}>
              <StatusIcon result={r} />
              <span className="row__name">{r.name}</span>
              <Detail result={r} />
              {r.status === "done" && (
                <>
                  {r.lossless ? (
                    <span className="row__score row__score--lossless" title={t.losslessHint}>
                      {t.lossless}
                    </span>
                  ) : (
                    <span className="row__score num" title="Similitud perceptual (SSIMULACRA 2)">
                      {r.score !== null ? formatScore(r.score) : ""}
                    </span>
                  )}
                  {r.converted ? (
                    <ConversionBadge result={r} />
                  ) : (
                    <span className="badge badge--signal num">−{formatPercent(saved)}</span>
                  )}
                </>
              )}
            </button>
          </li>
        );
      })}
    </ul>
  );
}
