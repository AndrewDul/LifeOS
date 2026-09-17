//! Task repository (M2 Task vertical slice, 2026-09-17).
//!
//! `pub(in crate::services)` - same visibility discipline as
//! `goal_repo`/`project_repo`/`goal_relation_repo`/`milestone_repo`: a
//! per-entity repo module is a *grandchild* of `services`, not an
//! immediate child, so `pub(super)` on its own functions would not reach
//! `services/mod.rs` two levels up (see `docs/LIFEOS_SPEC.md` §5.6).
//! Converts between `domain::Task` and the `tasks`/`task_audit_log`
//! tables (`migrations/V5__create_tasks.sql`). No validation happens
//! here - that is `actions`'/`services`' job; this module trusts its
//! caller to have already validated.

use super::{actor_str, confirmation_class_str, parse_rfc3339, provenance_str};
use crate::actions::{Actor, ConfirmationClass, Provenance};
use crate::domain::{GoalId, ProjectId, RecurringRuleId, Task, TaskId, TaskStatus};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

fn row_to_task(row: &Row) -> rusqlite::Result<Task> {
    let id_str: String = row.get("id")?;
    let status_str: String = row.get("status")?;
    let due_at_str: Option<String> = row.get("due_at")?;
    let project_id_str: Option<String> = row.get("project_id")?;
    let goal_id_str: Option<String> = row.get("goal_id")?;
    let recurring_rule_id_str: Option<String> = row.get("recurring_rule_id")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;

    Ok(Task {
        id: TaskId::parse(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        title: row.get("title")?,
        status: TaskStatus::parse(&status_str).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("unknown task status: {status_str}").into(),
            )
        })?,
        due_at: due_at_str.map(|s| parse_rfc3339(&s)).transpose()?,
        project_id: project_id_str
            .map(|s| {
                ProjectId::parse(&s).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
            })
            .transpose()?,
        goal_id: goal_id_str
            .map(|s| {
                GoalId::parse(&s).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
            })
            .transpose()?,
        // Always None in practice - `recurring_rule_id IS NULL` is
        // enforced by a database CHECK constraint this run (no real
        // RecurringRule table exists yet) - parsed generically anyway,
        // the same way every other nullable FK column here is, so this
        // code does not need to change the day that CHECK is lifted.
        recurring_rule_id: recurring_rule_id_str
            .map(|s| {
                RecurringRuleId::parse(&s).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })
            })
            .transpose()?,
        created_at: parse_rfc3339(&created_at_str)?,
        updated_at: parse_rfc3339(&updated_at_str)?,
    })
}

pub(in crate::services) fn insert_task(conn: &Connection, task: &Task) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO tasks (id, title, status, due_at, project_id, goal_id, recurring_rule_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            task.id.to_string(),
            task.title,
            task.status.as_str(),
            task.due_at.map(|d| d.to_rfc3339()),
            task.project_id.map(|p| p.to_string()),
            task.goal_id.map(|g| g.to_string()),
            task.recurring_rule_id.map(|r| r.to_string()),
            task.created_at.to_rfc3339(),
            task.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// Never touches `recurring_rule_id` or `created_at` - `UpdateTask`
/// does not expose the former (Task vertical-slice report §2C) and the
/// latter never changes after creation.
pub(in crate::services) fn update_task(conn: &Connection, task: &Task) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "UPDATE tasks SET title = ?2, status = ?3, due_at = ?4, project_id = ?5, goal_id = ?6, updated_at = ?7
         WHERE id = ?1",
        params![
            task.id.to_string(),
            task.title,
            task.status.as_str(),
            task.due_at.map(|d| d.to_rfc3339()),
            task.project_id.map(|p| p.to_string()),
            task.goal_id.map(|g| g.to_string()),
            task.updated_at.to_rfc3339(),
        ],
    )?;
    debug_assert_eq!(
        changed, 1,
        "update_task should affect exactly one row - caller must check existence first"
    );
    Ok(())
}

pub(in crate::services) fn get_task(
    conn: &Connection,
    id: TaskId,
) -> rusqlite::Result<Option<Task>> {
    conn.query_row(
        "SELECT id, title, status, due_at, project_id, goal_id, recurring_rule_id, created_at, updated_at
         FROM tasks WHERE id = ?1",
        params![id.to_string()],
        row_to_task,
    )
    .optional()
}

/// Deterministic technical ordering, NOT product prioritization (Task
/// vertical-slice report §13): `due_at ASC` (SQLite's default ASC
/// ordering sorts `NULL` due dates before any real timestamp - i.e.
/// undated tasks sort first), then `created_at ASC`, then `id ASC` as a
/// final stable tie-breaker for rows that still compare equal (e.g. two
/// tasks with the same `NULL` due_at and the same `created_at`
/// instant). No user-facing "priority" or "urgency" algorithm is
/// implemented here.
pub(in crate::services) fn get_tasks_for_project(
    conn: &Connection,
    project_id: ProjectId,
) -> rusqlite::Result<Vec<Task>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, status, due_at, project_id, goal_id, recurring_rule_id, created_at, updated_at
         FROM tasks WHERE project_id = ?1
         ORDER BY due_at ASC, created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![project_id.to_string()], row_to_task)?;
    rows.collect()
}

/// Same ordering/documentation as `get_tasks_for_project`, above.
pub(in crate::services) fn get_tasks_for_goal(
    conn: &Connection,
    goal_id: GoalId,
) -> rusqlite::Result<Vec<Task>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, status, due_at, project_id, goal_id, recurring_rule_id, created_at, updated_at
         FROM tasks WHERE goal_id = ?1
         ORDER BY due_at ASC, created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![goal_id.to_string()], row_to_task)?;
    rows.collect()
}

/// Same minimum audit shape as every other repo's `insert_audit_entry` -
/// one row per audited mutation, `previous_snapshot` `None` for
/// `AddTask`, populated for `UpdateTask`.
#[allow(clippy::too_many_arguments)]
pub(in crate::services) fn insert_audit_entry(
    conn: &Connection,
    task_id: TaskId,
    action_type: &str,
    actor: Actor,
    provenance: Provenance,
    confirmation_class: ConfirmationClass,
    previous_snapshot_json: Option<&str>,
    occurred_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO task_audit_log (id, task_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Uuid::now_v7().to_string(),
            task_id.to_string(),
            action_type,
            actor_str(actor),
            provenance_str(provenance),
            confirmation_class_str(confirmation_class),
            previous_snapshot_json,
            occurred_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
pub(in crate::services) fn count_audit_entries_for_task(
    conn: &Connection,
    task_id: TaskId,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM task_audit_log WHERE task_id = ?1",
        params![task_id.to_string()],
        |row| row.get(0),
    )
}
