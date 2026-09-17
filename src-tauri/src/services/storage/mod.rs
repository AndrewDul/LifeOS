//! Encrypted SQLite access for the M2 storage spike (2026-09-17).
//!
//! PRIVATE by construction: this module is declared as a plain `mod
//! storage;` in `services/mod.rs` (no `pub`), so nothing outside the
//! `services` module tree can name `services::storage` at all - not
//! `capability`, not `domain`, not an external crate. Every function
//! here is additionally `pub(super)` (visible only to `services/mod.rs`
//! itself) as defense in depth, in case a future edit ever loosens the
//! module declaration. See docs/reports/REPORT_M2_STORAGE_SPIKE_2026-09-17.md
//! for the captured compiler error proving this boundary for real.
//!
//! `spike_records` only - this is NOT the real LifeOS schema.

use rusqlite::Connection;
use std::path::Path;

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

pub(super) fn insert_record(conn: &Connection, secret_text: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO spike_records (secret_text, created_at) VALUES (?1, datetime('now'))",
        [secret_text],
    )?;
    Ok(conn.last_insert_rowid())
}

pub(super) fn read_all_records(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT id, secret_text FROM spike_records ORDER BY id")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub(super) fn rekey(conn: &Connection, new_key_hex: &str) -> rusqlite::Result<()> {
    conn.pragma_update(None, "rekey", format!("x'{new_key_hex}'"))
}

pub(super) fn integrity_check(conn: &Connection) -> rusqlite::Result<String> {
    conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
}

mod migrations {
    refinery::embed_migrations!("./migrations");
}

/// Runs pending versioned migrations (refinery) against `conn`.
/// Returns the applied migration versions for evidence purposes.
pub(super) fn run_migrations(conn: &mut Connection) -> Result<Vec<String>, refinery::Error> {
    let report = migrations::migrations::runner().run(conn)?;
    Ok(report
        .applied_migrations()
        .iter()
        .map(|m| m.to_string())
        .collect())
}
