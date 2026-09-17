import { ScreenShell } from "./ScreenShell";
import type { ScreenId } from "../state/navigation";
import type { FaceState } from "../state/faceState";

interface ChatScreenProps {
  faceState: FaceState;
  onNavigate: (id: ScreenId) => void;
  onHome: () => void;
}

interface DemoMessage {
  from: "nexa" | "user";
  text: string;
}

const DEMO_CONVERSATION: DemoMessage[] = [
  { from: "nexa", text: "Good evening. Here's a quick look at how conversation will feel once I'm connected." },
  { from: "user", text: "Looks good - show me around." },
  { from: "nexa", text: "This is a visual preview only for now. Real conversation arrives in M5." },
];

/**
 * Chat's M1 visual shell (LIFEOS_SPEC.md §9.4, build-start brief §10).
 * Visually representative demo conversation only - no AI, no backend,
 * no persistence. Traditional message bubbles are appropriate here
 * (unlike the unboxed CommunicationDisplay), kept restrained and dark.
 */
export function ChatScreen({ faceState, onNavigate, onHome }: ChatScreenProps) {
  return (
    <ScreenShell activeId="chat" faceState={faceState} onNavigate={onNavigate} onHome={onHome}>
      <div className="chat-screen">
        <div className="chat-history">
          {DEMO_CONVERSATION.map((m, i) => (
            <div key={i} className={`chat-bubble chat-bubble--${m.from}`}>
              {m.text}
            </div>
          ))}
        </div>

        <form
          className="chat-input-bar"
          onSubmit={(e) => e.preventDefault()}
        >
          <button type="button" className="chat-input-attach" aria-label="Attach" disabled>
            +
          </button>
          <input
            className="chat-input-field"
            type="text"
            placeholder="Message NeXa..."
            disabled
          />
          <button type="submit" className="chat-input-send" disabled>
            Send
          </button>
        </form>
      </div>
    </ScreenShell>
  );
}
