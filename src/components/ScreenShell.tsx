import type { ReactNode } from "react";
import { SideNavigation } from "./SideNavigation";
import { NexaIsland } from "./NexaIsland";
import type { ScreenId } from "../state/navigation";
import type { FaceState } from "../state/faceState";

interface ScreenShellProps {
  activeId: ScreenId;
  faceState: FaceState;
  onNavigate: (id: ScreenId) => void;
  onHome: () => void;
  children: ReactNode;
}

/**
 * Shared full-screen layout (M1 visual refinement): side navigation +
 * NeXa island + content area, all floating over the galaxy (which stays
 * mounted at the app root, not here). Every section - Chat and every
 * placeholder alike - is built on this so the app feels coherent instead
 * of each screen inventing its own chrome.
 */
export function ScreenShell({
  activeId,
  faceState,
  onNavigate,
  onHome,
  children,
}: ScreenShellProps) {
  return (
    <div className="screen-shell">
      <SideNavigation activeId={activeId} onNavigate={onNavigate} onHome={onHome} />
      <NexaIsland
        faceState={faceState}
        currentScreen={activeId}
        onHome={onHome}
        onNavigate={onNavigate}
      />
      <div className="screen-shell-content">{children}</div>
    </div>
  );
}
