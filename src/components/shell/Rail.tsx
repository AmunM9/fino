import { ChartNoAxesColumn, Images, SlidersHorizontal, type LucideIcon } from "lucide-react";
import { t } from "../../lib/strings";
import { useApp, type View } from "../../state/AppProvider";
import { LogoMark } from "./Logo";

const ITEMS: ReadonlyArray<{ view: View; icon: LucideIcon; label: string }> = [
  { view: "optimize", icon: Images, label: t.nav.optimize },
  { view: "history", icon: ChartNoAxesColumn, label: t.nav.history },
  { view: "settings", icon: SlidersHorizontal, label: t.nav.settings },
];

export function Rail() {
  const { view, setView, session } = useApp();
  return (
    <nav className="rail" aria-label="Navegación principal">
      <div className="rail__logo">
        <LogoMark />
      </div>
      {ITEMS.map(({ view: v, icon: Icon, label }) => (
        <button
          key={v}
          type="button"
          className="rail__item"
          data-label={label}
          aria-label={label}
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
