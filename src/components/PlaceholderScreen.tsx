import { ScreenShell } from "./ScreenShell";
import { screenLabel, type Direction, type ScreenId } from "../state/navigation";
import type { FaceState } from "../state/faceState";

interface PlaceholderScreenProps {
  id: ScreenId;
  faceState: FaceState;
  onNavigate: (id: ScreenId) => void;
  onHome: () => void;
  direction: Direction;
  closing: boolean;
}

const MILESTONE: Partial<Record<ScreenId, string>> = {
  today: "M3",
  calendar: "M3",
  goals: "M3",
  projects: "M3",
  money: "M8",
  me: "M8",
  insights: "M6",
  settings: "a later milestone",
  notifications: "M7",
};

/**
 * Refined placeholder for every full-screen section that doesn't have
 * dedicated M1 content yet (everything except Chat). Deliberately not a
 * fake dashboard - a clean heading and one honest line about when real
 * content arrives, on the shared ScreenShell chrome (no global sidebar -
 * just this content and the NeXa Island).
 */
export function PlaceholderScreen({
  id,
  faceState,
  onNavigate,
  onHome,
  direction,
  closing,
}: PlaceholderScreenProps) {
  const label = screenLabel(id);
  const milestone = MILESTONE[id] ?? "a later milestone";

  return (
    <ScreenShell
      activeId={id}
      faceState={faceState}
      onNavigate={onNavigate}
      onHome={onHome}
      direction={direction}
      closing={closing}
    >
      <div className="placeholder-screen">
        <h1 className="placeholder-title">{label}</h1>
        <p className="placeholder-copy">
          Real {label.toLowerCase()} tools arrive in {milestone}.
        </p>
      </div>
    </ScreenShell>
  );
}
