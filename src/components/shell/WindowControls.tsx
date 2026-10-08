import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { useEffect, useState } from "react";
import { t } from "../../lib/strings";

/**
 * Windows: Fino's window has no system title bar (like the reference app), so it draws the
 * caption buttons itself — Windows' own size, order and red close. The strip along the top
 * still drags the window, and double-clicking it maximizes. macOS keeps its traffic lights.
 */
export function WindowControls({ compact }: { compact: boolean }) {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const sync = () =>
      win
        .isMaximized()
        .then((value) => !disposed && setMaximized(value))
        .catch(() => undefined);
    void sync();
    win
      .onResized(sync)
      .then((stop) => (disposed ? stop() : (unlisten = stop)))
      .catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const win = () => getCurrentWindow();
  return (
    <div className="window-controls" role="group" aria-label={t.window.controls}>
      <button type="button" aria-label={t.window.minimize} title={t.window.minimize} onClick={() => void win().minimize()}>
        <Minus aria-hidden />
      </button>
      {!compact && (
        <button
          type="button"
          aria-label={maximized ? t.window.restore : t.window.maximize}
          title={maximized ? t.window.restore : t.window.maximize}
          onClick={() => void win().toggleMaximize()}
        >
          {maximized ? <Copy aria-hidden /> : <Square aria-hidden />}
        </button>
      )}
      <button
        type="button"
        className="window-controls__close"
        aria-label={t.window.close}
        title={t.window.close}
        onClick={() => void win().close()}
      >
        <X aria-hidden />
      </button>
    </div>
  );
}
