import { PictureInPicture2 } from "lucide-react";
import { CompareView } from "./components/compare/CompareView";
import { MiniView } from "./components/mini/MiniView";
import { HistoryView } from "./components/history/HistoryView";
import { OptimizeView } from "./components/optimize/OptimizeView";
import { SessionPanel } from "./components/panel/SessionPanel";
import { SettingsView } from "./components/settings/SettingsView";
import { Rail } from "./components/shell/Rail";
import { ConfirmProvider } from "./components/ui/ConfirmDialog";
import { Notice } from "./components/ui/controls";
import { useFileDrop } from "./hooks/useFileDrop";
import { t } from "./lib/strings";
import { AppProvider, useApp } from "./state/AppProvider";
import "./components/shell/shell.css";

/** Top-right corner, like the mini window's own expand button: the toggle stays put. */
function MiniToggle() {
  const { updateSettings } = useApp();
  return (
    <button
      type="button"
      className="icon-btn shell__mini"
      aria-label={t.mini.enter}
      title={t.mini.enterHint}
      onClick={() => updateSettings({ compactWindow: true })}
    >
      <PictureInPicture2 />
    </button>
  );
}

function Shell() {
  const { view, start, compare, notice, dismissNotice, settings } = useApp();
  const isOver = useFileDrop(start);

  if (settings?.compactWindow) {
    return (
      <>
        <MiniView isOver={isOver} />
        {notice && <Notice message={notice} onClose={dismissNotice} />}
      </>
    );
  }

  return (
    <div className="shell">
      <div className="shell__drag" data-tauri-drag-region />
      {view === "optimize" && <MiniToggle />}
      <Rail />
      <main className="shell__main">
        {view === "optimize" && <OptimizeView />}
        {view === "history" && <HistoryView />}
        {view === "settings" && <SettingsView />}
        {isOver && (
          <div className="drop-overlay" aria-hidden="true">
            <span>{t.drop.hovering}</span>
          </div>
        )}
      </main>
      {view === "optimize" && <SessionPanel />}
      {compare && <CompareView />}
      {notice && <Notice message={notice} onClose={dismissNotice} />}
    </div>
  );
}

export default function App() {
  return (
    <ConfirmProvider>
      <AppProvider>
        <Shell />
      </AppProvider>
    </ConfirmProvider>
  );
}
