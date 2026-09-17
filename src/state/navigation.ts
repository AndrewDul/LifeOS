/**
 * Primary radial menu ring (LIFEOS_SPEC.md §9.1, §9.2). Kept to 8 items
 * on purpose - "do not overload the first radial ring".
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
  | "settings"
  | "notifications";

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
 * Every navigable section, including ones off the radial ring
 * (Settings, Notifications). There is no permanent global sidebar
 * anymore - this list now only feeds the NeXa Island's compact quick
 * menu, which is the sole persistent way to jump between sections while
 * one is open.
 */
export const ALL_SCREENS: ScreenDef[] = [
  ...PRIMARY_SCREENS,
  { id: "settings", label: "Settings" },
  { id: "notifications", label: "Notifications" },
];

export function screenLabel(id: ScreenId): string {
  return ALL_SCREENS.find((s) => s.id === id)?.label ?? id;
}

/**
 * Where an opened screen entered from, driving its slide-in animation
 * (and, in reverse, its slide-out when returning Home). "radial" covers
 * both the radial menu and the NeXa Island's quick-menu navigation -
 * neither has a literal screen edge to slide from, so both use a calm
 * fade/scale instead of a directional slide.
 */
export type Direction = "top" | "bottom" | "left" | "right" | "radial";
