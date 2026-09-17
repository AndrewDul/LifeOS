-- LifeOS production schema, Goal vertical slice (M2, 2026-09-17).
-- LIFEOS_SPEC.md §5.1 frozen v0.1 domain model - Goal only, this slice.
-- Must never create spike_records (that lives under
-- src-tauri/src/services/storage/spike/migrations/, compiled for
-- #[cfg(test)] only).

CREATE TABLE goals (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description     TEXT,
    status          TEXT NOT NULL CHECK (status IN ('ACTIVE', 'PAUSED', 'COMPLETED', 'ABANDONED')),
    target_date     TEXT,
    priority_user   INTEGER CHECK (priority_user IS NULL OR (priority_user BETWEEN 1 AND 5)),
    priority_system INTEGER CHECK (priority_system IS NULL OR (priority_system BETWEEN 1 AND 5)),
    progress        INTEGER NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
    domain_tag      TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX idx_goals_status ON goals (status);
CREATE INDEX idx_goals_domain_tag ON goals (domain_tag);
CREATE INDEX idx_goals_target_date ON goals (target_date);

-- Minimum audit trail to support a future Undo (build brief Part C5) -
-- one row per audited mutation, not a full event-sourcing system.
-- IMPORTANT_CHANGE-and-above actions always write one of these
-- (LIFEOS_SPEC.md §5.5); EXPLICIT_ADD is not required to, but CreateGoal
-- writes one too in this slice for full Goal history from day one.
CREATE TABLE goal_audit_log (
    id                  TEXT PRIMARY KEY,
    goal_id             TEXT NOT NULL REFERENCES goals (id),
    action_type         TEXT NOT NULL CHECK (action_type IN ('CreateGoal', 'UpdateGoal')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI', 'CloudLLM', 'NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit', 'Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);

CREATE INDEX idx_goal_audit_log_goal_id ON goal_audit_log (goal_id);
