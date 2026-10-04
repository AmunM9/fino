import { Plus, X } from "lucide-react";
import { t } from "../../lib/strings";
import type { ResizeMode, SizePreset } from "../../lib/types";

const MAX_SIZES = 4;
const MIN_PIXELS = 16;
const MAX_PIXELS = 30000;

type ModeValue = ResizeMode | "original";

const MODE_OPTIONS: ReadonlyArray<{ value: ModeValue; label: string }> = [
  { value: "original", label: t.settings.originalSize },
  { value: "longEdge", label: t.settings.longEdge },
  { value: "maxWidth", label: t.settings.maxWidth },
  { value: "maxHeight", label: t.settings.maxHeight },
];

function labelFor(mode: ResizeMode | null, pixels: number | null): string {
  if (!mode || !pixels) return "Original";
  const prefix = mode === "maxWidth" ? "Ancho " : mode === "maxHeight" ? "Alto " : "";
  return `${prefix}${pixels} px`;
}

function makeSize(mode: ResizeMode | null, pixels: number | null): SizePreset {
  return { id: `${mode ?? "original"}-${pixels ?? 0}-${Date.now()}`, label: labelFor(mode, pixels), mode, pixels };
}

interface Props {
  sizes: SizePreset[];
  onChange: (sizes: SizePreset[]) => void;
}

export function SizeList({ sizes, onChange }: Props) {
  const update = (index: number, mode: ResizeMode | null, pixels: number | null) => {
    const next = sizes.map((s, i) => (i === index ? { ...s, mode, pixels, label: labelFor(mode, pixels) } : s));
    onChange(next);
  };
  const remove = (index: number) => onChange(sizes.filter((_, i) => i !== index));
  const add = (pixels: number) => onChange([...sizes, makeSize("longEdge", pixels)]);

  return (
    <div className="sizes">
      {sizes.map((size, index) => (
        <div className="sizes__row" key={size.id}>
          <select
            aria-label={t.settings.sizes}
            value={size.mode ?? "original"}
            onChange={(e) => {
              const value = e.target.value as ModeValue;
              if (value === "original") update(index, null, null);
              else update(index, value, size.pixels ?? 2048);
            }}
          >
            {MODE_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
          {size.mode && (
            <label className="sizes__px">
              <input
                type="number"
                inputMode="numeric"
                min={MIN_PIXELS}
                max={MAX_PIXELS}
                value={size.pixels ?? ""}
                onChange={(e) => {
                  const px = Math.round(Number(e.target.value));
                  if (Number.isFinite(px) && px >= MIN_PIXELS && px <= MAX_PIXELS) update(index, size.mode, px);
                }}
              />
              <span>px</span>
            </label>
          )}
          {sizes.length > 1 && (
            <button type="button" className="icon-btn" aria-label={t.settings.removeSize} onClick={() => remove(index)}>
              <X />
            </button>
          )}
        </div>
      ))}
      {sizes.length < MAX_SIZES && (
        <div className="sizes__add">
          <span className="sizes__add-label">
            <Plus aria-hidden /> {t.settings.addSize}
          </span>
          {t.presets.map((p) => (
            <button key={p.label} type="button" className="chip" onClick={() => add(p.pixels)}>
              {p.label} <span className="num">{p.pixels}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
