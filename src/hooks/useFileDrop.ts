import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useRef, useState } from "react";
import { ipc } from "../lib/ipc";

/**
 * Native file drops anywhere on the window, plus files opened from Finder
 * ("Open With → Fino" or dropped on the Dock icon).
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

    // Finder hands paths to the backend, which buffers them until we ask — so a cold
    // "Open With" launch can't fire before this listener exists.
    const drainOpened = () =>
      ipc
        .takeOpenedPaths()
        .then((paths) => paths.length > 0 && handler.current(paths))
        .catch(() => undefined);
    listen("open-paths", drainOpened).then(keep).catch(() => undefined);
    void drainOpened();

    return () => {
      disposed = true;
      unlisteners.forEach((u) => u());
    };
  }, []);

  return isOver;
}
