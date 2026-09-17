import { useState } from "react";
import { ScreenShell } from "./ScreenShell";
import { ChatSidebar, type ChatListItem } from "./ChatSidebar";
import type { Direction, ScreenId } from "../state/navigation";
import type { FaceState } from "../state/faceState";

interface ChatScreenProps {
  faceState: FaceState;
  onNavigate: (id: ScreenId) => void;
  onHome: () => void;
  direction: Direction;
  closing: boolean;
}

interface DemoMessage {
  from: "nexa" | "user";
  text: string;
}

const NEW_CHAT_ID = "new";

const DEMO_CHATS: ChatListItem[] = [
  { id: "1", title: "Planning tomorrow", preview: "Let's map out the morning..." },
  { id: "2", title: "Van purchase budget", preview: "Based on your safe-to-spend..." },
  { id: "3", title: "Weekly review", preview: "Here's what stood out this week..." },
];

const DEMO_TRANSCRIPTS: Record<string, DemoMessage[]> = {
  "1": [
    { from: "nexa", text: "Good evening. Here's a quick look at how conversation will feel once I'm connected." },
    { from: "user", text: "Looks good - show me around." },
    { from: "nexa", text: "This is a visual preview only for now. Real conversation arrives in M5." },
  ],
  "2": [
    { from: "user", text: "How much could I put toward the van this month?" },
    { from: "nexa", text: "This is a demo reply - real budget-aware answers arrive once Money and the cloud model are connected (M5, M8)." },
  ],
  "3": [
    { from: "nexa", text: "Here's a preview of what a weekly review conversation could look like." },
    { from: "user", text: "Nice - what did I actually get done?" },
    { from: "nexa", text: "Real review content depends on real data, which doesn't exist yet. This is visual only." },
  ],
};

/**
 * Chat's M1 visual shell (LIFEOS_SPEC.md §9.4, build-start brief §10, and
 * the follow-up visual-refinement brief §4). Visually representative
 * demo conversation only - no AI, no backend, no persistence. Traditional
 * message bubbles are appropriate here (unlike the unboxed
 * CommunicationDisplay), kept restrained and dark. The chat-specific
 * sidebar (New Chat / recent chats) is Chat's own and stays - ScreenShell
 * no longer provides a global sidebar at all, only the NeXa Island.
 */
export function ChatScreen({ faceState, onNavigate, onHome, direction, closing }: ChatScreenProps) {
  const [activeChatId, setActiveChatId] = useState<string>(DEMO_CHATS[0].id);

  const messages = activeChatId === NEW_CHAT_ID ? [] : DEMO_TRANSCRIPTS[activeChatId] ?? [];

  return (
    <ScreenShell
      activeId="chat"
      faceState={faceState}
      onNavigate={onNavigate}
      onHome={onHome}
      direction={direction}
      closing={closing}
    >
      <div className="chat-screen">
        <ChatSidebar
          chats={DEMO_CHATS}
          activeChatId={activeChatId}
          onSelectChat={setActiveChatId}
          onNewChat={() => setActiveChatId(NEW_CHAT_ID)}
        />

        <div className="chat-main">
          {messages.length > 0 ? (
            <div className="chat-history">
              {messages.map((m, i) => (
                <div key={i} className={`chat-bubble chat-bubble--${m.from}`}>
                  {m.text}
                </div>
              ))}
            </div>
          ) : (
            <div className="chat-empty">
              <p>Start a new conversation.</p>
              <p className="chat-empty-note">Visual preview only - real conversation arrives in M5.</p>
            </div>
          )}

          <form className="chat-input-bar" onSubmit={(e) => e.preventDefault()}>
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
      </div>
    </ScreenShell>
  );
}
