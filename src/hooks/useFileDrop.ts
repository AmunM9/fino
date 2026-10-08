import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useRef, useState } from "react";
import { ipc } from "../lib/ipc";
import { platform } from "../lib/platform";

/**
 * Windows launches Fino once per file chosen in Explorer's "Open with" (macOS hands them over
 * together): files arriving this close together are gathered into one batch.
 */
const GATHER_MS = platform === "windows" ? 400 : 0;

/**
 * Native file drops anywhere on the window, plus files opened with Fino from the system
 * (macOS: "Open With → Fino" or the Dock icon; Windows: "Open with" or the taskbar).
 */
export function useFileDrop(onPaths: (paths: string[]) => void): boolean {
  const [isOver, setIsOver] = useState(false);
  const handler = useRef(onPaths);
  handler.current = onPaths;

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    const keep = (unlisten: () => void) => (disposed ? unlisten() : unlisteners.push(unlisten));

    getCurrentWebview()
      .onDragDropEvent((event) => {
        const { type } = event.payload;
        if (type === "enter" || type === "over") setIsOver(true);
        else if (type === "leave") setIsOver(false);
        else if (type === "drop") {
          setIsOver(false);
          if (event.payload.paths.length > 0) handler.current(event.payload.paths);
        }
      })
      .then(keep)
      .catch(() => undefined);

    // The system hands paths to the backend, which buffers them until we ask — so a cold
    // "Open With" launch can't fire before this listener exists.
    let gathering: ReturnType<typeof setTimeout> | undefined;
    // Taking empties the backend's buffer, so it never happens after unmounting — and paths
    // once taken are always handed on.
    const take = () => {
      if (disposed) return;
      ipc
        .takeOpenedPaths()
        .then((paths) => paths.length > 0 && handler.current(paths))
        .catch(() => undefined);
    };
    const drainOpened = () => {
      clearTimeout(gathering);
      gathering = setTimeout(take, GATHER_MS);
    };
    // Drain once the listener exists too: paths that arrived while it was being registered
    // would otherwise wait for the next one.
    listen("open-paths", drainOpened)
      .then((unlisten) => {
        if (disposed) return unlisten();
        unlisteners.push(unlisten);
        drainOpened();
      })
      .catch(() => undefined);
    drainOpened();

    return () => {
      disposed = true;
      clearTimeout(gathering);
      unlisteners.forEach((u) => u());
    };
  }, []);

  return isOver;
}
