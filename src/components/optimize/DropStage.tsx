import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, FolderOutput, Images, ShieldCheck, ShieldOff } from "lucide-react";
import { prettyPath } from "../../lib/format";
import { errorMessage } from "../../lib/ipc";
import { t } from "../../lib/strings";
import type { Settings } from "../../lib/types";
import { useApp } from "../../state/AppProvider";

const JPEG_FILTER = [{ name: "JPEG", extensions: ["jpg", "jpeg", "jpe", "jfif"] }];

function asList(selection: string | string[] | null): string[] {
  if (!selection) return [];
  return Array.isArray(selection) ? selection : [selection];
}

function ModeHint({ settings }: { settings: Settings }) {
  if (settings.outputMode === "export") {
    const where = settings.exportDir ? prettyPath(settings.exportDir) : t.drop.nextToEach;
    return (
      <p className="mode-hint">
        <FolderOutput aria-hidden /> {t.drop.exportHint(where)}
      </p>
    );
  }
  const Icon = settings.keepBackups ? ShieldCheck : ShieldOff;
  return (
    <p className="mode-hint" data-warn={!settings.keepBackups}>
      <Icon aria-hidden /> {t.drop.replaceHint(settings.keepBackups ? settings.backupRetentionDays : null)}
    </p>
  );
}

export function DropStage() {
  const { settings, start, setView, notify } = useApp();

  const pick = async (directory: boolean) => {
    try {
      const selection = await open({ multiple: true, directory, filters: directory ? undefined : JPEG_FILTER });
      start(asList(selection));
    } catch (e) {
      notify(errorMessage(e));
    }
  };

  return (
    <section className="drop" aria-labelledby="drop-heading">
      <h1 id="drop-heading" className="drop__headline">
        {t.drop.headline.map((line) => (
          <span key={line}>{line}</span>
        ))}
      </h1>
      <div className="drop__actions">
        <button type="button" className="btn btn--primary" onClick={() => pick(false)}>
          <Images aria-hidden /> {t.drop.choosePhotos}
        </button>
        <button type="button" className="btn" onClick={() => pick(true)}>
          <FolderOpen aria-hidden /> {t.drop.chooseFolder}
        </button>
      </div>
      {settings && (
        <button type="button" className="drop__mode" onClick={() => setView("settings")}>
          <ModeHint settings={settings} />
        </button>
      )}
    </section>
  );
}
