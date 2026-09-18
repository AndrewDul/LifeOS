//! Personal Knowledge repository (M2 Personal Knowledge Slice A,
//! 2026-09-18).
//!
//! `pub(super)` only - reachable exclusively from `services/mod.rs`,
//! same as every other `services::storage` module. Converts between
//! `domain::{Entity, Source, Evidence, Fact}` and the
//! `entities`/`sources`/`evidence`/`facts` tables
//! (`migrations/V7__create_personal_knowledge.sql`). No validation
//! happens here - that is `actions`'/`services`' job; this module
//! trusts its caller to have already validated.
//!
//! Slice A (2026-09-18) added `Entity` insert/get (the SELF bootstrap
//! needs both) and `Fact`/`Source`/`Evidence` insert as test-fixture-only
//! helpers plus `Fact` get. Slice B (2026-09-18, `AddMemory`) promotes
//! `insert_fact`/`insert_source`/`insert_evidence`/
//! `insert_fact_evidence_link` to production (the first real Memory
//! action writes through them now) and adds `find_entity_by_domain_ref`/
//! `entity_exists`/`source_exists` (authoritative domain-ref and
//! Existing-reference validation) and `insert_fact_audit_log` (the first
//! production writer of `fact_audit_log`). `relations`/`episodes`/
//! `relation_evidence`/`episode_evidence`/`episode_entities` still have
//! no functions here at all - schema only (R019 §13); `AddMemory` must
//! never write to them, and tests that need to exercise their
//! constraints do so with deliberate raw SQL at the test boundary, never
//! through a production repo function (no `insert_relation`/
//! `insert_episode` exists).

use super::{actor_str, confirmation_class_str, parse_rfc3339, provenance_str};
use crate::actions::{Actor, ConfirmationClass, Provenance};
use crate::domain::{
    DomainRef, Entity, EntityId, Evidence, EvidenceId, Fact, FactId, FactValue, MemoryType,
    Sensitivity, Source, SourceId,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;

fn parse_sensitivity(s: &str) -> rusqlite::Result<Sensitivity> {
    Sensitivity::parse(s).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown sensitivity: {s}").into(),
        )
    })
}

fn row_to_entity(row: &Row) -> rusqlite::Result<Entity> {
    let id_str: String = row.get("id")?;
    let is_self_i64: i64 = row.get("is_self")?;
    let sensitivity_str: String = row.get("sensitivity")?;
    let domain_ref_type: Option<String> = row.get("domain_ref_type")?;
    let domain_ref_id: Option<String> = row.get("domain_ref_id")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;

    Ok(Entity {
        id: EntityId::parse(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        entity_type: row.get("entity_type")?,
        canonical_name: row.get("canonical_name")?,
        is_self: is_self_i64 != 0,
        sensitivity: parse_sensitivity(&sensitivity_str)?,
        domain_ref: match (domain_ref_type, domain_ref_id) {
            (Some(t), Some(i)) => Some(DomainRef {
                domain_ref_type: t,
                domain_ref_id: i,
            }),
            _ => None,
        },
        created_at: parse_rfc3339(&created_at_str)?,
        updated_at: parse_rfc3339(&updated_at_str)?,
    })
}

pub(in crate::services) fn insert_entity(
    conn: &Connection,
    entity: &Entity,
) -> rusqlite::Result<()> {
    let (domain_ref_type, domain_ref_id) = match &entity.domain_ref {
        Some(r) => (
            Some(r.domain_ref_type.as_str()),
            Some(r.domain_ref_id.as_str()),
        ),
        None => (None, None),
    };
    conn.execute(
        "INSERT INTO entities (id, entity_type, canonical_name, is_self, sensitivity, domain_ref_type, domain_ref_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            entity.id.to_string(),
            entity.entity_type,
            entity.canonical_name,
            entity.is_self as i64,
            entity.sensitivity.as_str(),
            domain_ref_type,
            domain_ref_id,
            entity.created_at.to_rfc3339(),
            entity.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub(in crate::services) fn get_entity(
    conn: &Connection,
    id: EntityId,
) -> rusqlite::Result<Option<Entity>> {
    conn.query_row(
        "SELECT id, entity_type, canonical_name, is_self, sensitivity, domain_ref_type, domain_ref_id, created_at, updated_at
         FROM entities WHERE id = ?1",
        params![id.to_string()],
        row_to_entity,
    )
    .optional()
}

/// The one `is_self = 1` row, if it exists - used by `services::
/// ensure_self_entity`. Never used to decide "which one to keep" when
/// more than one exists (that case is an integrity error the caller
/// detects via `count_self_entities`, not silently resolved here).
pub(in crate::services) fn get_self_entity(conn: &Connection) -> rusqlite::Result<Option<Entity>> {
    conn.query_row(
        "SELECT id, entity_type, canonical_name, is_self, sensitivity, domain_ref_type, domain_ref_id, created_at, updated_at
         FROM entities WHERE is_self = 1",
        [],
        row_to_entity,
    )
    .optional()
}

/// How many `is_self = 1` rows currently exist - `services::
/// ensure_self_entity`'s 0/1/>1 dispatch. The real DB partial unique
/// index (`idx_entities_single_self`) should make `>1` structurally
/// impossible, but this is queried and checked explicitly rather than
/// assumed (R019 §3's "detect anyway" requirement).
pub(in crate::services) fn count_self_entities(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM entities WHERE is_self = 1",
        [],
        |row| row.get(0),
    )
}

fn row_to_fact(row: &Row) -> rusqlite::Result<Fact> {
    let id_str: String = row.get("id")?;
    let subject_entity_id_str: String = row.get("subject_entity_id")?;
    let value_json: Option<String> = row.get("value_json")?;
    let object_entity_id_str: Option<String> = row.get("object_entity_id")?;
    let memory_type_str: String = row.get("memory_type")?;
    let epistemic_state_str: String = row.get("epistemic_state")?;
    let sensitivity_str: String = row.get("sensitivity")?;
    let valid_from_str: String = row.get("valid_from")?;
    let valid_until_str: Option<String> = row.get("valid_until")?;
    let superseded_by_fact_id_str: Option<String> = row.get("superseded_by_fact_id")?;
    let created_at_str: String = row.get("created_at")?;
    let updated_at_str: String = row.get("updated_at")?;

    let value = match (value_json, object_entity_id_str) {
        (Some(v), None) => FactValue::Scalar(v),
        (None, Some(e)) => FactValue::EntityRef(EntityId::parse(&e).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?),
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("fact row violates the one-semantic-object invariant: {other:?}").into(),
            ))
        }
    };

    Ok(Fact {
        id: FactId::parse(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        subject_entity_id: EntityId::parse(&subject_entity_id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        predicate: row.get("predicate")?,
        value,
        memory_type: MemoryType::parse(&memory_type_str).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("unknown memory_type: {memory_type_str}").into(),
            )
        })?,
        epistemic_state: crate::domain::EpistemicState::parse(&epistemic_state_str).ok_or_else(
            || {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    format!("unknown epistemic_state: {epistemic_state_str}").into(),
                )
            },
        )?,
        confidence: row.get("confidence")?,
        sensitivity: parse_sensitivity(&sensitivity_str)?,
        valid_from: parse_rfc3339(&valid_from_str)?,
        valid_until: valid_until_str.map(|s| parse_rfc3339(&s)).transpose()?,
        superseded_by_fact_id: superseded_by_fact_id_str
            .map(|s| {
                FactId::parse(&s).map_err(|e| {
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

pub(in crate::services) fn insert_fact(conn: &Connection, fact: &Fact) -> rusqlite::Result<()> {
    let (value_json, object_entity_id): (Option<&str>, Option<String>) = match &fact.value {
        FactValue::Scalar(v) => (Some(v.as_str()), None),
        FactValue::EntityRef(id) => (None, Some(id.to_string())),
    };

    conn.execute(
        "INSERT INTO facts (id, subject_entity_id, predicate, value_json, object_entity_id, memory_type, epistemic_state, confidence, sensitivity, valid_from, valid_until, superseded_by_fact_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            fact.id.to_string(),
            fact.subject_entity_id.to_string(),
            fact.predicate,
            value_json,
            object_entity_id,
            fact.memory_type.as_str(),
            fact.epistemic_state.as_str(),
            fact.confidence,
            fact.sensitivity.as_str(),
            fact.valid_from.to_rfc3339(),
            fact.valid_until.map(|d| d.to_rfc3339()),
            fact.superseded_by_fact_id.map(|id| id.to_string()),
            fact.created_at.to_rfc3339(),
            fact.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub(in crate::services) fn get_fact(
    conn: &Connection,
    id: FactId,
) -> rusqlite::Result<Option<Fact>> {
    conn.query_row(
        "SELECT id, subject_entity_id, predicate, value_json, object_entity_id, memory_type, epistemic_state, confidence, sensitivity, valid_from, valid_until, superseded_by_fact_id, created_at, updated_at
         FROM facts WHERE id = ?1",
        params![id.to_string()],
        row_to_fact,
    )
    .optional()
}

pub(in crate::services) fn insert_source(
    conn: &Connection,
    source: &Source,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sources (id, source_type, label, reference, sensitivity, captured_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            source.id.to_string(),
            source.source_type,
            source.label,
            source.reference,
            source.sensitivity.as_str(),
            source.captured_at.to_rfc3339(),
            source.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub(in crate::services) fn insert_evidence(
    conn: &Connection,
    evidence: &Evidence,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO evidence (id, source_id, content, locator, sensitivity, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            evidence.id.to_string(),
            evidence.source_id.to_string(),
            evidence.content,
            evidence.locator,
            evidence.sensitivity.as_str(),
            evidence.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
pub(in crate::services) fn fact_evidence_link_exists(
    conn: &Connection,
    fact_id: FactId,
    evidence_id: EvidenceId,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM fact_evidence WHERE fact_id = ?1 AND evidence_id = ?2",
        params![fact_id.to_string(), evidence_id.to_string()],
        |row| row.get::<_, i64>(0),
    )
    .map(|c| c > 0)
}

/// Links a Fact to Evidence via `fact_evidence`.
pub(in crate::services) fn insert_fact_evidence_link(
    conn: &Connection,
    fact_id: FactId,
    evidence_id: EvidenceId,
    created_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO fact_evidence (fact_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        params![
            fact_id.to_string(),
            evidence_id.to_string(),
            created_at.to_rfc3339()
        ],
    )?;
    Ok(())
}

/// Authoritative persisted-duplicate check for a `domain_ref` - the
/// second layer of `AddMemory`'s duplicate-domain-reference defense
/// (the first layer is the same-payload preflight in `actions::
/// validate_add_memory`; the real `idx_entities_domain_ref` UNIQUE
/// index remains defense-in-depth only, never the first detector).
pub(in crate::services) fn find_entity_by_domain_ref(
    conn: &Connection,
    domain_ref_type: &str,
    domain_ref_id: &str,
) -> rusqlite::Result<Option<Entity>> {
    conn.query_row(
        "SELECT id, entity_type, canonical_name, is_self, sensitivity, domain_ref_type, domain_ref_id, created_at, updated_at
         FROM entities WHERE domain_ref_type = ?1 AND domain_ref_id = ?2",
        params![domain_ref_type, domain_ref_id],
        row_to_entity,
    )
    .optional()
}

/// Lightweight existence check for `EntityInputRef::Existing` - matches
/// the `goal_exists`/`project_exists`/`milestone_exists`/`task_exists`
/// precedent (a dedicated `count(*)` check, not `get_entity(...).
/// is_some()`), since `AddMemory` only needs to know an Existing entity
/// is real, never its field values, to validate a reference.
pub(in crate::services) fn entity_exists(
    conn: &Connection,
    id: EntityId,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM entities WHERE id = ?1",
        params![id.to_string()],
        |row| row.get::<_, i64>(0),
    )
    .map(|c| c > 0)
}

/// Lightweight existence check for `SourceRef::Existing` - same
/// precedent as `entity_exists`.
pub(in crate::services) fn source_exists(
    conn: &Connection,
    id: SourceId,
) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT count(*) FROM sources WHERE id = ?1",
        params![id.to_string()],
        |row| row.get::<_, i64>(0),
    )
    .map(|c| c > 0)
}

/// First production writer of `fact_audit_log`. `previous_snapshot` is
/// always `None` for `AddMemory` (this creates a Fact, it does not
/// update one - `serialize_required_snapshot` is never called here,
/// matching every other Create* action's audit row, not the Update*
/// actions' mandatory-snapshot convention).
#[allow(clippy::too_many_arguments)]
pub(in crate::services) fn insert_fact_audit_log(
    conn: &Connection,
    fact_id: FactId,
    action_type: &str,
    actor: Actor,
    provenance: Provenance,
    confirmation_class: ConfirmationClass,
    previous_snapshot_json: Option<String>,
    occurred_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO fact_audit_log (id, fact_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Uuid::now_v7().to_string(),
            fact_id.to_string(),
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

/// `CorrectMemory`'s ONLY predecessor-mutation path (M2 Personal
/// Knowledge Slice C, 2026-09-18) - deliberately narrow rather than a
/// generic `update_fact`: its SQL touches exactly the four fields the
/// frozen `Fact` mutability contract allows (`domain::Fact`'s own doc
/// comment) - `valid_until`, `superseded_by_fact_id`,
/// `epistemic_state = 'STALE'` (a literal, not a parameter - this
/// function has exactly one purpose and it is always this transition),
/// and `updated_at`. `subject_entity_id`/`predicate`/`value`/
/// `memory_type`/`confidence`/`sensitivity`/`valid_from`/`created_at`
/// are structurally impossible to touch through this function's
/// parameter list - the storage API makes the frozen contract hard to
/// violate rather than merely documenting it.
pub(in crate::services) fn update_fact_as_superseded(
    conn: &Connection,
    predecessor_fact_id: FactId,
    successor_fact_id: FactId,
    correction_timestamp: DateTime<Utc>,
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "UPDATE facts SET valid_until = ?2, superseded_by_fact_id = ?3, epistemic_state = 'STALE', updated_at = ?4 WHERE id = ?1",
        params![
            predecessor_fact_id.to_string(),
            correction_timestamp.to_rfc3339(),
            successor_fact_id.to_string(),
            correction_timestamp.to_rfc3339(),
        ],
    )?;
    debug_assert_eq!(
        changed, 1,
        "update_fact_as_superseded should affect exactly one row - caller must check predecessor existence first"
    );
    Ok(())
}
