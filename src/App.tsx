import { useRef, useState } from "react";
import { Galaxy } from "./components/Galaxy";
import { NexaFace } from "./components/NexaFace";
import { RadialMenu } from "./components/RadialMenu";
import { EdgeDrawers } from "./components/EdgeDrawers";
import { Screen } from "./components/Screen";
import type { FaceState } from "./state/faceState";
import type { ScreenId } from "./state/navigation";
import "./App.css";

function App() {
  const [faceState, setFaceState] = useState<FaceState>("IDLE");
  const [menuOpen, setMenuOpen] = useState(false);
  const [activeScreen, setActiveScreen] = useState<ScreenId | null>(null);
  const attentionTimeout = useRef<number | undefined>(undefined);

  function handleFaceClick() {
    if (activeScreen) return;

    setFaceState("ATTENTION");
    window.clearTimeout(attentionTimeout.current);
    attentionTimeout.current = window.setTimeout(() => setFaceState("IDLE"), 260);

    setMenuOpen((open) => !open);
  }

  function handleSelect(id: ScreenId) {
    setMenuOpen(false);
    setActiveScreen(id);
  }

  function handleBack() {
    setActiveScreen(null);
  }

  return (
    <div className="app-root">
      <Galaxy />

      {activeScreen ? (
        <Screen id={activeScreen} onBack={handleBack} />
      ) : (
        <div className="home">
          <NexaFace state={faceState} shrunk={menuOpen} onClick={handleFaceClick} />
          <RadialMenu open={menuOpen} onSelect={handleSelect} />
        </div>
      )}

      <EdgeDrawers />
    </div>
  );
}

export default App;
