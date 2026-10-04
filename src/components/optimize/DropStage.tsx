import { FolderOpen, FolderOutput, Images, ShieldCheck, ShieldOff } from "lucide-react";
import { prettyPath } from "../../lib/format";
import { t } from "../../lib/strings";
import type { Settings } from "../../lib/types";
import { usePhotoPicker } from "../../hooks/usePhotoPicker";
import { useApp } from "../../state/AppProvider";

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
  const { settings, setView } = useApp();
  const pick = usePhotoPicker();

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
