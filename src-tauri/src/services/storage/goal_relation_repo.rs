//! GoalRelation repository (M2 GoalRelation vertical slice, 2026-09-17).
//!
//! `pub(in crate::services)` - same visibility discipline as
//! `goal_repo`/`project_repo`: a per-entity repo module is a
//! *grandchild* of `services`, not an immediate child, so `pub(super)`
//! on its own functions would not reach `services/mod.rs` two levels up
//! (see `docs/LIFEOS_SPEC.md` §5.6). Converts between
//! `domain::GoalRelation` and the `goal_relations`/
//! `goal_relation_audit_log` tables
//! (`migrations/V3__create_goal_relations.sql`). No validation happens
//! here - that is `actions`'/`services`' job; this module trusts its
//! caller to have already validated.

use super::{actor_str, confirmation_class_str, parse_rfc3339, provenance_str};
use crate::actions::{Actor, ConfirmationClass, Provenance};
use crate::domain::{GoalId, GoalRelation, GoalRelationId, GoalRelationType};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

fn row_to_goal_relation(row: &Row) -> rusqlite::Result<GoalRelation> {
    let id_str: String = row.get("id")?;
    let from_str: String = row.get("from_goal_id")?;
    let to_str: String = row.get("to_goal_id")?;
    let relation_type_str: String = row.get("relation_type")?;
    let created_at_str: String = row.get("created_at")?;

    Ok(GoalRelation {
        id: GoalRelationId::parse(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        from_goal_id: GoalId::parse(&from_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        to_goal_id: GoalId::parse(&to_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        relation_type: GoalRelationType::parse(&relation_type_str).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("unknown goal relation type: {relation_type_str}").into(),
            )
        })?,
        note: row.get("note")?,
        created_at: parse_rfc3339(&created_at_str)?,
    })
}

pub(in crate::services) fn insert_goal_relation(
    conn: &Connection,
    relation: &GoalRelation,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO goal_relations (id, from_goal_id, to_goal_id, relation_type, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            relation.id.to_string(),
            relation.from_goal_id.to_string(),
            relation.to_goal_id.to_string(),
            relation.relation_type.as_str(),
            relation.note,
            relation.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub(in crate::services) fn get_goal_relation(
    conn: &Connection,
    id: GoalRelationId,
) -> rusqlite::Result<Option<GoalRelation>> {
    conn.query_row(
        "SELECT id, from_goal_id, to_goal_id, relation_type, note, created_at
         FROM goal_relations WHERE id = ?1",
        params![id.to_string()],
        row_to_goal_relation,
    )
    .optional()
}

/// Relations "involving" `goal_id` on either side - queries both
/// `from_goal_id` and `to_goal_id`, per the frozen direction convention
/// (no inverse rows are ever fabricated; this returns exactly the rows
/// actually stored, in whichever direction they were written).
pub(in crate::services) fn get_goal_relations_for_goal(
    conn: &Connection,
    goal_id: GoalId,
) -> rusqlite::Result<Vec<GoalRelation>> {
    let mut stmt = conn.prepare(
        "SELECT id, from_goal_id, to_goal_id, relation_type, note, created_at
         FROM goal_relations WHERE from_goal_id = ?1 OR to_goal_id = ?1
         ORDER BY created_at",
    )?;
    let rows = stmt.query_map(params![goal_id.to_string()], row_to_goal_relation)?;
    rows.collect()
}

/// Exact-duplicate pre-check used by `services::link_goals` - the same
/// `(from_goal_id, to_goal_id, relation_type)` triple must not already
/// exist. A different `relation_type` for the same pair, or the same
/// `relation_type` in the reverse direction, is a distinct row and must
/// NOT be reported as a duplicate here.
pub(in crate::services) fn relation_exists(
    conn: &Connection,
    from_goal_id: GoalId,
    to_goal_id: GoalId,
    relation_type: GoalRelationType,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM goal_relations
         WHERE from_goal_id = ?1 AND to_goal_id = ?2 AND relation_type = ?3",
        params![
            from_goal_id.to_string(),
            to_goal_id.to_string(),
            relation_type.as_str()
        ],
        |row| row.get::<_, i64>(0),
    )
    .map(|c| c > 0)
}

/// Same minimum audit shape as `goal_repo`/`project_repo`'s
/// `insert_audit_entry` - one row per audited mutation.
/// `previous_snapshot` is always `None` today: `LinkGoals` is the only
/// action on this entity, and it is create-only (no
/// `UpdateGoalRelation`/`DeleteGoalRelation` action exists), so there is
/// never a "previous state" to snapshot - the parameter is kept for
/// shape-consistency with the other two audit tables/functions.
#[allow(clippy::too_many_arguments)]
pub(in crate::services) fn insert_audit_entry(
    conn: &Connection,
    goal_relation_id: GoalRelationId,
    action_type: &str,
    actor: Actor,
    provenance: Provenance,
    confirmation_class: ConfirmationClass,
    previous_snapshot_json: Option<&str>,
    occurred_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO goal_relation_audit_log (id, goal_relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Uuid::now_v7().to_string(),
            goal_relation_id.to_string(),
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
pub(in crate::services) fn count_audit_entries_for_goal_relation(
    conn: &Connection,
    goal_relation_id: GoalRelationId,
) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM goal_relation_audit_log WHERE goal_relation_id = ?1",
        params![goal_relation_id.to_string()],
        |row| row.get(0),
    )
}
