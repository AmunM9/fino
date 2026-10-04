import { fileUrl } from "../../lib/ipc";
import type { FileResult } from "../../lib/types";

const STACK_DEPTH = 3;

/** The last few photos, dealt onto a pile as they finish — progress you can see. */
export function PhotoStack({ results }: { results: FileResult[] }) {
  // Thumbnails only: decoding full 24 MP outputs here made the UI stutter mid-batch.
  const shown = results.filter((r) => r.previewPath !== null).slice(-STACK_DEPTH);
  if (shown.length === 0) {
    return (
      <div className="stack" aria-hidden="true">
        <div className="stack__card stack__card--empty" />
      </div>
    );
  }
  return (
    <div className="stack" aria-hidden="true">
      {shown.map((r, i) => (
        <figure
          key={r.id}
          className="stack__card"
          style={{ "--depth": shown.length - 1 - i } as React.CSSProperties}
        >
          <img src={fileUrl(r.previewPath ?? "")} alt="" decoding="async" draggable={false} />
        </figure>
      ))}
    </div>
  );
}
