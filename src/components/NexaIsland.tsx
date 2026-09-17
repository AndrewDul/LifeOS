import { useState } from "react";
import { NexaFace } from "./NexaFace";
import { ALL_SCREENS, type ScreenId } from "../state/navigation";
import type { FaceState } from "../state/faceState";

interface NexaIslandProps {
  faceState: FaceState;
  currentScreen: ScreenId;
  onHome: () => void;
  onNavigate: (id: ScreenId) => void;
}

/**
 * Compact floating "island" (LIFEOS_SPEC.md §9.1 direction; canonical as
 * of the M1 navigation correction) - the ONLY persistent global
 * navigation element while a full-screen section is open. A small dark
 * glass tile holding the animated face, so NeXa still feels present.
 * Clicking it opens a small quick menu (Home / Menu); "Menu" expands a
 * compact list of the other sections, attached to the island itself -
 * there is no permanent sidebar anywhere in the app anymore. Clicking
 * the island again closes the quick menu.
 */
export function NexaIsland({ faceState, currentScreen, onHome, onNavigate }: NexaIslandProps) {
  const [open, setOpen] = useState(false);
  const [showList, setShowList] = useState(false);

  function toggle() {
    setOpen((o) => {
      if (o) setShowList(false);
      return !o;
    });
  }

  function handleHome() {
    setOpen(false);
    setShowList(false);
    onHome();
  }

  function handleNavigate(id: ScreenId) {
    setOpen(false);
    setShowList(false);
    onNavigate(id);
  }

  return (
    <div className="nexa-island-wrap">
      <button
        type="button"
        className="nexa-island"
        onClick={toggle}
        aria-label="NeXa quick menu"
        aria-expanded={open}
      >
        <NexaFace state={faceState} size="island" />
      </button>

      {open && (
        <div className="island-menu">
          <button type="button" className="island-menu-item" onClick={handleHome}>
            Home
          </button>
          <button
            type="button"
            className="island-menu-item"
            onClick={() => setShowList((s) => !s)}
            aria-expanded={showList}
          >
            Menu
          </button>
          {showList && (
            <div className="island-menu-list">
              {ALL_SCREENS.map((s) => (
                <button
                  key={s.id}
                  type="button"
                  className={`island-menu-list-item${
                    s.id === currentScreen ? " island-menu-list-item--active" : ""
                  }`}
                  onClick={() => handleNavigate(s.id)}
                >
                  {s.label}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
