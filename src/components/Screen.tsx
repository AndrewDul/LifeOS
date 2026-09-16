import { PRIMARY_SCREENS, type ScreenId } from "../state/navigation";

interface ScreenProps {
  id: ScreenId;
  onBack: () => void;
}

/**
 * Generic placeholder for each primary screen (LIFEOS_SPEC.md §9.3-9.10).
 * Navigation shell only - no domain data. Each screen gets real content
 * in its own later milestone (M3 for Today/Goals/Projects/Calendar, M5
 * for Chat, M8 for Money/Me, M6 for Insights).
 */
export function Screen({ id, onBack }: ScreenProps) {
  const def = PRIMARY_SCREENS.find((s) => s.id === id);

  return (
    <div className="screen">
      <button type="button" className="screen-back" onClick={onBack}>
        &larr; Home
      </button>
      <h1 className="screen-title">{def?.label ?? id}</h1>
      <p className="screen-placeholder">
        This screen is a navigation placeholder. Real content arrives in a
        later milestone.
      </p>
    </div>
  );
}
