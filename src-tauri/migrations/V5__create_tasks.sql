-- LifeOS production schema, Task vertical slice (M2, 2026-09-17).
-- LIFEOS_SPEC.md §5.1 frozen v0.1 domain model - Task only, this slice
-- (AddTask/UpdateTask). Does NOT create RecurringRule, Calendar,
-- Reminder, Challenge, or Personal Knowledge tables - each gets its own
-- dedicated migration in a later slice.
--
-- V1 (Goal), V2 (Project), V3 (GoalRelation), and V4 (Milestone) are
-- historical production migrations and are not edited here.

CREATE TABLE tasks (
    id                  TEXT PRIMARY KEY,
    title               TEXT NOT NULL CHECK (length(trim(title)) > 0),
    -- Frozen, closed 4-value enum (LIFEOS_SPEC.md §5.1) - all four are
    -- peer, freely reversible states; no terminal-state CHECK is added
    -- (Task vertical-slice report §7).
    status              TEXT NOT NULL CHECK (status IN ('TODO', 'IN_PROGRESS', 'DONE', 'CANCELLED')),
    due_at              TEXT,
    -- Both parents are independently nullable - a Task may be
    -- standalone, Project-only, Goal-only, or linked to both (Task
    -- vertical-slice report §3, already frozen by the M2 Project Child
    -- Action Freeze report). ON DELETE RESTRICT on both, matching
    -- Milestone/GoalRelation's precedent exactly: neither Project nor
    -- Goal has a hard delete action today, so a Task must never
    -- silently disappear because a parent it references was removed.
    project_id          TEXT REFERENCES projects (id) ON DELETE RESTRICT,
    goal_id             TEXT REFERENCES goals (id) ON DELETE RESTRICT,
    -- Reserved for the future, NOT-YET-IMPLEMENTED RecurringRule entity
    -- (Task vertical-slice report §2/§3). There is currently no
    -- recurring_rules table and therefore no real FK target - rather
    -- than leave this column an unconstrained dangling TEXT, or omit it
    -- and silently drop a frozen field, this CHECK makes the deferred
    -- state explicit and database-safe: normal M2 production code can
    -- never write a non-NULL value here. A later RecurringRule/Calendar
    -- migration may create the real `recurring_rules` table and replace
    -- this temporary NULL-only CHECK with a real
    -- `REFERENCES recurring_rules(id)` foreign key - do not assume
    -- recurrence exists before that migration lands.
    recurring_rule_id   TEXT CHECK (recurring_rule_id IS NULL),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

-- Indexes for the obvious Task reads this slice actually implements
-- (get_tasks_for_project/get_tasks_for_goal, and due_at/status as the
-- two other columns any future read is near-certain to filter/sort by)
-- - deliberately single-column, no speculative composite index yet.
CREATE INDEX idx_tasks_project_id ON tasks (project_id);
CREATE INDEX idx_tasks_goal_id ON tasks (goal_id);
CREATE INDEX idx_tasks_due_at ON tasks (due_at);
CREATE INDEX idx_tasks_status ON tasks (status);

-- Minimum audit trail for AddTask/UpdateTask, same narrow shape as
-- goal_audit_log/project_audit_log/goal_relation_audit_log/
-- milestone_audit_log - one row per audited mutation, not a full
-- event-sourcing system.
CREATE TABLE task_audit_log (
    id                  TEXT PRIMARY KEY,
    task_id             TEXT NOT NULL REFERENCES tasks (id),
    action_type         TEXT NOT NULL CHECK (action_type IN ('AddTask', 'UpdateTask')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI', 'CloudLLM', 'NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit', 'Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);

CREATE INDEX idx_task_audit_log_task_id ON task_audit_log (task_id);
