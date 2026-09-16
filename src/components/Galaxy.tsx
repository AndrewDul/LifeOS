import { useMemo } from "react";

interface Star {
  left: number;
  top: number;
  size: number;
  delay: number;
  duration: number;
}

function makeStars(count: number): Star[] {
  const stars: Star[] = [];
  for (let i = 0; i < count; i++) {
    stars.push({
      left: Math.random() * 100,
      top: Math.random() * 100,
      size: Math.random() * 1.6 + 0.6,
      delay: Math.random() * 6,
      duration: Math.random() * 3 + 3,
    });
  }
  return stars;
}

/** Full-screen dark galaxy background (LIFEOS_SPEC.md §9.1). Stays behind
 * every other layer - Home, drawers, and screens all sit above it. */
export function Galaxy() {
  const stars = useMemo(() => makeStars(140), []);

  return (
    <div className="galaxy" aria-hidden="true">
      <div className="galaxy-nebula galaxy-nebula--a" />
      <div className="galaxy-nebula galaxy-nebula--b" />
      {stars.map((s, i) => (
        <span
          key={i}
          className="galaxy-star"
          style={{
            left: `${s.left}%`,
            top: `${s.top}%`,
            width: `${s.size}px`,
            height: `${s.size}px`,
            animationDelay: `${s.delay}s`,
            animationDuration: `${s.duration}s`,
          }}
        />
      ))}
    </div>
  );
}
