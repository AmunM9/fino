import { CompareView } from "./components/compare/CompareView";
import { HistoryView } from "./components/history/HistoryView";
import { OptimizeView } from "./components/optimize/OptimizeView";
import { SessionPanel } from "./components/panel/SessionPanel";
import { SettingsView } from "./components/settings/SettingsView";
import { Rail } from "./components/shell/Rail";
import { Notice } from "./components/ui/controls";
import { useFileDrop } from "./hooks/useFileDrop";
import { t } from "./lib/strings";
import { AppProvider, useApp } from "./state/AppProvider";
import "./components/shell/shell.css";

function Shell() {
  const { view, start, compare, notice, dismissNotice } = useApp();
  const isOver = useFileDrop(start);

  return (
    <div className="shell">
      <div className="shell__drag" data-tauri-drag-region />
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
    <AppProvider>
      <Shell />
    </AppProvider>
  );
}
