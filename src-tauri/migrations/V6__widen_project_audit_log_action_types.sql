-- LifeOS production schema, UpdateProject action-type widening (M2,
-- 2026-09-17).
--
-- V2__create_projects.sql's `project_audit_log.action_type` CHECK was
-- scoped to exactly the two Project actions that existed at the time
-- it was written: CHECK (action_type IN ('CreateProject',
-- 'UpdateProgress')). This is the same "audit CHECK enumerates exactly
-- this entity's actions" pattern every later entity's audit table also
-- uses (goal_relation_audit_log, milestone_audit_log, task_audit_log) -
-- it was correct when written, but it did not anticipate a later
-- action being added to an ALREADY-SHIPPED entity's audit table, the
-- way Milestone/Task's audit tables never had to (both of their
-- actions - Add*/Update* - were frozen and built in the same
-- migration).
--
-- The M2 UpdateProject implementation needs `action_type = 'UpdateProject'`
-- to be a legal value so its audit row can actually be inserted. SQLite
-- has no `ALTER TABLE ... DROP/MODIFY CONSTRAINT`, so the standard,
-- minimal fix is the well-known rebuild pattern: create a new table
-- with the corrected CHECK, copy every existing row across unchanged,
-- drop the old table, rename the new one into place, and recreate its
-- index. No column is added, removed, or renamed; no other constraint
-- changes; `projects` itself is completely untouched.
--
-- V1-V5 remain historical and are not edited here - this is a
-- deliberate, minimal, additive V6, not a rewrite of an already-shipped
-- migration.

CREATE TABLE project_audit_log_new (
    id                  TEXT PRIMARY KEY,
    project_id          TEXT NOT NULL REFERENCES projects (id),
    action_type         TEXT NOT NULL CHECK (action_type IN ('CreateProject', 'UpdateProgress', 'UpdateProject')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI', 'CloudLLM', 'NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit', 'Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);

INSERT INTO project_audit_log_new (id, project_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
SELECT id, project_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at
FROM project_audit_log;

DROP TABLE project_audit_log;

ALTER TABLE project_audit_log_new RENAME TO project_audit_log;

CREATE INDEX idx_project_audit_log_project_id ON project_audit_log (project_id);
