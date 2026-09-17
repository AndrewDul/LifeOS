import { useState } from "react";
import type { ScreenId } from "../state/navigation";

type Edge = "top" | "bottom" | "left" | "right";

interface DrawerAction {
  label: string;
  screenId: ScreenId;
}

interface DrawerTab {
  id: string;
  label: string;
  blurb: string;
  /** Omitted for tabs with no full screen yet (e.g. Notifications). */
  actions?: DrawerAction[];
}

interface EdgeConfig {
  edge: Edge;
  tabs: DrawerTab[];
}

const EDGES: EdgeConfig[] = [
  {
    edge: "top",
    tabs: [
      {
        id: "notifications",
        label: "Notifications",
        blurb: "No notifications yet. Proactive reminders and alerts arrive in M7.",
      },
      {
        id: "settings",
        label: "Settings",
        blurb: "Privacy, providers, and app preferences.",
        actions: [{ label: "Open Settings", screenId: "settings" }],
      },
    ],
  },
  {
    edge: "left",
    tabs: [
      {
        id: "dashboard",
        label: "Dashboard",
        blurb: "What matters today, at a glance.",
        actions: [{ label: "Open Dashboard", screenId: "today" }],
      },
      {
        id: "goals",
        label: "Goals",
        blurb: "Targets, deadlines, and progress.",
        actions: [{ label: "Open Goals", screenId: "goals" }],
      },
    ],
  },
  {
    edge: "right",
    tabs: [
      {
        id: "projects",
        label: "Projects",
        blurb: "Active projects, priorities, and blockers.",
        actions: [{ label: "Open Projects", screenId: "projects" }],
      },
      {
        id: "me-insights",
        label: "Me / Insights",
        blurb: "Wellbeing, journal, and the patterns LifeOS notices about you.",
        actions: [
          { label: "Open Me", screenId: "me" },
          { label: "Open Insights", screenId: "insights" },
        ],
      },
    ],
  },
  {
    edge: "bottom",
    tabs: [
      {
        id: "chat",
        label: "Chat",
        blurb: "Continue the conversation with NeXa.",
        actions: [{ label: "Open Chat", screenId: "chat" }],
      },
    ],
  },
];

interface EdgeDrawersProps {
  onNavigate: (id: ScreenId) => void;
}

/**
 * Hidden-by-default slide-out panels from the four screen edges (M1
 * visual refinement, Home only). Subtle edge handles open a glass panel
 * that slides in from that edge; clicking a handle again, a different
 * edge, or the scrim closes it and returns cleanly to Home. Each panel
 * is a light preview with an optional "Open X" action into the real
 * full-screen section - not a duplicate app, just a peek.
 */
export function EdgeDrawers({ onNavigate }: EdgeDrawersProps) {
  const [openEdge, setOpenEdge] = useState<Edge | null>(null);
  const [activeTab, setActiveTab] = useState<Record<Edge, string>>({
    top: "notifications",
    left: "dashboard",
    right: "projects",
    bottom: "chat",
  });

  function handleHandleClick(edge: Edge, tabId: string) {
    const isSameOpenTab = openEdge === edge && activeTab[edge] === tabId;
    setActiveTab((prev) => ({ ...prev, [edge]: tabId }));
    setOpenEdge(isSameOpenTab ? null : edge);
  }

  function close() {
    setOpenEdge(null);
  }

  function handleNavigate(screenId: ScreenId) {
    setOpenEdge(null);
    onNavigate(screenId);
  }

  return (
    <>
      {EDGES.map((cfg) => (
        <div key={cfg.edge} className={`edge-handles edge-handles--${cfg.edge}`}>
          {cfg.tabs.map((tab) => (
            <button
              key={tab.id}
              type="button"
              className={`edge-handle${
                openEdge === cfg.edge && activeTab[cfg.edge] === tab.id ? " edge-handle--active" : ""
              }`}
              onClick={() => handleHandleClick(cfg.edge, tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </div>
      ))}

      {EDGES.map((cfg) => {
        const tab = cfg.tabs.find((t) => t.id === activeTab[cfg.edge]) ?? cfg.tabs[0];
        const isOpen = openEdge === cfg.edge;
        return (
          <div
            key={cfg.edge}
            className={`edge-panel edge-panel--${cfg.edge}${isOpen ? " edge-panel--open" : ""}`}
            aria-hidden={!isOpen}
          >
            {cfg.tabs.length > 1 && (
              <div className="edge-panel-tabs">
                {cfg.tabs.map((t) => (
                  <button
                    key={t.id}
                    type="button"
                    className={`edge-panel-tab${t.id === tab.id ? " edge-panel-tab--active" : ""}`}
                    onClick={() => setActiveTab((prev) => ({ ...prev, [cfg.edge]: t.id }))}
                    tabIndex={isOpen ? 0 : -1}
                  >
                    {t.label}
                  </button>
                ))}
              </div>
            )}
            <h2 className="edge-panel-title">{tab.label}</h2>
            <p className="edge-panel-blurb">{tab.blurb}</p>
            {tab.actions && (
              <div className="edge-panel-actions">
                {tab.actions.map((a) => (
                  <button
                    key={a.screenId}
                    type="button"
                    className="edge-panel-action"
                    onClick={() => handleNavigate(a.screenId)}
                    tabIndex={isOpen ? 0 : -1}
                  >
                    {a.label}
                  </button>
                ))}
              </div>
            )}
          </div>
        );
      })}

      {openEdge && <div className="edge-scrim" onClick={close} />}
    </>
  );
}
