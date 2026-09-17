-- LifeOS production schema, GoalRelation vertical slice (M2, 2026-09-17).
-- LIFEOS_SPEC.md §5.2 frozen relation-type enum - GoalRelation only,
-- this slice (LinkGoals). Does NOT create Milestone, Task, Calendar, or
-- Personal Knowledge tables - each gets its own dedicated migration in
-- a later slice.
--
-- V1 (Goal) and V2 (Project) are historical production migrations and
-- are not edited here.

CREATE TABLE goal_relations (
    id              TEXT PRIMARY KEY,
    -- Both endpoints REFERENCE goals(id) with ON DELETE RESTRICT
    -- (architecture-freeze cascade-vs-block resolution for GoalRelation,
    -- M2 GoalRelation vertical-slice report §8): Goal has no hard
    -- DeleteGoal action today, and a relation must never silently
    -- disappear because its endpoint Goal was removed - any future real
    -- Goal deletion must explicitly deal with its relations first.
    -- LifecycleStatus transitions (PAUSED/COMPLETED/ABANDONED) never
    -- delete the Goal row, so they never touch this constraint.
    from_goal_id    TEXT NOT NULL REFERENCES goals (id) ON DELETE RESTRICT,
    to_goal_id      TEXT NOT NULL REFERENCES goals (id) ON DELETE RESTRICT,
    -- Frozen, closed 11-value enum (LIFEOS_SPEC.md §5.2) - exactly the
    -- architecture-freeze relation-type list, no more, no fewer.
    relation_type   TEXT NOT NULL CHECK (relation_type IN (
                        'SUPPORTS', 'CONFLICTS_WITH', 'DEPENDS_ON', 'BLOCKS',
                        'CONTRIBUTES_TO', 'REQUIRES', 'PARENT_OF', 'CHILD_OF',
                        'DUPLICATES', 'SUPERSEDES', 'COMPETES_FOR_RESOURCE'
                    )),
    note            TEXT,
    created_at      TEXT NOT NULL,
    -- A Goal cannot be linked to itself in GoalRelation v0.1 - every
    -- frozen relation type describes a relationship between distinct
    -- Goal records (M2 GoalRelation vertical-slice report §6).
    CHECK (from_goal_id <> to_goal_id),
    -- Exact duplicates ((from, to, type) triples) are rejected; a
    -- different relation_type between the same pair, or the same type
    -- in the reverse direction, is a distinct, allowed row (report §7).
    UNIQUE (from_goal_id, to_goal_id, relation_type)
);

CREATE INDEX idx_goal_relations_from_goal_id ON goal_relations (from_goal_id);
CREATE INDEX idx_goal_relations_to_goal_id ON goal_relations (to_goal_id);

-- Minimum audit trail for LinkGoals, same narrow shape as
-- goal_audit_log/project_audit_log - one row per audited mutation, not
-- a full event-sourcing system. LinkGoals is create-only in this slice
-- (no UpdateGoalRelation/DeleteGoalRelation action exists), so
-- previous_snapshot is always NULL today - the column is kept for
-- shape-consistency with the other two audit tables.
CREATE TABLE goal_relation_audit_log (
    id                  TEXT PRIMARY KEY,
    goal_relation_id    TEXT NOT NULL REFERENCES goal_relations (id),
    action_type         TEXT NOT NULL CHECK (action_type IN ('LinkGoals')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI', 'CloudLLM', 'NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit', 'Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);

CREATE INDEX idx_goal_relation_audit_log_goal_relation_id ON goal_relation_audit_log (goal_relation_id);
