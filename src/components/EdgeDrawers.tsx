import type { Direction, ScreenId } from "../state/navigation";

interface EdgeHandle {
  screenId: ScreenId;
  label: string;
}

type EdgeDirection = Exclude<Direction, "radial">;

interface EdgeGroup {
  direction: EdgeDirection;
  handles: EdgeHandle[];
}

const GROUPS: EdgeGroup[] = [
  {
    direction: "top",
    handles: [
      { screenId: "notifications", label: "Notifications" },
      { screenId: "settings", label: "Settings" },
    ],
  },
  {
    direction: "left",
    handles: [
      { screenId: "today", label: "Dashboard" },
      { screenId: "goals", label: "Goals" },
    ],
  },
  {
    direction: "right",
    handles: [
      { screenId: "projects", label: "Projects" },
      { screenId: "me", label: "Me" },
      { screenId: "insights", label: "Insights" },
    ],
  },
  {
    direction: "bottom",
    handles: [{ screenId: "chat", label: "Chat" }],
  },
];

interface EdgeDrawersProps {
  onOpen: (id: ScreenId, direction: Direction) => void;
}

/**
 * Home-only edge handles (build-start brief, then two visual-refinement
 * passes). Canonical behaviour as of this correction: the handle IS the
 * navigation - there is no intermediate preview panel. Clicking a handle
 * opens the real full-screen section immediately, sliding in from that
 * edge (App owns the actual screen/direction state and animation).
 */
export function EdgeDrawers({ onOpen }: EdgeDrawersProps) {
  return (
    <>
      {GROUPS.map((group) => (
        <div key={group.direction} className={`edge-handles edge-handles--${group.direction}`}>
          {group.handles.map((h) => (
            <button
              key={h.screenId}
              type="button"
              className="edge-handle"
              onClick={() => onOpen(h.screenId, group.direction)}
            >
              {h.label}
            </button>
          ))}
        </div>
      ))}
    </>
  );
}
