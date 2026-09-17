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
    blocks_until_confirmed, effective_class, ActionMeta, ActionOutcome, ConfirmationClass,
    CreateGoalPayload, CreateProjectPayload, FieldPatch, UpdateGoalPayload, UpdateProgressPayload,
    ValidationError,
};
use crate::core_error::CoreError;
use crate::domain::{Goal, GoalId, LifecycleStatus, Project, ProjectId};
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

// --- Project (M2 Project vertical slice, 2026-09-17) --------------------
//
// Same atomic-mutation-plus-audit, typed-`CoreError`, module-boundary
// pattern as Goal above - see `docs/reports/REPORT_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`.
// Only `CreateProject` and `UpdateProgress` are implemented (the frozen
// action set also has `AddMilestone`/`AddTask`, out of scope - they need
// Milestone/Task, which do not exist yet).

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
        match storage::project_repo::goal_exists(conn, goal_id) {
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

    let previous_snapshot = serde_json::to_string(&existing).ok();

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

#[cfg(test)]
mod tests;
