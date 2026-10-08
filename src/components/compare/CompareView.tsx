import { ChevronLeft, ChevronRight, Maximize, X, ZoomIn } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { formatBytes, formatPercent, formatScore, savedFraction } from "../../lib/format";
import { useElementSize } from "../../hooks/useElementSize";
import { fileUrl } from "../../lib/ipc";
import { t } from "../../lib/strings";
import { useApp } from "../../state/AppProvider";
import "./compare.css";

type Zoom = "fit" | "actual";

const KEY_STEP = 0.05;

function clamp01(v: number) {
  return Math.min(1, Math.max(0, v));
}

/** Pan offset so the point under the cursor stays under the cursor at 100 %. */
function panFor(pointer: number, viewport: number, content: number): number {
  if (content <= viewport) return (viewport - content) / 2;
  return -clamp01(pointer / viewport) * (content - viewport);
}

export function CompareView() {
  const { compare, openCompare } = useApp();
  const stageRef = useRef<HTMLDivElement>(null);
  const [split, setSplit] = useState(0.5);
  const [zoom, setZoom] = useState<Zoom>("fit");
  const [pointer, setPointer] = useState({ x: 0.5, y: 0.5 });
  const dragging = useRef(false);
  const stage = useElementSize(stageRef);
  /** Pairs whose files failed to load (moved or deleted while Fino was open). */
  const [broken, setBroken] = useState<ReadonlySet<string>>(new Set());
  /** The output as displayed, EXIF rotation applied: the stored size is sideways for portraits. */
  const [shown, setShown] = useState<{ src: string; width: number; height: number } | null>(null);

  const results = compare?.results ?? [];
  const index = compare?.index ?? 0;
  const current = results[index];
  const close = useCallback(() => openCompare(null), [openCompare]);
  const go = useCallback(
    (delta: number) => {
      if (!compare) return;
      const next = (compare.index + delta + compare.results.length) % compare.results.length;
      openCompare({ ...compare, index: next });
    },
    [compare, openCompare],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
      else if (e.key === "ArrowRight" && e.shiftKey) setSplit((s) => clamp01(s + KEY_STEP));
      else if (e.key === "ArrowLeft" && e.shiftKey) setSplit((s) => clamp01(s - KEY_STEP));
      else if (e.key === "ArrowRight") go(1);
      else if (e.key === "ArrowLeft") go(-1);
      else if (e.key.toLowerCase() === "z") setZoom((z) => (z === "fit" ? "actual" : "fit"));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close, go]);

  // Warm up the next pair only (never the whole list), so stepping forward is instant.
  useEffect(() => {
    if (results.length < 2) return;
    const next = results[(index + 1) % results.length];
    const paths = [next.originalPath, next.outputs[0]?.path].filter((p): p is string => Boolean(p));
    for (const path of paths) {
      const img = new Image();
      img.decoding = "async";
      img.src = fileUrl(path);
      img.decode().catch(() => {}); // a missing file is handled when it is actually shown
    }
  }, [results, index]);

  if (!current) return null;
  const output = current.outputs[0];
  const pair = `${current.originalPath}|${output.path}`;
  const original = broken.has(pair) ? null : current.originalPath;
  const markBroken = () => setBroken((prev) => new Set(prev).add(pair));

  const locate = (e: ReactPointerEvent) => {
    const rect = stageRef.current?.getBoundingClientRect();
    if (!rect) return null;
    return { x: clamp01((e.clientX - rect.left) / rect.width), y: clamp01((e.clientY - rect.top) / rect.height) };
  };
  const onPointerMove = (e: ReactPointerEvent) => {
    const at = locate(e);
    if (!at) return;
    // While the divider is held the photo stays put; only the divider follows the cursor.
    if (dragging.current) setSplit(at.x);
    else setPointer({ x: at.x, y: at.y });
  };
  const onPointerDown = (e: ReactPointerEvent) => {
    dragging.current = true;
    e.currentTarget.setPointerCapture(e.pointerId);
    const at = locate(e);
    if (at) setSplit(at.x);
  };
  const onPointerUp = () => {
    dragging.current = false;
  };

  // At 100 % both images are drawn at the output's pixel size and panned with the cursor;
  // the clip is converted from stage space into image space so it tracks the divider.
  const outputSrc = fileUrl(output.path);
  const size = shown?.src === outputSrc ? shown : null;
  const actual = zoom === "actual" && stage.width > 0 && size !== null;
  const panX = actual ? panFor(pointer.x * stage.width, stage.width, size.width) : 0;
  const panY = actual ? panFor(pointer.y * stage.height, stage.height, size.height) : 0;
  const actualStyle = actual
    ? { width: size.width, height: size.height, transform: `translate(${panX}px, ${panY}px)` }
    : undefined;
  const clip = actual ? clamp01((split * stage.width - panX) / size.width) : split;
  const saved = savedFraction(current.originalBytes, current.outputBytes);

  return (
    <div className="compare" role="dialog" aria-modal="true" aria-label={`${t.session.compare}: ${current.name}`}>
      {/* The bar covers the window's drag strip: it drags the window itself. */}
      <header className="compare__bar" data-tauri-drag-region>
        <div className="compare__title" data-tauri-drag-region>
          <strong>{current.name}</strong>
          <span className="num">
            {index + 1} / {results.length}
          </span>
        </div>
        <div className="compare__facts">
          {current.score !== null && (
            <span className="compare__fact">
              {t.compare.similarity} <b className="num">{formatScore(current.score)}</b>
            </span>
          )}
          <span className="badge badge--signal num">−{formatPercent(saved)}</span>
        </div>
        <div className="compare__tools">
          <button type="button" className="icon-btn" aria-label={t.compare.previous} onClick={() => go(-1)} disabled={results.length < 2}>
            <ChevronLeft />
          </button>
          <button type="button" className="icon-btn" aria-label={t.compare.next} onClick={() => go(1)} disabled={results.length < 2}>
            <ChevronRight />
          </button>
          <button
            type="button"
            className="btn btn--ghost"
            aria-pressed={zoom === "actual"}
            onClick={() => setZoom(zoom === "fit" ? "actual" : "fit")}
          >
            {zoom === "fit" ? <ZoomIn aria-hidden /> : <Maximize aria-hidden />}
            {zoom === "fit" ? t.compare.zoom : t.compare.fit}
          </button>
          <button type="button" className="icon-btn" aria-label={t.compare.close} onClick={close}>
            <X />
          </button>
        </div>
      </header>

      {original ? (
        <div
          ref={stageRef}
          className="compare__stage"
          data-zoom={actual ? "actual" : "fit"}
          onPointerMove={onPointerMove}
          onPointerDown={onPointerDown}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
          role="slider"
          aria-label={t.compare.divider}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(split * 100)}
          tabIndex={0}
        >
          <img
            className="compare__img"
            src={fileUrl(original)}
            alt={t.compare.original}
            style={actualStyle}
            draggable={false}
            decoding="async"
            onError={markBroken}
          />
          <img
            className="compare__img compare__img--after"
            src={outputSrc}
            alt={t.compare.fino}
            onLoad={(e) =>
              setShown({ src: outputSrc, width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight })
            }
            style={{ ...actualStyle, clipPath: `inset(0 0 0 ${clip * 100}%)` }}
            draggable={false}
            decoding="async"
            onError={markBroken}
          />
          <div className="compare__divider" style={{ transform: `translateX(${split * stage.width}px)` }} aria-hidden="true">
            <span />
          </div>
          <span className="compare__label compare__label--left">
            {t.compare.original} · <span className="num">{formatBytes(current.originalBytes)}</span>
          </span>
          <span className="compare__label compare__label--right">
            {t.compare.fino} · <span className="num">{formatBytes(output.bytes)}</span>
          </span>
        </div>
      ) : (
        <p className="compare__missing">{t.compare.unavailable}</p>
      )}
    </div>
  );
}
