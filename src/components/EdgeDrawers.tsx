import { useState } from "react";

type DrawerId = "left" | "bottom" | "top" | null;

/**
 * Left/bottom/top edge drawer architecture (LIFEOS_SPEC.md §9.1).
 * Desktop hit-area buttons for now, not touch gestures - the brief
 * explicitly allows this for the first run as long as the interaction
 * model (an edge opens a drawer) is in place. Content is placeholder
 * only; no domain data exists yet.
 */
export function EdgeDrawers() {
  const [open, setOpen] = useState<DrawerId>(null);

  const toggle = (id: DrawerId) => setOpen((current) => (current === id ? null : id));

  return (
    <>
      <button
        type="button"
        className="edge-tab edge-tab--left"
        onClick={() => toggle("left")}
        aria-label="Today drawer"
      >
        Today
      </button>
      <button
        type="button"
        className="edge-tab edge-tab--top"
        onClick={() => toggle("top")}
        aria-label="Notifications drawer"
      >
        Notifications
      </button>
      <button
        type="button"
        className="edge-tab edge-tab--bottom"
        onClick={() => toggle("bottom")}
        aria-label="Chat drawer"
      >
        Chat
      </button>

      <div
        className={`edge-drawer edge-drawer--left${open === "left" ? " edge-drawer--open" : ""}`}
      >
        <h2>Today</h2>
        <p>Dashboard drawer placeholder - content arrives with M3.</p>
      </div>
      <div
        className={`edge-drawer edge-drawer--top${open === "top" ? " edge-drawer--open" : ""}`}
      >
        <h2>Notifications</h2>
        <p>Questions and decisions drawer placeholder - content arrives with M7.</p>
      </div>
      <div
        className={`edge-drawer edge-drawer--bottom${open === "bottom" ? " edge-drawer--open" : ""}`}
      >
        <h2>Chat</h2>
        <p>New Chat / recent chats placeholder - content arrives with M5.</p>
      </div>

      {open && <div className="edge-scrim" onClick={() => setOpen(null)} />}
    </>
  );
}
