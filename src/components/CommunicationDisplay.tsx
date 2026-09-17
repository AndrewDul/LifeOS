interface CommunicationDisplayProps {
  message: string;
  onClose: () => void;
}

/**
 * Reusable focused-message display (LIFEOS_SPEC.md §9.1 direction, M1
 * visual refinement). NOT the Chat page - this is how NeXa surfaces a
 * single, editorial, unboxed message: no card, no bubble, no panel.
 * The face itself moves to make room for it (see .home-stage--message
 * in App.css) rather than this component owning its own face copy.
 */
export function CommunicationDisplay({ message, onClose }: CommunicationDisplayProps) {
  return (
    <div className="comm-display">
      <p className="comm-text">
        {message.split("\n").map((line, i) => (
          <span key={i} className="comm-line">
            {line}
          </span>
        ))}
      </p>
      <button type="button" className="comm-close" onClick={onClose}>
        Close
      </button>
    </div>
  );
}
