//! Encrypted SQLite access (M2 Goal vertical slice, 2026-09-17).
//!
//! PRIVATE by construction: this module is declared as a plain `mod
//! storage;` in `services/mod.rs` (no `pub`), so nothing outside the
//! `services` module tree can name `services::storage` at all - not
//! `domain`, not `actions`, not an external crate, not a Tauri command
//! or future capability layer. Every function here is additionally
//! `pub(super)` (visible only to `services/mod.rs` itself) as defense
//! in depth. Verified for real, not just claimed - see
//! `docs/reports/REPORT_M2_STORAGE_SPIKE_2026-09-17.md` (original
//! compiler-error proof) and
//! `docs/reports/REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`
//! (re-verified after this restructuring).
//!
//! `open_encrypted`/`verify_key`/`rekey`/`integrity_check` are generic
//! encrypted-SQLite primitives, proven in the storage spike and reused
//! here unchanged for production. Production migrations
//! (`../../../migrations/`) create ONLY real domain tables - `goals`
//! (V1), `projects`/`project_audit_log` (V2, M2 Project vertical slice,
//! 2026-09-17), `goal_relations`/`goal_relation_audit_log` (V3, M2
//! GoalRelation vertical slice, 2026-09-17), `milestones`/
//! `milestone_audit_log` (V4, M2 Milestone vertical slice, 2026-09-17),
//! and `tasks`/`task_audit_log` (V5, M2 Task vertical slice,
//! 2026-09-17) - the spike's `spike_records` schema lives under
//! `spike/`, compiled for `#[cfg(test)]` only, and is never reachable
//! from a non-test build.

// Not wired into `run()` yet this run - only exercised by tests, hence
// dead_code would otherwise warn on every non-test build.
#![allow(dead_code)]

use crate::actions::{Actor, ConfirmationClass, Provenance};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::path::Path;

pub(super) mod goal_relation_repo;
pub(super) mod goal_repo;
pub(super) mod milestone_repo;
pub(super) mod project_repo;
pub(super) mod task_repo;

#[cfg(test)]
pub(super) mod spike;

/// Shared by `goal_repo`/`project_repo` (M2 Project vertical slice,
/// 2026-09-17 - promoted here to avoid each repo module duplicating an
/// identical RFC3339 parse helper). `pub(super)` here means visible
/// throughout `crate::services` (this module's parent), which includes
/// `storage`'s own child repo modules.
pub(super) fn parse_rfc3339(s: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
}

/// Shared audit-column string mappings - same reasoning as
/// `parse_rfc3339` above. Moved out of `goal_repo` (where they
/// originated, Goal vertical slice) rather than duplicated into
/// `project_repo`.
pub(super) fn actor_str(actor: Actor) -> &'static str {
    match actor {
        Actor::Ui => "UI",
        Actor::CloudLlm => "CloudLLM",
        Actor::NeXa => "NeXa",
    }
}

pub(super) fn provenance_str(provenance: Provenance) -> &'static str {
    match provenance {
        Provenance::Explicit => "Explicit",
        Provenance::Inferred => "Inferred",
    }
}

pub(super) fn confirmation_class_str(class: ConfirmationClass) -> &'static str {
    match class {
        ConfirmationClass::SafeRead => "SAFE_READ",
        ConfirmationClass::ExplicitAdd => "EXPLICIT_ADD",
        ConfirmationClass::InferredChange => "INFERRED_CHANGE",
        ConfirmationClass::ImportantChange => "IMPORTANT_CHANGE",
        ConfirmationClass::Delete => "DELETE",
        ConfirmationClass::Sensitive => "SENSITIVE",
    }
}

/// Opens (creating if absent) an encrypted database at `path` using a
/// raw 256-bit hex key (`PRAGMA key = "x'<64 hex chars>'"` - the raw-key
/// form, avoiding SQLCipher's PBKDF2 derivation since our key is already
/// a random 256-bit master key, not a human password). Also enables
/// SQLite foreign-key constraint enforcement (`enable_foreign_keys`,
/// below) - this is the single connection-opening primitive every real
/// production connection, version-targeted migration test connection
/// (`services::bootstrap_db_at_version`), and the storage spike's test
/// connections all go through, so none of them can silently diverge
/// from production FK behavior (M2 Project/storage integrity
/// hardening, 2026-09-17 - previously `projects.goal_id REFERENCES
/// goals(id)` was declared in the V2 migration but never actually
/// enforced at runtime, since this pragma was never set anywhere).
pub(super) fn open_encrypted(path: &Path, key_hex: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "key", format!("x'{key_hex}'"))?;
    enable_foreign_keys(&conn)?;
    Ok(conn)
}

/// `PRAGMA foreign_keys = ON` is a per-connection setting SQLite does
/// not persist in the database file itself - every new connection must
/// set it again, and unlike most pragmas a silently-ignored
/// `foreign_keys` change is a real, documented SQLite footgun (e.g. it
/// is a no-op while a transaction is active). This reads the pragma
/// back immediately and returns a typed error rather than assuming the
/// `PRAGMA` succeeded - "do not silently ignore failure" (M2
/// Project/storage integrity hardening brief). Proven for real, not
/// just asserted - see
/// `services::tests::req_i1_foreign_keys_enabled_on_fresh_production_connection`
/// and `..req_i2_foreign_keys_enabled_after_reopen`.
fn enable_foreign_keys(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let enabled: i64 = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    if enabled != 1 {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
            Some("PRAGMA foreign_keys = ON did not take effect on this connection".to_string()),
        ));
    }
    Ok(())
}

/// SQLCipher only verifies the key lazily on first real read of the
/// database header - this forces that check immediately, so a wrong key
/// fails here with a clean, typed error rather than surfacing later as a
/// confusing failure on some unrelated query.
pub(super) fn verify_key(conn: &Connection) -> rusqlite::Result<()> {
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map(|_| ())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn rekey(conn: &Connection, new_key_hex: &str) -> rusqlite::Result<()> {
    conn.pragma_update(None, "rekey", format!("x'{new_key_hex}'"))
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn integrity_check(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
}

/// Production migrations only - `src-tauri/migrations/`. Must never
/// create `spike_records` (proven by a test - see
/// `services::tests::req_production_db_has_no_spike_records`).
mod migrations {
    refinery::embed_migrations!("./migrations");
}

/// Runs pending PRODUCTION versioned migrations (refinery) against
/// `conn`. Returns the applied migration versions for evidence/logging.
pub(super) fn run_production_migrations(
    conn: &mut Connection,
) -> Result<Vec<String>, refinery::Error> {
    let report = migrations::migrations::runner().run(conn)?;
    Ok(report
        .applied_migrations()
        .iter()
        .map(|m| m.to_string())
        .collect())
}

/// Test-only: applies production migrations only up to `version`, so a
/// test can construct a database frozen at an old schema version (e.g.
/// V1/Goal only) and then separately apply the rest later - simulating
/// a real user's existing database being upgraded across an app update,
/// rather than only ever testing a fresh DB that gets every migration
/// at once (M2 Project vertical slice, 2026-09-17 - see
/// `services::tests::req_p_v1_goal_database_upgrades_cleanly_to_v2_project`).
#[cfg(test)]
pub(super) fn run_production_migrations_to_version(
    conn: &mut Connection,
    version: i32,
) -> Result<Vec<String>, refinery::Error> {
    let report = migrations::migrations::runner()
        .set_target(refinery::Target::Version(version))
        .run(conn)?;
    Ok(report
        .applied_migrations()
        .iter()
        .map(|m| m.to_string())
        .collect())
}
