//! LifeOS Core service layer — M2 storage spike (2026-09-17).
//!
//! This is a proof of the encrypted-storage architecture, not the real
//! LifeOS domain. It only knows about a `spike_records` table; Goal/
//! Project/Fact and the real action layer are not implemented here.
//!
//! `storage` is declared as a private submodule below (no `pub`) - this
//! is the corrected M2 module-boundary enforcement (see
//! `docs/LIFEOS_SPEC.md` §5.6 and the storage-spike report's errata):
//! nothing outside this file can reach `services::storage` at all,
//! proven by deliberately attempting it and capturing the compiler's
//! rejection (see the report, section "A1 proof").

// Spike code isn't called from `run()` yet (no capability/Tauri-command
// layer exists this run) - only from its own tests, hence `dead_code`
// would otherwise warn on every non-test build.
#![allow(dead_code)]

mod storage;

#[cfg(test)]
mod tests;

use rand::Rng;
use std::path::Path;

/// A typed error for spike operations - never a bare string, matching
/// the frozen action-layer convention (`LIFEOS_SPEC.md` §5.4).
#[derive(Debug)]
pub(crate) enum SpikeError {
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

/// Generates a random 256-bit key, hex-encoded (64 chars) for use both
/// as the SQLCipher raw key material and as the string stored in the OS
/// keyring.
pub(crate) fn generate_key_hex() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn create_and_seed(
    path: &Path,
    key_hex: &str,
    secret_text: &str,
) -> Result<i64, SpikeError> {
    let conn = storage::open_encrypted(path, key_hex)?;
    storage::verify_key(&conn)?;
    let mut conn = conn;
    storage::run_migrations(&mut conn)?;
    let id = storage::insert_record(&conn, secret_text)?;
    Ok(id)
}

pub(crate) fn reopen_and_read(path: &Path, key_hex: &str) -> Result<Vec<(i64, String)>, SpikeError> {
    let conn = storage::open_encrypted(path, key_hex)?;
    storage::verify_key(&conn)?;
    Ok(storage::read_all_records(&conn)?)
}

pub(crate) fn run_migrations_only(path: &Path, key_hex: &str) -> Result<Vec<String>, SpikeError> {
    let mut conn = storage::open_encrypted(path, key_hex)?;
    storage::verify_key(&conn)?;
    Ok(storage::run_migrations(&mut conn)?)
}

pub(crate) fn rekey_db(path: &Path, old_key_hex: &str, new_key_hex: &str) -> Result<(), SpikeError> {
    let conn = storage::open_encrypted(path, old_key_hex)?;
    storage::verify_key(&conn)?;
    storage::rekey(&conn, new_key_hex)?;
    Ok(())
}

pub(crate) fn check_integrity(path: &Path, key_hex: &str) -> Result<String, SpikeError> {
    let conn = storage::open_encrypted(path, key_hex)?;
    storage::verify_key(&conn)?;
    Ok(storage::integrity_check(&conn)?)
}

/// Stores `key_hex` in the OS credential store under a spike-specific
/// service/username pair (never a real LifeOS key path).
pub(crate) fn keyring_store(service: &str, username: &str, key_hex: &str) -> Result<(), SpikeError> {
    let entry = keyring::Entry::new(service, username).map_err(|e| SpikeError::Keyring(e.to_string()))?;
    entry
        .set_password(key_hex)
        .map_err(|e| SpikeError::Keyring(e.to_string()))
}

pub(crate) fn keyring_retrieve(service: &str, username: &str) -> Result<String, SpikeError> {
    let entry = keyring::Entry::new(service, username).map_err(|e| SpikeError::Keyring(e.to_string()))?;
    entry.get_password().map_err(|e| SpikeError::Keyring(e.to_string()))
}

pub(crate) fn keyring_delete(service: &str, username: &str) -> Result<(), SpikeError> {
    let entry = keyring::Entry::new(service, username).map_err(|e| SpikeError::Keyring(e.to_string()))?;
    entry.delete_credential().map_err(|e| SpikeError::Keyring(e.to_string()))
}
