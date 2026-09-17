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
    CreateGoalPayload, UpdateGoalPayload,
};
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

pub(crate) fn create_goal(
    conn: &Connection,
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

    if let Err(e) = storage::goal_repo::insert_goal(conn, &goal) {
        return ActionOutcome::StorageError(e.to_string());
    }

    let _ = storage::goal_repo::insert_audit_entry(
        conn,
        goal.id,
        "CreateGoal",
        meta.actor,
        meta.provenance,
        class,
        None,
        now,
    );

    ActionOutcome::Success {
        value: goal,
        confirmation_class: class,
    }
}

pub(crate) fn update_goal(
    conn: &Connection,
    meta: ActionMeta,
    payload: UpdateGoalPayload,
) -> ActionOutcome<Goal> {
    let existing = match storage::goal_repo::get_goal(conn, payload.id) {
        Ok(Some(g)) => g,
        Ok(None) => return ActionOutcome::NotFound,
        Err(e) => return ActionOutcome::StorageError(e.to_string()),
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
    if let Some(description) = payload.description {
        updated.description = Some(description);
    }
    if let Some(status) = payload.status {
        updated.status = status;
    }
    if let Some(target_date) = payload.target_date {
        updated.target_date = Some(target_date);
    }
    if let Some(priority_user) = payload.priority_user {
        updated.priority_user = Some(priority_user);
    }
    if let Some(progress) = payload.progress {
        updated.progress = progress;
    }
    updated.updated_at = meta.requested_at;

    if let Err(e) = storage::goal_repo::update_goal(conn, &updated) {
        return ActionOutcome::StorageError(e.to_string());
    }

    let _ = storage::goal_repo::insert_audit_entry(
        conn,
        updated.id,
        "UpdateGoal",
        meta.actor,
        meta.provenance,
        class,
        previous_snapshot.as_deref(),
        meta.requested_at,
    );

    ActionOutcome::Success {
        value: updated,
        confirmation_class: class,
    }
}

/// `SAFE_READ` - the frozen v0.1 capability API's `get_active_goals()`
/// (`LIFEOS_SPEC.md` §10.1). This function (and `get_goal` below) *is*
/// the capability boundary for this slice - a dedicated `capability`
/// module is not added until a real Tauri command/NeXa transport exists
/// to sit behind it (would be a premature, empty pass-through today).
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
