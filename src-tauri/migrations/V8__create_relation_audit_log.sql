-- LifeOS production schema, Relation audit log (M2 Personal Knowledge
-- Relation Foundation, R026, 2026-09-18). Adds exactly one new,
-- independent table - relation_audit_log - and nothing else. `relations`
-- itself (V7__create_personal_knowledge.sql) is NOT altered: no
-- `updated_at` column, no `superseded_by_relation_id` column. This
-- exact scope was frozen by R025 (M2 Personal Knowledge Relation
-- Action-Family Architecture Freeze) and its two post-freeze addenda -
-- see that report for the full reasoning, not repeated here.
--
-- Structurally identical to `fact_audit_log` (V7), substituting
-- Relation-specific identifiers/action names - the same "every M2
-- write path gets its own dedicated audit table" precedent every
-- earlier entity family already established (goal_audit_log,
-- project_audit_log, goal_relation_audit_log, milestone_audit_log,
-- task_audit_log, fact_audit_log).
--
-- `relation_id` is deliberately NOT a foreign key to `relations.id` -
-- same reasoning as `fact_audit_log.fact_id`: a future `ForgetRelation`
-- must be able to hard-delete the Relation row while this table's own
-- (payload-free, once its `previous_snapshot` is scrubbed) structural
-- audit marker survives. `action_type`'s CHECK enumerates the full
-- frozen v0.1 Relation action family up front (`AddRelation`/
-- `CorrectRelation`/`ConfirmRelation`/`ForgetRelation`) even though only
-- `AddRelation` has a production write path after this migration - the
-- same reasoning Milestone/Task's audit tables used (both actions
-- frozen and built in the same migration), avoiding a future
-- `V6`-style CHECK-widening migration merely to add an action that was
-- already known when this table was created.
--
-- V1 (Goal) through V7 (Personal Knowledge foundation) are historical
-- production migrations and are not edited here.

CREATE TABLE relation_audit_log (
    id                  TEXT PRIMARY KEY,
    relation_id         TEXT NOT NULL,
    action_type         TEXT NOT NULL CHECK (action_type IN ('AddRelation', 'CorrectRelation', 'ConfirmRelation', 'ForgetRelation')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI', 'CloudLLM', 'NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit', 'Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);
