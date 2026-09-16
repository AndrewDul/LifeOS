/**
 * Primary radial menu ring (LIFEOS_SPEC.md §9.1, §9.2).
 * Secondary ring (Memory, Quick Forms, Model/AI, Settings) is not built
 * in M1 - the first ring must not be overloaded.
 */
export type ScreenId =
  | "today"
  | "chat"
  | "calendar"
  | "goals"
  | "projects"
  | "money"
  | "me"
  | "insights";

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
