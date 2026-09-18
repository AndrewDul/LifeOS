//! Key management for LifeOS's encrypted local database
//! (M2 Goal vertical slice, 2026-09-17).
//!
//! Matches `LIFEOS_SPEC.md` §5's target design: a random 256-bit master
//! key, generated once, stored via the OS credential store, never
//! logged, never written to plaintext config. Proven functionally in
//! the storage spike (`docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md`
//! requirement 10) - this module gives it the real, stable identity
//! LifeOS itself will use, in place of the spike's throwaway naming.

/// Stable credential identity for the production database master key.
/// `service` names the application (not a per-install or per-user
/// value); `account` is versioned so a future key-rotation scheme
/// (`database-master-key-v2`, etc.) can coexist with or replace this
/// one without ambiguity about which key an entry is.
const SERVICE_NAME: &str = "LifeOS";
const MASTER_KEY_ACCOUNT: &str = "database-master-key-v1";

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

/// Retrieves the existing key under (`service`, `account`), or generates
/// and stores a new random one if none exists yet. Idempotent: safe to
/// call on every app startup. Parameterized so tests can exercise the
/// exact same logic under a test-specific account, never the real
/// production credential.
fn get_or_create_master_key(service: &str, account: &str) -> Result<String, KeyError> {
    let e = entry(service, account)?;
    match e.get_password() {
        Ok(key_hex) => Ok(key_hex),
        Err(keyring::Error::NoEntry) => {
            let key_hex = generate_key_hex();
            e.set_password(&key_hex)
                .map_err(|err| KeyError::Keyring(err.to_string()))?;
            Ok(key_hex)
        }
        Err(other) => Err(KeyError::Keyring(other.to_string())),
    }
}

/// Retrieves the existing production master key, or generates and
/// stores a new random one on first run. Idempotent: safe to call on
/// every app startup.
#[allow(dead_code)]
pub(crate) fn get_or_create_production_master_key() -> Result<String, KeyError> {
    get_or_create_master_key(SERVICE_NAME, MASTER_KEY_ACCOUNT)
}

#[cfg(test)]
pub(crate) fn get_or_create_master_key_for_test(account: &str) -> Result<String, KeyError> {
    get_or_create_master_key("LifeOS-test", account)
}

#[cfg(test)]
pub(crate) fn delete_key_for_test(account: &str) -> Result<(), KeyError> {
    let e = entry("LifeOS-test", account)?;
    match e.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(other) => Err(KeyError::Keyring(other.to_string())),
    }
}
