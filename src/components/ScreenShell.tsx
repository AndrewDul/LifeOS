import type { ReactNode } from "react";
import { NexaIsland } from "./NexaIsland";
import type { Direction, ScreenId } from "../state/navigation";
import type { FaceState } from "../state/faceState";

interface ScreenShellProps {
  activeId: ScreenId;
  faceState: FaceState;
  onNavigate: (id: ScreenId) => void;
  onHome: () => void;
  direction: Direction;
  closing: boolean;
  children: ReactNode;
}

/**
 * Clean full-screen section shell (M1 navigation correction). No global
 * sidebar - the NeXa Island is the ONLY persistent global navigation
 * element while a section is open. The opened section fills the
 * available viewport and slides in from `direction` (the edge or radial
 * menu it was opened from); `closing` reverses that same motion when the
 * user returns Home. The galaxy stays visible behind this (mounted once
 * at the app root, not here) since this shell has no opaque background
 * of its own.
 */
export function ScreenShell({
  activeId,
  faceState,
  onNavigate,
  onHome,
  direction,
  closing,
  children,
}: ScreenShellProps) {
  return (
    <div
      className="screen-shell"
      data-direction={direction}
      data-closing={closing ? "true" : undefined}
    >
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
