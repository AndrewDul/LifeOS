import type { FaceState } from "../state/faceState";

interface NexaFaceProps {
  state: FaceState;
  shrunk: boolean;
  onClick: () => void;
}

/**
 * The animated NeXa face (LIFEOS_SPEC.md §9.1): two white rounded
 * bean-like eyes, a white mouth/smile. State-driven CSS classes provide
 * the animation infrastructure; only IDLE and ATTENTION are exercised in
 * M1 (LISTENING / LOCAL_PROCESSING / WAITING_FOR_CLOUD / SPEAKING have no
 * real trigger yet - voice and AI arrive in later milestones).
 */
export function NexaFace({ state, shrunk, onClick }: NexaFaceProps) {
  const speaking = state === "SPEAKING";

  return (
    <button
      type="button"
      className={`nexa-face-wrap${shrunk ? " nexa-face-wrap--shrunk" : ""}`}
      data-state={state.toLowerCase()}
      onClick={onClick}
      aria-label="LifeOS - open menu"
    >
      <span className="nexa-face-ring" aria-hidden="true" />
      <svg
        className="nexa-face"
        viewBox="0 0 200 200"
        width="200"
        height="200"
        aria-hidden="true"
      >
        <rect
          className="nexa-eye nexa-eye--left"
          x="58"
          y="82"
          width="28"
          height="40"
          rx="14"
        />
        <rect
          className="nexa-eye nexa-eye--right"
          x="114"
          y="82"
          width="28"
          height="40"
          rx="14"
        />
        {speaking ? (
          <ellipse className="nexa-mouth nexa-mouth--open" cx="100" cy="142" rx="20" ry="14" />
        ) : (
          <path
            className="nexa-mouth nexa-mouth--smile"
            d="M 72 138 Q 100 162 128 138"
            fill="none"
            strokeLinecap="round"
          />
        )}
      </svg>
    </button>
  );
}
