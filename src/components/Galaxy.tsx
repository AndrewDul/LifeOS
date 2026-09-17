import { useMemo } from "react";

interface Star {
  left: number;
  top: number;
  size: number;
  peakOpacity: number;
  delay: number;
  duration: number;
  color: string;
}

// 97% neutral cool-white, 3% sparse tasteful color (split evenly).
const NEUTRAL_COLORS = ["#eef1ff", "#e4e9ff", "#ffffff"];
const ACCENT_COLORS = ["#ffbf86", "#ffe3a0", "#a9c8ff"]; // warm orange / yellow / cool blue

function pick<T>(arr: T[]): T {
  return arr[Math.floor(Math.random() * arr.length)];
}

/** Sum of three uniforms approximates a soft bell curve - enough to make
 * clusters feel organic without pulling in a real distribution library. */
function clusteredOffset(spread: number) {
  return ((Math.random() + Math.random() + Math.random()) / 3 - 0.5) * 2 * spread;
}

function clamp(v: number, min: number, max: number) {
  return Math.min(max, Math.max(min, v));
}

function makeStar(left: number, top: number, prominent: boolean): Star {
  const colored = Math.random() < 0.03;
  return {
    left,
    top,
    size: prominent ? Math.random() * 1.4 + 1.1 : Math.random() * 0.9 + 0.5,
    peakOpacity: prominent ? Math.random() * 0.25 + 0.55 : Math.random() * 0.25 + 0.3,
    delay: Math.random() * 10,
    duration: Math.random() * 5 + 5,
    color: colored ? pick(ACCENT_COLORS) : pick(NEUTRAL_COLORS),
  };
}

function makeStarField(): Star[] {
  const stars: Star[] = [];

  // A handful of soft cluster centres, not an even grid - large areas
  // of the sky should stay dark between them.
  const clusters = Array.from({ length: 6 }, () => ({
    cx: Math.random() * 100,
    cy: Math.random() * 100,
    spread: Math.random() * 9 + 7,
  }));

  for (const c of clusters) {
    const count = 14 + Math.floor(Math.random() * 10);
    for (let i = 0; i < count; i++) {
      const left = clamp(c.cx + clusteredOffset(c.spread), 0, 100);
      const top = clamp(c.cy + clusteredOffset(c.spread), 0, 100);
      stars.push(makeStar(left, top, true));
    }
  }

  // Sparse background coverage so the rest of the sky isn't empty.
  for (let i = 0; i < 60; i++) {
    stars.push(makeStar(Math.random() * 100, Math.random() * 100, false));
  }

  return stars;
}

/** Full-screen, almost-black cosmic background (LIFEOS_SPEC.md §9.1).
 * Provides atmosphere only - it must stay behind and out of the way of
 * the NeXa face and content. Motion is deliberately near-imperceptible. */
export function Galaxy() {
  const stars = useMemo(() => makeStarField(), []);

  return (
    <div className="galaxy" aria-hidden="true">
      <div className="galaxy-haze galaxy-haze--a" />
      <div className="galaxy-haze galaxy-haze--b" />
      <div className="galaxy-stars">
        {stars.map((s, i) => (
          <span
            key={i}
            className="galaxy-star"
            style={{
              left: `${s.left}%`,
              top: `${s.top}%`,
              width: `${s.size}px`,
              height: `${s.size}px`,
              background: s.color,
              animationDelay: `${s.delay}s`,
              animationDuration: `${s.duration}s`,
              // @ts-expect-error custom property
              "--peak-opacity": s.peakOpacity,
            }}
          />
        ))}
      </div>
    </div>
  );
}
