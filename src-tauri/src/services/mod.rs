//! LifeOS Core service layer (M2 Goal vertical slice, 2026-09-17).
//!
//! `storage` is a private submodule (no `pub` at all) - this is the
//! corrected M2 module-boundary enforcement (`docs/LIFEOS_SPEC.md`
//! §5.6): nothing outside this file can reach `services::storage`,
//! proven by deliberately attempting it and capturing the compiler's
//! rejection (`docs/reports/REPORT_M2_STORAGE_SPIKE_2026-09-17.md`).
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
    effective_class, goal_blocks_until_confirmed, ActionMeta, ActionOutcome, ConfirmationClass,
    CreateGoalPayload, FieldPatch, UpdateGoalPayload,
};
use crate::core_error::CoreError;
use crate::domain::{Goal, GoalId, GoalStatus};
use crate::security;
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
    Ok(conn)
}

/// Full production bootstrap: resolves the real DB path, obtains/
/// generates the real master key via the OS credential store
/// (`LifeOS` / `database-master-key-v1`), opens the encrypted database,
/// and applies migrations. This is what a future Tauri command / app
/// setup hook calls - nothing calls it yet this run.
#[allow(dead_code)]
pub(crate) fn bootstrap_production_db() -> Result<Connection, BootstrapError> {
    let path = production_db_path()?;
    let key_hex = security::get_or_create_production_master_key().map_err(BootstrapError::Key)?;
    bootstrap_db_at(&path, &key_hex)
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
    if goal_blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let now = meta.requested_at;
    let goal = Goal {
        id: GoalId::new(),
        title: normalized_title,
        description: payload.description,
        status: GoalStatus::Active,
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
    if goal_blocks_until_confirmed(class) && !meta.confirmed {
        return ActionOutcome::ConfirmationRequired {
            confirmation_class: class,
        };
    }

    let previous_snapshot = serde_json::to_string(&existing).ok();

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
        previous_snapshot.as_deref(),
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

#[cfg(test)]
mod tests;
