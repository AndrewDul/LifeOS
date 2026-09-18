//! M2 encrypted-storage foundation spike (2026-09-17), kept as a
//! `#[cfg(test)]`-only regression suite after the Goal vertical slice.
//!
//! **Compiled for tests only - never part of a release build.**
//! `spike_records` is NOT the real LifeOS schema; see
//! `docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md` for what this
//! originally proved (encrypted creation, restart persistence, wrong-key
//! rejection, encrypted-at-rest, migration, rekey, backup/restore,
//! corruption handling, OS keyring round trip, Windows release build).
//! Kept here, unmodified in behavior, purely so those 10 requirements
//! stay regression-tested going forward - not because the app needs
//! `spike_records` for anything.
//!
//! Reuses the parent `storage` module's generic encrypted-SQLite
//! primitives (`open_encrypted`/`verify_key`/`rekey`/`integrity_check`)
//! via `super::` - those are real production code, proven once, not
//! duplicated. Only the spike's own schema/migrations/table access are
//! specific to this module.

use rusqlite::Connection;
use std::path::Path;

#[derive(Debug)]
pub(super) enum SpikeError {
    Storage(rusqlite::Error),
    Migration(refinery::Error),
    Keyring(String),
}

impl std::fmt::Display for SpikeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpikeError::Storage(e) => write!(f, "storage error: {e}"),
            SpikeError::Migration(e) => write!(f, "migration error: {e}"),
            SpikeError::Keyring(e) => write!(f, "keyring error: {e}"),
        }
    }
}

impl From<rusqlite::Error> for SpikeError {
    fn from(e: rusqlite::Error) -> Self {
        SpikeError::Storage(e)
    }
}

impl From<refinery::Error> for SpikeError {
    fn from(e: refinery::Error) -> Self {
        SpikeError::Migration(e)
    }
}

pub(super) fn generate_key_hex() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn insert_record(conn: &Connection, secret_text: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO spike_records (secret_text, created_at) VALUES (?1, datetime('now'))",
        [secret_text],
    )?;
    Ok(conn.last_insert_rowid())
}

fn read_all_records(conn: &Connection) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT id, secret_text FROM spike_records ORDER BY id")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

/// Spike-only migrations - a SEPARATE directory from the real
/// `src-tauri/migrations/` (which now contains only the production Goal
/// schema). This is what enforces "spike migrations never silently
/// become LifeOS production migrations": they are not even the same
/// files, embedded from a different path, compiled only under `cfg(test)`.
mod spike_migrations {
    refinery::embed_migrations!("./src/services/storage/spike/migrations");
}

fn run_spike_migrations(conn: &mut Connection) -> Result<Vec<String>, refinery::Error> {
    let report = spike_migrations::migrations::runner().run(conn)?;
    Ok(report
        .applied_migrations()
        .iter()
        .map(|m| m.to_string())
        .collect())
}

pub(super) fn create_and_seed(
    path: &Path,
    key_hex: &str,
    secret_text: &str,
) -> Result<i64, SpikeError> {
    let mut conn = super::open_encrypted(path, key_hex)?;
    super::verify_key(&conn)?;
    run_spike_migrations(&mut conn)?;
    Ok(insert_record(&conn, secret_text)?)
}

pub(super) fn reopen_and_read(
    path: &Path,
    key_hex: &str,
) -> Result<Vec<(i64, String)>, SpikeError> {
    let conn = super::open_encrypted(path, key_hex)?;
    super::verify_key(&conn)?;
    Ok(read_all_records(&conn)?)
}

pub(super) fn run_migrations_only(path: &Path, key_hex: &str) -> Result<Vec<String>, SpikeError> {
    let mut conn = super::open_encrypted(path, key_hex)?;
    super::verify_key(&conn)?;
    Ok(run_spike_migrations(&mut conn)?)
}

pub(super) fn rekey_db(
    path: &Path,
    old_key_hex: &str,
    new_key_hex: &str,
) -> Result<(), SpikeError> {
    let conn = super::open_encrypted(path, old_key_hex)?;
    super::verify_key(&conn)?;
    super::rekey(&conn, new_key_hex)?;
    Ok(())
}

pub(super) fn check_integrity(path: &Path, key_hex: &str) -> Result<String, SpikeError> {
    let conn = super::open_encrypted(path, key_hex)?;
    super::verify_key(&conn)?;
    Ok(super::integrity_check(&conn)?)
}

pub(super) fn keyring_store(
    service: &str,
    username: &str,
    key_hex: &str,
) -> Result<(), SpikeError> {
    let entry =
        keyring::Entry::new(service, username).map_err(|e| SpikeError::Keyring(e.to_string()))?;
    entry
        .set_password(key_hex)
        .map_err(|e| SpikeError::Keyring(e.to_string()))
}

pub(super) fn keyring_retrieve(service: &str, username: &str) -> Result<String, SpikeError> {
    let entry =
        keyring::Entry::new(service, username).map_err(|e| SpikeError::Keyring(e.to_string()))?;
    entry
        .get_password()
        .map_err(|e| SpikeError::Keyring(e.to_string()))
}

pub(super) fn keyring_delete(service: &str, username: &str) -> Result<(), SpikeError> {
    let entry =
        keyring::Entry::new(service, username).map_err(|e| SpikeError::Keyring(e.to_string()))?;
    entry
        .delete_credential()
        .map_err(|e| SpikeError::Keyring(e.to_string()))
}

#[cfg(test)]
mod tests;
