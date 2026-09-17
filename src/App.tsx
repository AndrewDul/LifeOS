import { useEffect, useRef, useState } from "react";
import { Galaxy } from "./components/Galaxy";
import { NexaFace } from "./components/NexaFace";
import { RadialMenu } from "./components/RadialMenu";
import { EdgeDrawers } from "./components/EdgeDrawers";
import { CommunicationDisplay } from "./components/CommunicationDisplay";
import { ChatScreen } from "./components/ChatScreen";
import { PlaceholderScreen } from "./components/PlaceholderScreen";
import type { FaceState } from "./state/faceState";
import type { Direction, ScreenId } from "./state/navigation";
import "./App.css";

const DEMO_GREETING =
  "Good evening.\nI organized your day and found space for focused work.\nOpen Chat when you're ready.";

// Must match the CSS closing transition duration on .screen-shell.
const CLOSE_ANIM_MS = 420;

function App() {
  const [faceState, setFaceState] = useState<FaceState>("IDLE");
  const [menuOpen, setMenuOpen] = useState(false);
  const [currentScreen, setCurrentScreen] = useState<ScreenId | null>(null);
  const [entryDirection, setEntryDirection] = useState<Direction>("radial");
  const [closing, setClosing] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const attentionTimeout = useRef<number | undefined>(undefined);
  const closeTimeout = useRef<number | undefined>(undefined);

  // Demo-only: shows the reusable CommunicationDisplay once on first
  // load so it's visually reachable without adding a prototype trigger
  // button. Real triggers (AI, notifications) arrive in later milestones.
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setMessage((current) => current ?? DEMO_GREETING);
    }, 1100);
    return () => window.clearTimeout(timer);
  }, []);

  function handleFaceClick() {
    if (currentScreen || message) return;

    setFaceState("ATTENTION");
    window.clearTimeout(attentionTimeout.current);
    attentionTimeout.current = window.setTimeout(() => setFaceState("IDLE"), 260);

    setMenuOpen((open) => !open);
  }

  /** Opens (or switches directly to) a full-screen section. `direction`
   * is "radial" for the radial menu and the NeXa Island's quick menu
   * (neither has a literal edge to slide from); edge handles pass their
   * own edge. Switching screen-to-screen this way is an instant swap
   * with a fade-in of the new screen, not an animated exit of the old
   * one - only returning Home (closeScreen) animates an exit. */
  function openScreen(id: ScreenId, direction: Direction) {
    window.clearTimeout(closeTimeout.current);
    setClosing(false);
    setEntryDirection(direction);
    setCurrentScreen(id);
    setMenuOpen(false);
  }

  /** Returns Home, sliding the current screen back out the same edge it
   * entered from (or fading out, for a radial-opened screen) before
   * actually unmounting it. */
  function closeScreen() {
    setClosing(true);
    window.clearTimeout(closeTimeout.current);
    closeTimeout.current = window.setTimeout(() => {
      setCurrentScreen(null);
      setClosing(false);
    }, CLOSE_ANIM_MS);
  }

  function handleCloseMessage() {
    setMessage(null);
  }

  const homeMode = message ? "message" : menuOpen ? "menu" : "idle";

  return (
    <div className="app-root">
      <Galaxy />

      {currentScreen === "chat" ? (
        <ChatScreen
          faceState={faceState}
          onNavigate={(id) => openScreen(id, "radial")}
          onHome={closeScreen}
          direction={entryDirection}
          closing={closing}
        />
      ) : currentScreen ? (
        <PlaceholderScreen
          id={currentScreen}
          faceState={faceState}
          onNavigate={(id) => openScreen(id, "radial")}
          onHome={closeScreen}
          direction={entryDirection}
          closing={closing}
        />
      ) : (
        <div className="home-stage" data-mode={homeMode}>
          <div className="home-face-slot">
            <NexaFace
              state={faceState}
              size="hero"
              shrunk={menuOpen || !!message}
              onClick={handleFaceClick}
            />
          </div>

          <RadialMenu open={menuOpen && !message} onSelect={(id) => openScreen(id, "radial")} />

          {message && <CommunicationDisplay message={message} onClose={handleCloseMessage} />}

          <EdgeDrawers onOpen={openScreen} />
        </div>
      )}
    </div>
  );
}

export default App;
