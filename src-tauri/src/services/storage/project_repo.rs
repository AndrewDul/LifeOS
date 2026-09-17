//! Project repository (M2 Project vertical slice, 2026-09-17).
//!
//! `pub(in crate::services)` - same visibility discipline as
//! `goal_repo`: a per-entity repo module is a *grandchild* of
//! `services`, not an immediate child, so `pub(super)` on its own
//! functions would not reach `services/mod.rs` two levels up (see
//! `docs/LIFEOS_SPEC.md` §5.6). Converts between `domain::Project` and
//! the `projects`/`project_audit_log` tables
//! (`migrations/V2__create_projects.sql`). No validation happens here -
//! that is `actions`' job; this module trusts its caller (`services`)
//! to have already validated.

use super::{actor_str, confirmation_class_str, parse_rfc3339, provenance_str};
use crate::actions::{Actor, ConfirmationClass, Provenance};
use crate::domain::{GoalId, LifecycleStatus, Progress, Project, ProjectId};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

fn row_to_project(row: &Row) -> rusqlite::Result<Project> {
    let id_str: String = row.get("id")?;
    let status_str: String = row.get("status")?;
    let goal_id_str: Option<String> = row.get("goal_id")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;
    let progress_i64: i64 = row.get("progress")?;

    Ok(Project {
        id: ProjectId::parse(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        title: row.get("title")?,
        description: row.get("description")?,
        status: LifecycleStatus::parse(&status_str).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("unknown project status: {status_str}").into(),
            )
        })?,
        priority: row.get("priority")?,
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
        progress: progress_i64 as u8,
        created_at: parse_rfc3339(&created_at_str)?,
        updated_at: parse_rfc3339(&updated_at_str)?,
    })
}

pub(in crate::services) fn insert_project(
    conn: &Connection,
    project: &Project,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO projects (id, title, description, status, priority, goal_id, progress, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            project.id.to_string(),
            project.title,
            project.description,
            project.status.as_str(),
            project.priority,
            project.goal_id.map(|g| g.to_string()),
            project.progress as i64,
            project.created_at.to_rfc3339(),
            project.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// The frozen general-purpose `UpdateProject` action (M2 UpdateProject
/// implementation, 2026-09-17) - distinct from `update_project_progress`
/// below: mutates `title`/`description`/`status`/`priority`/`goal_id`/
/// `updated_at`. Deliberately never touches `progress` (that stays
/// `UpdateProgress`'s exclusive field, §2 of the UpdateProject brief) or
/// `created_at`/`id` (neither ever changes after creation).
pub(in crate::services) fn update_project(
    conn: &Connection,
    project: &Project,
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "UPDATE projects SET title = ?2, description = ?3, status = ?4, priority = ?5, goal_id = ?6, updated_at = ?7
         WHERE id = ?1",
        params![
            project.id.to_string(),
            project.title,
            project.description,
            project.status.as_str(),
            project.priority,
            project.goal_id.map(|g| g.to_string()),
            project.updated_at.to_rfc3339(),
        ],
    )?;
    debug_assert_eq!(
        changed, 1,
        "update_project should affect exactly one row - caller must check existence first"
    );
    Ok(())
}

/// `UpdateProgress` is deliberately narrow - it only ever changes
/// `progress`/`updated_at`, never any other Project field - kept as its
/// own function alongside the general-purpose `update_project` above
/// rather than merged into it, preserving the frozen ownership split
/// (§2/§21 of the UpdateProject brief).
pub(in crate::services) fn update_project_progress(
    conn: &Connection,
    id: ProjectId,
    progress: Progress,
    updated_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "UPDATE projects SET progress = ?2, updated_at = ?3 WHERE id = ?1",
        params![id.to_string(), progress as i64, updated_at.to_rfc3339()],
    )?;
    debug_assert_eq!(
        changed, 1,
        "update_project_progress should affect exactly one row - caller must check existence first"
    );
    Ok(())
}

pub(in crate::services) fn get_project(
    conn: &Connection,
    id: ProjectId,
) -> rusqlite::Result<Option<Project>> {
    conn.query_row(
        "SELECT id, title, description, status, priority, goal_id, progress, created_at, updated_at
         FROM projects WHERE id = ?1",
        params![id.to_string()],
        row_to_project,
    )
    .optional()
}

/// Used by `services::add_milestone` to validate a supplied `project_id`
/// references a real `Project` *before* persistence - same reasoning as
/// `goal_repo::goal_exists`: a friendly, typed check kept as the first
/// line of defense alongside the real, enforced `milestones.project_id`
/// FK constraint (M2 Milestone vertical slice, 2026-09-17).
pub(in crate::services) fn project_exists(
    conn: &Connection,
    id: ProjectId,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM projects WHERE id = ?1",
        params![id.to_string()],
        |row| row.get::<_, i64>(0),
    )
    .map(|c| c > 0)
}

/// Same minimum audit shape as `goal_repo::insert_audit_entry` - one row
/// per audited mutation, `previous_snapshot` NULL for a fresh create.
#[allow(clippy::too_many_arguments)]
pub(in crate::services) fn insert_audit_entry(
    conn: &Connection,
    project_id: ProjectId,
    action_type: &str,
    actor: Actor,
    provenance: Provenance,
    confirmation_class: ConfirmationClass,
    previous_snapshot_json: Option<&str>,
    occurred_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO project_audit_log (id, project_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Uuid::now_v7().to_string(),
            project_id.to_string(),
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
pub(in crate::services) fn count_audit_entries_for_project(
    conn: &Connection,
    project_id: ProjectId,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM project_audit_log WHERE project_id = ?1",
        params![project_id.to_string()],
        |row| row.get(0),
    )
}
