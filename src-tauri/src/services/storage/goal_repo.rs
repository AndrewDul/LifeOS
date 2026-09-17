//! Goal repository (M2 Goal vertical slice, 2026-09-17).
//!
//! `pub(super)` only - reachable exclusively from `services/mod.rs`,
//! same as every other function in `services::storage`. Converts
//! between `domain::Goal` and the `goals`/`goal_audit_log` tables
//! (`migrations/V1__create_goals.sql`). No validation happens here -
//! that is `actions`' job; this module trusts its caller (`services`)
//! to have already validated.

use crate::actions::{Actor, ConfirmationClass, Provenance};
use crate::domain::{Goal, GoalId, GoalStatus};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

fn parse_rfc3339(s: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
}

fn row_to_goal(row: &Row) -> rusqlite::Result<Goal> {
    let id_str: String = row.get("id")?;
    let status_str: String = row.get("status")?;
    let target_date_str: Option<String> = row.get("target_date")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;
    let progress_i64: i64 = row.get("progress")?;

    Ok(Goal {
        id: GoalId::parse(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        title: row.get("title")?,
        description: row.get("description")?,
        status: GoalStatus::parse(&status_str).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("unknown goal status: {status_str}").into(),
            )
        })?,
        target_date: target_date_str.map(|s| parse_rfc3339(&s)).transpose()?,
        priority_user: row.get("priority_user")?,
        priority_system: row.get("priority_system")?,
        progress: progress_i64 as u8,
        domain_tag: row.get("domain_tag")?,
        created_at: parse_rfc3339(&created_at_str)?,
        updated_at: parse_rfc3339(&updated_at_str)?,
    })
}

pub(in crate::services) fn insert_goal(conn: &Connection, goal: &Goal) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO goals (id, title, description, status, target_date, priority_user, priority_system, progress, domain_tag, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            goal.id.to_string(),
            goal.title,
            goal.description,
            goal.status.as_str(),
            goal.target_date.map(|d| d.to_rfc3339()),
            goal.priority_user,
            goal.priority_system,
            goal.progress as i64,
            goal.domain_tag,
            goal.created_at.to_rfc3339(),
            goal.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub(in crate::services) fn update_goal(conn: &Connection, goal: &Goal) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "UPDATE goals SET title = ?2, description = ?3, status = ?4, target_date = ?5,
             priority_user = ?6, priority_system = ?7, progress = ?8, domain_tag = ?9, updated_at = ?10
         WHERE id = ?1",
        params![
            goal.id.to_string(),
            goal.title,
            goal.description,
            goal.status.as_str(),
            goal.target_date.map(|d| d.to_rfc3339()),
            goal.priority_user,
            goal.priority_system,
            goal.progress as i64,
            goal.domain_tag,
            goal.updated_at.to_rfc3339(),
        ],
    )?;
    debug_assert_eq!(
        changed, 1,
        "update_goal should affect exactly one row - caller must check existence first"
    );
    Ok(())
}

pub(in crate::services) fn get_goal(
    conn: &Connection,
    id: GoalId,
) -> rusqlite::Result<Option<Goal>> {
    conn.query_row(
        "SELECT id, title, description, status, target_date, priority_user, priority_system, progress, domain_tag, created_at, updated_at
         FROM goals WHERE id = ?1",
        params![id.to_string()],
        row_to_goal,
    )
    .optional()
}

pub(in crate::services) fn get_active_goals(conn: &Connection) -> rusqlite::Result<Vec<Goal>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, description, status, target_date, priority_user, priority_system, progress, domain_tag, created_at, updated_at
         FROM goals WHERE status = 'ACTIVE' ORDER BY created_at",
    )?;
    let rows = stmt.query_map([], row_to_goal)?;
    rows.collect()
}

fn actor_str(actor: Actor) -> &'static str {
    match actor {
        Actor::Ui => "UI",
        Actor::CloudLlm => "CloudLLM",
        Actor::NeXa => "NeXa",
    }
}

fn provenance_str(provenance: Provenance) -> &'static str {
    match provenance {
        Provenance::Explicit => "Explicit",
        Provenance::Inferred => "Inferred",
    }
}

fn confirmation_class_str(class: ConfirmationClass) -> &'static str {
    match class {
        ConfirmationClass::SafeRead => "SAFE_READ",
        ConfirmationClass::ExplicitAdd => "EXPLICIT_ADD",
        ConfirmationClass::InferredChange => "INFERRED_CHANGE",
        ConfirmationClass::ImportantChange => "IMPORTANT_CHANGE",
        ConfirmationClass::Delete => "DELETE",
        ConfirmationClass::Sensitive => "SENSITIVE",
    }
}

/// Minimum state to support a future Undo (build brief Part C5) -
/// deliberately not a full event-sourcing system: one row per audited
/// mutation, with `previous_snapshot` (JSON, NULL for a fresh create)
/// as the only "how do I undo this" payload.
#[allow(clippy::too_many_arguments)]
pub(in crate::services) fn insert_audit_entry(
    conn: &Connection,
    goal_id: GoalId,
    action_type: &str,
    actor: Actor,
    provenance: Provenance,
    confirmation_class: ConfirmationClass,
    previous_snapshot_json: Option<&str>,
    occurred_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO goal_audit_log (id, goal_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Uuid::now_v7().to_string(),
            goal_id.to_string(),
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
pub(in crate::services) fn count_audit_entries_for_goal(
    conn: &Connection,
    goal_id: GoalId,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM goal_audit_log WHERE goal_id = ?1",
        params![goal_id.to_string()],
        |row| row.get(0),
    )
}
