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
//! (`../../../migrations/`) create ONLY the real `goals` table - the
//! spike's `spike_records` schema lives under `spike/`, compiled for
//! `#[cfg(test)]` only, and is never reachable from a non-test build.

// Not wired into `run()` yet this run - only exercised by tests, hence
// dead_code would otherwise warn on every non-test build.
#![allow(dead_code)]

use rusqlite::Connection;
use std::path::Path;

pub(super) mod goal_repo;

#[cfg(test)]
pub(super) mod spike;

/// Opens (creating if absent) an encrypted database at `path` using a
/// raw 256-bit hex key (`PRAGMA key = "x'<64 hex chars>'"` - the raw-key
/// form, avoiding SQLCipher's PBKDF2 derivation since our key is already
/// a random 256-bit master key, not a human password).
pub(super) fn open_encrypted(path: &Path, key_hex: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "key", format!("x'{key_hex}'"))?;
    Ok(conn)
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
