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

interface ConstellationPoint {
  x: number;
  y: number;
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
    size: prominent ? Math.random() * 1.5 + 1.1 : Math.random() * 0.9 + 0.5,
    peakOpacity: prominent ? Math.random() * 0.25 + 0.55 : Math.random() * 0.25 + 0.3,
    delay: Math.random() * 10,
    duration: Math.random() * 5 + 5,
    color: colored ? pick(ACCENT_COLORS) : pick(NEUTRAL_COLORS),
  };
}

function makeStarField(): Star[] {
  const stars: Star[] = [];

  // Several soft cluster centres, not an even grid - large areas of the
  // sky should stay dark between them.
  const clusters = Array.from({ length: 8 }, () => ({
    cx: Math.random() * 100,
    cy: Math.random() * 100,
    spread: Math.random() * 9 + 7,
  }));

  for (const c of clusters) {
    const count = 20 + Math.floor(Math.random() * 16);
    for (let i = 0; i < count; i++) {
      const left = clamp(c.cx + clusteredOffset(c.spread), 0, 100);
      const top = clamp(c.cy + clusteredOffset(c.spread), 0, 100);
      stars.push(makeStar(left, top, true));
    }
  }

  // Sparse background coverage so the rest of the sky isn't empty.
  for (let i = 0; i < 100; i++) {
    stars.push(makeStar(Math.random() * 100, Math.random() * 100, false));
  }

  return stars;
}

/** One deliberately larger, brighter formation with faint connecting
 * lines - a single visible "constellation" anchor rather than a field of
 * uniform clusters, placed off-centre like a real night sky would have. */
function makeConstellation(): { points: ConstellationPoint[]; stars: Star[] } {
  const centre = { x: 74 + Math.random() * 10, y: 18 + Math.random() * 12 };
  const pointCount = 6 + Math.floor(Math.random() * 2);
  const points: ConstellationPoint[] = [];

  for (let i = 0; i < pointCount; i++) {
    const angle = (i / pointCount) * Math.PI * 1.5 + Math.random() * 0.4;
    const radius = 4 + Math.random() * 8;
    points.push({
      x: clamp(centre.x + Math.cos(angle) * radius, 2, 98),
      y: clamp(centre.y + Math.sin(angle) * radius * 0.7, 2, 98),
    });
  }

  const stars = points.map((p) => ({
    left: p.x,
    top: p.y,
    size: Math.random() * 1.1 + 1.9,
    peakOpacity: Math.random() * 0.15 + 0.8,
    delay: Math.random() * 10,
    duration: Math.random() * 4 + 6,
    color: pick(NEUTRAL_COLORS),
  }));

  return { points, stars };
}

/** Full-screen, almost-black cosmic background (LIFEOS_SPEC.md §9.1).
 * Provides atmosphere only - it must stay behind and out of the way of
 * the NeXa face and content. Motion is calm and slow, never distracting,
 * but should read as clearly alive rather than a static image. */
export function Galaxy() {
  const stars = useMemo(() => makeStarField(), []);
  const constellation = useMemo(() => makeConstellation(), []);

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

        <svg className="galaxy-constellation" viewBox="0 0 100 100" preserveAspectRatio="none">
          <polyline
            points={constellation.points.map((p) => `${p.x},${p.y}`).join(" ")}
            fill="none"
            stroke="rgba(230, 235, 255, 0.16)"
            strokeWidth="0.12"
            vectorEffect="non-scaling-stroke"
          />
        </svg>

        {constellation.stars.map((s, i) => (
          <span
            key={`c-${i}`}
            className="galaxy-star galaxy-star--constellation"
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
