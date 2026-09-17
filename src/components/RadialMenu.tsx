import { PRIMARY_SCREENS, type ScreenId } from "../state/navigation";

interface RadialMenuProps {
  open: boolean;
  onSelect: (id: ScreenId) => void;
}

// vmin-based radius so the ring clears the larger shrunk hero face at
// any reasonable desktop window size, and stays inside the viewport.
const RADIUS_VMIN = 30;

/**
 * Elegant dark-glass tiles revealed around the shrunk face, like numbers
 * around a clock face (LIFEOS_SPEC.md §9.1). Primary ring only - the
 * secondary ring (Memory, Quick Forms, Model/AI, Settings) does not
 * belong here.
 */
export function RadialMenu({ open, onSelect }: RadialMenuProps) {
  const count = PRIMARY_SCREENS.length;

  return (
    <div className={`radial-menu${open ? " radial-menu--open" : ""}`} aria-hidden={!open}>
      {PRIMARY_SCREENS.map((screen, i) => {
        const angle = (i / count) * 2 * Math.PI - Math.PI / 2;
        const x = Math.cos(angle) * RADIUS_VMIN;
        const y = Math.sin(angle) * RADIUS_VMIN;
        return (
          <button
            key={screen.id}
            type="button"
            className="radial-tile"
            style={{
              transform: `translate(-50%, -50%) translate(${x}vmin, ${y}vmin)`,
              transitionDelay: open ? `${i * 28}ms` : "0ms",
            }}
            onClick={() => onSelect(screen.id)}
            tabIndex={open ? 0 : -1}
          >
            {screen.label}
          </button>
        );
      })}
    </div>
  );
}
