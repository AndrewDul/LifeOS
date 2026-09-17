/**
 * Primary radial menu ring (LIFEOS_SPEC.md §9.1, §9.2). Secondary ring
 * items (Memory, Quick Forms, Model/AI, Settings) stay off the radial
 * ring per spec - Settings is still reachable, but only from the side
 * navigation on full-screen sections, not the first ring.
 */
export type ScreenId =
  | "today"
  | "chat"
  | "calendar"
  | "goals"
  | "projects"
  | "money"
  | "me"
  | "insights"
  | "settings";

export interface ScreenDef {
  id: ScreenId;
  label: string;
}

export const PRIMARY_SCREENS: ScreenDef[] = [
  { id: "today", label: "Today" },
  { id: "chat", label: "Chat" },
  { id: "calendar", label: "Calendar" },
  { id: "goals", label: "Goals" },
  { id: "projects", label: "Projects" },
  { id: "money", label: "Money" },
  { id: "me", label: "Me" },
  { id: "insights", label: "Insights" },
];

/**
 * Side navigation order for full-screen sections. Same order as the
 * radial ring (kept consistent on purpose) plus Settings appended -
 * Settings has no real content yet ("Settings later"), it is just
 * navigable as a placeholder like the others.
 */
export const SIDEBAR_SCREENS: ScreenDef[] = [
  ...PRIMARY_SCREENS,
  { id: "settings", label: "Settings" },
];

export function screenLabel(id: ScreenId): string {
  return SIDEBAR_SCREENS.find((s) => s.id === id)?.label ?? id;
}
