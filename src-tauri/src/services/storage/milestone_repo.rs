//! Milestone repository (M2 Milestone vertical slice, 2026-09-17).
//!
//! `pub(in crate::services)` - same visibility discipline as
//! `goal_repo`/`project_repo`/`goal_relation_repo`: a per-entity repo
//! module is a *grandchild* of `services`, not an immediate child, so
//! `pub(super)` on its own functions would not reach `services/mod.rs`
//! two levels up (see `docs/LIFEOS_SPEC.md` §5.6). Converts between
//! `domain::Milestone` and the `milestones`/`milestone_audit_log` tables
//! (`migrations/V4__create_milestones.sql`). No validation happens here -
//! that is `actions`'/`services`' job; this module trusts its caller to
//! have already validated.

use super::{actor_str, confirmation_class_str, parse_rfc3339, provenance_str};
use crate::actions::{Actor, ConfirmationClass, Provenance};
use crate::domain::{Milestone, MilestoneId, MilestoneStatus, OrderIndex, ProjectId};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

fn row_to_milestone(row: &Row) -> rusqlite::Result<Milestone> {
    let id_str: String = row.get("id")?;
    let project_id_str: String = row.get("project_id")?;
    let target_date_str: Option<String> = row.get("target_date")?;
    let status_str: String = row.get("status")?;

    Ok(Milestone {
        id: MilestoneId::parse(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        project_id: ProjectId::parse(&project_id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        title: row.get("title")?,
        target_date: target_date_str.map(|s| parse_rfc3339(&s)).transpose()?,
        status: MilestoneStatus::parse(&status_str).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("unknown milestone status: {status_str}").into(),
            )
        })?,
        order_index: row.get("order_index")?,
    })
}

pub(in crate::services) fn insert_milestone(
    conn: &Connection,
    milestone: &Milestone,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO milestones (id, project_id, title, target_date, status, order_index)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            milestone.id.to_string(),
            milestone.project_id.to_string(),
            milestone.title,
            milestone.target_date.map(|d| d.to_rfc3339()),
            milestone.status.as_str(),
            milestone.order_index,
        ],
    )?;
    Ok(())
}

pub(in crate::services) fn update_milestone(
    conn: &Connection,
    milestone: &Milestone,
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "UPDATE milestones SET title = ?2, target_date = ?3, status = ?4, order_index = ?5
         WHERE id = ?1",
        params![
            milestone.id.to_string(),
            milestone.title,
            milestone.target_date.map(|d| d.to_rfc3339()),
            milestone.status.as_str(),
            milestone.order_index,
        ],
    )?;
    debug_assert_eq!(
        changed, 1,
        "update_milestone should affect exactly one row - caller must check existence first"
    );
    Ok(())
}

pub(in crate::services) fn get_milestone(
    conn: &Connection,
    id: MilestoneId,
) -> rusqlite::Result<Option<Milestone>> {
    conn.query_row(
        "SELECT id, project_id, title, target_date, status, order_index
         FROM milestones WHERE id = ?1",
        params![id.to_string()],
        row_to_milestone,
    )
    .optional()
}

/// Deterministic read order (`ORDER_INDEX ASC`) - unambiguous because
/// `(project_id, order_index)` is UNIQUE (`migrations/V4__create_milestones.sql`),
/// so no two rows returned here ever share a position.
pub(in crate::services) fn get_milestones_for_project(
    conn: &Connection,
    project_id: ProjectId,
) -> rusqlite::Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, title, target_date, status, order_index
         FROM milestones WHERE project_id = ?1
         ORDER BY order_index ASC",
    )?;
    let rows = stmt.query_map(params![project_id.to_string()], row_to_milestone)?;
    rows.collect()
}

/// Used by both `services::add_milestone` (no `exclude_id` - any
/// existing row at the slot is a conflict) and `services::update_milestone`
/// (`exclude_id = Some(self)` - the Milestone being updated keeping its
/// own current `order_index` must NOT be reported as a conflict with
/// itself; the M2 Milestone vertical-slice report §9 - moving to a
/// DIFFERENT Milestone's slot within the same Project is still a
/// conflict). Scoped to `project_id` - a different Project reusing the
/// same `order_index` is never a conflict (the resolved uniqueness rule
/// is `(project_id, order_index)` together, not `order_index` alone).
pub(in crate::services) fn order_index_taken(
    conn: &Connection,
    project_id: ProjectId,
    order_index: OrderIndex,
    exclude_id: Option<MilestoneId>,
) -> rusqlite::Result<bool> {
    match exclude_id {
        Some(id) => conn.query_row(
            "SELECT count(*) FROM milestones WHERE project_id = ?1 AND order_index = ?2 AND id != ?3",
            params![project_id.to_string(), order_index, id.to_string()],
            |row| row.get::<_, i64>(0),
        ),
        None => conn.query_row(
            "SELECT count(*) FROM milestones WHERE project_id = ?1 AND order_index = ?2",
            params![project_id.to_string(), order_index],
            |row| row.get::<_, i64>(0),
        ),
    }
    .map(|c| c > 0)
}

/// Same minimum audit shape as `goal_repo`/`project_repo`/
/// `goal_relation_repo`'s `insert_audit_entry` - one row per audited
/// mutation, `previous_snapshot` `None` for `AddMilestone`, populated for
/// `UpdateMilestone`.
#[allow(clippy::too_many_arguments)]
pub(in crate::services) fn insert_audit_entry(
    conn: &Connection,
    milestone_id: MilestoneId,
    action_type: &str,
    actor: Actor,
    provenance: Provenance,
    confirmation_class: ConfirmationClass,
    previous_snapshot_json: Option<&str>,
    occurred_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO milestone_audit_log (id, milestone_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Uuid::now_v7().to_string(),
            milestone_id.to_string(),
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

pub(in crate::services) fn milestone_exists(
    conn: &Connection,
    id: MilestoneId,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM milestones WHERE id = ?1",
        params![id.to_string()],
        |row| row.get::<_, i64>(0),
    )
    .map(|c| c > 0)
}

#[cfg(test)]
pub(in crate::services) fn count_audit_entries_for_milestone(
    conn: &Connection,
    milestone_id: MilestoneId,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM milestone_audit_log WHERE milestone_id = ?1",
        params![milestone_id.to_string()],
        |row| row.get(0),
    )
}
