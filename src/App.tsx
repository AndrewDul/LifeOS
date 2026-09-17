import { useEffect, useRef, useState } from "react";
import { Galaxy } from "./components/Galaxy";
import { NexaFace } from "./components/NexaFace";
import { RadialMenu } from "./components/RadialMenu";
import { CommunicationDisplay } from "./components/CommunicationDisplay";
import { ChatScreen } from "./components/ChatScreen";
import { PlaceholderScreen } from "./components/PlaceholderScreen";
import type { FaceState } from "./state/faceState";
import type { ScreenId } from "./state/navigation";
import "./App.css";

const DEMO_GREETING =
  "Good evening.\nI organized your day and found space for focused work.\nOpen Chat when you're ready.";

function App() {
  const [faceState, setFaceState] = useState<FaceState>("IDLE");
  const [menuOpen, setMenuOpen] = useState(false);
  const [currentScreen, setCurrentScreen] = useState<ScreenId | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const attentionTimeout = useRef<number | undefined>(undefined);

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

  function handleSelect(id: ScreenId) {
    setMenuOpen(false);
    setCurrentScreen(id);
  }

  function handleHome() {
    setCurrentScreen(null);
    setMenuOpen(false);
  }

  function handleCloseMessage() {
    setMessage(null);
  }

  const homeMode = message ? "message" : menuOpen ? "menu" : "idle";

  return (
    <div className="app-root">
      <Galaxy />

      {currentScreen === "chat" ? (
        <ChatScreen faceState={faceState} onNavigate={handleSelect} onHome={handleHome} />
      ) : currentScreen ? (
        <PlaceholderScreen
          id={currentScreen}
          faceState={faceState}
          onNavigate={handleSelect}
          onHome={handleHome}
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

          <RadialMenu open={menuOpen && !message} onSelect={handleSelect} />

          {message && <CommunicationDisplay message={message} onClose={handleCloseMessage} />}
        </div>
      )}
    </div>
  );
}

export default App;
