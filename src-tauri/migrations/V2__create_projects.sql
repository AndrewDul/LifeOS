-- LifeOS production schema, Project vertical slice (M2, 2026-09-17).
-- LIFEOS_SPEC.md §5.1 frozen v0.1 domain model - Project only, this
-- slice. Does NOT create Milestone, Task, GoalRelation, or Calendar
-- tables - each gets its own dedicated migration in a later slice.
--
-- V1 (Goal) is historical production migration and is not edited here.

CREATE TABLE projects (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description     TEXT,
    status          TEXT NOT NULL CHECK (status IN ('ACTIVE', 'PAUSED', 'COMPLETED', 'ABANDONED')),
    priority        INTEGER CHECK (priority IS NULL OR (priority BETWEEN 1 AND 5)),
    -- Nullable, frozen Project field (architecture-freeze report §C) -
    -- the first real cross-entity relationship in production M2. No
    -- ON DELETE/ON UPDATE cascade rule is frozen/documented anywhere,
    -- so none is invented here: this is a plain FK reference with
    -- SQLite's normal explicit behavior. Enforcement of "does this Goal
    -- actually exist" happens in application code
    -- (services::create_project), not solely via this constraint - see
    -- the Project vertical-slice report §5.
    goal_id         TEXT REFERENCES goals (id),
    progress        INTEGER NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX idx_projects_goal_id ON projects (goal_id);
CREATE INDEX idx_projects_status ON projects (status);
CREATE INDEX idx_projects_priority ON projects (priority);
CREATE INDEX idx_projects_updated_at ON projects (updated_at);

-- Minimum audit trail, same narrow shape as goal_audit_log - one row per
-- audited mutation, not a full event-sourcing system.
CREATE TABLE project_audit_log (
    id                  TEXT PRIMARY KEY,
    project_id          TEXT NOT NULL REFERENCES projects (id),
    action_type         TEXT NOT NULL CHECK (action_type IN ('CreateProject', 'UpdateProgress')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI', 'CloudLLM', 'NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit', 'Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);

CREATE INDEX idx_project_audit_log_project_id ON project_audit_log (project_id);
