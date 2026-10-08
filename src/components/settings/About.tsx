import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { language, t } from "../../lib/strings";
import { copyright, studio } from "../../lib/studio";
import { useApp } from "../../state/AppProvider";
import { LogoMark } from "../shell/Logo";

/** Element id the rail's logo scrolls to. */
export const ABOUT_ID = "about";

export function About() {
  const { notify } = useApp();
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  // The studio's site has a page per language: English at the root, Spanish under /es.
  const site = language === "es" ? `${studio.url}es` : studio.url;

  return (
    <section className="about" id={ABOUT_ID} aria-labelledby="about-heading">
      <h2 className="section-title" id="about-heading">
        {t.about.title}
      </h2>
      <div className="about__card">
        <LogoMark size={36} />
        <div className="about__text">
          <p className="about__name">
            Fino {version && <span className="about__version num">{t.about.version(version)}</span>}
          </p>
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
        </div>
      </div>
    </section>
  );
}
