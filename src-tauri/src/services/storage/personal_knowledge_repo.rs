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
//! This slice implements only what Slice A actually needs: `Entity`
//! insert/get (the SELF bootstrap needs both), `Fact` insert (test
//! fixture only, `#[cfg(test)]`)/get, and `Source`/`Evidence` insert
//! (test fixtures only, `#[cfg(test)]`, needed to seed `Fact`/`Evidence`
//! round-trip and defense-in-depth tests without an `AddMemory` action,
//! which does not exist yet). `relations`/`episodes`/
//! `relation_evidence`/`episode_evidence`/`episode_entities` have no
//! functions here at all this slice - schema only (R019 §13); tests
//! that need to exercise their constraints do so with deliberate raw
//! SQL at the test boundary, never through a production repo function
//! (no `insert_relation`/`insert_episode` exists).

use super::parse_rfc3339;
use crate::domain::{
    DomainRef, Entity, EntityId, Fact, FactId, FactValue, MemoryType, Sensitivity,
};
#[cfg(test)]
use crate::domain::{Evidence, EvidenceId, Source};
#[cfg(test)]
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

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

/// Test-fixture only - no `AddMemory`/`CorrectMemory` action exists
/// yet to produce a real `Fact` row (Slice B+). Used solely to seed
/// `get_fact` round-trip proofs and defense-in-depth tests.
#[cfg(test)]
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

/// Test-fixture only - no `AddMemory` action exists yet to create a
/// `Source` row for real (Slice B+).
#[cfg(test)]
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

/// Test-fixture only - no `AddMemory` action exists yet to create an
/// `Evidence` row for real (Slice B+).
#[cfg(test)]
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

/// Test-fixture only - links a Fact to Evidence via `fact_evidence`.
/// No `AddMemory` action exists yet to create this link for real.
#[cfg(test)]
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
