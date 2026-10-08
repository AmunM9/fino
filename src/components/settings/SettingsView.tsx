import { open } from "@tauri-apps/plugin-dialog";
import { useConfirm } from "../ui/ConfirmDialog";
import { FolderOutput, Monitor, Moon, RefreshCw, Sun, Trash2 } from "lucide-react";
import type { ReactNode } from "react";
import { formatBytes, prettyPath } from "../../lib/format";
import { errorMessage } from "../../lib/ipc";
import { languageNames, strengthCopy, t } from "../../lib/strings";
import type { Appearance, LanguageChoice, OutputMode, Strength } from "../../lib/types";
import { useApp } from "../../state/AppProvider";
import { Segmented, Toggle } from "../ui/controls";
import { About } from "./About";
import { SizeList } from "./SizeList";
import "../history/history.css";
import "./settings.css";
import { heicConversionAvailable } from "../../lib/platform";

const RETENTION_OPTIONS = [1, 7, 30] as const;
const LANGUAGES: LanguageChoice[] = ["system", "en", "es"];
const STRENGTHS: Strength[] = ["pristine", "identical", "compact"];
const themeOptions = () =>
  [
    { value: "system", label: <Monitor aria-hidden />, title: t.settings.themes.system },
    { value: "light", label: <Sun aria-hidden />, title: t.settings.themes.light },
    { value: "dark", label: <Moon aria-hidden />, title: t.settings.themes.dark },
  ] as const satisfies ReadonlyArray<{ value: Appearance; label: ReactNode; title: string }>;

function Field({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <div className="field">
      <div className="field__text">
        <span className="field__title">{title}</span>
        {hint && <span className="field__hint">{hint}</span>}
      </div>
      <div className="field__control">{children}</div>
    </div>
  );
}

/** Always visible while backups exist: turning backups off does not delete the ones kept. */
function BackupSpace() {
  const { history, freeBackups, clearing, undoing } = useApp();
  const bytes = history?.backupBytes ?? 0;
  const undoable = history?.sessions.filter((s) => s.canUndo && s.backupBytes > 0).length ?? 0;
  const size = formatBytes(bytes);
  const confirm = useConfirm();

  const confirmFree = async () => {
    const { ok } = await confirm({
      title: t.settings.freeTitle,
      body: t.settings.freeConfirm(size),
      okLabel: t.settings.freeOk,
      danger: true,
    });
    if (ok) await freeBackups();
  };

  return (
    <Field title={t.settings.backupSpace} hint={bytes > 0 ? t.settings.backupSpaceHint(undoable) : t.settings.noBackups}>
      {bytes > 0 && <span className="field__value num">{size}</span>}
      <button type="button" className="btn btn--danger" onClick={confirmFree} disabled={bytes === 0 || clearing || undoing}>
        <Trash2 aria-hidden /> {t.settings.free}
      </button>
    </Field>
  );
}

export function SettingsView() {
  const { settings, updateSettings, notify, history } = useApp();
  if (!settings) return null;
  const hasBackups = (history?.backupBytes ?? 0) > 0;

  const chooseFolder = async () => {
    try {
      const dir = await open({ directory: true, multiple: false });
      if (typeof dir === "string") updateSettings({ exportDir: dir });
    } catch (e) {
      notify(errorMessage(e));
    }
  };
  const setMode = (outputMode: OutputMode) => updateSettings({ outputMode });

  return (
    <section className="page" aria-labelledby="settings-heading">
      <header className="page__header page__header--with-tool">
        <div>
          <h1 id="settings-heading">{t.settings.title}</h1>
          <p>{t.settings.subtitle}</p>
        </div>
        <Segmented
          label={t.settings.theme}
          value={settings.appearance}
          options={themeOptions()}
          onChange={(appearance) => updateSettings({ appearance })}
          iconOnly
        />
      </header>

      <h2 className="section-title">{t.settings.output}</h2>
      <div className="modes" role="radiogroup" aria-label={t.settings.output}>
        {(["replace", "export"] as const).map((mode) => (
          <button key={mode} type="button" role="radio" aria-checked={settings.outputMode === mode} className="mode-card" onClick={() => setMode(mode)}>
            <span className="mode-card__radio" aria-hidden />
            <span className="mode-card__title">{mode === "replace" ? t.settings.replace : t.settings.export}</span>
            <span className="mode-card__hint">{mode === "replace" ? t.settings.replaceHint : t.settings.exportHint}</span>
          </button>
        ))}
      </div>

      <div className="group">
        {settings.outputMode === "replace" ? (
          <>
            <Field title={t.settings.warn}>
              <Toggle label={t.settings.warn} checked={settings.warnBeforeReplace} onChange={(v) => updateSettings({ warnBeforeReplace: v })} />
            </Field>
            <Field title={t.settings.backups}>
              <Toggle label={t.settings.backups} checked={settings.keepBackups} onChange={(v) => updateSettings({ keepBackups: v })} />
            </Field>
            {settings.keepBackups && (
              <Field title={t.settings.retention} hint={t.settings.retentionHint}>
                <select
                  aria-label={t.settings.retention}
                  value={settings.backupRetentionDays}
                  onChange={(e) => updateSettings({ backupRetentionDays: Number(e.target.value) })}
                >
                  {RETENTION_OPTIONS.map((d) => (
                    <option key={d} value={d}>
                      {t.settings.days(d)}
                    </option>
                  ))}
                </select>
              </Field>
            )}
            <BackupSpace />
          </>
        ) : (
          <>
            <Field title={t.settings.destination} hint={settings.exportDir ? prettyPath(settings.exportDir) : t.settings.nextToEach}>
              <button type="button" className="btn" onClick={chooseFolder}>
                <FolderOutput aria-hidden /> {t.settings.change}
              </button>
              {settings.exportDir && (
                <button type="button" className="icon-btn" aria-label={t.settings.reset} title={t.settings.reset} onClick={() => updateSettings({ exportDir: null })}>
                  <RefreshCw />
                </button>
              )}
            </Field>
            <Field title={t.settings.sizes} hint={t.settings.sizesHint}>
              <SizeList sizes={settings.sizes} onChange={(sizes) => updateSettings({ sizes })} />
            </Field>
            {hasBackups && <BackupSpace />}
          </>
        )}
      </div>

      <h2 className="section-title">{t.settings.strength}</h2>
      <div className="strengths" role="radiogroup" aria-label={t.settings.strength}>
        {STRENGTHS.map((s) => (
          <button key={s} type="button" role="radio" aria-checked={settings.strength === s} className="strength" onClick={() => updateSettings({ strength: s })}>
            <span className="strength__meter" data-level={s} aria-hidden>
              <i />
              <i />
              <i />
            </span>
            <span className="strength__name">{strengthCopy[s].name}</span>
            <span className="strength__typical num">{strengthCopy[s].typical}</span>
            <span className="strength__promise">{strengthCopy[s].promise}</span>
          </button>
        ))}
      </div>

      <h2 className="section-title">{t.settings.privacy}</h2>
      <div className="group">
        <Field title={t.settings.stripLocation} hint={t.settings.stripLocationHint}>
          <Toggle label={t.settings.stripLocation} checked={settings.stripLocation} onChange={(v) => updateSettings({ stripLocation: v })} />
        </Field>
        <Field title={t.settings.skipOptimized} hint={t.settings.skipOptimizedHint}>
          <Toggle label={t.settings.skipOptimized} checked={settings.skipOptimized} onChange={(v) => updateSettings({ skipOptimized: v })} />
        </Field>
        {heicConversionAvailable && (
          <Field title={t.settings.convertHeic} hint={t.settings.convertHeicHint}>
            <Toggle label={t.settings.convertHeic} checked={settings.heicToJpeg} onChange={(v) => updateSettings({ heicToJpeg: v })} />
          </Field>
        )}
      </div>

      <h2 className="section-title">{t.settings.general}</h2>
      <div className="group">
        <Field title={t.settings.language} hint={t.settings.languageHint}>
          <select
            aria-label={t.settings.language}
            value={settings.language}
            onChange={(e) => updateSettings({ language: e.target.value as LanguageChoice })}
          >
            {LANGUAGES.map((choice) => (
              <option key={choice} value={choice} lang={choice === "system" ? undefined : choice}>
                {choice === "system" ? t.settings.systemLanguage : languageNames[choice]}
              </option>
            ))}
          </select>
        </Field>
      </div>

      <About />
    </section>
  );
}
