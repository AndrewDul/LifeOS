import type { FaceState } from "../state/faceState";

export type NexaFaceSize = "hero" | "compact" | "island";

interface NexaFaceProps {
  state: FaceState;
  size: NexaFaceSize;
  /** Menu-open shrink, hero size only. */
  shrunk?: boolean;
  onClick?: () => void;
  ariaLabel?: string;
}

/**
 * The animated NeXa face (LIFEOS_SPEC.md §9.1): two simple rounded white
 * eyes, a simple white mouth. One shared implementation for the large
 * Home ("hero"), the message-display ("compact") and NexaIsland
 * ("island") contexts - size and glow intensity change, the design does
 * not. State-driven CSS classes carry the animation infrastructure; only
 * IDLE and a click-triggered ATTENTION bounce are exercised for real in
 * M1 (LISTENING / LOCAL_PROCESSING / WAITING_FOR_CLOUD / SPEAKING have no
 * real trigger yet - voice and AI arrive in later milestones).
 */
export function NexaFace({ state, size, shrunk = false, onClick, ariaLabel }: NexaFaceProps) {
  const speaking = state === "SPEAKING";
  const classes = [
    "nexa-face-wrap",
    `nexa-face-wrap--${size}`,
    shrunk ? "nexa-face-wrap--shrunk" : "",
  ]
    .filter(Boolean)
    .join(" ");

  const inner = (
    <>
      <span className="nexa-face-ring" aria-hidden="true" />
      <svg className="nexa-face" viewBox="0 0 200 200" aria-hidden="true">
        <rect
          className="nexa-eye nexa-eye--left"
          x="58"
          y="61"
          width="26"
          height="52"
          rx="13"
        />
        <rect
          className="nexa-eye nexa-eye--right"
          x="116"
          y="61"
          width="26"
          height="52"
          rx="13"
        />
        {speaking ? (
          <ellipse className="nexa-mouth nexa-mouth--open" cx="100" cy="176" rx="22" ry="16" />
        ) : (
          <path
            className="nexa-mouth nexa-mouth--smile"
            d="M 66 166 Q 100 192 134 166"
            fill="none"
            strokeLinecap="round"
          />
        )}
      </svg>
    </>
  );

  if (onClick) {
    return (
      <button
        type="button"
        className={classes}
        data-state={state.toLowerCase()}
        onClick={onClick}
        aria-label={ariaLabel ?? "LifeOS"}
      >
        {inner}
      </button>
    );
  }

  return (
    <div className={classes} data-state={state.toLowerCase()} aria-hidden="true">
      {inner}
    </div>
  );
}
