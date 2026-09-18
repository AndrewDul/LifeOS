//! LifeOS Core service layer (M2 Goal vertical slice, 2026-09-17).
//!
//! `storage` is a private submodule (no `pub` at all) - this is the
//! corrected M2 module-boundary enforcement (`docs/LIFEOS_SPEC.md`
//! §5.6): nothing outside this file can reach `services::storage`,
//! proven by deliberately attempting it and capturing the compiler's
//! rejection (`docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md`).
//! `services` is the only module allowed to call `storage`; `domain`
//! and `actions` are pure and never touch SQL. Nothing outside this
//! crate calls any of this yet - there is no Tauri command layer this
//! run ("No React wiring yet. No UI changes yet.").

// Not wired into `run()` yet this run - only exercised by
// services::tests, hence dead_code would otherwise warn on every
// non-test build.
#![allow(dead_code)]

mod storage;

use crate::actions::{
    blocks_until_confirmed, effective_class, ActionMeta, ActionOutcome, AddMemoryPayload,
    AddMilestonePayload, AddTaskPayload, ConfirmationClass, CorrectMemoryPayload,
    CreateGoalPayload, CreateProjectPayload, EntityInputRef, FactValueInput, FieldPatch,
    LinkGoalsPayload, Provenance, SourceRef, UpdateGoalPayload, UpdateMilestonePayload,
    UpdateProgressPayload, UpdateProjectPayload, UpdateTaskPayload, ValidationError,
};
use crate::core_error::{CoreError, CoreErrorKind};
use crate::domain::{
    is_active_fact, Entity, EntityId, EpistemicState, Evidence, EvidenceId, Fact, FactId,
    FactValue, Goal, GoalId, GoalRelation, GoalRelationId, LifecycleStatus, Milestone, MilestoneId,
    MilestoneStatus, Project, ProjectId, Sensitivity, Source, SourceId, Task, TaskId, TaskStatus,
};
use crate::security;
use chrono::Utc;
use rusqlite::Connection;
use std::path::PathBuf;

/// Matches `tauri.conf.json`'s `identifier` - used as the app-data
/// subdirectory name via the same convention Tauri's own path resolver
/// uses (there is no running `AppHandle` to ask this run, since nothing
/// wires this into `run()` yet - see the module doc above).
const APP_IDENTIFIER: &str = "com.lifeos.app";
const DB_FILE_NAME: &str = "lifeos.db";

#[derive(Debug)]
pub(crate) enum BootstrapError {
    NoDataDir,
    Io(std::io::Error),
    Key(security::KeyError),
    Storage(rusqlite::Error),
    Migration(refinery::Error),
    /// Case C of the master-key/database state machine (M2 storage/
    /// audit safety hardening, 2026-09-18): an encrypted database
    /// already exists at the production path, but no matching OS-
    /// credential master key is available. This is a RECOVERY
    /// situation, not first-run setup - LifeOS must never silently
    /// generate/store a replacement key (it cannot decrypt the
    /// existing database and would only make real recovery more
    /// confusing). No key was generated, no key was stored, the
    /// existing database file was not touched in any way. A future UI
    /// may offer recovery/backup options; M2 only returns this typed
    /// error.
    MasterKeyMissingForExistingDatabase,
}

impl std::fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootstrapError::NoDataDir => {
                write!(f, "could not determine the platform app-data directory")
            }
            BootstrapError::Io(e) => write!(f, "filesystem error: {e}"),
            BootstrapError::Key(e) => write!(f, "{e}"),
            BootstrapError::Storage(e) => write!(f, "storage error: {e}"),
            BootstrapError::Migration(e) => write!(f, "migration error: {e}"),
            BootstrapError::MasterKeyMissingForExistingDatabase => write!(
                f,
                "an existing LifeOS database was found, but its master key is not available \
                 in the OS credential store - this requires manual recovery, not automatic \
                 key replacement"
            ),
        }
    }
}

impl From<rusqlite::Error> for BootstrapError {
    fn from(e: rusqlite::Error) -> Self {
        BootstrapError::Storage(e)
    }
}

impl From<refinery::Error> for BootstrapError {
    fn from(e: refinery::Error) -> Self {
        BootstrapError::Migration(e)
    }
}

/// Resolves the real production database path via the platform's
/// user-data directory (`dirs::data_dir()` - the same underlying
/// mechanism Tauri's `app.path().app_data_dir()` uses). Windows:
/// `%APPDATA%\com.lifeos.app\lifeos.db`.
pub(crate) fn production_db_path() -> Result<PathBuf, BootstrapError> {
    let base = dirs::data_dir().ok_or(BootstrapError::NoDataDir)?;
    Ok(base.join(APP_IDENTIFIER).join(DB_FILE_NAME))
}

/// The reusable bootstrap core: create the parent directory if needed,
/// open (or create) the encrypted database at `path` with `key_hex`,
/// verify the key, and apply production migrations. Parameterized by
/// path and key so it is independently testable against a temp
/// location/key, never the real production path/keyring entry, in
/// automated tests.
fn bootstrap_db_at(path: &std::path::Path, key_hex: &str) -> Result<Connection, BootstrapError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(BootstrapError::Io)?;
    }
    let mut conn = storage::open_encrypted(path, key_hex)?;
    storage::verify_key(&conn)?;
    storage::run_production_migrations(&mut conn)?;
    // Personal Knowledge SELF bootstrap (M2 Slice A, R019 §3/R020 §21-22)
    // - runs on every bootstrap, after migrations, before the connection
    // is considered ready for Personal Knowledge use. NOT called by
    // `bootstrap_db_at_version` below (test-only, freezes at an
    // arbitrary older schema version that may predate V7 entirely).
    ensure_self_entity(&conn)?;
    Ok(conn)
}

/// Guarantees exactly one canonical SELF `Entity` exists, together with
/// the DB-level partial unique index (`idx_entities_single_self`,
/// `V7__create_personal_knowledge.sql`) - that index alone only proves
/// AT MOST one; this proves AT LEAST one (R019 §3). Idempotent - safe
/// to call on every bootstrap and repeatedly in tests. Deliberately
/// NOT placed inside the V7 migration itself: a migration is schema,
/// not Core-level bootstrap logic.
fn ensure_self_entity(conn: &Connection) -> Result<Entity, BootstrapError> {
    let count = storage::personal_knowledge_repo::count_self_entities(conn)?;
    match count {
        0 => {
            let now = Utc::now();
            let entity = Entity {
                id: EntityId::new(),
                entity_type: "person".to_string(),
                canonical_name: "SELF".to_string(),
                is_self: true,
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
                created_at: now,
                updated_at: now,
            };
            storage::personal_knowledge_repo::insert_entity(conn, &entity)?;
            Ok(entity)
        }
        1 => storage::personal_knowledge_repo::get_self_entity(conn)?.ok_or_else(|| {
            BootstrapError::Storage(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
                Some("SELF entity count was 1 but the row could not be read back".to_string()),
            ))
        }),
        // Should be structurally impossible given the DB partial unique
        // index, but detected and reported explicitly anyway rather
        // than silently picking the first row and continuing (R019 §3).
        _ => Err(BootstrapError::Storage(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
            Some(format!(
                "integrity violation: {count} SELF entities found, expected at most 1"
            )),
        ))),
    }
}

/// Test-only: same as `bootstrap_db_at` but stops migrating at
/// `version` - lets a test build a database frozen at an old schema
/// version (e.g. V1/Goal only, before `Project` existed) and then
/// upgrade it later with a normal `bootstrap_db_at` call, proving the
/// real upgrade path rather than only ever testing a fresh all-at-once
/// migration (M2 Project vertical slice, 2026-09-17).
#[cfg(test)]
fn bootstrap_db_at_version(
    path: &std::path::Path,
    key_hex: &str,
    version: i32,
) -> Result<Connection, BootstrapError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(BootstrapError::Io)?;
    }
    let mut conn = storage::open_encrypted(path, key_hex)?;
    storage::verify_key(&conn)?;
    storage::run_production_migrations_to_version(&mut conn, version)?;
    Ok(conn)
}

/// Which of the four canonical database/master-key cases
/// `resolve_master_key` resolved to (M2 storage/audit safety hardening,
/// 2026-09-18) - callers need to know whether the returned key was
/// just freshly generated by THIS call (Case A) or reused from an
/// existing credential (Cases B/D), specifically so a subsequent
/// database-initialization failure can safely clean up only a
/// credential this same call just created.
enum ResolvedMasterKey {
    /// Case B (database + credential both already existed) or Case D
    /// (credential existed, database did not) - this key was NOT
    /// created by this call and must never be deleted, even if
    /// database initialization subsequently fails.
    Reused(String),
    /// Case A (neither database nor credential existed) - this call
    /// generated and stored a brand-new credential. If database
    /// initialization subsequently fails, this - and only this -
    /// newly-created credential is a safe cleanup target.
    FreshlyGenerated(String),
}

impl ResolvedMasterKey {
    fn key_hex(&self) -> &str {
        match self {
            ResolvedMasterKey::Reused(k) | ResolvedMasterKey::FreshlyGenerated(k) => k,
        }
    }
}

/// Deliberately hand-written, NOT `#[derive(Debug)]` (R016) - this type
/// carries the actual master key in plaintext; a derived `Debug` would
/// print it verbatim in any `{:?}`/`panic!`/test-assertion-failure
/// message a caller writes (e.g. a test asserting on a
/// `Result<ResolvedMasterKey, BootstrapError>`). Only the variant name is
/// shown; the key itself is never included.
impl std::fmt::Debug for ResolvedMasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolvedMasterKey::Reused(_) => write!(f, "ResolvedMasterKey::Reused(<redacted>)"),
            ResolvedMasterKey::FreshlyGenerated(_) => {
                write!(f, "ResolvedMasterKey::FreshlyGenerated(<redacted>)")
            }
        }
    }
}

/// The canonical master-key/database state machine (M2 storage/audit
/// safety hardening, 2026-09-18) - see
/// `docs/reports/R015_M2_STORAGE_AND_AUDIT_SAFETY_HARDENING_2026-09-18.md`
/// for the full four-case table. `path` must be the SAME path
/// production opening actually uses (never a second, competing
/// existence check - `bootstrap_db_with_key_lifecycle` below always
/// passes the one it was itself given); (`service`, `account`) identify
/// the OS-credential entry. Any existing file at `path` is treated as
/// existing data, full stop - zero length, corruption, or a
/// wrong-key failure are separate recovery cases, never grounds to
/// assume "safe to overwrite" here.
///
/// - DB absent, credential absent (Case A, normal first run): generates
///   and stores a fresh key.
/// - DB exists, credential exists (Case B): reuses the existing key.
///   Never replaced.
/// - DB exists, credential absent (Case C, CRITICAL): a RECOVERY
///   situation, not first-run setup - returns
///   `BootstrapError::MasterKeyMissingForExistingDatabase` immediately.
///   Never generates/stores a replacement key (it could not decrypt the
///   existing database anyway), never touches the existing database
///   file in any way.
/// - DB absent, credential exists (Case D - e.g. after intentional DB
///   deletion, restore preparation, or interrupted setup): reuses the
///   existing credential to create the new database. No key-rotation
///   flow is invented here.
fn resolve_master_key(
    path: &std::path::Path,
    service: &str,
    account: &str,
) -> Result<ResolvedMasterKey, BootstrapError> {
    resolve_master_key_with_store(&security::KeyringCredentialStore, path, service, account)
}

/// Generic core of `resolve_master_key`, operating over any
/// `security::CredentialStore` rather than always the real OS keyring
/// (M2 key lifecycle determinism and failure-atomicity hardening,
/// 2026-09-18, R016). `resolve_master_key` (above) is a thin wrapper
/// binding this to `security::KeyringCredentialStore` - the real
/// production identity path and every deterministic in-memory-store
/// test share this exact logic, never a parallel reimplementation that
/// could drift from what production actually does.
///
/// A backend `get` failure (`store.get` returning `Err`) propagates
/// immediately as `BootstrapError::Key` via `?` - it is never silently
/// treated as "credential absent" (which would risk misclassifying a
/// transient backend outage as Case A/C and generating an unwarranted
/// key or unwarranted recovery-required error).
fn resolve_master_key_with_store<S: security::CredentialStore>(
    store: &S,
    path: &std::path::Path,
    service: &str,
    account: &str,
) -> Result<ResolvedMasterKey, BootstrapError> {
    let db_exists = path.exists();
    let existing_key = store.get(service, account).map_err(BootstrapError::Key)?;

    match (db_exists, existing_key) {
        (_, Some(key_hex)) => Ok(ResolvedMasterKey::Reused(key_hex)),
        (false, None) => {
            let key_hex = security::generate_key_hex();
            store
                .set(service, account, &key_hex)
                .map_err(BootstrapError::Key)?;
            Ok(ResolvedMasterKey::FreshlyGenerated(key_hex))
        }
        (true, None) => Err(BootstrapError::MasterKeyMissingForExistingDatabase),
    }
}

/// Resolves the master key via `resolve_master_key`, then opens/creates
/// the encrypted database at `path` (M2 storage/audit safety hardening,
/// 2026-09-18). If this call generated a brand-new credential (Case A)
/// and database initialization subsequently fails before a usable
/// database exists, the orphaned credential is cleaned up automatically
/// - a pre-existing credential (Cases B/D) is never touched, even on
/// failure, since `resolve_master_key` only ever returns
/// `FreshlyGenerated` for Case A. Cleanup is best-effort: if deleting
/// the orphan credential itself fails, the ORIGINAL initialization
/// error is still what gets returned unchanged - a secondary cleanup
/// failure must never mask the real failure the caller needs to see,
/// and no secret is ever included in either error. Parameterized by
/// path/service/account so both `bootstrap_production_db` (real
/// identity) and tests (an isolated identity) share this exact logic,
/// not a parallel reimplementation.
fn bootstrap_db_with_key_lifecycle(
    path: &std::path::Path,
    service: &str,
    account: &str,
) -> Result<Connection, BootstrapError> {
    bootstrap_db_with_key_lifecycle_with_store(
        &security::KeyringCredentialStore,
        path,
        service,
        account,
        bootstrap_db_at,
    )
}

/// Generic core of `bootstrap_db_with_key_lifecycle`, operating over any
/// `security::CredentialStore` AND any database initializer closure (M2
/// key lifecycle determinism and failure-atomicity hardening,
/// 2026-09-18, R016). `initializer` is always `bootstrap_db_at` in every
/// real caller (`bootstrap_db_with_key_lifecycle` above, and therefore
/// `bootstrap_production_db`) - only deterministic tests substitute a
/// different closure, to force a failure AFTER the database file has
/// already been created, a case `bootstrap_db_at` itself cannot be made
/// to hit portably/deterministically (it depends on SQLCipher/refinery
/// internals, not a simple filesystem precondition).
///
/// **Corrected cleanup rule (R016, following a review finding against
/// R015's original version of this function - see
/// `docs/reports/R016_M2_KEY_LIFECYCLE_DETERMINISM_AND_FAILURE_ATOMICITY_HARDENING_2026-09-18.md`
/// §3-4):** if this call generated a brand-new credential (Case A) and
/// `initializer` subsequently fails, the fresh credential is deleted
/// ONLY IF `path` still does NOT exist afterward.
///
/// R015's original rule deleted the fresh credential on ANY
/// initialization failure, unconditionally. That is only safe if
/// `initializer` failed *before* creating any database file. If
/// `initializer` instead (1) creates/encrypts the database file using
/// the fresh key, then (2) fails later - during key verification or
/// migration, say - deleting the credential at that point would
/// self-inflict exactly the state this whole hardening pass exists to
/// prevent: an existing database file with no matching credential (Case
/// C), discovered on the very next launch. Once a file may contain bytes
/// encrypted under that fresh key, losing the key is strictly more
/// dangerous than retaining the credential - the database file is never
/// deleted by this cleanup path either way, so keeping the credential
/// preserves the DB+key pairing and lets a subsequent bootstrap retry
/// with the same key; deleting it would destroy that pairing
/// permanently. A pre-existing credential (Cases B/D) is never a cleanup
/// candidate regardless of this rule, since `resolved` only carries
/// `FreshlyGenerated` for Case A.
fn bootstrap_db_with_key_lifecycle_with_store<S, F>(
    store: &S,
    path: &std::path::Path,
    service: &str,
    account: &str,
    initializer: F,
) -> Result<Connection, BootstrapError>
where
    S: security::CredentialStore,
    F: FnOnce(&std::path::Path, &str) -> Result<Connection, BootstrapError>,
{
    let resolved = resolve_master_key_with_store(store, path, service, account)?;
    match initializer(path, resolved.key_hex()) {
        Ok(conn) => Ok(conn),
        Err(init_err) => {
            if let ResolvedMasterKey::FreshlyGenerated(_) = resolved {
                if !path.exists() {
                    // Pre-file-creation failure: no database bytes were
                    // ever written under this key, so the orphaned
                    // credential is a safe cleanup target. Best-effort
                    // only, deliberately: a failed deletion here does
                    // not change the fact that database initialization
                    // itself failed, and must never mask that original
                    // error by replacing it with a cleanup error
                    // instead.
                    let _ = store.delete(service, account);
                }
                // else: a file now exists at `path` that may contain
                // bytes encrypted under this freshly-generated key -
                // deliberately KEEP the credential, see doc comment
                // above. Never reached for a pre-existing credential
                // (Cases B/D).
            }
            Err(init_err)
        }
    }
}

/// Full production bootstrap: resolves the real DB path, then resolves
/// the master key via the canonical four-case state machine
/// (`resolve_master_key`, above) against the real production credential
/// identity (`LifeOS` / `database-master-key-v1`), opens the encrypted
/// database, and applies migrations. This is what a future Tauri
/// command / app setup hook calls - nothing calls it yet this run.
///
/// **Changed 2026-09-18 (M2 storage/audit safety hardening):**
/// previously called `security::get_or_create_production_master_key()`
/// directly, which could not distinguish "an existing database's
/// credential is missing" from a genuine first run - it would have
/// silently generated and stored an unusable replacement key for a
/// real user's existing, now-undecryptable database. That helper has
/// been removed; this function now goes through the explicit state
/// machine instead.
#[allow(dead_code)]
pub(crate) fn bootstrap_production_db() -> Result<Connection, BootstrapError> {
    let path = production_db_path()?;
    bootstrap_db_with_key_lifecycle(&path, security::SERVICE_NAME, security::MASTER_KEY_ACCOUNT)
}

/// Mandatory audit `previous_snapshot` serialization for every update
/// action that must record pre-mutation state (M2 storage/audit safety
/// hardening, 2026-09-18). Every `Update*` action below previously used
/// `serde_json::to_string(&existing).ok()`, silently turning a
/// hypothetical serialization failure into `previous_snapshot = None` -
/// an update could then commit with no audit history of what it
/// changed, weakening audit integrity and any future Undo support. This
/// helper is called BEFORE the mutation transaction starts (matching
/// every call site below); on failure the caller returns a typed
/// `CoreError` immediately with zero mutation and zero audit write,
/// instead of silently degrading to a `NULL` snapshot. `Create*`
/// actions are unaffected - they legitimately pass `None` directly
/// (there is no previous entity to snapshot), never through this
/// helper.
///
/// **Diagnostic redaction (R016, hardening a review finding against
/// R015):** the returned `CoreError`'s diagnostic text is a fixed
/// string - it deliberately does NOT include the underlying
/// `serde_json::Error`'s own message. R015 had argued this was already
/// safe because `serde_json`'s *current* built-in errors only describe
/// the failure mode (e.g. "recursion limit exceeded") and never echo
/// field values - true today, but an assumption about a dependency's
/// wording, not a structural guarantee: a future custom `Serialize`
/// implementation (on some as-yet-unwritten domain type) could return
/// `serde::ser::Error::custom(...)` with arbitrary text, including
/// content derived from the value being serialized. Excluding the raw
/// error text here makes "no personal snapshot content in this
/// diagnostic" true by construction, not by an assumption about what
/// serializers currently choose to say. Proven with a deliberately
/// failing test-only `Serialize` impl whose error message contains an
/// obvious marker string, asserting the marker never appears in this
/// error's `Display` or `Debug` output (`req_snap1`,
/// `services::tests`).
fn serialize_required_snapshot<T: serde::Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value).map_err(|_e| {
        CoreError::new(
            CoreErrorKind::Internal,
            "mandatory audit previous_snapshot serialization failed",
        )
    })
}

/// Goal mutation + its audit-log row are one atomic unit (M2 Goal
/// hardening, 2026-09-17). Previously the audit insert used
/// `let _ = storage::goal_repo::insert_audit_entry(...)`, silently
/// discarding a failed audit write while still returning
/// `ActionOutcome::Success` - the frozen architecture requires a
/// persisted `IMPORTANT_CHANGE`-and-above (and, in this slice,
/// `EXPLICIT_ADD`) mutation to have an audit record, so a caller must
/// never see `Success` when the domain write succeeded but the audit
/// write failed. Both writes now happen inside one `rusqlite`
/// transaction: if either fails, the transaction is never committed and
/// `Transaction`'s `Drop` impl issues the `ROLLBACK` automatically -
/// proven for real (not just by code inspection) in
/// `services::tests::req_h1_create_goal_audit_failure_rolls_back_goal_insert`
/// and `..req_h2_update_goal_audit_failure_rolls_back_goal_update` via a
/// forced-failure SQLite trigger. This is why `conn` is now
/// `&mut Connection` - `Connection::transaction()` requires unique
/// access.
pub(crate) fn create_goal(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: CreateGoalPayload,
) -> ActionOutcome<Goal> {
    let normalized_title = match crate::actions::validate_create_goal(&payload) {
        Ok(t) => t,
        Err(errors) => return ActionOutcome::ValidationFailed(errors),
    };

    let class = effective_class(ConfirmationClass::ExplicitAdd, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let now = meta.requested_at;
    let goal = Goal {
        id: GoalId::new(),
        title: normalized_title,
        description: payload.description,
        status: LifecycleStatus::Active,
        target_date: payload.target_date,
        priority_user: payload.priority_user,
        priority_system: None,
        progress: 0,
        domain_tag: payload.domain_tag,
        created_at: now,
        updated_at: now,
    };

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(e) = storage::goal_repo::insert_goal(&tx, &goal) {
        // `tx` drops here without a commit - rusqlite rolls it back.
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = storage::goal_repo::insert_audit_entry(
        &tx,
        goal.id,
        "CreateGoal",
        meta.actor,
        meta.provenance,
        class,
        None,
        now,
    ) {
        // Audit write failed - roll back the goal insert too, do not
        // return Success for a partially-recorded mutation.
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: goal,
        confirmation_class: class,
    }
}

pub(crate) fn update_goal(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: UpdateGoalPayload,
) -> ActionOutcome<Goal> {
    let existing = match storage::goal_repo::get_goal(conn, payload.id) {
        Ok(Some(g)) => g,
        Ok(None) => return ActionOutcome::NotFound,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(errors) = crate::actions::validate_update_goal(&existing, &payload) {
        return ActionOutcome::ValidationFailed(errors);
    }

    let class = effective_class(ConfirmationClass::ImportantChange, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let previous_snapshot = match serialize_required_snapshot(&existing) {
        Ok(s) => s,
        Err(e) => return ActionOutcome::StorageError(e),
    };

    let mut updated = existing;
    if let Some(title) = payload.title {
        updated.title = title.trim().to_string();
    }
    match payload.description {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(description) => updated.description = Some(description),
        FieldPatch::Clear => updated.description = None,
    }
    if let Some(status) = payload.status {
        updated.status = status;
    }
    match payload.target_date {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(target_date) => updated.target_date = Some(target_date),
        FieldPatch::Clear => updated.target_date = None,
    }
    match payload.priority_user {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(priority_user) => updated.priority_user = Some(priority_user),
        FieldPatch::Clear => updated.priority_user = None,
    }
    match payload.domain_tag {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(domain_tag) => updated.domain_tag = Some(domain_tag),
        FieldPatch::Clear => updated.domain_tag = None,
    }
    if let Some(progress) = payload.progress {
        updated.progress = progress;
    }
    updated.updated_at = meta.requested_at;

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(e) = storage::goal_repo::update_goal(&tx, &updated) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = storage::goal_repo::insert_audit_entry(
        &tx,
        updated.id,
        "UpdateGoal",
        meta.actor,
        meta.provenance,
        class,
        Some(previous_snapshot.as_str()),
        meta.requested_at,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: updated,
        confirmation_class: class,
    }
}

/// `SAFE_READ` - matches the name of the frozen v0.1 capability API's
/// `get_active_goals()` (`LIFEOS_SPEC.md` §10.1/§5.6). The capability
/// API *contract* (these function names/signatures) is frozen; a
/// dedicated `capability` module that actually sits behind Tauri
/// commands/cloud-tool handlers/NeXa is **not implemented yet** - it
/// must exist before any such external caller is wired up, and no
/// external caller may reach `services` (or `services::storage`)
/// directly once it does. Today this function (and `get_goal` below)
/// exists only so `services::tests` can exercise `create_goal`/
/// `update_goal` without a dedicated capability layer that has nothing
/// real to front yet - not a claim that the capability boundary itself
/// is done.
pub(crate) fn get_active_goals(conn: &Connection) -> Result<Vec<Goal>, rusqlite::Error> {
    storage::goal_repo::get_active_goals(conn)
}

/// Narrow internal read, useful for testing `create_goal`/`update_goal`
/// without growing the public capability surface beyond what the frozen
/// API actually lists.
pub(crate) fn get_goal(conn: &Connection, id: GoalId) -> Result<Option<Goal>, rusqlite::Error> {
    storage::goal_repo::get_goal(conn, id)
}

// --- Project (M2 Project vertical slice, 2026-09-17; UpdateProject
// added 2026-09-17, M2 UpdateProject implementation) --------------------
//
// Same atomic-mutation-plus-audit, typed-`CoreError`, module-boundary
// pattern as Goal above - see `docs/reports/R007_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`
// and `docs/reports/R014_M2_UPDATE_PROJECT_IMPLEMENTATION_2026-09-17.md`.
// `CreateProject`, `UpdateProject`, and `UpdateProgress` are all
// implemented now - `AddMilestone`/`AddTask` live under their own
// entities (`add_milestone`/`add_task` below), not here.

/// `CreateProject`'s confirmation floor is `EXPLICIT_ADD` - same as
/// `CreateGoal` - per the architecture-freeze report §F's confirmation
/// table, which explicitly lists `CreateProject` under `EXPLICIT_ADD`.
/// If `payload.goal_id` is supplied, it must reference an existing
/// `Goal` - checked here (a database read) BEFORE persistence, reported
/// through the same friendly, typed `ValidationFailed` channel as
/// structural checks, rather than surfacing a raw SQL foreign-key
/// failure to the caller. The FK constraint itself **is** genuinely
/// enforced at the database level too as of the M2 Project/storage
/// integrity hardening pass (2026-09-17) - `PRAGMA foreign_keys = ON`
/// is set on every connection (`storage::open_encrypted`) - so this
/// check is deliberately the first, friendlier line of defense, not the
/// only one.
pub(crate) fn create_project(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: CreateProjectPayload,
) -> ActionOutcome<Project> {
    let normalized_title = match crate::actions::validate_create_project(&payload) {
        Ok(t) => t,
        Err(errors) => return ActionOutcome::ValidationFailed(errors),
    };

    if let Some(goal_id) = payload.goal_id {
        match storage::goal_repo::goal_exists(conn, goal_id) {
            Ok(true) => {}
            Ok(false) => {
                return ActionOutcome::ValidationFailed(vec![
                    ValidationError::ReferencedGoalNotFound,
                ]);
            }
            Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
        }
    }

    let class = effective_class(ConfirmationClass::ExplicitAdd, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let now = meta.requested_at;
    let project = Project {
        id: ProjectId::new(),
        title: normalized_title,
        description: payload.description,
        status: LifecycleStatus::Active,
        priority: payload.priority,
        goal_id: payload.goal_id,
        progress: 0,
        created_at: now,
        updated_at: now,
    };

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(e) = storage::project_repo::insert_project(&tx, &project) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = storage::project_repo::insert_audit_entry(
        &tx,
        project.id,
        "CreateProject",
        meta.actor,
        meta.provenance,
        class,
        None,
        now,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: project,
        confirmation_class: class,
    }
}

/// `UpdateProject`'s confirmation floor is `IMPORTANT_CHANGE` - the
/// frozen floor from the M2 Project Child Action Freeze report, closing
/// the M2 Project/storage integrity hardening report's action-set gap
/// (§6 of that report). `Project.progress` is deliberately **not** part
/// of this payload - `UpdateProgress` (below) remains its exclusive
/// owner, floor `EXPLICIT_ADD`, unchanged; the two actions' payloads and
/// audit semantics are never merged. If `payload.goal_id` is
/// `FieldPatch::Set(x)`, `x` must reference a real, existing Goal -
/// checked here (a database read) BEFORE persistence, the same pattern
/// `create_project`'s own `goal_id` check already uses; `Clear`/
/// `Unchanged` need no lookup. No status-transition restriction: all
/// four `LifecycleStatus` values remain peer states for Project, exactly
/// as already established for Goal - a completed or abandoned Project
/// may be reopened.
pub(crate) fn update_project(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: UpdateProjectPayload,
) -> ActionOutcome<Project> {
    let existing = match storage::project_repo::get_project(conn, payload.id) {
        Ok(Some(p)) => p,
        Ok(None) => return ActionOutcome::NotFound,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(errors) = crate::actions::validate_update_project(&existing, &payload) {
        return ActionOutcome::ValidationFailed(errors);
    }

    if let FieldPatch::Set(goal_id) = payload.goal_id {
        match storage::goal_repo::goal_exists(conn, goal_id) {
            Ok(true) => {}
            Ok(false) => {
                return ActionOutcome::ValidationFailed(vec![
                    ValidationError::ReferencedGoalNotFound,
                ]);
            }
            Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
        }
    }

    let class = effective_class(ConfirmationClass::ImportantChange, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let previous_snapshot = match serialize_required_snapshot(&existing) {
        Ok(s) => s,
        Err(e) => return ActionOutcome::StorageError(e),
    };

    let mut updated = existing;
    if let Some(title) = payload.title {
        updated.title = title.trim().to_string();
    }
    match payload.description {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(description) => updated.description = Some(description),
        FieldPatch::Clear => updated.description = None,
    }
    if let Some(status) = payload.status {
        updated.status = status;
    }
    match payload.priority {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(priority) => updated.priority = Some(priority),
        FieldPatch::Clear => updated.priority = None,
    }
    match payload.goal_id {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(goal_id) => updated.goal_id = Some(goal_id),
        FieldPatch::Clear => updated.goal_id = None,
    }
    updated.updated_at = meta.requested_at;

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(e) = storage::project_repo::update_project(&tx, &updated) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = storage::project_repo::insert_audit_entry(
        &tx,
        updated.id,
        "UpdateProject",
        meta.actor,
        meta.provenance,
        class,
        Some(previous_snapshot.as_str()),
        meta.requested_at,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: updated,
        confirmation_class: class,
    }
}

/// `UpdateProgress`'s confirmation floor is also `EXPLICIT_ADD` (not
/// `IMPORTANT_CHANGE`, unlike `UpdateGoal`) - per the same frozen
/// confirmation table, which lists `UpdateProgress` under
/// `EXPLICIT_ADD` explicitly. This is a deliberate frozen difference
/// from Goal, not an oversight - do not "fix" it to match `UpdateGoal`
/// without `LIFEOS_SPEC.md` changing first.
pub(crate) fn update_progress(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: UpdateProgressPayload,
) -> ActionOutcome<Project> {
    let existing = match storage::project_repo::get_project(conn, payload.id) {
        Ok(Some(p)) => p,
        Ok(None) => return ActionOutcome::NotFound,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(errors) = crate::actions::validate_update_progress(&payload) {
        return ActionOutcome::ValidationFailed(errors);
    }

    let class = effective_class(ConfirmationClass::ExplicitAdd, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let previous_snapshot = match serialize_required_snapshot(&existing) {
        Ok(s) => s,
        Err(e) => return ActionOutcome::StorageError(e),
    };

    let mut updated = existing;
    updated.progress = payload.progress;
    updated.updated_at = meta.requested_at;

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(e) = storage::project_repo::update_project_progress(
        &tx,
        updated.id,
        updated.progress,
        updated.updated_at,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = storage::project_repo::insert_audit_entry(
        &tx,
        updated.id,
        "UpdateProgress",
        meta.actor,
        meta.provenance,
        class,
        Some(previous_snapshot.as_str()),
        meta.requested_at,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: updated,
        confirmation_class: class,
    }
}

/// Narrow internal read, same reasoning as `get_goal` above. The frozen
/// capability API's minimum function set (architecture-freeze report
/// §I) does not list any Project-specific read (no `get_projects()`/
/// `get_active_projects()` - only a generic, unspecified
/// `get_progress()`), so none is added here beyond what tests and
/// `update_progress` actually need.
pub(crate) fn get_project(
    conn: &Connection,
    id: ProjectId,
) -> Result<Option<Project>, rusqlite::Error> {
    storage::project_repo::get_project(conn, id)
}

// --- GoalRelation (M2 GoalRelation vertical slice, 2026-09-17) ----------
//
// The first true relationship-graph entity in production M2 - two
// foreign keys into the same `goals` table, ON DELETE RESTRICT on both
// (Goal has no hard DeleteGoal action; a relation must never silently
// disappear because an endpoint Goal was removed). Only `LinkGoals` is
// implemented - it is the entire frozen action for this graph, there is
// no separate `CreateGoalRelation`. Same atomic-mutation-plus-audit,
// typed-`CoreError` pattern as Goal/Project above - see
// `docs/reports/R009_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md`.

/// `LinkGoals`'s confirmation floor is `EXPLICIT_ADD` per the
/// architecture-freeze report §F's confirmation table. Validation order
/// is pure-first: `validate_link_goals` (self-link, note length) runs
/// before any database read; then `from_goal_id` existence, then
/// `to_goal_id` existence, then the exact-duplicate
/// `(from, to, relation_type)` check - each a real database read,
/// reported through the same deterministic `ValidationFailed` channel
/// as the structural checks rather than left to SQLite's FK/UNIQUE
/// constraints to catch first (those constraints are still real and
/// enforced - see `migrations/V3__create_goal_relations.sql` - this is
/// the friendlier first line of defense, the same relationship
/// `create_project`'s `ReferencedGoalNotFound` check has with the FK
/// constraint on `projects.goal_id`).
pub(crate) fn link_goals(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: LinkGoalsPayload,
) -> ActionOutcome<GoalRelation> {
    if let Err(errors) = crate::actions::validate_link_goals(&payload) {
        return ActionOutcome::ValidationFailed(errors);
    }

    match storage::goal_repo::goal_exists(conn, payload.from_goal_id) {
        Ok(true) => {}
        Ok(false) => {
            return ActionOutcome::ValidationFailed(vec![
                ValidationError::RelationFromGoalNotFound,
            ]);
        }
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    }
    match storage::goal_repo::goal_exists(conn, payload.to_goal_id) {
        Ok(true) => {}
        Ok(false) => {
            return ActionOutcome::ValidationFailed(vec![ValidationError::RelationToGoalNotFound]);
        }
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    }
    match storage::goal_relation_repo::relation_exists(
        conn,
        payload.from_goal_id,
        payload.to_goal_id,
        payload.relation_type,
    ) {
        Ok(false) => {}
        Ok(true) => {
            return ActionOutcome::ValidationFailed(vec![ValidationError::DuplicateGoalRelation]);
        }
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    }

    let class = effective_class(ConfirmationClass::ExplicitAdd, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let now = meta.requested_at;
    let relation = GoalRelation {
        id: GoalRelationId::new(),
        from_goal_id: payload.from_goal_id,
        to_goal_id: payload.to_goal_id,
        relation_type: payload.relation_type,
        note: payload.note,
        created_at: now,
    };

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(e) = storage::goal_relation_repo::insert_goal_relation(&tx, &relation) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = storage::goal_relation_repo::insert_audit_entry(
        &tx,
        relation.id,
        "LinkGoals",
        meta.actor,
        meta.provenance,
        class,
        None,
        now,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: relation,
        confirmation_class: class,
    }
}

/// Narrow internal read, same reasoning as `get_goal`/`get_project`.
pub(crate) fn get_goal_relation(
    conn: &Connection,
    id: GoalRelationId,
) -> Result<Option<GoalRelation>, rusqlite::Error> {
    storage::goal_relation_repo::get_goal_relation(conn, id)
}

/// Relations involving `goal_id` on either side (`from_goal_id` OR
/// `to_goal_id`) - the actual stored direction, never a fabricated
/// inverse row.
pub(crate) fn get_goal_relations_for_goal(
    conn: &Connection,
    goal_id: GoalId,
) -> Result<Vec<GoalRelation>, rusqlite::Error> {
    storage::goal_relation_repo::get_goal_relations_for_goal(conn, goal_id)
}

// --- Milestone (M2 Milestone vertical slice, 2026-09-17) ----------------
//
// The fourth real M2 domain entity, and the first required-parent child
// entity - `milestones.project_id REFERENCES projects(id) ON DELETE
// RESTRICT` (Project has no hard DeleteProject action; a Milestone must
// never silently disappear if one is ever added, same reasoning as
// GoalRelation's endpoints). Only `AddMilestone`/`UpdateMilestone` are
// implemented - Task is explicitly out of scope for this slice. Same
// atomic-mutation-plus-audit, typed-`CoreError` pattern as every entity
// above - see
// `docs/reports/R011_M2_MILESTONE_VERTICAL_SLICE_2026-09-17.md`.

/// `AddMilestone`'s confirmation floor is `EXPLICIT_ADD` per the
/// architecture-freeze report §F's confirmation table. Validation order
/// is pure-first (`validate_add_milestone` - title, order_index bounds)
/// before any database read; then `project_id` existence, then
/// `(project_id, order_index)` occupancy - each a real database read,
/// reported through the same deterministic `ValidationFailed` channel as
/// the structural checks rather than left to SQLite's FK/UNIQUE
/// constraints to catch first (those constraints are still real and
/// enforced - see `migrations/V4__create_milestones.sql` - this is the
/// friendlier first line of defense, the same relationship
/// `create_project`'s `ReferencedGoalNotFound` check has with the FK
/// constraint on `projects.goal_id`). `status` is never taken from the
/// payload - a new Milestone always starts `PENDING`.
pub(crate) fn add_milestone(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: AddMilestonePayload,
) -> ActionOutcome<Milestone> {
    let normalized_title = match crate::actions::validate_add_milestone(&payload) {
        Ok(t) => t,
        Err(errors) => return ActionOutcome::ValidationFailed(errors),
    };

    match storage::project_repo::project_exists(conn, payload.project_id) {
        Ok(true) => {}
        Ok(false) => {
            return ActionOutcome::ValidationFailed(vec![
                ValidationError::ReferencedProjectNotFound,
            ]);
        }
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    }
    match storage::milestone_repo::order_index_taken(
        conn,
        payload.project_id,
        payload.order_index,
        None,
    ) {
        Ok(false) => {}
        Ok(true) => {
            return ActionOutcome::ValidationFailed(vec![
                ValidationError::DuplicateMilestoneOrderIndex,
            ]);
        }
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    }

    let class = effective_class(ConfirmationClass::ExplicitAdd, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let milestone = Milestone {
        id: MilestoneId::new(),
        project_id: payload.project_id,
        title: normalized_title,
        target_date: payload.target_date,
        status: MilestoneStatus::Pending,
        order_index: payload.order_index,
    };

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(e) = storage::milestone_repo::insert_milestone(&tx, &milestone) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = storage::milestone_repo::insert_audit_entry(
        &tx,
        milestone.id,
        "AddMilestone",
        meta.actor,
        meta.provenance,
        class,
        None,
        meta.requested_at,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: milestone,
        confirmation_class: class,
    }
}

/// `UpdateMilestone`'s confirmation floor is also `EXPLICIT_ADD` - the
/// frozen floor from the M2 Project Child Action Freeze report, not
/// `IMPORTANT_CHANGE` like `UpdateGoal` (do not invent a stricter floor
/// than frozen). If `payload.order_index` is `Some` and differs from the
/// existing Milestone's current value, the target slot must not already
/// belong to a DIFFERENT Milestone in the SAME Project - checked via
/// `order_index_taken`'s `exclude_id`, so the Milestone keeping its own
/// current index is never reported as a conflict with itself.
pub(crate) fn update_milestone(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: UpdateMilestonePayload,
) -> ActionOutcome<Milestone> {
    let existing = match storage::milestone_repo::get_milestone(conn, payload.id) {
        Ok(Some(m)) => m,
        Ok(None) => return ActionOutcome::NotFound,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(errors) = crate::actions::validate_update_milestone(&existing, &payload) {
        return ActionOutcome::ValidationFailed(errors);
    }

    if let Some(order_index) = payload.order_index {
        if order_index != existing.order_index {
            match storage::milestone_repo::order_index_taken(
                conn,
                existing.project_id,
                order_index,
                Some(existing.id),
            ) {
                Ok(false) => {}
                Ok(true) => {
                    return ActionOutcome::ValidationFailed(vec![
                        ValidationError::DuplicateMilestoneOrderIndex,
                    ]);
                }
                Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
            }
        }
    }

    let class = effective_class(ConfirmationClass::ExplicitAdd, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let previous_snapshot = match serialize_required_snapshot(&existing) {
        Ok(s) => s,
        Err(e) => return ActionOutcome::StorageError(e),
    };

    let mut updated = existing;
    if let Some(title) = payload.title {
        updated.title = title.trim().to_string();
    }
    match payload.target_date {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(target_date) => updated.target_date = Some(target_date),
        FieldPatch::Clear => updated.target_date = None,
    }
    if let Some(status) = payload.status {
        updated.status = status;
    }
    if let Some(order_index) = payload.order_index {
        updated.order_index = order_index;
    }

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(e) = storage::milestone_repo::update_milestone(&tx, &updated) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = storage::milestone_repo::insert_audit_entry(
        &tx,
        updated.id,
        "UpdateMilestone",
        meta.actor,
        meta.provenance,
        class,
        Some(previous_snapshot.as_str()),
        meta.requested_at,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: updated,
        confirmation_class: class,
    }
}

/// Narrow internal read, same reasoning as `get_goal`/`get_project`.
pub(crate) fn get_milestone(
    conn: &Connection,
    id: MilestoneId,
) -> Result<Option<Milestone>, rusqlite::Error> {
    storage::milestone_repo::get_milestone(conn, id)
}

/// Deterministic `ORDER BY order_index ASC` read - unambiguous since
/// `(project_id, order_index)` is UNIQUE.
pub(crate) fn get_milestones_for_project(
    conn: &Connection,
    project_id: ProjectId,
) -> Result<Vec<Milestone>, rusqlite::Error> {
    storage::milestone_repo::get_milestones_for_project(conn, project_id)
}

// --- Task (M2 Task vertical slice, 2026-09-17) --------------------------
//
// The fifth real M2 domain entity, and the first with two independent
// nullable parent references - `tasks.project_id`/`tasks.goal_id`, both
// `ON DELETE RESTRICT` (Project/Goal have no hard delete action; a Task
// must never silently disappear if one is ever added, same reasoning as
// Milestone/GoalRelation). Only `AddTask`/`UpdateTask` are implemented -
// RecurringRule/Calendar/Reminder/Challenge/Personal Knowledge are all
// explicitly out of scope for this slice. Same atomic-mutation-plus-
// audit, typed-`CoreError` pattern as every entity above - see
// `docs/reports/R012_M2_TASK_VERTICAL_SLICE_2026-09-17.md`.

/// `AddTask`'s confirmation floor is `EXPLICIT_ADD` per the
/// architecture-freeze report §F's confirmation table / the M2 Project
/// Child Action Freeze report. Validation order is pure-first
/// (`validate_add_task` - title only) before any database read; then
/// `project_id` existence (if supplied) AND `goal_id` existence (if
/// supplied) are both checked - each independently, a real database
/// read, neither ever skipped because the other was checked or failed
/// (Task vertical-slice report §10 - a Task's Project and Goal need not
/// have any relationship to each other). If both are supplied and both
/// are invalid, both `ValidationError`s are returned together, in
/// stable `ReferencedProjectNotFound`-then-`ReferencedGoalNotFound`
/// order (M2 Task parent-validation hardening pass, 2026-09-17 -
/// corrects an earlier short-circuit that returned only the first
/// missing reference). A real storage error from either lookup still
/// aborts immediately as a `StorageError`, never silently downgraded to
/// a validation failure. `status` is never taken from the payload - a
/// new Task always starts `TODO`; `recurring_rule_id` is never taken
/// from the payload either - it always starts `None` (report §2C).
pub(crate) fn add_task(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: AddTaskPayload,
) -> ActionOutcome<Task> {
    let normalized_title = match crate::actions::validate_add_task(&payload) {
        Ok(t) => t,
        Err(errors) => return ActionOutcome::ValidationFailed(errors),
    };

    let mut reference_errors: Vec<ValidationError> = Vec::new();
    if let Some(project_id) = payload.project_id {
        match storage::project_repo::project_exists(conn, project_id) {
            Ok(true) => {}
            Ok(false) => reference_errors.push(ValidationError::ReferencedProjectNotFound),
            Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
        }
    }
    if let Some(goal_id) = payload.goal_id {
        match storage::goal_repo::goal_exists(conn, goal_id) {
            Ok(true) => {}
            Ok(false) => reference_errors.push(ValidationError::ReferencedGoalNotFound),
            Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
        }
    }
    if !reference_errors.is_empty() {
        return ActionOutcome::ValidationFailed(reference_errors);
    }

    let class = effective_class(ConfirmationClass::ExplicitAdd, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let now = meta.requested_at;
    let task = Task {
        id: TaskId::new(),
        title: normalized_title,
        status: TaskStatus::Todo,
        due_at: payload.due_at,
        project_id: payload.project_id,
        goal_id: payload.goal_id,
        recurring_rule_id: None,
        created_at: now,
        updated_at: now,
    };

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(e) = storage::task_repo::insert_task(&tx, &task) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = storage::task_repo::insert_audit_entry(
        &tx,
        task.id,
        "AddTask",
        meta.actor,
        meta.provenance,
        class,
        None,
        now,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: task,
        confirmation_class: class,
    }
}

/// `UpdateTask`'s confirmation floor is also `EXPLICIT_ADD` - the frozen
/// floor from the M2 Project Child Action Freeze report, not
/// `IMPORTANT_CHANGE` like `UpdateGoal` (do not invent a stricter floor
/// than frozen). If `payload.project_id`/`payload.goal_id` is
/// `FieldPatch::Set(x)`, `x` must reference a real, existing Project/
/// Goal - checked independently, same reasoning as `add_task` above; if
/// both are `Set` to a nonexistent target, both `ValidationError`s are
/// returned together in stable `ReferencedProjectNotFound`-then-
/// `ReferencedGoalNotFound` order, with zero mutation (M2 Task
/// parent-validation hardening pass, 2026-09-17 - corrects an earlier
/// short-circuit that returned only the first missing reference).
/// `FieldPatch::Clear`/`Unchanged` need no existence check. A real
/// storage error from either lookup still aborts immediately as a
/// `StorageError`. `recurring_rule_id` is never touched -
/// `UpdateTaskPayload` has no such field.
pub(crate) fn update_task(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: UpdateTaskPayload,
) -> ActionOutcome<Task> {
    let existing = match storage::task_repo::get_task(conn, payload.id) {
        Ok(Some(t)) => t,
        Ok(None) => return ActionOutcome::NotFound,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(errors) = crate::actions::validate_update_task(&existing, &payload) {
        return ActionOutcome::ValidationFailed(errors);
    }

    let mut reference_errors: Vec<ValidationError> = Vec::new();
    if let FieldPatch::Set(project_id) = payload.project_id {
        match storage::project_repo::project_exists(conn, project_id) {
            Ok(true) => {}
            Ok(false) => reference_errors.push(ValidationError::ReferencedProjectNotFound),
            Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
        }
    }
    if let FieldPatch::Set(goal_id) = payload.goal_id {
        match storage::goal_repo::goal_exists(conn, goal_id) {
            Ok(true) => {}
            Ok(false) => reference_errors.push(ValidationError::ReferencedGoalNotFound),
            Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
        }
    }
    if !reference_errors.is_empty() {
        return ActionOutcome::ValidationFailed(reference_errors);
    }

    let class = effective_class(ConfirmationClass::ExplicitAdd, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let previous_snapshot = match serialize_required_snapshot(&existing) {
        Ok(s) => s,
        Err(e) => return ActionOutcome::StorageError(e),
    };

    let mut updated = existing;
    if let Some(title) = payload.title {
        updated.title = title.trim().to_string();
    }
    if let Some(status) = payload.status {
        updated.status = status;
    }
    match payload.due_at {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(due_at) => updated.due_at = Some(due_at),
        FieldPatch::Clear => updated.due_at = None,
    }
    match payload.project_id {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(project_id) => updated.project_id = Some(project_id),
        FieldPatch::Clear => updated.project_id = None,
    }
    match payload.goal_id {
        FieldPatch::Unchanged => {}
        FieldPatch::Set(goal_id) => updated.goal_id = Some(goal_id),
        FieldPatch::Clear => updated.goal_id = None,
    }
    updated.updated_at = meta.requested_at;

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Err(e) = storage::task_repo::update_task(&tx, &updated) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = storage::task_repo::insert_audit_entry(
        &tx,
        updated.id,
        "UpdateTask",
        meta.actor,
        meta.provenance,
        class,
        Some(previous_snapshot.as_str()),
        meta.requested_at,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: updated,
        confirmation_class: class,
    }
}

/// Narrow internal read, same reasoning as `get_goal`/`get_project`/
/// `get_milestone`.
pub(crate) fn get_task(conn: &Connection, id: TaskId) -> Result<Option<Task>, rusqlite::Error> {
    storage::task_repo::get_task(conn, id)
}

/// Deterministic technical ordering (`due_at ASC, created_at ASC, id
/// ASC` - NOT product prioritization; see `task_repo::get_tasks_for_project`'s
/// doc comment).
pub(crate) fn get_tasks_for_project(
    conn: &Connection,
    project_id: ProjectId,
) -> Result<Vec<Task>, rusqlite::Error> {
    storage::task_repo::get_tasks_for_project(conn, project_id)
}

/// Same ordering/documentation as `get_tasks_for_project`, above.
pub(crate) fn get_tasks_for_goal(
    conn: &Connection,
    goal_id: GoalId,
) -> Result<Vec<Task>, rusqlite::Error> {
    storage::task_repo::get_tasks_for_goal(conn, goal_id)
}

// --- Personal Knowledge (M2 Slice A foundation, 2026-09-18) -----------
//
// Schema (`V7__create_personal_knowledge.sql`) + SELF bootstrap
// (`ensure_self_entity`, above, wired into `bootstrap_db_at`) + this
// narrow read side + Slice B's `AddMemory` (below) - see
// `docs/reports/R020_M2_PERSONAL_KNOWLEDGE_SLICE_A_FOUNDATION_2026-09-18.md`
// and `docs/reports/R021_M2_PERSONAL_KNOWLEDGE_ADDMEMORY_SLICE_B_2026-09-18.md`.
// `CorrectMemory`/`ConfirmMemory`/`ForgetMemory` still do not exist
// (Slice C/D+) - `AddMemory` never performs correction, confirmation, or
// deletion logic of any kind.

/// Narrow internal read (M2 Personal Knowledge Slice A) - same "read
/// side stays narrow, add only when a real caller needs it" precedent
/// as every other M2 entity's `get_*` function. No production action
/// creates `Entity` rows yet besides `ensure_self_entity`'s own SELF
/// row; this exists for that bootstrap path's own use and for Slice B+.
pub(crate) fn get_entity(
    conn: &Connection,
    id: EntityId,
) -> Result<Option<Entity>, rusqlite::Error> {
    storage::personal_knowledge_repo::get_entity(conn, id)
}

/// Same reasoning as `get_entity`, above. No production action creates
/// `Fact` rows yet (Slice B's `AddMemory`) - this exists for Slice B+
/// and for this slice's own read-round-trip proofs against test
/// fixtures.
pub(crate) fn get_fact(conn: &Connection, id: FactId) -> Result<Option<Fact>, rusqlite::Error> {
    storage::personal_knowledge_repo::get_fact(conn, id)
}

/// A resolved `EntityInputRef` ready for `Fact` construction:
/// `id` is always populated (SELF/Existing/New all resolve to a real
/// `EntityId`), `to_insert` is `Some` only for `New` - the entity that
/// must be inserted inside `add_memory`'s transaction. `SelfEntity`/
/// `Existing` are never mutated - `to_insert` stays `None` for both, so
/// nothing about them is ever written.
struct ResolvedEntityRef {
    id: EntityId,
    to_insert: Option<Entity>,
}

/// Same shape as `ResolvedEntityRef`, for `SourceRef`.
struct ResolvedSourceRef {
    id: SourceId,
    to_insert: Option<Source>,
}

/// Authoritative domain-ref existence check (R021 §7) - reuses each
/// domain module's own `*_exists` read, never duplicating SQL against
/// Goal/Project/Milestone/Task tables inside Personal Knowledge. The
/// exact currently-supported `domain_ref_type` set is `Goal`/`Project`/
/// `Milestone`/`Task` - the minimum the Slice B brief requires. An
/// unknown type pushes `UnsupportedDomainReferenceType` (does not close
/// the open `domain_ref_type` vocabulary, only limits what Core can
/// validate today); a supported type with a syntactically-invalid or
/// well-formed-but-nonexistent id pushes `ReferencedDomainObjectNotFound`
/// (deliberately the same variant for both - see `ValidationError`'s
/// doc). Returns `Ok(true)` only when the reference is confirmed to
/// exist; any `Ok(false)` means a `ValidationError` was already pushed.
fn validate_domain_ref_authoritative(
    conn: &Connection,
    domain_ref: &crate::domain::DomainRef,
    errors: &mut Vec<ValidationError>,
) -> Result<bool, CoreError> {
    let exists = match domain_ref.domain_ref_type.as_str() {
        "Goal" => match GoalId::parse(&domain_ref.domain_ref_id) {
            Ok(id) => storage::goal_repo::goal_exists(conn, id)?,
            Err(_) => false,
        },
        "Project" => match ProjectId::parse(&domain_ref.domain_ref_id) {
            Ok(id) => storage::project_repo::project_exists(conn, id)?,
            Err(_) => false,
        },
        "Milestone" => match MilestoneId::parse(&domain_ref.domain_ref_id) {
            Ok(id) => storage::milestone_repo::milestone_exists(conn, id)?,
            Err(_) => false,
        },
        "Task" => match TaskId::parse(&domain_ref.domain_ref_id) {
            Ok(id) => storage::task_repo::task_exists(conn, id)?,
            Err(_) => false,
        },
        _ => {
            errors.push(ValidationError::UnsupportedDomainReferenceType);
            return Ok(false);
        }
    };
    if exists {
        Ok(true)
    } else {
        errors.push(ValidationError::ReferencedDomainObjectNotFound);
        Ok(false)
    }
}

/// Resolves one `EntityInputRef` (subject or, when the value is
/// `FactValueInput::Entity`, the object) against the database.
/// `SelfEntity` reads the existing SELF row through the same trusted
/// path `ensure_self_entity`/`get_self_entity` use - `AddMemory` never
/// creates, searches for, or mutates SELF. `Existing` requires the
/// referenced `Entity` to already exist (`ReferencedEntityNotFound`
/// otherwise) and is never mutated on reuse. `New` validates the
/// `domain_ref` authoritatively (if present) and checks it against
/// already-persisted Entities (`find_entity_by_domain_ref` -
/// `DuplicateDomainReference`) before allocating a fresh `EntityId` in
/// memory - nothing is written here; the caller inserts `to_insert`
/// inside the transaction. `Ok(None)` means one or more
/// `ValidationError`s were pushed into `errors`; `Err` is a genuine
/// storage failure that must abort the whole action immediately.
fn resolve_entity_input_ref(
    conn: &Connection,
    input: &EntityInputRef,
    now: chrono::DateTime<Utc>,
    errors: &mut Vec<ValidationError>,
) -> Result<Option<ResolvedEntityRef>, CoreError> {
    match input {
        EntityInputRef::SelfEntity => {
            match storage::personal_knowledge_repo::get_self_entity(conn)? {
                Some(e) => Ok(Some(ResolvedEntityRef {
                    id: e.id,
                    to_insert: None,
                })),
                None => Err(CoreError::from(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
                    Some(
                        "SELF entity missing at AddMemory time - bootstrap invariant violated"
                            .to_string(),
                    ),
                ))),
            }
        }
        EntityInputRef::Existing(id) => {
            if storage::personal_knowledge_repo::entity_exists(conn, *id)? {
                Ok(Some(ResolvedEntityRef {
                    id: *id,
                    to_insert: None,
                }))
            } else {
                errors.push(ValidationError::ReferencedEntityNotFound);
                Ok(None)
            }
        }
        EntityInputRef::New(data) => {
            if let Some(domain_ref) = &data.domain_ref {
                if !validate_domain_ref_authoritative(conn, domain_ref, errors)? {
                    return Ok(None);
                }
                if storage::personal_knowledge_repo::find_entity_by_domain_ref(
                    conn,
                    &domain_ref.domain_ref_type,
                    &domain_ref.domain_ref_id,
                )?
                .is_some()
                {
                    errors.push(ValidationError::DuplicateDomainReference);
                    return Ok(None);
                }
            }
            let id = EntityId::new();
            let entity = Entity {
                id,
                entity_type: data.entity_type.clone(),
                canonical_name: data.canonical_name.clone(),
                is_self: false,
                sensitivity: data.sensitivity,
                domain_ref: data.domain_ref.clone(),
                created_at: now,
                updated_at: now,
            };
            Ok(Some(ResolvedEntityRef {
                id,
                to_insert: Some(entity),
            }))
        }
    }
}

/// Same reasoning as `resolve_entity_input_ref`, for `SourceRef`.
fn resolve_source_ref(
    conn: &Connection,
    input: &SourceRef,
    now: chrono::DateTime<Utc>,
    errors: &mut Vec<ValidationError>,
) -> Result<Option<ResolvedSourceRef>, CoreError> {
    match input {
        SourceRef::Existing(id) => {
            if storage::personal_knowledge_repo::source_exists(conn, *id)? {
                Ok(Some(ResolvedSourceRef {
                    id: *id,
                    to_insert: None,
                }))
            } else {
                errors.push(ValidationError::ReferencedSourceNotFound);
                Ok(None)
            }
        }
        SourceRef::New(data) => {
            let id = SourceId::new();
            let source = Source {
                id,
                source_type: data.source_type.clone(),
                label: data.label.clone(),
                reference: data.reference.clone(),
                sensitivity: data.sensitivity,
                captured_at: data.captured_at,
                created_at: now,
            };
            Ok(Some(ResolvedSourceRef {
                id,
                to_insert: Some(source),
            }))
        }
    }
}

/// `AddMemory` - the first production Personal Knowledge mutation (M2
/// Personal Knowledge Slice B, 2026-09-18). Confirmation floor is
/// `ExplicitAdd`; for `Provenance::Inferred` the generic `effective_class`
/// formula alone already forces a pre-write block (`InferredChange`
/// dominates `ExplicitAdd`), so no Personal-Knowledge-specific override
/// is needed here (unlike `CorrectMemory`'s floor) - see
/// `services::tests::req_am_inferred_unconfirmed_blocks_with_zero_writes`.
///
/// Execution order (corrected post-R021 verification hardening,
/// 2026-09-18 - see R021's "Post-run correctness and verification
/// hardening" addendum): (1) pure structural validation
/// (`actions::validate_add_memory` - lengths, JSON syntax, confidence
/// bounds, same-payload duplicate-domain-ref) - no SQL, no mutation, no
/// external reads; (2) the confirmation gate, immediately after pure
/// validation succeeds - `effective_class`/`blocks_until_confirmed`
/// against `meta.provenance`/`meta.confirmed`; (3) only once
/// confirmation is satisfied does subject/object/source database-
/// dependent resolution run, aggregating every reference error found
/// rather than stopping at the first (matches `add_task`'s
/// `project_id`/`goal_id` aggregation precedent) - Existing-not-found,
/// domain-ref authoritative existence, and persisted-duplicate-domain-ref
/// are all checked here; (4) the transaction opens only after every
/// check above has succeeded. **Deliberately not the other way around**:
/// an inferred/unconfirmed model proposal must not read Personal
/// Knowledge or authoritative domain objects merely to prepare a
/// mutation the user has not yet approved - the gate blocks before any
/// database-dependent validation is even attempted, proven directly by
/// `req_am_conf_gate_precedes_db_dependent_validation`. Nothing is
/// written to any table before all validation succeeds, in either
/// case.
///
/// Atomic transaction, in order: new subject Entity (if any) -> new
/// object Entity (if any) -> new Source (if `SourceRef::New`) -> Fact ->
/// Evidence -> `fact_evidence` link -> `fact_audit_log`. Any failure at
/// any step means `tx` drops without a commit and rusqlite rolls back
/// everything, including any newly-created Entities/Source - pre-
/// existing Entity/Source rows are never touched regardless (`to_insert`
/// is `None` for both `SelfEntity` and `Existing`).
///
/// `fact_audit_log.previous_snapshot` is always `None` - this creates a
/// Fact, it never updates one, so `serialize_required_snapshot` (the
/// Update* actions' mandatory-snapshot helper) is never called here.
/// `epistemic_state` persists `USER_CONFIRMED` when this is an
/// `Inferred`, now-confirmed resubmission (regardless of the payload's
/// originally-proposed `epistemic_state`), or exactly the caller's
/// requested `epistemic_state` for a direct `Explicit` call - the only
/// path that can persist a deliberate `HYPOTHESIS`+`INFERRED` memory
/// (R018 §5). `provenance` itself is stored in the audit row exactly as
/// given - confirming an inferred action never silently rewrites it to
/// `Explicit` (confirmation and provenance are distinct concepts).
///
/// Never performs implicit deduplication, contradiction detection,
/// correction, auto-`STALE` transition, similarity merge, or entity-
/// alias merge - two explicit `AddMemory` calls may create two Facts
/// that look alike (R021 §16). Never writes to `relations`/`episodes`/
/// `relation_evidence`/`episode_evidence`/`episode_entities` - those
/// remain schema-only.
pub(crate) fn add_memory(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: AddMemoryPayload,
) -> ActionOutcome<Fact> {
    let normalized = match crate::actions::validate_add_memory(&payload) {
        Ok(n) => n,
        Err(errors) => return ActionOutcome::ValidationFailed(errors),
    };

    // Confirmation gate runs immediately after pure structural
    // validation and BEFORE any database-dependent resolution - an
    // inferred/unconfirmed proposal must not read Personal Knowledge or
    // authoritative domain objects to prepare a mutation the user has
    // not yet approved (R021 post-run correctness hardening).
    let class = effective_class(ConfirmationClass::ExplicitAdd, meta.provenance);
    if blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let now = meta.requested_at;
    let mut errors: Vec<ValidationError> = Vec::new();

    let subject = match resolve_entity_input_ref(conn, &normalized.subject, now, &mut errors) {
        Ok(r) => r,
        Err(e) => return ActionOutcome::StorageError(e),
    };

    let object = match &normalized.value {
        FactValueInput::Entity(entity_ref) => {
            match resolve_entity_input_ref(conn, entity_ref, now, &mut errors) {
                Ok(r) => r,
                Err(e) => return ActionOutcome::StorageError(e),
            }
        }
        FactValueInput::ScalarJson(_) => None,
    };

    let source = match resolve_source_ref(conn, &normalized.source, now, &mut errors) {
        Ok(r) => r,
        Err(e) => return ActionOutcome::StorageError(e),
    };

    if !errors.is_empty() {
        return ActionOutcome::ValidationFailed(errors);
    }

    let subject = subject.expect("no reference errors implies subject resolved");
    let source = source.expect("no reference errors implies source resolved");

    let persisted_epistemic_state = if meta.provenance == Provenance::Inferred && meta.confirmed {
        EpistemicState::UserConfirmed
    } else {
        normalized.epistemic_state
    };

    let fact_value = match &normalized.value {
        FactValueInput::ScalarJson(s) => FactValue::Scalar(s.clone()),
        FactValueInput::Entity(_) => FactValue::EntityRef(
            object
                .as_ref()
                .expect("FactValueInput::Entity implies object resolved")
                .id,
        ),
    };

    let fact = Fact {
        id: FactId::new(),
        subject_entity_id: subject.id,
        predicate: normalized.predicate.clone(),
        value: fact_value,
        memory_type: normalized.memory_type,
        epistemic_state: persisted_epistemic_state,
        confidence: normalized.confidence,
        sensitivity: normalized.sensitivity,
        valid_from: now,
        valid_until: None,
        superseded_by_fact_id: None,
        created_at: now,
        updated_at: now,
    };

    let evidence = Evidence {
        id: EvidenceId::new(),
        source_id: source.id,
        content: normalized.evidence.content.clone(),
        locator: normalized.evidence.locator.clone(),
        sensitivity: normalized.evidence.sensitivity,
        created_at: now,
    };

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Some(new_subject) = &subject.to_insert {
        if let Err(e) = storage::personal_knowledge_repo::insert_entity(&tx, new_subject) {
            return ActionOutcome::StorageError(CoreError::from(e));
        }
    }
    if let Some(resolved_object) = &object {
        if let Some(new_object) = &resolved_object.to_insert {
            if let Err(e) = storage::personal_knowledge_repo::insert_entity(&tx, new_object) {
                return ActionOutcome::StorageError(CoreError::from(e));
            }
        }
    }
    if let Some(new_source) = &source.to_insert {
        if let Err(e) = storage::personal_knowledge_repo::insert_source(&tx, new_source) {
            return ActionOutcome::StorageError(CoreError::from(e));
        }
    }
    if let Err(e) = storage::personal_knowledge_repo::insert_fact(&tx, &fact) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }
    if let Err(e) = storage::personal_knowledge_repo::insert_evidence(&tx, &evidence) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }
    if let Err(e) =
        storage::personal_knowledge_repo::insert_fact_evidence_link(&tx, fact.id, evidence.id, now)
    {
        return ActionOutcome::StorageError(CoreError::from(e));
    }
    if let Err(e) = storage::personal_knowledge_repo::insert_fact_audit_log(
        &tx,
        fact.id,
        "AddMemory",
        meta.actor,
        meta.provenance,
        class,
        None,
        now,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: fact,
        confirmation_class: class,
    }
}

/// The Personal-Knowledge-specific confirmation overlay (R018 §6): a
/// Memory action's `ImportantChange` floor (severity 3, `CorrectMemory`)
/// already dominates `InferredChange` (severity 2) in the generic
/// `effective_class` ladder, so `blocks_until_confirmed(class)` alone
/// does NOT force a block for an inferred, unconfirmed proposal the way
/// it does for `AddMemory`'s lower `ExplicitAdd` floor (where
/// `InferredChange` dominates instead). This narrow helper restores the
/// required block WITHOUT changing `ConfirmationClass`, `effective_
/// class`, or `blocks_until_confirmed` themselves - `class` (returned to
/// the caller as `confirmation_class` either way) stays `ImportantChange`
/// regardless; only the blocking DECISION differs from the generic
/// ladder's own verdict. Confirmed calls (`meta.confirmed`) never block,
/// regardless of provenance.
fn memory_action_requires_confirmation(meta: &ActionMeta, class: ConfirmationClass) -> bool {
    if meta.confirmed {
        return false;
    }
    blocks_until_confirmed(class) || meta.provenance == Provenance::Inferred
}

/// `CorrectMemory` - the second production Personal Knowledge mutation
/// (M2 Personal Knowledge Slice C, 2026-09-18). "The retained semantic
/// knowledge is wrong/outdated; replace it while preserving history" -
/// never an in-place semantic edit. Creates a brand-new successor `Fact`
/// inheriting `subject_entity_id`/`predicate` EXACTLY from the
/// predecessor, and closes the predecessor (`valid_until`/
/// `superseded_by_fact_id`/`epistemic_state = Stale`/`updated_at`) in
/// the same atomic transaction.
///
/// Execution order (matches the corrected R021 `AddMemory` safety
/// boundary exactly): (1) pure structural validation
/// (`actions::validate_correct_memory` - no SQL); (2) `effective_class`
/// computed against the `ImportantChange` floor; (3)/(4) the
/// Personal-Knowledge inferred-confirmation overlay
/// (`memory_action_requires_confirmation`, above) - an inferred,
/// unconfirmed correction returns `ConfirmationRequired` here, before
/// the predecessor is ever fetched; (5) only once confirmation is
/// satisfied does the predecessor get read, its active/inactive state
/// checked (`domain::is_active_fact`), the value's object Entity (if
/// any) and Source resolved, and any domain-ref/duplicate-domain-ref
/// checks run; (6) a `NotFound` (missing predecessor) or
/// `ValidationFailed` (inactive predecessor, unresolved references)
/// result short-circuits before any write; (7) the mandatory
/// `previous_snapshot` of the predecessor's PRE-correction state is
/// serialized (`serialize_required_snapshot`, never `.ok()`-downgraded)
/// before (8) the atomic transaction opens.
///
/// Successor `epistemic_state` is unconditionally `UserConfirmed` -
/// there is no v0.1 path to `INFERRED`/`OBSERVED`/`DISPUTED`/`STALE`
/// through `CorrectMemory` (R018). `provenance` in the audit row is
/// never rewritten - confirming an inferred correction persists
/// `USER_CONFIRMED` on the successor while the audit row's own
/// `provenance` stays `Inferred` (confirmation and provenance are
/// distinct concepts, matching `AddMemory`'s same guarantee).
///
/// The audit row's `fact_id` is the PREDECESSOR's id, not the
/// successor's - the row records the mutation of the targeted
/// predecessor and carries that predecessor's `previous_snapshot`,
/// preserving the frozen future `ForgetMemory` rule that it can scrub
/// `fact_audit_log.previous_snapshot WHERE fact_id = <target>` before
/// deleting that exact Fact.
///
/// Transaction order: new object `Entity` (if `FactValueInput::
/// Entity(New(..))`) -> new `Source` (if `SourceRef::New`) -> successor
/// `Fact` -> new `Evidence` -> successor's `fact_evidence` link ->
/// predecessor update (`update_fact_as_superseded` - the only path that
/// can ever touch `valid_until`/`superseded_by_fact_id`/
/// `epistemic_state`/`updated_at` on an existing Fact) -> one
/// `fact_audit_log` row for the predecessor. Any failure at any step
/// rolls back everything, including the predecessor update.
///
/// Never writes to `relations`/`episodes`/`relation_evidence`/
/// `episode_evidence`/`episode_entities`. Never performs contradiction
/// detection, never searches for or mutates a sibling Fact sharing the
/// same subject+predicate, never touches an ancestor Fact further back
/// than the immediate predecessor.
pub(crate) fn correct_memory(
    conn: &mut Connection,
    meta: ActionMeta,
    payload: CorrectMemoryPayload,
) -> ActionOutcome<Fact> {
    let normalized = match crate::actions::validate_correct_memory(&payload) {
        Ok(n) => n,
        Err(errors) => return ActionOutcome::ValidationFailed(errors),
    };

    let class = effective_class(ConfirmationClass::ImportantChange, meta.provenance);
    if memory_action_requires_confirmation(&meta, class) {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let now = meta.requested_at;

    let predecessor =
        match storage::personal_knowledge_repo::get_fact(conn, normalized.predecessor_fact_id) {
            Ok(Some(f)) => f,
            Ok(None) => return ActionOutcome::NotFound,
            Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
        };

    let mut errors: Vec<ValidationError> = Vec::new();
    if !is_active_fact(&predecessor) {
        errors.push(ValidationError::FactNotActive);
    }

    let object = match &normalized.value {
        FactValueInput::Entity(entity_ref) => {
            match resolve_entity_input_ref(conn, entity_ref, now, &mut errors) {
                Ok(r) => r,
                Err(e) => return ActionOutcome::StorageError(e),
            }
        }
        FactValueInput::ScalarJson(_) => None,
    };

    let source = match resolve_source_ref(conn, &normalized.source, now, &mut errors) {
        Ok(r) => r,
        Err(e) => return ActionOutcome::StorageError(e),
    };

    if !errors.is_empty() {
        return ActionOutcome::ValidationFailed(errors);
    }

    let source = source.expect("no reference errors implies source resolved");

    let previous_snapshot = match serialize_required_snapshot(&predecessor) {
        Ok(s) => s,
        Err(e) => return ActionOutcome::StorageError(e),
    };

    let fact_value = match &normalized.value {
        FactValueInput::ScalarJson(s) => FactValue::Scalar(s.clone()),
        FactValueInput::Entity(_) => FactValue::EntityRef(
            object
                .as_ref()
                .expect("FactValueInput::Entity implies object resolved")
                .id,
        ),
    };

    let confidence = match normalized.confidence {
        FieldPatch::Unchanged => predecessor.confidence,
        FieldPatch::Set(c) => Some(c),
        FieldPatch::Clear => None,
    };

    let successor = Fact {
        id: FactId::new(),
        subject_entity_id: predecessor.subject_entity_id,
        predicate: predecessor.predicate.clone(),
        value: fact_value,
        memory_type: normalized.memory_type.unwrap_or(predecessor.memory_type),
        epistemic_state: EpistemicState::UserConfirmed,
        confidence,
        sensitivity: normalized.sensitivity.unwrap_or(predecessor.sensitivity),
        valid_from: now,
        valid_until: None,
        superseded_by_fact_id: None,
        created_at: now,
        updated_at: now,
    };

    let evidence = Evidence {
        id: EvidenceId::new(),
        source_id: source.id,
        content: normalized.evidence.content.clone(),
        locator: normalized.evidence.locator.clone(),
        sensitivity: normalized.evidence.sensitivity,
        created_at: now,
    };

    let tx = match conn.transaction() {
        Ok(tx) => tx,
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    };

    if let Some(resolved_object) = &object {
        if let Some(new_object) = &resolved_object.to_insert {
            if let Err(e) = storage::personal_knowledge_repo::insert_entity(&tx, new_object) {
                return ActionOutcome::StorageError(CoreError::from(e));
            }
        }
    }
    if let Some(new_source) = &source.to_insert {
        if let Err(e) = storage::personal_knowledge_repo::insert_source(&tx, new_source) {
            return ActionOutcome::StorageError(CoreError::from(e));
        }
    }
    if let Err(e) = storage::personal_knowledge_repo::insert_fact(&tx, &successor) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }
    if let Err(e) = storage::personal_knowledge_repo::insert_evidence(&tx, &evidence) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }
    if let Err(e) = storage::personal_knowledge_repo::insert_fact_evidence_link(
        &tx,
        successor.id,
        evidence.id,
        now,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }
    if let Err(e) = storage::personal_knowledge_repo::update_fact_as_superseded(
        &tx,
        predecessor.id,
        successor.id,
        now,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }
    if let Err(e) = storage::personal_knowledge_repo::insert_fact_audit_log(
        &tx,
        predecessor.id,
        "CorrectMemory",
        meta.actor,
        meta.provenance,
        class,
        Some(previous_snapshot),
        now,
    ) {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    if let Err(e) = tx.commit() {
        return ActionOutcome::StorageError(CoreError::from(e));
    }

    ActionOutcome::Success {
        value: successor,
        confirmation_class: class,
    }
}

#[cfg(test)]
mod tests;
