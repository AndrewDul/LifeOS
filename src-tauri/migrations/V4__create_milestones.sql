-- LifeOS production schema, Milestone vertical slice (M2, 2026-09-17).
-- LIFEOS_SPEC.md §5.1 frozen v0.1 domain model - Milestone only, this
-- slice (AddMilestone/UpdateMilestone). Does NOT create Task, Calendar,
-- or Personal Knowledge tables - each gets its own dedicated migration
-- in a later slice.
--
-- V1 (Goal), V2 (Project), and V3 (GoalRelation) are historical
-- production migrations and are not edited here.

CREATE TABLE milestones (
    id              TEXT PRIMARY KEY,
    -- Required parent, ON DELETE RESTRICT (M2 Project Child Action
    -- Freeze report §9 / Milestone vertical-slice report §9): Project
    -- has no hard DeleteProject action today, and a Milestone must
    -- never silently disappear if one is ever added - matches
    -- GoalRelation's precedent exactly.
    project_id      TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    title           TEXT NOT NULL CHECK (length(trim(title)) > 0),
    target_date     TEXT,
    -- Frozen, closed 2-value enum (LIFEOS_SPEC.md §5.1) - deliberately
    -- NOT the same type/CHECK list as Goal/Project's 4-value status.
    status          TEXT NOT NULL CHECK (status IN ('PENDING', 'DONE')),
    -- A position cannot be negative - the one canonical numeric bound
    -- that follows directly from what order_index *is*; no upper bound
    -- is invented (no canonical policy for one exists) - see the
    -- Milestone vertical-slice report §3.
    order_index     INTEGER NOT NULL CHECK (order_index >= 0),
    -- Resolved 2026-09-17 (M2 Milestone vertical-slice report §3,
    -- closing the M2 Project Child Action Freeze report's open
    -- question): a Project may not have two Milestones at the same
    -- position. Different Projects MAY reuse the same order_index
    -- values freely - this constraint is scoped to (project_id,
    -- order_index) together, not order_index alone.
    UNIQUE (project_id, order_index)
);

-- No standalone project_id index: the UNIQUE(project_id, order_index)
-- constraint above already creates a composite index with project_id
-- as its leading column, which SQLite can use directly for
-- "WHERE project_id = ?" - a second, single-column project_id index
-- would be redundant (deliberately not added - "no speculative
-- indexes"). No target_date index either - nothing currently queries
-- or sorts by it directly.

-- Minimum audit trail for AddMilestone/UpdateMilestone, same narrow
-- shape as goal_audit_log/project_audit_log/goal_relation_audit_log -
-- one row per audited mutation, not a full event-sourcing system.
CREATE TABLE milestone_audit_log (
    id                  TEXT PRIMARY KEY,
    milestone_id        TEXT NOT NULL REFERENCES milestones (id),
    action_type         TEXT NOT NULL CHECK (action_type IN ('AddMilestone', 'UpdateMilestone')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI', 'CloudLLM', 'NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit', 'Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);

CREATE INDEX idx_milestone_audit_log_milestone_id ON milestone_audit_log (milestone_id);
