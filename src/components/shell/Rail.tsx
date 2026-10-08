import { ChartNoAxesColumn, Images, SlidersHorizontal, type LucideIcon } from "lucide-react";
import { t } from "../../lib/strings";
import { useApp, type View } from "../../state/AppProvider";
import { useState } from "react";
import { AboutDialog } from "./AboutDialog";
import { LogoMark } from "./Logo";

const ITEMS: ReadonlyArray<{ view: View; icon: LucideIcon; label: () => string }> = [
  { view: "optimize", icon: Images, label: () => t.nav.optimize },
  { view: "history", icon: ChartNoAxesColumn, label: () => t.nav.history },
  { view: "settings", icon: SlidersHorizontal, label: () => t.nav.settings },
];

export function Rail() {
  const { view, setView, session } = useApp();
  const [aboutOpen, setAboutOpen] = useState(false);
  return (
    <nav className="rail" aria-label={t.nav.label}>
      <button
        type="button"
        className="rail__logo"
        aria-label={t.nav.about}
        title={t.nav.about}
        aria-haspopup="dialog"
        onClick={() => setAboutOpen(true)}
      >
        <LogoMark />
      </button>
      {aboutOpen && <AboutDialog onClose={() => setAboutOpen(false)} />}
      {ITEMS.map(({ view: v, icon: Icon, label }) => (
        <button
          key={v}
          type="button"
          className="rail__item"
          data-label={label()}
          aria-label={label()}
          aria-current={view === v ? "page" : undefined}
          onClick={() => setView(v)}
        >
          <Icon />
        </button>
      ))}
      {session.phase === "running" && <span className="rail__busy" aria-label={t.session.working} />}

    </nav>
  );
}
