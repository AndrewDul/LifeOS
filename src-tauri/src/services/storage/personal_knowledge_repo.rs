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
    Relation, RelationId, Sensitivity, Source, SourceId,
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
///
/// **Post-run temporal-integrity hardening (2026-09-18)**: the `WHERE`
/// clause itself now re-checks the exact `domain::is_active_fact`
/// predicate (`valid_until IS NULL AND epistemic_state <> 'STALE'`) at
/// mutation time, as defense-in-depth alongside `services::
/// correct_memory`'s own preflight check - this row can never be
/// superseded twice, and a row that became inactive between the
/// preflight and this statement (or a nonexistent id) cannot silently
/// succeed. `changed != 1` (0 rows: missing id, already-inactive
/// predecessor; >1 is structurally impossible against a primary key) is
/// now a REAL runtime error, not a `debug_assert!` that compiles out of
/// release builds - the caller's transaction rolls back the successor
/// Fact/Entity/Source/Evidence/link/audit exactly as any other failure
/// in this action does. The diagnostic is fixed and carries no Fact
/// content.
pub(in crate::services) fn update_fact_as_superseded(
    conn: &Connection,
    predecessor_fact_id: FactId,
    successor_fact_id: FactId,
    correction_timestamp: DateTime<Utc>,
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "UPDATE facts SET valid_until = ?2, superseded_by_fact_id = ?3, epistemic_state = 'STALE', updated_at = ?4
         WHERE id = ?1 AND valid_until IS NULL AND epistemic_state <> 'STALE'",
        params![
            predecessor_fact_id.to_string(),
            correction_timestamp.to_rfc3339(),
            successor_fact_id.to_string(),
            correction_timestamp.to_rfc3339(),
        ],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
            Some(
                "CorrectMemory predecessor update did not affect exactly one row - predecessor missing or no longer active"
                    .to_string(),
            ),
        ));
    }
    Ok(())
}

/// `ConfirmMemory`'s ONLY mutation path (M2 Personal Knowledge Slice D,
/// 2026-09-18) - the same narrow-storage-function discipline
/// `update_fact_as_superseded` established: its SQL touches exactly
/// `epistemic_state` (a literal `'USER_CONFIRMED'`, not a parameter -
/// this function has exactly one purpose and it is always this
/// transition) and `updated_at`. `subject_entity_id`/`predicate`/
/// `value`/`memory_type`/`confidence`/`sensitivity`/`valid_from`/
/// `valid_until`/`superseded_by_fact_id`/`created_at` are structurally
/// impossible to touch through this function's parameter list.
///
/// The `WHERE` clause re-checks, at mutation time, both preconditions
/// `services::confirm_memory` already checked at preflight - active
/// (`valid_until IS NULL AND epistemic_state <> 'STALE'`) and not
/// already confirmed (`epistemic_state <> 'USER_CONFIRMED'`) - as
/// defense-in-depth, matching `update_fact_as_superseded`'s exact
/// precedent. `changed != 1` (0 rows: missing id, inactive, or already
/// confirmed; >1 structurally impossible against a primary key) is a
/// real runtime error in every build profile, never a `debug_assert!`.
/// The diagnostic is fixed and carries no Fact content.
pub(in crate::services) fn confirm_fact_epistemic_state(
    conn: &Connection,
    fact_id: FactId,
    confirmed_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "UPDATE facts SET epistemic_state = 'USER_CONFIRMED', updated_at = ?2
         WHERE id = ?1 AND valid_until IS NULL AND epistemic_state <> 'STALE' AND epistemic_state <> 'USER_CONFIRMED'",
        params![fact_id.to_string(), confirmed_at.to_rfc3339()],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
            Some(
                "ConfirmMemory update did not affect exactly one row - Fact missing, no longer active, or already confirmed"
                    .to_string(),
            ),
        ));
    }
    Ok(())
}

// --- ForgetMemory (M2 Personal Knowledge Slice E, 2026-09-18) ----------
//
// R019's final 14-step privacy-safe hard-deletion algorithm -
// `services::forget_memory` orchestrates these narrow, single-purpose
// functions in the exact frozen order. No generic `delete_entity`/
// `delete_evidence_link`/`update_fact` exists - each function here does
// exactly one thing and nothing else, matching `update_fact_as_
// superseded`/`confirm_fact_epistemic_state`'s established discipline.

/// Step 5 - scrubs every `fact_audit_log.previous_snapshot` for the
/// target Fact, covering payloads left by `CorrectMemory`/
/// `ConfirmMemory` (or any future snapshot-bearing action). Touches
/// ONLY `previous_snapshot`; every other audit column (`id`/`fact_id`/
/// `action_type`/`actor`/`provenance`/`confirmation_class`/
/// `occurred_at`) survives untouched - old audit rows become
/// payload-free structural history, never deleted outright. May affect
/// 0..N rows (a Fact with no snapshot-bearing prior audit row is a
/// normal, not an error, case).
pub(in crate::services) fn scrub_fact_audit_snapshots(
    conn: &Connection,
    fact_id: FactId,
) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE fact_audit_log SET previous_snapshot = NULL WHERE fact_id = ?1 AND previous_snapshot IS NOT NULL",
        params![fact_id.to_string()],
    )
}

/// Step 6 - clears every INBOUND `superseded_by_fact_id` reference
/// pointing at the target Fact, WITHOUT rewiring the chain (R019 - a
/// predecessor's own successor pointer becomes `NULL`, never
/// reassigned to some other Fact further down the chain; that would
/// falsify history). Touches ONLY `superseded_by_fact_id`/`updated_at`
/// - `valid_until`/`epistemic_state`/every semantic field on the
/// affected predecessor(s) survive untouched. May affect 0..N rows
/// (unlike `update_fact_as_superseded`'s exactly-one guarantee) - a
/// standalone or terminal Fact being forgotten has none.
pub(in crate::services) fn clear_inbound_supersession_refs(
    conn: &Connection,
    target_fact_id: FactId,
    timestamp: DateTime<Utc>,
) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE facts SET superseded_by_fact_id = NULL, updated_at = ?2 WHERE superseded_by_fact_id = ?1",
        params![target_fact_id.to_string(), timestamp.to_rfc3339()],
    )
}

/// Step 3 - every `EvidenceId` linked to the target Fact through
/// `fact_evidence`. Supports the genuine 0..N cases `V7` allows (no DB
/// `CHECK` enforces "exactly one Evidence per Fact") - a Fact with zero
/// linked Evidence rows returns an empty `Vec`, not an error.
pub(in crate::services) fn list_evidence_ids_for_fact(
    conn: &Connection,
    fact_id: FactId,
) -> rusqlite::Result<Vec<EvidenceId>> {
    let mut stmt = conn.prepare("SELECT evidence_id FROM fact_evidence WHERE fact_id = ?1")?;
    let mut rows = stmt.query(params![fact_id.to_string()])?;
    let mut ids = Vec::new();
    while let Some(row) = rows.next()? {
        let id_str: String = row.get(0)?;
        ids.push(EvidenceId::parse(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?);
    }
    Ok(ids)
}

/// Step 4 - the `SourceId` a single collected `Evidence` row points at
/// (`caller` deduplicates across every collected `EvidenceId` before
/// deciding which Sources are candidates for orphan cleanup, since
/// multiple Evidence rows may legitimately share one Source).
pub(in crate::services) fn get_source_id_for_evidence(
    conn: &Connection,
    evidence_id: EvidenceId,
) -> rusqlite::Result<Option<SourceId>> {
    let source_id_str: Option<String> = conn
        .query_row(
            "SELECT source_id FROM evidence WHERE id = ?1",
            params![evidence_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    source_id_str
        .map(|s| {
            SourceId::parse(&s).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
        })
        .transpose()
}

/// Steps 7 - deletes every join row referencing one collected
/// `EvidenceId`, across all three Evidence-referencing join tables
/// (`fact_evidence`/`relation_evidence`/`episode_evidence`) - even a
/// link belonging to a DIFFERENT, retained Fact/Relation/Episode.
/// Privacy of the forgotten Evidence outranks preservation of a shared
/// join row (R018/R019's Evidence privacy-first rule, §14 of the Slice
/// E brief) - the retained Fact/Relation/Episode OBJECT survives, it
/// merely loses that one piece of Evidence support.
pub(in crate::services) fn delete_evidence_links_for_evidence(
    conn: &Connection,
    evidence_id: EvidenceId,
) -> rusqlite::Result<()> {
    let id = evidence_id.to_string();
    conn.execute(
        "DELETE FROM fact_evidence WHERE evidence_id = ?1",
        params![id],
    )?;
    conn.execute(
        "DELETE FROM relation_evidence WHERE evidence_id = ?1",
        params![id],
    )?;
    conn.execute(
        "DELETE FROM episode_evidence WHERE evidence_id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Step 8 - hard-deletes one collected `Evidence` row. Always called,
/// even when that Evidence was also linked to another Fact/Relation/
/// Episode (its join rows were already removed by `delete_evidence_
/// links_for_evidence` first, so this delete is FK-safe against
/// `fact_evidence`/`relation_evidence`/`episode_evidence`'s `ON DELETE
/// RESTRICT`).
pub(in crate::services) fn delete_evidence(
    conn: &Connection,
    evidence_id: EvidenceId,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM evidence WHERE id = ?1",
        params![evidence_id.to_string()],
    )?;
    Ok(())
}

/// Step 9 - deletes a collected `Source` if and only if it is now
/// orphaned (zero surviving `Evidence` rows anywhere reference it) - a
/// single guarded, atomic statement rather than a separate
/// check-then-delete (no race, naturally idempotent if ever called
/// twice for the same id: the second call's `NOT EXISTS` still holds
/// and simply matches zero rows). A Source still referenced by a
/// RETAINED Evidence row (shared across Facts) is left byte-for-byte
/// unchanged - never deleted merely because ITS use by the forgotten
/// Fact is gone.
pub(in crate::services) fn delete_source_if_orphan(
    conn: &Connection,
    source_id: SourceId,
) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "DELETE FROM sources WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM evidence WHERE source_id = ?1)",
        params![source_id.to_string()],
    )?;
    Ok(changed > 0)
}

/// Step 10 - a defensive sweep deleting any remaining `fact_evidence`
/// row for the target Fact. Normally a no-op in production (step 7
/// already removed every link reachable through the Evidence ids
/// collected in step 3), kept anyway as R019's own explicit defense-in-
/// depth requirement, never removed merely because it is redundant in
/// the ordinary case.
pub(in crate::services) fn delete_fact_evidence_links_for_fact(
    conn: &Connection,
    fact_id: FactId,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM fact_evidence WHERE fact_id = ?1",
        params![fact_id.to_string()],
    )?;
    Ok(())
}

/// Step 11 - the ONLY hard-delete path for a `facts` row. `changed !=
/// 1` is a real runtime error in every build profile (matching
/// `update_fact_as_superseded`/`confirm_fact_epistemic_state`'s
/// established discipline, never a `debug_assert!`) - the target must
/// already have been validated to exist by `services::forget_memory`'s
/// own preflight `get_fact`, so any other outcome here means something
/// changed unexpectedly between preflight and this statement. The
/// diagnostic is fixed and carries no Fact content.
pub(in crate::services) fn delete_fact_forget_target(
    conn: &Connection,
    fact_id: FactId,
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "DELETE FROM facts WHERE id = ?1",
        params![fact_id.to_string()],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
            Some("ForgetMemory target Fact delete did not affect exactly one row".to_string()),
        ));
    }
    Ok(())
}

/// Step 12 - deletes ONE candidate Entity (the target Fact's subject,
/// or its object if the value was an `EntityRef`) if and only if it is
/// now orphaned - a single guarded, atomic statement enforcing the
/// exact frozen retention predicate (R019 §12 of the Slice E brief):
/// retained if ANY surviving row references it via
/// `facts.subject_entity_id`/`facts.object_entity_id`/
/// `relations.from_entity_id`/`relations.to_entity_id`/
/// `episode_entities.entity_id`; `domain_ref` alone never retains it.
/// `is_self = 0` is part of the guard itself - this function can NEVER
/// delete the SELF Entity, structurally, regardless of what
/// `entity_id` the caller passes. Must only ever be called AFTER the
/// target Fact itself has been deleted (step 11) - otherwise the
/// target Fact's own reference would always count as "surviving" and
/// nothing would ever be considered orphaned. Returns whether a row
/// was actually deleted.
pub(in crate::services) fn delete_entity_if_orphan_non_self(
    conn: &Connection,
    entity_id: EntityId,
) -> rusqlite::Result<bool> {
    let id = entity_id.to_string();
    let changed = conn.execute(
        "DELETE FROM entities WHERE id = ?1 AND is_self = 0
         AND NOT EXISTS (SELECT 1 FROM facts WHERE subject_entity_id = ?1)
         AND NOT EXISTS (SELECT 1 FROM facts WHERE object_entity_id = ?1)
         AND NOT EXISTS (SELECT 1 FROM relations WHERE from_entity_id = ?1)
         AND NOT EXISTS (SELECT 1 FROM relations WHERE to_entity_id = ?1)
         AND NOT EXISTS (SELECT 1 FROM episode_entities WHERE entity_id = ?1)",
        params![id],
    )?;
    Ok(changed > 0)
}

// --- Relation (M2 Personal Knowledge Relation Foundation, R026,
// 2026-09-18) -------------------------------------------------------------
//
// `AddRelation`'s production write path - the first production writer
// of `relations`/`relation_evidence`/`relation_audit_log` (all three
// previously schema-only, R019 §13/R025). Mirrors `insert_fact`/
// `get_fact`/`insert_fact_evidence_link`/`insert_fact_audit_log`'s exact
// shape and discipline. No `update_relation`/`delete_relation`/
// `confirm_relation`/`list_relations`/`search_relations` exist -
// `CorrectRelation`/`ConfirmRelation`/`ForgetRelation` and any read
// beyond `get_relation` are explicitly out of scope for R026 (R025 §21 -
// "not freezing broad retrieval/Context Pack here").

fn row_to_relation(row: &Row) -> rusqlite::Result<Relation> {
    let id_str: String = row.get("id")?;
    let from_entity_id_str: String = row.get("from_entity_id")?;
    let to_entity_id_str: String = row.get("to_entity_id")?;
    let epistemic_state_str: String = row.get("epistemic_state")?;
    let sensitivity_str: String = row.get("sensitivity")?;
    let valid_from_str: String = row.get("valid_from")?;
    let valid_until_str: Option<String> = row.get("valid_until")?;
    let created_at_str: String = row.get("created_at")?;

    Ok(Relation {
        id: RelationId::parse(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        from_entity_id: EntityId::parse(&from_entity_id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        to_entity_id: EntityId::parse(&to_entity_id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        relation_type: row.get("relation_type")?,
        note: row.get("note")?,
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
        created_at: parse_rfc3339(&created_at_str)?,
    })
}

pub(in crate::services) fn insert_relation(
    conn: &Connection,
    relation: &Relation,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO relations (id, from_entity_id, to_entity_id, relation_type, note, epistemic_state, confidence, sensitivity, valid_from, valid_until, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            relation.id.to_string(),
            relation.from_entity_id.to_string(),
            relation.to_entity_id.to_string(),
            relation.relation_type,
            relation.note,
            relation.epistemic_state.as_str(),
            relation.confidence,
            relation.sensitivity.as_str(),
            relation.valid_from.to_rfc3339(),
            relation.valid_until.map(|d| d.to_rfc3339()),
            relation.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub(in crate::services) fn get_relation(
    conn: &Connection,
    id: RelationId,
) -> rusqlite::Result<Option<Relation>> {
    conn.query_row(
        "SELECT id, from_entity_id, to_entity_id, relation_type, note, epistemic_state, confidence, sensitivity, valid_from, valid_until, created_at
         FROM relations WHERE id = ?1",
        params![id.to_string()],
        row_to_relation,
    )
    .optional()
}

/// Links a Relation to Evidence via `relation_evidence` - same shape as
/// `insert_fact_evidence_link`.
pub(in crate::services) fn insert_relation_evidence_link(
    conn: &Connection,
    relation_id: RelationId,
    evidence_id: EvidenceId,
    created_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO relation_evidence (relation_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        params![
            relation_id.to_string(),
            evidence_id.to_string(),
            created_at.to_rfc3339()
        ],
    )?;
    Ok(())
}

/// First production writer of `relation_audit_log` (`V8__create_relation_audit_log.sql`)
/// - same shape and discipline as `insert_fact_audit_log`.
/// `previous_snapshot` is always `None` for `AddRelation` (this creates a
/// Relation, it does not update one).
#[allow(clippy::too_many_arguments)]
pub(in crate::services) fn insert_relation_audit_log(
    conn: &Connection,
    relation_id: RelationId,
    action_type: &str,
    actor: Actor,
    provenance: Provenance,
    confirmation_class: ConfirmationClass,
    previous_snapshot_json: Option<String>,
    occurred_at: DateTime<Utc>,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO relation_audit_log (id, relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Uuid::now_v7().to_string(),
            relation_id.to_string(),
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

/// `CorrectRelation`'s ONLY mutation path (M2 Personal Knowledge
/// CorrectRelation, R027, 2026-09-19) - the same narrow-storage-function
/// discipline `update_fact_as_superseded`/`confirm_fact_epistemic_state`
/// established: its SQL touches exactly `note`/`confidence`/
/// `sensitivity`, nothing else. `id`/`from_entity_id`/`to_entity_id`/
/// `relation_type`/`epistemic_state`/`valid_from`/`valid_until`/
/// `created_at` are structurally impossible to touch through this
/// function's parameter list - there is deliberately no
/// `relations.updated_at` to set either (R025's post-freeze schema-scope
/// addendum).
///
/// The `WHERE` clause re-checks the exact `domain::is_active_relation`
/// predicate (`valid_until IS NULL AND epistemic_state <> 'STALE'`) at
/// mutation time, as defense-in-depth alongside `services::
/// correct_relation`'s own preflight check - a Relation that became
/// inactive between the preflight and this statement (or a nonexistent
/// id) cannot silently succeed. `changed != 1` (0 rows: missing id or
/// no-longer-active target; >1 is structurally impossible against a
/// primary key) is a real runtime error in every build profile, never a
/// `debug_assert!`/`assert!`/`panic!`. The diagnostic is fixed and
/// carries no Relation content.
pub(in crate::services) fn update_relation_metadata_if_active(
    conn: &Connection,
    relation_id: RelationId,
    note: Option<&str>,
    confidence: Option<f32>,
    sensitivity: Sensitivity,
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "UPDATE relations SET note = ?2, confidence = ?3, sensitivity = ?4
         WHERE id = ?1 AND valid_until IS NULL AND epistemic_state <> 'STALE'",
        params![
            relation_id.to_string(),
            note,
            confidence,
            sensitivity.as_str(),
        ],
    )?;
    if changed != 1 {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
            Some(
                "CorrectRelation metadata update did not affect exactly one row - Relation missing or no longer active"
                    .to_string(),
            ),
        ));
    }
    Ok(())
}
