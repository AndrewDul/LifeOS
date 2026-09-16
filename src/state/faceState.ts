/**
 * Face animation states (LIFEOS_SPEC.md §9.1).
 * Infrastructure only in M1 - nothing yet drives LISTENING,
 * LOCAL_PROCESSING, WAITING_FOR_CLOUD or SPEAKING for real; those arrive
 * with voice/AI in later milestones. ATTENTION is exercised now as a
 * brief click acknowledgement.
 */
export type FaceState =
  | "IDLE"
  | "LISTENING"
  | "LOCAL_PROCESSING"
  | "WAITING_FOR_CLOUD"
  | "SPEAKING"
  | "ATTENTION";
