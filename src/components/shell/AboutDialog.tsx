import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import { language, t } from "../../lib/strings";
import { copyright, studio } from "../../lib/studio";
import { useApp } from "../../state/AppProvider";
import { LogoMark } from "./Logo";
import "../ui/confirm.css";
import "./about.css";

/** Opened from the rail's leaf: name, version, studio and copyright. */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  const { notify } = useApp();
  const ref = useRef<HTMLDialogElement>(null);
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  // showModal(): focus trap, inert background and Escape to close.
  useEffect(() => {
    const dialog = ref.current;
    if (dialog && !dialog.open) dialog.showModal();
  }, []);

  // The studio's site has a page per language: English at the root, Spanish under /es.
  const site = language === "es" ? `${studio.url}es` : studio.url;

  return (
    <dialog
      ref={ref}
      className="confirm about"
      aria-labelledby="about-title"
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      // A click on the backdrop lands on the dialog element itself.
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <LogoMark size={44} />
      <h2 id="about-title">Fino</h2>
      {version && <p className="about__version num">{t.about.version(version)}</p>}
      <p className="about__credit">
        {t.about.madeBy}{" "}
        <a
          href={site}
          onClick={(e) => {
            e.preventDefault();
            openUrl(site).catch((err) => notify(String(err)));
          }}
        >
          {studio.name}
        </a>
      </p>
      <p className="about__copyright">{copyright}</p>
      <button type="button" className="btn about__close" onClick={onClose} autoFocus>
        {t.about.close}
      </button>
    </dialog>
  );
}
