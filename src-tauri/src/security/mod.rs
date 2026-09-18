//! Key management for LifeOS's encrypted local database
//! (M2 Goal vertical slice, 2026-09-17; key-lifecycle safety hardened
//! 2026-09-18, M2 storage/audit safety hardening).
//!
//! Matches `LIFEOS_SPEC.md` §5's target design: a random 256-bit master
//! key, generated once, stored via the OS credential store, never
//! logged, never written to plaintext config. Proven functionally in
//! the storage spike (`docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md`
//! requirement 10) - this module gives it the real, stable identity
//! LifeOS itself will use, in place of the spike's throwaway naming.
//!
//! This module deliberately exposes narrow primitives
//! (`get_existing_key`/`generate_and_store_key`/`delete_key`) rather
//! than only an opaque "get or create" helper - `services::
//! resolve_master_key` composes them into the canonical four-case
//! master-key/database state machine (database absent/exists ×
//! credential absent/exists), which needs to distinguish those cases
//! explicitly rather than have this module silently collapse them. See
//! `docs/reports/R015_M2_STORAGE_AND_AUDIT_SAFETY_HARDENING_2026-09-18.md`
//! for the full reasoning - a blind "credential missing -> generate a
//! new key" (the old `get_or_create_production_master_key`, removed
//! this pass) is unsafe once a real encrypted database can already
//! exist: it would silently generate an unusable replacement key
//! instead of surfacing a typed recovery-required error.

/// Stable credential identity for the production database master key.
/// `service` names the application (not a per-install or per-user
/// value); `account` is versioned so a future key-rotation scheme
/// (`database-master-key-v2`, etc.) can coexist with or replace this
/// one without ambiguity about which key an entry is. `pub(crate)`
/// (M2 storage/audit safety hardening, 2026-09-18) - `services::
/// bootstrap_production_db` now drives the master-key/database state
/// machine explicitly (see `services::resolve_master_key`) rather than
/// through a single opaque "get or create" call, so it needs to name
/// the real identity itself.
pub(crate) const SERVICE_NAME: &str = "LifeOS";
pub(crate) const MASTER_KEY_ACCOUNT: &str = "database-master-key-v1";

#[derive(Debug)]
pub(crate) enum KeyError {
    Keyring(String),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Deliberately never includes the key material itself, even
            // in error paths - only the underlying keyring backend's own
            // error text (e.g. "no matching entry found"), which never
            // contains secret bytes.
            KeyError::Keyring(e) => write!(f, "key store error: {e}"),
        }
    }
}

fn entry(service: &str, account: &str) -> Result<keyring::Entry, KeyError> {
    keyring::Entry::new(service, account).map_err(|e| KeyError::Keyring(e.to_string()))
}

fn generate_key_hex() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads back an existing credential's value WITHOUT creating one -
/// `Ok(None)` means no entry exists yet, not an error (M2 storage/audit
/// safety hardening, 2026-09-18). This is the primitive the production
/// bootstrap state machine (`services::resolve_master_key`) needs to
/// distinguish "credential exists" from "credential absent" BEFORE
/// deciding whether generating a new key is safe -
/// `get_or_create_master_key` below cannot be used for that decision,
/// because it always creates on absence and therefore cannot tell its
/// caller which case it hit.
pub(crate) fn get_existing_key(service: &str, account: &str) -> Result<Option<String>, KeyError> {
    let e = entry(service, account)?;
    match e.get_password() {
        Ok(key_hex) => Ok(Some(key_hex)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(other) => Err(KeyError::Keyring(other.to_string())),
    }
}

/// Generates a fresh random 256-bit key and stores it under (`service`,
/// `account`), unconditionally overwriting any existing entry (M2
/// storage/audit safety hardening, 2026-09-18). Callers must only
/// invoke this once they have already established - via
/// `get_existing_key` - that doing so is safe/intended: the production
/// bootstrap state machine only ever calls this for a database that
/// does not yet exist (Case A - first run). Never call this merely
/// because a credential lookup failed to find one for an EXISTING
/// database (Case C) - that is a recovery situation, not a "generate a
/// replacement" situation.
pub(crate) fn generate_and_store_key(service: &str, account: &str) -> Result<String, KeyError> {
    let key_hex = generate_key_hex();
    let e = entry(service, account)?;
    e.set_password(&key_hex)
        .map_err(|err| KeyError::Keyring(err.to_string()))?;
    Ok(key_hex)
}

/// Deletes the credential at (`service`, `account`) if present - a
/// no-op, not an error, if it is already absent (M2 storage/audit
/// safety hardening, 2026-09-18). Used for orphan-credential cleanup:
/// `services::bootstrap_db_with_key_lifecycle` only ever calls this
/// against a credential THAT SAME invocation just created via
/// `generate_and_store_key` moments earlier, never against one that
/// might have already existed - never delete a pre-existing credential.
pub(crate) fn delete_key(service: &str, account: &str) -> Result<(), KeyError> {
    let e = entry(service, account)?;
    match e.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(other) => Err(KeyError::Keyring(other.to_string())),
    }
}

/// Retrieves the existing key under (`service`, `account`), or generates
/// and stores a new random one if none exists yet. Idempotent: safe to
/// call on every app startup. Parameterized so tests can exercise the
/// exact same logic under a test-specific account, never the real
/// production credential. **Not used by the production bootstrap path**
/// (M2 storage/audit safety hardening, 2026-09-18) - it cannot
/// distinguish "database already exists but credential is missing"
/// (Case C, a recovery situation) from "genuine first run" (Case A),
/// so using it directly for bootstrap risked silently generating a
/// replacement key for an existing, now-undecryptable database. Kept
/// as a small generic primitive for callers (`get_or_create_master_key_for_test`
/// below) that genuinely want plain idempotent get-or-create semantics
/// without that distinction.
#[cfg_attr(not(test), allow(dead_code))]
fn get_or_create_master_key(service: &str, account: &str) -> Result<String, KeyError> {
    match get_existing_key(service, account)? {
        Some(key_hex) => Ok(key_hex),
        None => generate_and_store_key(service, account),
    }
}

#[cfg(test)]
pub(crate) fn get_or_create_master_key_for_test(account: &str) -> Result<String, KeyError> {
    get_or_create_master_key("LifeOS-test", account)
}

#[cfg(test)]
pub(crate) fn delete_key_for_test(account: &str) -> Result<(), KeyError> {
    delete_key("LifeOS-test", account)
}
