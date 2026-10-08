import type { ReactNode } from "react";
import { useCountUp } from "../../hooks/useCountUp";
import { sizeParts } from "../../lib/format";
import { t } from "../../lib/strings";

const SWEEP = 240; // degrees of arc
const RADIUS = 92;
const CENTER = 110;

function polar(angle: number, radius = RADIUS) {
  const rad = ((angle - 90) * Math.PI) / 180;
  return { x: CENTER + radius * Math.cos(rad), y: CENTER + radius * Math.sin(rad) };
}

function arcPath(): string {
  const start = polar(-SWEEP / 2);
  const end = polar(SWEEP / 2);
  return `M ${start.x} ${start.y} A ${RADIUS} ${RADIUS} 0 1 1 ${end.x} ${end.y}`;
}

const ARC_LENGTH = (2 * Math.PI * RADIUS * SWEEP) / 360;
const TICKS = Array.from({ length: 25 }, (_, i) => -SWEEP / 2 + (SWEEP / 24) * i);

interface Props {
  savedBytes: number;
  /** 0–1 share of the batch processed; drives the arc. */
  progress: number;
  active: boolean;
  /** Shown above the figure, e.g. a check once a batch is done. */
  badge?: ReactNode;
}

/** Instrument-style dial: the arc fills as the batch progresses; the figure counts the bytes
 *  saved so far. (The share saved is spelled out next to it, in the panel.) */
export function SavingsGauge({ savedBytes, progress, active, badge }: Props) {
  const animated = useCountUp(savedBytes);
  const shown = useCountUp(progress);
  const { value, unit } = sizeParts(animated);
  const offset = ARC_LENGTH * (1 - Math.min(1, Math.max(0, shown)));

  return (
    <figure className="gauge" data-active={active} aria-label={`${value} ${unit} ${t.panel.saved}`}>
      <svg
        viewBox="-4 -4 228 204"
        className="gauge__svg"
        role="progressbar"
        aria-label={t.panel.progress}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(Math.min(1, Math.max(0, progress)) * 100)}
      >
        {TICKS.map((angle, i) => {
          const inner = polar(angle, 101);
          const outer = polar(angle, i % 6 === 0 ? 108 : 105);
          return <line key={angle} x1={inner.x} y1={inner.y} x2={outer.x} y2={outer.y} className="gauge__tick" data-major={i % 6 === 0} />;
        })}
        <path d={arcPath()} className="gauge__track" />
        <path d={arcPath()} className="gauge__fill" strokeDasharray={ARC_LENGTH} strokeDashoffset={offset} />
      </svg>
      <figcaption className="gauge__readout">
        {badge && <span className="gauge__badge">{badge}</span>}
        <span className="gauge__value num">{savedBytes > 0 ? value : "0"}</span>
        <span className="gauge__unit">
          {savedBytes > 0 ? unit : "MB"} {t.panel.saved}
        </span>
      </figcaption>
    </figure>
  );
}
