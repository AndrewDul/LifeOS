import { SIDEBAR_SCREENS, type ScreenId } from "../state/navigation";

interface SideNavigationProps {
  activeId: ScreenId;
  onNavigate: (id: ScreenId) => void;
  onHome: () => void;
}

/**
 * Slim vertical glass sidebar for full-screen sections (M1 visual
 * refinement). Order matches the radial ring (Today first) with
 * Settings appended - a deliberate consistency choice over a different
 * ad-hoc ordering, since the same items also appear on the radial menu.
 */
export function SideNavigation({ activeId, onNavigate, onHome }: SideNavigationProps) {
  return (
    <nav className="side-nav" aria-label="Sections">
      <button type="button" className="side-nav-item side-nav-item--home" onClick={onHome}>
        <span className="side-nav-glyph">N</span>
        <span className="side-nav-label">Home</span>
      </button>

      <div className="side-nav-divider" />

      {SIDEBAR_SCREENS.map((s) => (
        <button
          key={s.id}
          type="button"
          className={`side-nav-item${s.id === activeId ? " side-nav-item--active" : ""}`}
          onClick={() => onNavigate(s.id)}
          aria-current={s.id === activeId ? "page" : undefined}
        >
          <span className="side-nav-glyph">{s.label.charAt(0)}</span>
          <span className="side-nav-label">{s.label}</span>
        </button>
      ))}
    </nav>
  );
}
