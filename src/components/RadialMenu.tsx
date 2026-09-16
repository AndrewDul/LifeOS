import { PRIMARY_SCREENS, type ScreenId } from "../state/navigation";

interface RadialMenuProps {
  open: boolean;
  onSelect: (id: ScreenId) => void;
}

const RADIUS_VMIN = 26;

/**
 * Glass-effect tiles revealed around the shrunk face, like numbers around
 * a clock face (LIFEOS_SPEC.md §9.1). Primary ring only - do not add the
 * secondary ring (Memory, Quick Forms, Model/AI, Settings) here.
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
              transitionDelay: open ? `${i * 25}ms` : "0ms",
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
