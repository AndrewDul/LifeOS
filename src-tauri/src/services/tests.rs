//! M2 Goal vertical slice + hardening test suite (2026-09-17).
//!
//! Evidence for `docs/reports/R005_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`
//! and `docs/reports/R006_M2_GOAL_HARDENING_2026-09-17.md`. Run with:
//! `cargo test --manifest-path src-tauri/Cargo.toml services::tests -- --nocapture --test-threads=1`
//! (`--test-threads=1` matters for the keyring-touching test.)

use super::*;
use crate::actions::{
    ActionMeta, ActionOutcome, Actor, ConfirmationClass, FieldPatch, Provenance, ValidationError,
};
use crate::core_error::CoreErrorKind;
use crate::domain::{GoalRelationType, LifecycleStatus, MilestoneStatus, TaskStatus};
use std::fs;
use std::io::Read;

/// Independent of `storage::spike`'s key helper on purpose - this test
/// module should not depend on the spike's internals, which are a
/// separate, self-contained regression suite (and are only visible
/// within `services::storage`'s own subtree, not to `services::tests`,
/// a sibling - the module-privacy boundary working exactly as intended).
fn generate_test_key_hex() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn temp_db(dir: &tempfile::TempDir, name: &str) -> (std::path::PathBuf, String) {
    (dir.path().join(name), generate_test_key_hex())
}

fn ui_explicit() -> ActionMeta {
    ActionMeta::now(Actor::Ui, Provenance::Explicit)
}

fn mk_create_payload(title: &str) -> CreateGoalPayload {
    CreateGoalPayload {
        title: title.to_string(),
        description: None,
        target_date: None,
        priority_user: None,
        domain_tag: None,
    }
}

/// Installs a `BEFORE INSERT` trigger on `goal_audit_log` that always
/// fails via `RAISE(FAIL, ...)` - the reproducible forced-audit-failure
/// mechanism used to prove real SQLite transactional rollback (M2 Goal
/// hardening report §3), not just code inspection. `RAISE(FAIL, ...)`
/// (rather than `RAISE(ROLLBACK, ...)`) deliberately does NOT roll back
/// the transaction itself - it only aborts the failing statement - so a
/// passing test proves *our* `services::create_goal`/`update_goal` code
/// performs the rollback (via `Transaction`'s `Drop`), not SQLite doing
/// it on our behalf.
fn install_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER force_audit_failure
         BEFORE INSERT ON goal_audit_log
         BEGIN
             SELECT RAISE(FAIL, 'forced audit failure');
         END;",
    )
    .expect("failed to install the forced-audit-failure trigger");
}

fn remove_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch("DROP TRIGGER IF EXISTS force_audit_failure;")
        .expect("failed to remove the forced-audit-failure trigger");
}

fn goal_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM goals", [], |r| r.get(0))
        .unwrap()
}

fn audit_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM goal_audit_log", [], |r| r.get(0))
        .unwrap()
}

/// Requirement 1 & 2: fresh production database initializes encrypted
/// and contains the real Goal schema.
#[test]
fn req1_2_fresh_production_db_encrypted_with_goal_schema() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod1.db");

    let conn = bootstrap_db_at(&path, &key).expect("bootstrap should succeed");

    let has_goals_table: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='goals'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap();
    assert!(
        has_goals_table,
        "[req1/2] FAIL - production DB missing the goals table"
    );

    let has_audit_table: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='goal_audit_log'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap();
    assert!(
        has_audit_table,
        "[req1/2] FAIL - production DB missing goal_audit_log"
    );

    drop(conn);
    let mut raw = Vec::new();
    fs::File::open(&path)
        .unwrap()
        .read_to_end(&mut raw)
        .unwrap();
    assert!(
        !raw.windows(6).any(|w| w == b"SQLite"),
        "[req1] FAIL - file starts with the plaintext SQLite header, not encrypted"
    );
    println!("[req1/2] PASS - fresh production DB ({} bytes) is encrypted (no plaintext SQLite header) and has goals + goal_audit_log tables", raw.len());
}

/// Requirement 3: production DB never contains spike_records.
#[test]
fn req3_production_db_has_no_spike_records() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod3.db");

    let conn = bootstrap_db_at(&path, &key).unwrap();
    let has_spike_table: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='spike_records'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap();
    assert!(
        !has_spike_table,
        "[req3] FAIL - production database contains spike_records!"
    );
    println!("[req3] PASS - production database schema does not contain spike_records");
}

/// Requirement 4: master key survives reopen through the real `keyring`
/// integration - a TEST-specific service/account, never the real
/// production credential.
#[test]
fn req4_master_key_survives_reopen_via_keyring() {
    let account = "vertical-slice-test-key";
    let _ = security::delete_key_for_test(account);

    let key1 = match security::get_or_create_master_key_for_test(account) {
        Ok(k) => k,
        Err(e) => {
            println!("[req4] BLOCKED - could not write to OS credential store: {e}");
            return;
        }
    };

    // "Reopen": call the same get-or-create again - it must retrieve the
    // SAME key, not generate a new one.
    let key2 = match security::get_or_create_master_key_for_test(account) {
        Ok(k) => k,
        Err(e) => {
            println!("[req4] BLOCKED - could not read back from OS credential store: {e}");
            let _ = security::delete_key_for_test(account);
            return;
        }
    };

    assert_eq!(
        key1, key2,
        "[req4] FAIL - key changed across a simulated reopen"
    );
    security::delete_key_for_test(account).expect("cleanup: failed to delete test credential");
    println!(
        "[req4] PASS - master key survived a simulated reopen via the real OS credential store"
    );
}

/// Requirement 5 & 6: valid CreateGoal succeeds and persists across
/// close/reopen.
#[test]
fn req5_6_create_goal_succeeds_and_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod5.db");

    let goal_id;
    {
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let payload = CreateGoalPayload {
            title: "  Get first paying client  ".to_string(),
            description: Some("DeVDul freelance".to_string()),
            target_date: None,
            priority_user: Some(3),
            domain_tag: Some("career".to_string()),
        };
        match create_goal(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success {
                value,
                confirmation_class,
            } => {
                assert_eq!(
                    value.title, "Get first paying client",
                    "[req5] FAIL - title was not trimmed"
                );
                assert_eq!(value.status, LifecycleStatus::Active);
                assert_eq!(value.progress, 0);
                assert_eq!(confirmation_class, ConfirmationClass::ExplicitAdd);
                goal_id = value.id;
                println!(
                    "[req5] PASS - CreateGoal succeeded: {value:?}, class={confirmation_class:?}"
                );
            }
            other => panic!("[req5] FAIL - expected Success, got {other:?}"),
        }
    }

    // Reopen with a fresh Connection - proves persistence, not an
    // in-memory artifact of the first connection.
    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        let fetched = get_goal(&conn, goal_id).unwrap();
        assert!(
            fetched.is_some(),
            "[req6] FAIL - goal not found after reopen"
        );
        assert_eq!(fetched.unwrap().title, "Get first paying client");
        println!("[req6] PASS - goal persisted across a full connection close/reopen");
    }
}

/// Requirement 7: empty/whitespace title rejected before write.
#[test]
fn req7_empty_and_whitespace_title_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod7.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    for bad_title in ["", "   ", "\t\n"] {
        let payload = mk_create_payload(bad_title);
        match create_goal(&mut conn, ui_explicit(), payload) {
            ActionOutcome::ValidationFailed(errors) => {
                assert!(
                    errors.contains(&ValidationError::EmptyTitle),
                    "[req7] FAIL - expected EmptyTitle for {bad_title:?}, got {errors:?}"
                );
            }
            other => {
                panic!("[req7] FAIL - expected ValidationFailed for {bad_title:?}, got {other:?}")
            }
        }
    }

    assert_eq!(
        goal_row_count(&conn),
        0,
        "[req7] FAIL - a row was written despite validation failure"
    );
    println!(
        "[req7] PASS - empty/whitespace-only titles rejected before any write (0 rows in goals)"
    );
}

/// Requirement 8: overlong/invalid fields rejected deterministically.
#[test]
fn req8_overlong_fields_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod8.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let payload = CreateGoalPayload {
        priority_user: Some(99),
        ..mk_create_payload(&"x".repeat(201))
    };
    match create_goal(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ValidationError::TitleTooLong { .. })),
                "[req8] FAIL - expected TitleTooLong, got {errors:?}"
            );
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ValidationError::PriorityOutOfBounds { .. })),
                "[req8] FAIL - expected PriorityOutOfBounds, got {errors:?}"
            );
            println!("[req8] PASS - overlong title AND out-of-bounds priority both reported in one call: {errors:?}");
        }
        other => panic!("[req8] FAIL - expected ValidationFailed, got {other:?}"),
    }

    assert_eq!(
        goal_row_count(&conn),
        0,
        "[req8] FAIL - a row was written despite validation failure"
    );
}

/// Requirement 9: valid UpdateGoal succeeds.
#[test]
fn req9_valid_update_goal_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod9.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Ship M2")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let update = UpdateGoalPayload {
        id: created.id,
        progress: Some(40),
        status: Some(LifecycleStatus::Active),
        ..Default::default()
    };
    match update_goal(&mut conn, ui_explicit(), update) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(value.progress, 40);
            assert_eq!(confirmation_class, ConfirmationClass::ImportantChange);
            println!(
                "[req9] PASS - UpdateGoal succeeded: progress=40, class={confirmation_class:?}"
            );
        }
        other => panic!("[req9] FAIL - expected Success, got {other:?}"),
    }
}

/// Requirement 10: UpdateGoal on a nonexistent ID returns structured
/// NotFound.
#[test]
fn req10_update_nonexistent_goal_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod10.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_id = crate::domain::GoalId::new();
    let update = UpdateGoalPayload {
        id: fake_id,
        progress: Some(10),
        ..Default::default()
    };
    match update_goal(&mut conn, ui_explicit(), update) {
        ActionOutcome::NotFound => {
            println!("[req10] PASS - UpdateGoal on a nonexistent id returned structured NotFound")
        }
        other => panic!("[req10] FAIL - expected NotFound, got {other:?}"),
    }
}

/// Requirement 11: an invalid UpdateGoal makes ZERO DB changes.
///
/// Rewritten (M2 hardening, 2026-09-17): the original version of this
/// test proved an invalid COMPLETED -> ACTIVE *status* transition was
/// rejected - that whole rule has since been removed as an unapproved
/// invention (§5 of the hardening brief). The zero-changes guarantee
/// itself is still real and still required, so this now exercises it
/// via an out-of-bounds `priority_user`, a validation rule that is
/// still frozen policy.
#[test]
fn req11_invalid_update_makes_zero_changes() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod11.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Stable title")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_goal(&conn, created.id).unwrap().unwrap();

    let invalid = UpdateGoalPayload {
        id: created.id,
        priority_user: FieldPatch::Set(99),
        ..Default::default()
    };
    match update_goal(&mut conn, ui_explicit(), invalid) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::PriorityOutOfBounds { .. })));
            println!("[req11] PASS - out-of-bounds priority_user rejected: {errors:?}");
        }
        other => panic!("[req11] FAIL - expected ValidationFailed, got {other:?}"),
    }

    let after = get_goal(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req11] FAIL - goal changed despite validation failure"
    );
    println!(
        "[req11] PASS - zero changes persisted after the invalid update (goal identical to before)"
    );
}

/// Requirement 12: get_active_goals returns only ACTIVE goals.
#[test]
fn req12_get_active_goals_filters_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod12.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Active goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(
        &mut conn,
        ui_explicit(),
        mk_create_payload("Will be paused"),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match update_goal(
        &mut conn,
        ui_explicit(),
        UpdateGoalPayload {
            id: b.id,
            status: Some(LifecycleStatus::Paused),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let active = get_active_goals(&conn).unwrap();
    assert_eq!(
        active.len(),
        1,
        "[req12] FAIL - expected exactly 1 active goal, got {}",
        active.len()
    );
    assert_eq!(active[0].id, a.id);
    println!("[req12] PASS - get_active_goals returned exactly the 1 ACTIVE goal, excluding the paused one");
}

/// Requirement 13: SQLCipher wrong-key protection remains intact for
/// the production bootstrap path specifically (not just the spike's).
#[test]
fn req13_wrong_key_rejected_for_production_db() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod13.db");
    let wrong_key = generate_test_key_hex();
    assert_ne!(key, wrong_key);

    bootstrap_db_at(&path, &key).unwrap();

    match bootstrap_db_at(&path, &wrong_key) {
        Err(BootstrapError::Storage(e)) => {
            println!("[req13] PASS - wrong key rejected on the production bootstrap path: {e}");
        }
        other => panic!("[req13] FAIL - expected a Storage error for the wrong key, got {other:?}"),
    }
}

/// Requirement 15: static safety net - no source file prints a variable
/// that looks like key material. Not a runtime capture test (no
/// logging framework is wired up in this slice; the key only ever
/// exists as a local `String` passed directly between `security::` and
/// `storage::` functions, never stored in a struct with a Display/Debug
/// impl) - this is documented explicitly rather than overclaimed.
#[test]
fn req15_no_key_material_in_source_prints() {
    let suspicious = [
        "{key}",
        "{key_hex}",
        "{master_key}",
        "{new_key_hex}",
        "{old_key_hex}",
        "{retrieved_key}",
    ];
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    scan_dir(&src_dir, &mut offenders, &suspicious);
    assert!(
        offenders.is_empty(),
        "[req15] FAIL - possible key material in a print statement: {offenders:?}"
    );
    println!("[req15] PASS - no print!/println!/eprintln! call in src/ interpolates a key-shaped variable (static scan)");
}

fn scan_dir(dir: &std::path::Path, offenders: &mut Vec<String>, patterns: &[&str]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, offenders, patterns);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in contents.lines().enumerate() {
                let is_print_call = line.contains("println!")
                    || line.contains("print!")
                    || line.contains("eprintln!");
                if is_print_call {
                    for pat in patterns {
                        if line.contains(pat) {
                            offenders.push(format!(
                                "{}:{}: {}",
                                path.display(),
                                i + 1,
                                line.trim()
                            ));
                        }
                    }
                }
            }
        }
    }
}

// =========================================================================
// M2 Goal hardening (2026-09-17) - new evidence below.
// =========================================================================

/// Hardening §1/§2 (A): CreateGoal's goal-insert + audit-insert are one
/// atomic transaction. A forced audit-insert failure must leave ZERO
/// goal rows behind, and the action must report a typed `StorageError`,
/// never `Success`.
#[test]
fn req_h1_create_goal_audit_failure_rolls_back_goal_insert() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "hardening_h1.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    install_audit_failure_trigger(&conn);

    match create_goal(
        &mut conn,
        ui_explicit(),
        mk_create_payload("Should never persist"),
    ) {
        ActionOutcome::StorageError(e) => {
            assert_eq!(e.kind(), CoreErrorKind::Storage);
            println!(
                "[hardening h1] PASS - forced audit failure surfaced as a typed StorageError: {e:?}"
            );
        }
        other => panic!("[hardening h1] FAIL - expected StorageError, got {other:?}"),
    }

    assert_eq!(
        goal_row_count(&conn),
        0,
        "[hardening h1] FAIL - a goal row survived a rolled-back transaction"
    );
    assert_eq!(
        audit_row_count(&conn),
        0,
        "[hardening h1] FAIL - an audit row survived a rolled-back transaction"
    );

    remove_audit_failure_trigger(&conn);

    // The connection/DB must remain fully usable after the rollback -
    // proves the transaction was cleanly rolled back, not left in a
    // broken half-open state.
    match create_goal(&mut conn, ui_explicit(), mk_create_payload("Now it works")) {
        ActionOutcome::Success { .. } => {
            println!("[hardening h1] PASS - connection remains usable after the rollback")
        }
        other => panic!("[hardening h1] FAIL - connection unusable after rollback: {other:?}"),
    }
}

/// Hardening §1/§2 (B): UpdateGoal's goal-update + audit-insert are one
/// atomic transaction. A forced audit-insert failure must leave the
/// existing goal byte/logically identical to its pre-update state.
#[test]
fn req_h2_update_goal_audit_failure_rolls_back_goal_update() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "hardening_h2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Stable")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_goal(&conn, created.id).unwrap().unwrap();

    install_audit_failure_trigger(&conn);

    let update = UpdateGoalPayload {
        id: created.id,
        progress: Some(77),
        ..Default::default()
    };
    match update_goal(&mut conn, ui_explicit(), update) {
        ActionOutcome::StorageError(e) => {
            assert_eq!(e.kind(), CoreErrorKind::Storage);
            println!(
                "[hardening h2] PASS - forced audit failure surfaced as a typed StorageError: {e:?}"
            );
        }
        other => panic!("[hardening h2] FAIL - expected StorageError, got {other:?}"),
    }

    remove_audit_failure_trigger(&conn);

    let after = get_goal(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[hardening h2] FAIL - goal mutated despite a rolled-back transaction"
    );
    // Only the one audit row from the initial CreateGoal should exist -
    // the failed UpdateGoal's attempted row must not have survived.
    let audit_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM goal_audit_log WHERE goal_id = ?1",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        audit_count, 1,
        "[hardening h2] FAIL - expected exactly 1 audit row (the original CreateGoal), got {audit_count}"
    );
    println!("[hardening h2] PASS - goal left byte/logically identical to its pre-update state, no orphan audit row");
}

/// Hardening §5: COMPLETED is no longer terminal - a real user may
/// reopen a completed goal, and the transition remains audited.
#[test]
fn req_h3_completed_goal_can_be_reopened_and_transition_is_audited() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "hardening_h3.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Finish thesis")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let completed = match update_goal(
        &mut conn,
        ui_explicit(),
        UpdateGoalPayload {
            id: created.id,
            status: Some(LifecycleStatus::Completed),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(completed.status, LifecycleStatus::Completed);

    let reopened = match update_goal(
        &mut conn,
        ui_explicit(),
        UpdateGoalPayload {
            id: created.id,
            status: Some(LifecycleStatus::Active),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!(
            "[hardening h3] FAIL - reopening a COMPLETED goal should now be allowed, got {other:?}"
        ),
    };
    assert_eq!(reopened.status, LifecycleStatus::Active);

    let audit_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM goal_audit_log WHERE goal_id = ?1",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        audit_count, 3,
        "[hardening h3] FAIL - expected 3 audit rows (create, complete, reopen), got {audit_count}"
    );
    println!(
        "[hardening h3] PASS - COMPLETED -> ACTIVE transition succeeded and is audited ({audit_count} rows for this goal)"
    );
}

/// Hardening §4: `FieldPatch::Set`/`Clear`/`Unchanged` each behave
/// exactly as specified against real persisted rows.
#[test]
fn req_h4_update_goal_field_patch_set_clear_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "hardening_h4.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_goal(
        &mut conn,
        ui_explicit(),
        CreateGoalPayload {
            title: "Patch semantics".to_string(),
            description: Some("original description".to_string()),
            target_date: Some(chrono::Utc::now()),
            priority_user: Some(2),
            domain_tag: Some("career".to_string()),
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert!(created.description.is_some());
    assert!(created.priority_user.is_some());
    assert!(created.domain_tag.is_some());
    assert!(created.target_date.is_some());

    let update = UpdateGoalPayload {
        id: created.id,
        description: FieldPatch::Set("new description".to_string()),
        priority_user: FieldPatch::Clear,
        domain_tag: FieldPatch::Unchanged,
        target_date: FieldPatch::Unchanged,
        ..Default::default()
    };
    let updated = match update_goal(&mut conn, ui_explicit(), update) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[hardening h4] FAIL - expected Success, got {other:?}"),
    };

    assert_eq!(
        updated.description.as_deref(),
        Some("new description"),
        "[hardening h4] FAIL - Set did not write the new value"
    );
    assert_eq!(
        updated.priority_user, None,
        "[hardening h4] FAIL - Clear did not null the field"
    );
    assert_eq!(
        updated.domain_tag.as_deref(),
        Some("career"),
        "[hardening h4] FAIL - Unchanged modified a field it shouldn't have touched"
    );
    assert_eq!(
        updated.target_date, created.target_date,
        "[hardening h4] FAIL - Unchanged target_date was modified"
    );

    // Re-fetch with a fresh read to prove this is really persisted, not
    // just the in-memory returned value.
    let refetched = get_goal(&conn, created.id).unwrap().unwrap();
    assert_eq!(refetched, updated);

    println!("[hardening h4] PASS - FieldPatch::Set/Clear/Unchanged each behaved exactly as specified and persisted correctly");
}

/// Hardening §6/§8: CreateGoal with `Provenance::Inferred` must return
/// `ConfirmationRequired` and write NOTHING, proven against a real DB -
/// not deferred to M5 just because CloudLLM does not exist yet.
#[test]
fn req_h5_inferred_create_goal_requires_confirmation_and_does_not_write() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "hardening_h5.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let payload = mk_create_payload("AI-proposed goal");
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    assert!(!meta.confirmed);

    match create_goal(&mut conn, meta, payload.clone()) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!("[hardening h5] FAIL - expected ConfirmationRequired, got {other:?}"),
    }
    assert_eq!(
        goal_row_count(&conn),
        0,
        "[hardening h5] FAIL - a goal was written before confirmation"
    );
    println!(
        "[hardening h5] PASS - Inferred CreateGoal blocked with ConfirmationRequired, zero rows written"
    );

    // Resubmission with confirmed:true must then succeed and persist.
    let mut confirmed_meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    confirmed_meta.confirmed = true;
    match create_goal(&mut conn, confirmed_meta, payload) {
        ActionOutcome::Success {
            confirmation_class, ..
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
            assert_eq!(goal_row_count(&conn), 1);
            println!("[hardening h5] PASS - resubmission with confirmed=true persisted the goal");
        }
        other => panic!(
            "[hardening h5] FAIL - expected Success on confirmed resubmission, got {other:?}"
        ),
    }
}

// =========================================================================
// M2 Project vertical slice (2026-09-17) - new evidence below.
// Evidence for `docs/reports/R007_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`.
// =========================================================================

fn mk_create_project_payload(
    title: &str,
    goal_id: Option<crate::domain::GoalId>,
) -> CreateProjectPayload {
    CreateProjectPayload {
        title: title.to_string(),
        description: None,
        priority: None,
        goal_id,
    }
}

fn project_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
        .unwrap()
}

fn project_audit_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM project_audit_log", [], |r| r.get(0))
        .unwrap()
}

/// Same forced-failure mechanism as `install_audit_failure_trigger`
/// (hardening report §3), targeting `project_audit_log` instead.
fn install_project_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER force_project_audit_failure
         BEFORE INSERT ON project_audit_log
         BEGIN
             SELECT RAISE(FAIL, 'forced project audit failure');
         END;",
    )
    .expect("failed to install the forced project-audit-failure trigger");
}

fn remove_project_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch("DROP TRIGGER IF EXISTS force_project_audit_failure;")
        .expect("failed to remove the forced project-audit-failure trigger");
}

/// Requirement 1: V2 migration creates the Project schema.
#[test]
fn req_p1_v2_migration_creates_project_schema() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    for table in ["projects", "project_audit_log"] {
        let exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(exists, "[req_p1] FAIL - missing table {table}");
    }
    println!("[req_p1] PASS - V2 migration created projects + project_audit_log");
}

/// Requirements 2 & 14 (migration compatibility): an existing database
/// created at V1 only (real Goal data, no Project schema yet - the real
/// shape of an existing user's database before this app update) upgrades
/// cleanly to V2: original Goal data is unchanged, and a Project can
/// then reference that pre-existing Goal. Not a destructive migration;
/// `V1__create_goals.sql` itself was not touched by this run.
#[test]
fn req_p2_v1_goal_database_upgrades_cleanly_to_v2_project() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project2.db");

    let goal_id;
    {
        // Simulate an existing production DB that only ever ran V1.
        let mut conn = bootstrap_db_at_version(&path, &key, 1).unwrap();
        let has_projects_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='projects'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(
            !has_projects_table,
            "[req_p2] FAIL - a V1-only database already has the projects table"
        );

        let created = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Pre-V2 goal"))
        {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        goal_id = created.id;
    }

    {
        // Reopen normally - this applies every pending migration,
        // i.e. just V2 on top of the already-applied V1.
        let mut conn = bootstrap_db_at(&path, &key).unwrap();

        let goal_after_upgrade = get_goal(&conn, goal_id).unwrap();
        assert_eq!(
            goal_after_upgrade.as_ref().map(|g| g.title.as_str()),
            Some("Pre-V2 goal"),
            "[req_p2] FAIL - Goal data did not survive the V1 -> V2 upgrade"
        );

        let has_projects_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='projects'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(
            has_projects_table,
            "[req_p2] FAIL - projects table missing after upgrade"
        );

        match create_project(
            &mut conn,
            ui_explicit(),
            mk_create_project_payload("Project on the pre-existing goal", Some(goal_id)),
        ) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.goal_id, Some(goal_id));
                println!("[req_p2] PASS - V1 Goal data survived the upgrade to V2, and a new Project can reference the pre-existing Goal");
            }
            other => panic!(
                "[req_p2] FAIL - expected CreateProject to succeed against the pre-existing goal, got {other:?}"
            ),
        }
    }
}

/// Requirement 3 (and Project->Goal case A): a Project without a
/// `goal_id` can be created.
#[test]
fn req_p3_project_without_goal_id_can_be_created() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project3.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Standalone project", None),
    ) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.goal_id, None);
            assert_eq!(value.status, LifecycleStatus::Active);
            assert_eq!(value.progress, 0);
            println!("[req_p3] PASS - Project created with no goal_id: {value:?}");
        }
        other => panic!("[req_p3] FAIL - expected Success, got {other:?}"),
    }
}

/// Requirements 4, 5, 7 (and Project->Goal cases B and D): a Project
/// linked to an existing Goal can be created, the link + the project
/// itself survive a full connection close/reopen, and reading it back
/// returns the same linked GoalId.
#[test]
fn req_p4_5_7_project_with_valid_goal_id_persists_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project4.db");

    let (goal_id, project_id);
    {
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Linked goal")) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        goal_id = goal.id;

        match create_project(
            &mut conn,
            ui_explicit(),
            mk_create_project_payload("Linked project", Some(goal_id)),
        ) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.goal_id, Some(goal_id));
                project_id = value.id;
                println!("[req_p4] PASS - CreateProject succeeded linked to goal {goal_id}");
            }
            other => panic!("[req_p4] FAIL - expected Success, got {other:?}"),
        }
    }

    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        let fetched = get_project(&conn, project_id).unwrap();
        assert!(
            fetched.is_some(),
            "[req_p5/7] FAIL - project not found after reopen"
        );
        let fetched = fetched.unwrap();
        assert_eq!(
            fetched.goal_id,
            Some(goal_id),
            "[req_p5] FAIL - goal_id link did not survive close/reopen"
        );
        println!("[req_p5/7] PASS - project and its goal_id link both survived a full connection close/reopen: {fetched:?}");
    }
}

/// Requirement 6 (and Project->Goal case C): a nonexistent GoalId is
/// rejected before any write - zero project rows written.
#[test]
fn req_p6_nonexistent_goal_id_rejected_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project6.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_goal_id = crate::domain::GoalId::new();
    match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Dangling reference", Some(fake_goal_id)),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(
                errors.contains(&ValidationError::ReferencedGoalNotFound),
                "[req_p6] FAIL - expected ReferencedGoalNotFound, got {errors:?}"
            );
        }
        other => panic!("[req_p6] FAIL - expected ValidationFailed, got {other:?}"),
    }

    assert_eq!(
        project_row_count(&conn),
        0,
        "[req_p6] FAIL - a project row was written despite the dangling goal_id"
    );
    println!("[req_p6] PASS - nonexistent goal_id rejected before persistence, zero project rows written");
}

/// Requirement 8: invalid Project input makes zero writes.
#[test]
fn req_p8_invalid_project_input_makes_zero_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project8.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let payload = CreateProjectPayload {
        priority: Some(99),
        ..mk_create_project_payload(&"x".repeat(201), None)
    };
    match create_project(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::TitleTooLong { .. })));
            assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::PriorityOutOfBounds { .. })));
            println!("[req_p8] PASS - overlong title AND out-of-bounds priority both reported: {errors:?}");
        }
        other => panic!("[req_p8] FAIL - expected ValidationFailed, got {other:?}"),
    }

    assert_eq!(
        project_row_count(&conn),
        0,
        "[req_p8] FAIL - a row was written despite validation failure"
    );
}

/// Requirement 9: UpdateProgress succeeds.
#[test]
fn req_p9_update_progress_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project9.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Ship the van", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let update = UpdateProgressPayload {
        id: created.id,
        progress: 65,
    };
    match update_progress(&mut conn, ui_explicit(), update) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(value.progress, 65);
            assert_eq!(confirmation_class, ConfirmationClass::ExplicitAdd);
            println!(
                "[req_p9] PASS - UpdateProgress succeeded: progress=65, class={confirmation_class:?}"
            );
        }
        other => panic!("[req_p9] FAIL - expected Success, got {other:?}"),
    }
}

/// Requirement 10: out-of-bounds progress is rejected. `Progress` is
/// `u8` (0..=255 representable), so a negative value cannot even be
/// constructed through this API - the "<0 is rejected" half of this
/// requirement is a *stronger*, type-level guarantee than a runtime
/// check, not merely untested; only the ">100" half is a runtime
/// validation check.
#[test]
fn req_p10_progress_out_of_bounds_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project10.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Bounded progress", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let update = UpdateProgressPayload {
        id: created.id,
        progress: 101,
    };
    match update_progress(&mut conn, ui_explicit(), update) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::ProgressOutOfBounds { .. })));
            println!("[req_p10] PASS - progress=101 rejected: {errors:?}");
        }
        other => panic!("[req_p10] FAIL - expected ValidationFailed, got {other:?}"),
    }
}

/// Requirement 11: UpdateProgress on a missing Project returns
/// structured NotFound.
#[test]
fn req_p11_update_progress_missing_project_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project11.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_id = crate::domain::ProjectId::new();
    let update = UpdateProgressPayload {
        id: fake_id,
        progress: 10,
    };
    match update_progress(&mut conn, ui_explicit(), update) {
        ActionOutcome::NotFound => {
            println!(
                "[req_p11] PASS - UpdateProgress on a nonexistent id returned structured NotFound"
            )
        }
        other => panic!("[req_p11] FAIL - expected NotFound, got {other:?}"),
    }
}

/// Requirement 12: an invalid UpdateProgress causes zero mutation.
#[test]
fn req_p12_invalid_update_progress_causes_zero_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project12.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Stable project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_project(&conn, created.id).unwrap().unwrap();

    let invalid = UpdateProgressPayload {
        id: created.id,
        progress: 255,
    };
    match update_progress(&mut conn, ui_explicit(), invalid) {
        ActionOutcome::ValidationFailed(_) => {}
        other => panic!("[req_p12] FAIL - expected ValidationFailed, got {other:?}"),
    }

    let after = get_project(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_p12] FAIL - project changed despite validation failure"
    );
    println!("[req_p12] PASS - zero changes persisted after the invalid UpdateProgress");
}

/// Requirement 13: CreateProject's project-insert + audit-insert are
/// one atomic transaction - same forced-failure proof as Goal hardening
/// report §3.
#[test]
fn req_p13_create_project_audit_failure_rolls_back_project_insert() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project13.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    install_project_audit_failure_trigger(&conn);

    match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Should never persist", None),
    ) {
        ActionOutcome::StorageError(e) => {
            assert_eq!(e.kind(), CoreErrorKind::Storage);
            println!(
                "[req_p13] PASS - forced audit failure surfaced as a typed StorageError: {e:?}"
            );
        }
        other => panic!("[req_p13] FAIL - expected StorageError, got {other:?}"),
    }

    assert_eq!(
        project_row_count(&conn),
        0,
        "[req_p13] FAIL - a project row survived a rolled-back transaction"
    );
    assert_eq!(
        project_audit_row_count(&conn),
        0,
        "[req_p13] FAIL - an audit row survived a rolled-back transaction"
    );

    remove_project_audit_failure_trigger(&conn);

    match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Now it works", None),
    ) {
        ActionOutcome::Success { .. } => {
            println!("[req_p13] PASS - connection remains usable after the rollback")
        }
        other => panic!("[req_p13] FAIL - connection unusable after rollback: {other:?}"),
    }
}

/// Requirement 14: UpdateProgress's project-update + audit-insert are
/// one atomic transaction.
#[test]
fn req_p14_update_progress_audit_failure_rolls_back_project_update() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project14.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Stable", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_project(&conn, created.id).unwrap().unwrap();

    install_project_audit_failure_trigger(&conn);

    let update = UpdateProgressPayload {
        id: created.id,
        progress: 77,
    };
    match update_progress(&mut conn, ui_explicit(), update) {
        ActionOutcome::StorageError(e) => {
            assert_eq!(e.kind(), CoreErrorKind::Storage);
            println!(
                "[req_p14] PASS - forced audit failure surfaced as a typed StorageError: {e:?}"
            );
        }
        other => panic!("[req_p14] FAIL - expected StorageError, got {other:?}"),
    }

    remove_project_audit_failure_trigger(&conn);

    let after = get_project(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_p14] FAIL - project mutated despite a rolled-back transaction"
    );
    let audit_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM project_audit_log WHERE project_id = ?1",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        audit_count, 1,
        "[req_p14] FAIL - expected exactly 1 audit row (the original CreateProject), got {audit_count}"
    );
    println!(
        "[req_p14] PASS - project left identical to its pre-update state, no orphan audit row"
    );
}

/// Requirement 15: Project audit rows contain the required actor/
/// provenance/confirmation metadata.
#[test]
fn req_p15_project_audit_rows_contain_required_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project15.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Audited project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let (action_type, actor, provenance, confirmation_class): (String, String, String, String) = conn
        .query_row(
            "SELECT action_type, actor, provenance, confirmation_class FROM project_audit_log WHERE project_id = ?1",
            [created.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();

    assert_eq!(action_type, "CreateProject");
    assert_eq!(actor, "UI");
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "EXPLICIT_ADD");
    println!(
        "[req_p15] PASS - audit row carries action_type={action_type}, actor={actor}, provenance={provenance}, confirmation_class={confirmation_class}"
    );
}

/// Requirement 16: Inferred CreateProject blocks before write.
#[test]
fn req_p16_inferred_create_project_requires_confirmation_and_does_not_write() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "project16.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let payload = mk_create_project_payload("AI-proposed project", None);
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    assert!(!meta.confirmed);

    match create_project(&mut conn, meta, payload.clone()) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!("[req_p16] FAIL - expected ConfirmationRequired, got {other:?}"),
    }
    assert_eq!(
        project_row_count(&conn),
        0,
        "[req_p16] FAIL - a project was written before confirmation"
    );
    println!("[req_p16] PASS - Inferred CreateProject blocked with ConfirmationRequired, zero rows written");

    let mut confirmed_meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    confirmed_meta.confirmed = true;
    match create_project(&mut conn, confirmed_meta, payload) {
        ActionOutcome::Success {
            confirmation_class, ..
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
            assert_eq!(project_row_count(&conn), 1);
            println!("[req_p16] PASS - resubmission with confirmed=true persisted the project");
        }
        other => {
            panic!("[req_p16] FAIL - expected Success on confirmed resubmission, got {other:?}")
        }
    }
}

// =========================================================================
// M2 UpdateProject implementation (2026-09-17) - new evidence below.
// Evidence for `docs/reports/R014_M2_UPDATE_PROJECT_IMPLEMENTATION_2026-09-17.md`.
// Closes the M2 Project action-set gap both in design (already frozen,
// M2 Project Child Action Freeze report) and now in code.
// =========================================================================

/// Same forced-failure mechanism as every other entity's audit-failure
/// trigger, reusing `project_audit_log` (no new audit table) -
/// `install_project_audit_failure_trigger`/
/// `remove_project_audit_failure_trigger` above are shared verbatim with
/// `CreateProject`'s own hardening tests.

/// Requirements 1, 19: a valid title UpdateProject succeeds; `id` never
/// changes across the update.
#[test]
fn req_up1_19_valid_title_update_succeeds_and_id_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject1.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Original title", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let updated = match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            title: Some("  Renamed  ".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::ImportantChange);
            value
        }
        other => panic!("[req_up1] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(
        updated.title, "Renamed",
        "[req_up1] FAIL - title was not trimmed/set"
    );
    assert_eq!(
        updated.id, created.id,
        "[req_up19] FAIL - id changed across update"
    );
    println!(
        "[req_up1/19] PASS - UpdateProject title succeeded, id unchanged, class=IMPORTANT_CHANGE"
    );
}

/// Requirements 2, 3, 5, 6, 7, 8: `description` Set/Clear, `priority`
/// Set/Clear, and `goal_id` Set(valid)/Clear all succeed and persist.
#[test]
fn req_up2_3_5_6_7_8_full_field_patch_semantics_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Real goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert!(created.description.is_none());
    assert!(created.priority.is_none());
    assert!(created.goal_id.is_none());

    // Requirement 2: description Set.
    let with_desc = match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            description: FieldPatch::Set("A real description".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_up2] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(with_desc.description.as_deref(), Some("A real description"));
    println!("[req_up2] PASS - description Set succeeded");

    // Requirement 3: description Clear -> SQL NULL.
    let cleared_desc = match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            description: FieldPatch::Clear,
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_up3] FAIL - expected Success, got {other:?}"),
    };
    assert!(cleared_desc.description.is_none());
    let refetched = get_project(&conn, created.id).unwrap().unwrap();
    assert!(
        refetched.description.is_none(),
        "[req_up3] FAIL - description Clear did not persist as NULL"
    );
    println!("[req_up3] PASS - description Clear persisted as NULL");

    // Requirement 5: priority Set.
    let with_priority = match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            priority: FieldPatch::Set(4),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_up5] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(with_priority.priority, Some(4));
    println!("[req_up5] PASS - priority Set succeeded");

    // Requirement 6: priority Clear -> SQL NULL.
    let cleared_priority = match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            priority: FieldPatch::Clear,
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_up6] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(cleared_priority.priority, None);
    println!("[req_up6] PASS - priority Clear succeeded");

    // Requirement 7: goal_id Set to a valid Goal.
    let with_goal = match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            goal_id: FieldPatch::Set(goal.id),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_up7] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(with_goal.goal_id, Some(goal.id));
    println!("[req_up7] PASS - goal_id Set to a valid Goal succeeded");

    // Requirement 8: goal_id Clear -> SQL NULL.
    let cleared_goal = match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            goal_id: FieldPatch::Clear,
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_up8] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(cleared_goal.goal_id, None);
    println!("[req_up8] PASS - goal_id Clear succeeded");
}

/// Requirement 4: a status update succeeds (paired with the dedicated
/// transition-matrix proof, req_up14_to_18, below).
#[test]
fn req_up4_status_update_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject4.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(created.status, LifecycleStatus::Active);

    let updated = match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            status: Some(LifecycleStatus::Paused),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_up4] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(updated.status, LifecycleStatus::Paused);
    println!("[req_up4] PASS - status update succeeded");
}

/// Requirement 9: `goal_id` Set to a nonexistent Goal is rejected with
/// zero mutation.
#[test]
fn req_up9_goal_id_set_nonexistent_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject9.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_project(&conn, created.id).unwrap().unwrap();

    let fake_goal_id = crate::domain::GoalId::new();
    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            goal_id: FieldPatch::Set(fake_goal_id),
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::ReferencedGoalNotFound));
        }
        other => panic!("[req_up9] FAIL - expected ValidationFailed, got {other:?}"),
    }
    let after = get_project(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_up9] FAIL - project changed despite the dangling goal_id"
    );
    println!("[req_up9] PASS - goal_id Set to a nonexistent Goal rejected, zero mutation");
}

/// Requirement 10: UpdateProject on a nonexistent Project returns
/// structured NotFound.
#[test]
fn req_up10_update_nonexistent_project_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject10.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_id = crate::domain::ProjectId::new();
    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: fake_id,
            title: Some("Doesn't matter".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::NotFound => {
            println!(
                "[req_up10] PASS - UpdateProject on a nonexistent id returned structured NotFound"
            )
        }
        other => panic!("[req_up10] FAIL - expected NotFound, got {other:?}"),
    }
}

/// Requirement 11: a no-fields UpdateProject payload is rejected.
#[test]
fn req_up11_no_fields_payload_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject11.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
        }
        other => panic!("[req_up11] FAIL - expected ValidationFailed, got {other:?}"),
    }
    println!("[req_up11] PASS - no-op UpdateProject rejected with NoFieldsToUpdate");
}

/// Requirements 12, 13: an invalid title, and an overlong description,
/// both make zero mutation.
#[test]
fn req_up12_13_invalid_title_and_overlong_description_make_zero_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject12.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Stable title", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_project(&conn, created.id).unwrap().unwrap();

    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            title: Some("   ".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::EmptyTitle));
        }
        other => panic!("[req_up12] FAIL - expected ValidationFailed, got {other:?}"),
    }
    let after_title = get_project(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after_title,
        "[req_up12] FAIL - project changed despite the invalid title"
    );
    println!("[req_up12] PASS - invalid title rejected, zero mutation");

    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            description: FieldPatch::Set("x".repeat(crate::domain::DESCRIPTION_MAX_LEN + 1)),
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::DescriptionTooLong { .. })));
        }
        other => panic!("[req_up13] FAIL - expected ValidationFailed, got {other:?}"),
    }
    let after_description = get_project(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after_description,
        "[req_up13] FAIL - project changed despite the overlong description"
    );
    println!("[req_up13] PASS - overlong description rejected, zero mutation");
}

/// Requirements 14-18: representative real-DB status transitions prove
/// no terminal-state rule exists - ACTIVE -> PAUSED -> COMPLETED ->
/// ACTIVE (reopen) -> ABANDONED -> ACTIVE (reopen again).
#[test]
fn req_up14_to_18_representative_status_transitions_all_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject14.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Transitions", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(created.status, LifecycleStatus::Active);

    let transitions = [
        (LifecycleStatus::Paused, "req_up14", "ACTIVE -> PAUSED"),
        (
            LifecycleStatus::Completed,
            "req_up15",
            "PAUSED -> COMPLETED",
        ),
        (LifecycleStatus::Active, "req_up16", "COMPLETED -> ACTIVE"),
        (
            LifecycleStatus::Abandoned,
            "req_up17",
            "ACTIVE -> ABANDONED",
        ),
        (LifecycleStatus::Active, "req_up18", "ABANDONED -> ACTIVE"),
    ];

    for (target, tag, label) in transitions {
        let updated = match update_project(
            &mut conn,
            ui_explicit(),
            UpdateProjectPayload {
                id: created.id,
                status: Some(target),
                ..Default::default()
            },
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("[{tag}] FAIL - {label} should succeed, got {other:?}"),
        };
        assert_eq!(
            updated.status, target,
            "[{tag}] FAIL - status did not update for {label}"
        );
        println!("[{tag}] PASS - {label} succeeded");
    }
}

/// Requirements 20, 21, 22: `created_at` never changes, `updated_at`
/// advances on a successful update, and a rejected update leaves both
/// timestamps unchanged.
#[test]
fn req_up20_21_22_timestamp_behavior_proof() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject20.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Timestamped", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let original_created_at = created.created_at;
    let original_updated_at = created.updated_at;

    // Requirement 22: a rejected update (no fields touched) leaves both
    // timestamps unchanged.
    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(_) => {}
        other => panic!("setup failed: {other:?}"),
    }
    let after_rejected = get_project(&conn, created.id).unwrap().unwrap();
    assert_eq!(after_rejected.created_at, original_created_at);
    assert_eq!(
        after_rejected.updated_at, original_updated_at,
        "[req_up22] FAIL - a rejected update must not change updated_at"
    );
    println!("[req_up22] PASS - rejected update left both timestamps unchanged");

    // Requirements 20, 21: a successful update preserves created_at and
    // advances updated_at.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let updated = match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            title: Some("Timestamped, renamed".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_up20/21] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(
        updated.created_at, original_created_at,
        "[req_up20] FAIL - created_at must never change"
    );
    assert!(
        updated.updated_at > original_updated_at,
        "[req_up21] FAIL - updated_at must advance on a successful update"
    );
    println!("[req_up20/21] PASS - created_at preserved, updated_at advanced on successful update");
}

/// Requirements 23, 24: `Project.progress` is completely unaffected by
/// `UpdateProject`, and `UpdateProgress` continues to work independently
/// after `UpdateProject` has run - the core regression proof that the
/// two actions' ownership of `progress` stays separate (§2/§12/§21 of
/// the UpdateProject brief).
#[test]
fn req_up23_24_progress_preserved_across_update_project_and_update_progress_still_works() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject23.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Progress-bearing project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(created.progress, 0);

    let with_progress = match update_progress(
        &mut conn,
        ui_explicit(),
        UpdateProgressPayload {
            id: created.id,
            progress: 63,
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(with_progress.progress, 63);

    // Requirement 23: UpdateProject changing structural fields must not
    // touch progress.
    let after_update_project = match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            title: Some("Renamed via UpdateProject".to_string()),
            status: Some(LifecycleStatus::Paused),
            priority: FieldPatch::Set(2),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_up23] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(
        after_update_project.progress, 63,
        "[req_up23] FAIL - UpdateProject must never change progress"
    );
    assert_eq!(after_update_project.title, "Renamed via UpdateProject");
    assert_eq!(after_update_project.status, LifecycleStatus::Paused);
    println!("[req_up23] PASS - Project.progress (63) unchanged after UpdateProject touched title/status/priority");

    // Requirement 24: UpdateProgress must still work independently
    // after UpdateProject has run against the same Project.
    let after_update_progress_again = match update_progress(
        &mut conn,
        ui_explicit(),
        UpdateProgressPayload {
            id: created.id,
            progress: 80,
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_up24] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(after_update_progress_again.progress, 80);
    // UpdateProgress must not revert the structural fields UpdateProject set.
    assert_eq!(
        after_update_progress_again.title,
        "Renamed via UpdateProject"
    );
    assert_eq!(after_update_progress_again.status, LifecycleStatus::Paused);
    println!("[req_up24] PASS - UpdateProgress still works independently after UpdateProject, and does not revert UpdateProject's own changes");
}

/// Requirements 25, 26: the audit row carries the required metadata,
/// and its `previous_snapshot` contains the exact pre-update Project
/// state.
#[test]
fn req_up25_26_audit_metadata_and_previous_snapshot_correct() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject25.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Pre-update title", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            title: Some("Post-update title".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let (action_type, actor, provenance, confirmation_class, previous_snapshot): (
        String,
        String,
        String,
        String,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT action_type, actor, provenance, confirmation_class, previous_snapshot FROM project_audit_log WHERE project_id = ?1 AND action_type = 'UpdateProject'",
            [created.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();

    assert_eq!(action_type, "UpdateProject");
    assert_eq!(actor, "UI");
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "IMPORTANT_CHANGE");
    let snapshot = previous_snapshot
        .expect("[req_up26] FAIL - UpdateProject audit row must have a previous_snapshot");
    assert!(
        snapshot.contains("Pre-update title"),
        "[req_up26] FAIL - previous_snapshot does not contain the pre-update title, got {snapshot}"
    );
    println!("[req_up25] PASS - audit row carries action_type={action_type}, actor={actor}, provenance={provenance}, confirmation_class={confirmation_class}");
    println!("[req_up26] PASS - previous_snapshot contains the pre-update state: {snapshot}");
}

/// Requirements 27, 28, 29, 30: UpdateProject's project-update +
/// audit-insert are one atomic transaction. A forced audit-insert
/// failure must leave the Project byte/logically identical - including
/// `updated_at` and `progress` - and the connection must remain usable
/// afterward.
#[test]
fn req_up27_28_29_30_forced_audit_failure_rolls_back_full_update() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject27.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Stable", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match update_progress(
        &mut conn,
        ui_explicit(),
        UpdateProgressPayload {
            id: created.id,
            progress: 42,
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }
    let before = get_project(&conn, created.id).unwrap().unwrap();
    assert_eq!(before.progress, 42);

    install_project_audit_failure_trigger(&conn);

    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            title: Some("Should not persist".to_string()),
            description: FieldPatch::Set("Should not persist either".to_string()),
            status: Some(LifecycleStatus::Completed),
            priority: FieldPatch::Set(5),
            ..Default::default()
        },
    ) {
        ActionOutcome::StorageError(e) => {
            assert_eq!(e.kind(), CoreErrorKind::Storage);
            println!(
                "[req_up27] PASS - forced audit failure surfaced as a typed StorageError: {e:?}"
            );
        }
        other => panic!("[req_up27] FAIL - expected StorageError, got {other:?}"),
    }

    remove_project_audit_failure_trigger(&conn);

    let after = get_project(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_up27] FAIL - project mutated despite a rolled-back transaction"
    );
    assert_eq!(
        after.updated_at, before.updated_at,
        "[req_up28] FAIL - updated_at leaked from a rolled-back update"
    );
    assert_eq!(
        after.progress, 42,
        "[req_up29] FAIL - progress leaked/changed from a rolled-back update"
    );

    let audit_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM project_audit_log WHERE project_id = ?1",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        audit_count, 2, // CreateProject + UpdateProgress, no orphan UpdateProject row
        "[req_up27] FAIL - expected exactly 2 prior audit rows (CreateProject, UpdateProgress), got {audit_count}"
    );

    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            title: Some("Now it works".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {
            println!("[req_up30] PASS - connection remains usable after the rollback")
        }
        other => panic!("[req_up30] FAIL - connection unusable after rollback: {other:?}"),
    }
}

/// Requirements 31, 32: `Project.goal_id` Set/Clear must never create,
/// delete, or otherwise modify a `Goal` or any `GoalRelation` row - the
/// two concepts remain fully separate.
#[test]
fn req_up31_32_goal_id_changes_do_not_affect_goal_or_goal_relations() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject31.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal_a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    // An unrelated GoalRelation must be unaffected by anything Project-side.
    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(goal_a.id, goal_b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }
    let relation_count_before = goal_relation_row_count(&conn);
    let goal_a_before = get_goal(&conn, goal_a.id).unwrap().unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    // goal_id Set.
    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            goal_id: FieldPatch::Set(goal_a.id),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_up31] FAIL - expected Success, got {other:?}"),
    }
    assert_eq!(
        goal_relation_row_count(&conn),
        relation_count_before,
        "[req_up32] FAIL - goal_id Set must not create a GoalRelation row"
    );
    assert_eq!(
        get_goal(&conn, goal_a.id).unwrap().unwrap(),
        goal_a_before,
        "[req_up31] FAIL - goal_id Set must not modify the Goal itself"
    );

    // goal_id Clear (unlink).
    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            goal_id: FieldPatch::Clear,
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_up31] FAIL - expected Success, got {other:?}"),
    }
    assert_eq!(
        goal_relation_row_count(&conn),
        relation_count_before,
        "[req_up32] FAIL - goal_id Clear must not delete a GoalRelation row"
    );
    assert_eq!(
        get_goal(&conn, goal_a.id).unwrap().unwrap(),
        goal_a_before,
        "[req_up31] FAIL - goal_id Clear (unlink) must not modify or delete the Goal itself"
    );
    println!("[req_up31/32] PASS - Project.goal_id Set/Clear left the Goal and every GoalRelation row completely untouched");
}

/// V6 migration compatibility (not in the original UpdateProject brief's
/// numbered list - added because this run required an unplanned V6
/// migration, and every other migration in this codebase has a
/// dedicated non-destructive-upgrade proof; see
/// `docs/reports/R014_M2_UPDATE_PROJECT_IMPLEMENTATION_2026-09-17.md` §3a). Builds a
/// database frozen at V5 (real `project_audit_log` rows using only the
/// pre-V6 `action_type` values `'CreateProject'`/`'UpdateProgress'` -
/// the actual shape of an existing user's database before this update),
/// then reopens it normally (applying only the newly-pending V6
/// migration) and confirms: the pre-existing audit rows survive with
/// their original `action_type` values intact, the Project/Goal data
/// they reference is unchanged, and a new `UpdateProject` call - which
/// needs the widened `action_type` CHECK - now succeeds against the
/// pre-existing Project.
#[test]
fn req_up_v6_v5_populated_database_upgrades_cleanly_to_v6() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "updateproject_v6.db");

    let (project_id, pre_v6_audit_count);
    {
        let mut conn = bootstrap_db_at_version(&path, &key, 5).unwrap();

        let created = match create_project(
            &mut conn,
            ui_explicit(),
            mk_create_project_payload("Pre-V6 project", None),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        project_id = created.id;
        match update_progress(
            &mut conn,
            ui_explicit(),
            UpdateProgressPayload {
                id: project_id,
                progress: 55,
            },
        ) {
            ActionOutcome::Success { .. } => {}
            other => panic!("setup failed: {other:?}"),
        }
        pre_v6_audit_count = project_audit_row_count(&conn);
        assert_eq!(
            pre_v6_audit_count, 2,
            "[req_up_v6] FAIL - expected exactly 2 pre-V6 audit rows (CreateProject, UpdateProgress)"
        );
    }

    {
        let mut conn = bootstrap_db_at(&path, &key).unwrap();

        let project_after = get_project(&conn, project_id).unwrap();
        assert_eq!(
            project_after.as_ref().map(|p| p.progress),
            Some(55),
            "[req_up_v6] FAIL - pre-V6 Project data did not survive the V5 -> V6 upgrade"
        );

        let audit_count_after: i64 = conn
            .query_row(
                "SELECT count(*) FROM project_audit_log WHERE project_id = ?1",
                [project_id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            audit_count_after, pre_v6_audit_count,
            "[req_up_v6] FAIL - pre-V6 audit rows were lost during the upgrade"
        );
        let action_types: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT action_type FROM project_audit_log WHERE project_id = ?1 ORDER BY occurred_at")
                .unwrap();
            stmt.query_map([project_id.to_string()], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<String>>>()
                .unwrap()
        };
        assert_eq!(
            action_types,
            vec!["CreateProject".to_string(), "UpdateProgress".to_string()],
            "[req_up_v6] FAIL - pre-V6 audit action_type values were not preserved exactly"
        );

        match update_project(
            &mut conn,
            ui_explicit(),
            UpdateProjectPayload {
                id: project_id,
                title: Some("Post-V6 rename".to_string()),
                ..Default::default()
            },
        ) {
            ActionOutcome::Success { .. } => println!(
                "[req_up_v6] PASS - V5 data (incl. pre-existing audit rows) survived the upgrade to V6, and a new UpdateProject now works against the pre-existing Project"
            ),
            other => panic!(
                "[req_up_v6] FAIL - expected UpdateProject to succeed after the V6 upgrade, got {other:?}"
            ),
        }
    }
}

// =========================================================================
// M2 Project/storage integrity hardening (2026-09-17) - new evidence
// below. Evidence for
// `docs/reports/R008_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md`.
// =========================================================================

fn foreign_keys_pragma_value(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap()
}

/// Requirement 1: `PRAGMA foreign_keys` reads back `1` on a fresh
/// production connection - not merely "the `PRAGMA` statement was sent
/// and returned no error," which `enable_foreign_keys` itself already
/// guards against silently failing (`storage/mod.rs`), but re-proven
/// here as an integration-level assertion against the real bootstrap
/// path.
#[test]
fn req_i1_foreign_keys_enabled_on_fresh_production_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "integrity1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    assert_eq!(
        foreign_keys_pragma_value(&conn),
        1,
        "[req_i1] FAIL - foreign_keys not enabled on a fresh production connection"
    );
    println!("[req_i1] PASS - PRAGMA foreign_keys = 1 on a fresh production connection");
}

/// Requirement 2: `foreign_keys` is a per-connection SQLite setting,
/// not persisted in the database file - a full close + reopen must set
/// it again, and does, since every real connection goes through the
/// same `open_encrypted` primitive.
#[test]
fn req_i2_foreign_keys_enabled_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "integrity2.db");
    {
        let _conn = bootstrap_db_at(&path, &key).unwrap();
    }
    let conn = bootstrap_db_at(&path, &key).unwrap();

    assert_eq!(
        foreign_keys_pragma_value(&conn),
        1,
        "[req_i2] FAIL - foreign_keys not enabled after a full connection close/reopen"
    );
    println!("[req_i2] PASS - PRAGMA foreign_keys = 1 after a full connection close/reopen");
}

/// Requirement 3 (and Project/storage hardening report §3.B):
/// intentionally bypasses `CreateProject`'s application-level
/// `ReferencedGoalNotFound` pre-check entirely - a direct, raw SQL
/// `INSERT` naming a `goal_id` that does not exist - to prove the
/// database itself, not just Core's validation layer, rejects invalid
/// state now that `PRAGMA foreign_keys = ON` is enforced. This raw SQL
/// stays local to this test (never exposed as a repository/production
/// function) - the point is to bypass the normal path, not to add a
/// new one.
#[test]
fn req_i3_invalid_direct_project_goal_id_rejected_by_sqlite_fk() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "integrity3.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_goal_id = crate::domain::GoalId::new().to_string();
    let project_id = crate::domain::ProjectId::new().to_string();
    let result = conn.execute(
        "INSERT INTO projects (id, title, description, status, priority, goal_id, progress, created_at, updated_at)
         VALUES (?1, 'Bypasses CreateProject validation', NULL, 'ACTIVE', NULL, ?2, 0, '2026-09-17T00:00:00Z', '2026-09-17T00:00:00Z')",
        rusqlite::params![project_id, fake_goal_id],
    );

    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "[req_i3] FAIL - expected a constraint-violation error, got {err:?}"
            );
            println!(
                "[req_i3] PASS - direct raw SQL insert with a nonexistent goal_id was rejected by SQLite's own FK enforcement: {err:?}"
            );
        }
        other => panic!("[req_i3] FAIL - expected an SQLite FK constraint error, got {other:?}"),
    }

    assert_eq!(
        project_row_count(&conn),
        0,
        "[req_i3] FAIL - a project row was inserted despite the FK violation"
    );
}

/// Requirement 4: the same direct, raw-SQL insert path succeeds when
/// `goal_id` references a real, existing `Goal` - proving the FK
/// constraint distinguishes valid from invalid references rather than
/// just rejecting everything.
#[test]
fn req_i4_valid_direct_project_goal_id_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "integrity4.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Real goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let project_id = crate::domain::ProjectId::new().to_string();
    let result = conn.execute(
        "INSERT INTO projects (id, title, description, status, priority, goal_id, progress, created_at, updated_at)
         VALUES (?1, 'Direct insert, valid goal', NULL, 'ACTIVE', NULL, ?2, 0, '2026-09-17T00:00:00Z', '2026-09-17T00:00:00Z')",
        rusqlite::params![project_id, goal.id.to_string()],
    );

    assert!(
        result.is_ok(),
        "[req_i4] FAIL - direct insert with a valid, existing goal_id was rejected: {result:?}"
    );
    assert_eq!(project_row_count(&conn), 1);
    println!("[req_i4] PASS - direct raw SQL insert with a valid, existing goal_id succeeded");
}

// =========================================================================
// M2 GoalRelation vertical slice (2026-09-17) - new evidence below.
// Evidence for
// `docs/reports/R009_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md`.
// =========================================================================

fn mk_link_payload(
    from: crate::domain::GoalId,
    to: crate::domain::GoalId,
    relation_type: GoalRelationType,
) -> LinkGoalsPayload {
    LinkGoalsPayload {
        from_goal_id: from,
        to_goal_id: to,
        relation_type,
        note: None,
    }
}

fn goal_relation_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM goal_relations", [], |r| r.get(0))
        .unwrap()
}

fn goal_relation_audit_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM goal_relation_audit_log", [], |r| {
        r.get(0)
    })
    .unwrap()
}

/// Same forced-failure mechanism as `install_audit_failure_trigger`/
/// `install_project_audit_failure_trigger`, targeting
/// `goal_relation_audit_log`.
fn install_goal_relation_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER force_goal_relation_audit_failure
         BEFORE INSERT ON goal_relation_audit_log
         BEGIN
             SELECT RAISE(FAIL, 'forced goal relation audit failure');
         END;",
    )
    .expect("failed to install the forced goal-relation-audit-failure trigger");
}

fn remove_goal_relation_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch("DROP TRIGGER IF EXISTS force_goal_relation_audit_failure;")
        .expect("failed to remove the forced goal-relation-audit-failure trigger");
}

/// Requirement 1: V3 migration creates the GoalRelation schema.
#[test]
fn req_gr1_v3_migration_creates_goal_relation_schema() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    for table in ["goal_relations", "goal_relation_audit_log"] {
        let exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(exists, "[req_gr1] FAIL - missing table {table}");
    }
    println!("[req_gr1] PASS - V3 migration created goal_relations + goal_relation_audit_log");
}

/// Requirement 2: a fresh V1->V2->V3 database works end to end - Goal,
/// Project, and GoalRelation all created together in one migration run.
#[test]
fn req_gr2_fresh_v1_v2_v3_database_works_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal_a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project linked to A", Some(goal_a.id)),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(project.goal_id, Some(goal_a.id));

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(goal_a.id, goal_b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.from_goal_id, goal_a.id);
            assert_eq!(value.to_goal_id, goal_b.id);
        }
        other => panic!("[req_gr2] FAIL - expected Success, got {other:?}"),
    }
    println!("[req_gr2] PASS - fresh V1->V2->V3 database supports Goal, Project, and GoalRelation together");
}

/// Requirement 3: an existing database populated only through V2 (real
/// Goal rows, a real Project linked to a Goal - the actual shape of an
/// existing user's database before this update) upgrades cleanly to V3.
#[test]
fn req_gr3_v2_populated_database_upgrades_cleanly_to_v3() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation3.db");

    let (goal_id, project_id);
    {
        let mut conn = bootstrap_db_at_version(&path, &key, 2).unwrap();
        let has_relations_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='goal_relations'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(
            !has_relations_table,
            "[req_gr3] FAIL - a V2-only database already has goal_relations"
        );

        let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Pre-V3 goal")) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        goal_id = goal.id;
        let project = match create_project(
            &mut conn,
            ui_explicit(),
            mk_create_project_payload("Pre-V3 project", Some(goal_id)),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        project_id = project.id;
    }

    {
        let mut conn = bootstrap_db_at(&path, &key).unwrap();

        let goal_after = get_goal(&conn, goal_id).unwrap();
        assert_eq!(
            goal_after.as_ref().map(|g| g.title.as_str()),
            Some("Pre-V3 goal"),
            "[req_gr3] FAIL - Goal data did not survive the V2 -> V3 upgrade"
        );

        let project_after = get_project(&conn, project_id).unwrap();
        assert_eq!(
            project_after.as_ref().map(|p| p.goal_id),
            Some(Some(goal_id)),
            "[req_gr3] FAIL - Project.goal_id did not survive the upgrade"
        );

        let has_relations_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='goal_relations'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(
            has_relations_table,
            "[req_gr3] FAIL - goal_relations table missing after upgrade"
        );

        let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Post-V3 goal"))
        {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        match link_goals(
            &mut conn,
            ui_explicit(),
            mk_link_payload(goal_id, goal_b.id, GoalRelationType::Supports),
        ) {
            ActionOutcome::Success { .. } => println!(
                "[req_gr3] PASS - V2 data survived the upgrade to V3, and a new LinkGoals works between pre-existing Goals"
            ),
            other => panic!(
                "[req_gr3] FAIL - expected LinkGoals to succeed after upgrade, got {other:?}"
            ),
        }
    }
}

/// Requirements 4 & 5: a valid LinkGoals persists, and the relation
/// survives a full connection close/reopen.
#[test]
fn req_gr4_5_valid_link_goals_persists_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation4.db");

    let (goal_a_id, goal_b_id, relation_id);
    {
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let goal_a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        goal_a_id = goal_a.id;
        goal_b_id = goal_b.id;

        match link_goals(
            &mut conn,
            ui_explicit(),
            mk_link_payload(goal_a_id, goal_b_id, GoalRelationType::DependsOn),
        ) {
            ActionOutcome::Success {
                value,
                confirmation_class,
            } => {
                assert_eq!(value.from_goal_id, goal_a_id);
                assert_eq!(value.to_goal_id, goal_b_id);
                assert_eq!(value.relation_type, GoalRelationType::DependsOn);
                assert_eq!(confirmation_class, ConfirmationClass::ExplicitAdd);
                relation_id = value.id;
                println!("[req_gr4] PASS - LinkGoals succeeded: {value:?}");
            }
            other => panic!("[req_gr4] FAIL - expected Success, got {other:?}"),
        }
    }

    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        let fetched = get_goal_relation(&conn, relation_id).unwrap();
        assert!(
            fetched.is_some(),
            "[req_gr5] FAIL - relation not found after reopen"
        );
        let fetched = fetched.unwrap();
        assert_eq!(fetched.from_goal_id, goal_a_id);
        assert_eq!(fetched.to_goal_id, goal_b_id);
        println!("[req_gr5] PASS - relation survived a full connection close/reopen: {fetched:?}");
    }
}

/// Requirement 6: a missing `from_goal_id` is rejected before
/// persistence with a structured error, zero writes.
#[test]
fn req_gr6_from_goal_missing_rejected_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation6.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let real_goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Real goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let fake_id = crate::domain::GoalId::new();

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(fake_id, real_goal.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(
                errors.contains(&ValidationError::RelationFromGoalNotFound),
                "expected RelationFromGoalNotFound, got {errors:?}"
            );
        }
        other => panic!("[req_gr6] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        goal_relation_row_count(&conn),
        0,
        "[req_gr6] FAIL - a relation row was written despite the missing from_goal_id"
    );
    println!(
        "[req_gr6] PASS - missing from_goal_id rejected before persistence, zero rows written"
    );
}

/// Requirement 7: a missing `to_goal_id` is rejected before persistence
/// with a structured error, zero writes.
#[test]
fn req_gr7_to_goal_missing_rejected_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation7.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let real_goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Real goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let fake_id = crate::domain::GoalId::new();

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(real_goal.id, fake_id, GoalRelationType::Supports),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(
                errors.contains(&ValidationError::RelationToGoalNotFound),
                "expected RelationToGoalNotFound, got {errors:?}"
            );
        }
        other => panic!("[req_gr7] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        goal_relation_row_count(&conn),
        0,
        "[req_gr7] FAIL - a relation row was written despite the missing to_goal_id"
    );
    println!("[req_gr7] PASS - missing to_goal_id rejected before persistence, zero rows written");
}

/// Requirement 8: a self-relation is rejected before persistence
/// (integration-level proof, complementing the pure unit test).
#[test]
fn req_gr8_self_relation_rejected_before_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation8.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Solo goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(goal.id, goal.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::SelfGoalRelation));
        }
        other => panic!("[req_gr8] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(goal_relation_row_count(&conn), 0);
    println!("[req_gr8] PASS - self-relation rejected before persistence, zero rows written");
}

/// Requirement 9: an exact duplicate `(from, to, relation_type)` is
/// rejected before persistence.
#[test]
fn req_gr9_exact_duplicate_rejected_before_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation9.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::DuplicateGoalRelation));
        }
        other => panic!("[req_gr9] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        goal_relation_row_count(&conn),
        1,
        "[req_gr9] FAIL - expected exactly 1 row (the duplicate must not have been written)"
    );
    println!("[req_gr9] PASS - exact duplicate rejected before persistence, still exactly 1 row");
}

/// Requirement 10: different relation types for the same Goal pair both
/// succeed (A SUPPORTS B and A DEPENDS_ON B coexist).
#[test]
fn req_gr10_different_relation_types_for_same_pair_both_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation10.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_gr10] FAIL - expected Success for SUPPORTS, got {other:?}"),
    }
    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::DependsOn),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_gr10] FAIL - expected Success for DEPENDS_ON, got {other:?}"),
    }

    assert_eq!(goal_relation_row_count(&conn), 2);
    println!("[req_gr10] PASS - A SUPPORTS B and A DEPENDS_ON B coexist as 2 distinct rows");
}

/// Requirement 11: the reversed direction is a distinct stored relation
/// - A SUPPORTS B is not database-identical to B SUPPORTS A.
#[test]
fn req_gr11_reversed_direction_is_a_distinct_stored_relation() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation11.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_gr11] FAIL - expected Success for A->B, got {other:?}"),
    }
    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(b.id, a.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!(
            "[req_gr11] FAIL - reversed direction B->A must not be treated as a duplicate, got {other:?}"
        ),
    }

    assert_eq!(goal_relation_row_count(&conn), 2);
    println!("[req_gr11] PASS - A SUPPORTS B and B SUPPORTS A are 2 distinct stored rows, not auto-collapsed");
}

/// Requirement 12: a direct, raw SQL insert naming a nonexistent
/// `from_goal_id` is rejected by SQLite's own FK enforcement - bypasses
/// `LinkGoals` entirely, raw SQL kept local to this test.
#[test]
fn req_gr12_raw_sql_missing_from_goal_rejected_by_fk() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation12.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let fake_from = crate::domain::GoalId::new().to_string();
    let relation_id = crate::domain::GoalRelationId::new().to_string();
    let result = conn.execute(
        "INSERT INTO goal_relations (id, from_goal_id, to_goal_id, relation_type, note, created_at)
         VALUES (?1, ?2, ?3, 'SUPPORTS', NULL, '2026-09-17T00:00:00Z')",
        rusqlite::params![relation_id, fake_from, b.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!(
                "[req_gr12] PASS - raw SQL insert with a nonexistent from_goal_id rejected by FK: {err:?}"
            );
        }
        other => panic!("[req_gr12] FAIL - expected an SQLite FK constraint error, got {other:?}"),
    }
    assert_eq!(goal_relation_row_count(&conn), 0);
}

/// Requirement 13: same as requirement 12, for `to_goal_id`.
#[test]
fn req_gr13_raw_sql_missing_to_goal_rejected_by_fk() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation13.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let fake_to = crate::domain::GoalId::new().to_string();
    let relation_id = crate::domain::GoalRelationId::new().to_string();
    let result = conn.execute(
        "INSERT INTO goal_relations (id, from_goal_id, to_goal_id, relation_type, note, created_at)
         VALUES (?1, ?2, ?3, 'SUPPORTS', NULL, '2026-09-17T00:00:00Z')",
        rusqlite::params![relation_id, a.id.to_string(), fake_to],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!(
                "[req_gr13] PASS - raw SQL insert with a nonexistent to_goal_id rejected by FK: {err:?}"
            );
        }
        other => panic!("[req_gr13] FAIL - expected an SQLite FK constraint error, got {other:?}"),
    }
    assert_eq!(goal_relation_row_count(&conn), 0);
}

/// Requirement 14: a direct, raw SQL self-edge insert is rejected by the
/// `CHECK (from_goal_id <> to_goal_id)` constraint.
#[test]
fn req_gr14_raw_sql_self_edge_rejected_by_check() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation14.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let relation_id = crate::domain::GoalRelationId::new().to_string();
    let result = conn.execute(
        "INSERT INTO goal_relations (id, from_goal_id, to_goal_id, relation_type, note, created_at)
         VALUES (?1, ?2, ?3, 'SUPPORTS', NULL, '2026-09-17T00:00:00Z')",
        rusqlite::params![relation_id, a.id.to_string(), a.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!("[req_gr14] PASS - raw SQL self-edge insert rejected by CHECK: {err:?}");
        }
        other => {
            panic!("[req_gr14] FAIL - expected an SQLite CHECK constraint error, got {other:?}")
        }
    }
    assert_eq!(goal_relation_row_count(&conn), 0);
}

/// Requirement 15: a direct, raw SQL duplicate `(from, to, type)` insert
/// is rejected by the `UNIQUE` constraint.
#[test]
fn req_gr15_raw_sql_duplicate_rejected_by_unique() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation15.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let relation_id = crate::domain::GoalRelationId::new().to_string();
    let result = conn.execute(
        "INSERT INTO goal_relations (id, from_goal_id, to_goal_id, relation_type, note, created_at)
         VALUES (?1, ?2, ?3, 'SUPPORTS', NULL, '2026-09-17T00:00:00Z')",
        rusqlite::params![relation_id, a.id.to_string(), b.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!("[req_gr15] PASS - raw SQL duplicate insert rejected by UNIQUE: {err:?}");
        }
        other => {
            panic!("[req_gr15] FAIL - expected an SQLite UNIQUE constraint error, got {other:?}")
        }
    }
    assert_eq!(goal_relation_row_count(&conn), 1);
}

/// Requirement 16: a direct, raw SQL insert with an invalid
/// `relation_type` text is rejected by the `CHECK` constraint.
#[test]
fn req_gr16_raw_sql_invalid_relation_type_rejected_by_check() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation16.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let relation_id = crate::domain::GoalRelationId::new().to_string();
    let result = conn.execute(
        "INSERT INTO goal_relations (id, from_goal_id, to_goal_id, relation_type, note, created_at)
         VALUES (?1, ?2, ?3, 'NOT_A_REAL_TYPE', NULL, '2026-09-17T00:00:00Z')",
        rusqlite::params![relation_id, a.id.to_string(), b.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!(
                "[req_gr16] PASS - raw SQL insert with an invalid relation_type rejected by CHECK: {err:?}"
            );
        }
        other => {
            panic!("[req_gr16] FAIL - expected an SQLite CHECK constraint error, got {other:?}")
        }
    }
    assert_eq!(goal_relation_row_count(&conn), 0);
}

/// Requirement 17: LinkGoals's relation-insert + audit-insert are one
/// atomic transaction - same forced-failure proof as Goal/Project
/// hardening.
#[test]
fn req_gr17_link_goals_audit_failure_rolls_back_relation_insert() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation17.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    install_goal_relation_audit_failure_trigger(&conn);

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::StorageError(e) => {
            assert_eq!(e.kind(), CoreErrorKind::Storage);
            println!(
                "[req_gr17] PASS - forced audit failure surfaced as a typed StorageError: {e:?}"
            );
        }
        other => panic!("[req_gr17] FAIL - expected StorageError, got {other:?}"),
    }
    assert_eq!(
        goal_relation_row_count(&conn),
        0,
        "[req_gr17] FAIL - a relation row survived a rolled-back transaction"
    );
    assert_eq!(
        goal_relation_audit_row_count(&conn),
        0,
        "[req_gr17] FAIL - an audit row survived a rolled-back transaction"
    );

    remove_goal_relation_audit_failure_trigger(&conn);

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {
            println!("[req_gr17] PASS - connection remains usable after the rollback")
        }
        other => panic!("[req_gr17] FAIL - connection unusable after rollback: {other:?}"),
    }
}

/// Requirement 18: the audit row carries the required actor/provenance/
/// confirmation-class metadata.
#[test]
fn req_gr18_audit_row_contains_required_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation18.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let relation = match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let (action_type, actor, provenance, confirmation_class): (String, String, String, String) = conn
        .query_row(
            "SELECT action_type, actor, provenance, confirmation_class FROM goal_relation_audit_log WHERE goal_relation_id = ?1",
            [relation.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();

    assert_eq!(action_type, "LinkGoals");
    assert_eq!(actor, "UI");
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "EXPLICIT_ADD");
    println!(
        "[req_gr18] PASS - audit row carries action_type={action_type}, actor={actor}, provenance={provenance}, confirmation_class={confirmation_class}"
    );
}

/// Requirements 19 & 20: Inferred LinkGoals blocks before write, then a
/// confirmed resubmission succeeds.
#[test]
fn req_gr19_20_inferred_link_goals_requires_confirmation_then_confirmed_resubmission_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation19.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let payload = mk_link_payload(a.id, b.id, GoalRelationType::Supports);
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    assert!(!meta.confirmed);

    match link_goals(&mut conn, meta, payload.clone()) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!("[req_gr19] FAIL - expected ConfirmationRequired, got {other:?}"),
    }
    assert_eq!(
        goal_relation_row_count(&conn),
        0,
        "[req_gr19] FAIL - a relation was written before confirmation"
    );
    println!(
        "[req_gr19] PASS - Inferred LinkGoals blocked with ConfirmationRequired, zero rows written"
    );

    let mut confirmed_meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    confirmed_meta.confirmed = true;
    match link_goals(&mut conn, confirmed_meta, payload) {
        ActionOutcome::Success {
            confirmation_class, ..
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
            assert_eq!(goal_relation_row_count(&conn), 1);
            println!("[req_gr20] PASS - resubmission with confirmed=true persisted the relation");
        }
        other => {
            panic!("[req_gr20] FAIL - expected Success on confirmed resubmission, got {other:?}")
        }
    }
}

/// Requirements 21 & 22: `get_goal_relations_for_goal` finds both
/// incoming and outgoing relations for the queried Goal.
#[test]
fn req_gr21_22_get_goal_relations_for_goal_finds_incoming_and_outgoing() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation21.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let c = match create_goal(&mut conn, ui_explicit(), mk_create_payload("C")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    // Outgoing from A's perspective.
    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }
    // Incoming to A's perspective.
    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(c.id, a.id, GoalRelationType::DependsOn),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let relations = get_goal_relations_for_goal(&conn, a.id).unwrap();
    assert_eq!(
        relations.len(),
        2,
        "[req_gr21/22] FAIL - expected 2 relations involving A, got {}",
        relations.len()
    );
    assert!(
        relations
            .iter()
            .any(|r| r.from_goal_id == a.id && r.to_goal_id == b.id),
        "[req_gr21] FAIL - outgoing A->B relation not found"
    );
    assert!(
        relations
            .iter()
            .any(|r| r.from_goal_id == c.id && r.to_goal_id == a.id),
        "[req_gr22] FAIL - incoming C->A relation not found"
    );
    println!(
        "[req_gr21/22] PASS - get_goal_relations_for_goal found both the outgoing and incoming relation"
    );
}

/// Requirement 23: the frozen PARENT_OF convention stores exactly one
/// row - `LinkGoals` must never auto-insert a `CHILD_OF` inverse.
#[test]
fn req_gr23_parent_of_convention_stores_exactly_one_row() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation23.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let parent = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Parent")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let child = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Child")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(parent.id, child.id, GoalRelationType::ParentOf),
    ) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.from_goal_id, parent.id);
            assert_eq!(value.to_goal_id, child.id);
            assert_eq!(value.relation_type, GoalRelationType::ParentOf);
        }
        other => panic!("[req_gr23] FAIL - expected Success, got {other:?}"),
    }

    assert_eq!(
        goal_relation_row_count(&conn),
        1,
        "[req_gr23] FAIL - expected exactly 1 row - LinkGoals must never auto-insert a CHILD_OF inverse"
    );

    let relations_for_child = get_goal_relations_for_goal(&conn, child.id).unwrap();
    assert_eq!(relations_for_child.len(), 1);
    assert_eq!(relations_for_child[0].from_goal_id, parent.id);
    assert_eq!(relations_for_child[0].to_goal_id, child.id);
    assert_eq!(
        relations_for_child[0].relation_type,
        GoalRelationType::ParentOf
    );
    println!(
        "[req_gr23] PASS - PARENT_OF(parent, child) stored as exactly one row; querying relations for Child returns that same row, not a fabricated CHILD_OF inverse"
    );
}

/// Requirement 24: `GoalRelationType::ChildOf` remains a valid,
/// storable, parseable relation type when deliberately requested - the
/// freeze keeps it even though `LinkGoals` never auto-generates it.
#[test]
fn req_gr24_explicit_child_of_remains_valid_when_deliberately_requested() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation24.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let child = match create_goal(
        &mut conn,
        ui_explicit(),
        mk_create_payload("Child (stated from its own side)"),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let parent = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Parent")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(child.id, parent.id, GoalRelationType::ChildOf),
    ) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.relation_type, GoalRelationType::ChildOf);
            println!(
                "[req_gr24] PASS - explicit CHILD_OF stored and retrievable when deliberately requested: {value:?}"
            );
        }
        other => panic!(
            "[req_gr24] FAIL - CHILD_OF must remain a valid, storable relation type, got {other:?}"
        ),
    }
}

/// Requirement 25: LinkGoals is never blocked by LifecycleStatus -
/// relations involving PAUSED/COMPLETED/ABANDONED goals all succeed
/// (guards against accidentally inventing an "active-only graph").
#[test]
fn req_gr25_relations_work_with_paused_completed_abandoned_goals() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation25.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let active = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Active")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let paused = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Paused")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let completed = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Completed")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let abandoned = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Abandoned")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    for (goal, status) in [
        (&paused, LifecycleStatus::Paused),
        (&completed, LifecycleStatus::Completed),
        (&abandoned, LifecycleStatus::Abandoned),
    ] {
        match update_goal(
            &mut conn,
            ui_explicit(),
            UpdateGoalPayload {
                id: goal.id,
                status: Some(status),
                ..Default::default()
            },
        ) {
            ActionOutcome::Success { .. } => {}
            other => panic!("setup failed: {other:?}"),
        }
    }

    for (other_goal, label) in [
        (&paused, "paused"),
        (&completed, "completed"),
        (&abandoned, "abandoned"),
    ] {
        match link_goals(
            &mut conn,
            ui_explicit(),
            mk_link_payload(active.id, other_goal.id, GoalRelationType::Supports),
        ) {
            ActionOutcome::Success { .. } => {}
            other_outcome => panic!(
                "[req_gr25] FAIL - LinkGoals involving a {label} goal should succeed, got {other_outcome:?}"
            ),
        }
    }

    assert_eq!(goal_relation_row_count(&conn), 3);
    println!(
        "[req_gr25] PASS - LinkGoals succeeded for relations involving PAUSED, COMPLETED, and ABANDONED goals - no active-only restriction exists"
    );
}

/// Requirement 26: a direct, raw SQL hard delete of a Goal referenced by
/// a relation is rejected by `ON DELETE RESTRICT`.
#[test]
fn req_gr26_raw_hard_delete_of_referenced_goal_is_restricted() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "relation26.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(a.id, b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let result = conn.execute(
        "DELETE FROM goals WHERE id = ?1",
        rusqlite::params![a.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!(
                "[req_gr26] PASS - raw DELETE of a referenced Goal rejected by ON DELETE RESTRICT: {err:?}"
            );
        }
        other => panic!(
            "[req_gr26] FAIL - expected an SQLite FK constraint error blocking the delete, got {other:?}"
        ),
    }

    let goal_still_exists = get_goal(&conn, a.id).unwrap();
    assert!(
        goal_still_exists.is_some(),
        "[req_gr26] FAIL - Goal A was deleted despite the RESTRICT constraint"
    );
    assert_eq!(
        goal_relation_row_count(&conn),
        1,
        "[req_gr26] FAIL - relation row disappeared despite the delete being rejected"
    );
    println!(
        "[req_gr26] PASS - Goal A remains, and the relation remains - no dangling relation exists"
    );
}

// =========================================================================
// M2 Milestone vertical slice (2026-09-17) - new evidence below. Evidence
// for `docs/reports/R011_M2_MILESTONE_VERTICAL_SLICE_2026-09-17.md`.
// The fourth real M2 domain entity, and the first required-parent child
// entity - `milestones.project_id REFERENCES projects(id) ON DELETE
// RESTRICT`.
// =========================================================================

fn mk_add_milestone_payload(
    project_id: crate::domain::ProjectId,
    title: &str,
    order_index: crate::domain::OrderIndex,
) -> AddMilestonePayload {
    AddMilestonePayload {
        project_id,
        title: title.to_string(),
        target_date: None,
        order_index,
    }
}

fn milestone_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM milestones", [], |r| r.get(0))
        .unwrap()
}

fn milestone_audit_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM milestone_audit_log", [], |r| r.get(0))
        .unwrap()
}

/// Same forced-failure mechanism as `install_audit_failure_trigger`/
/// `install_project_audit_failure_trigger`/
/// `install_goal_relation_audit_failure_trigger`, targeting
/// `milestone_audit_log`.
fn install_milestone_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER force_milestone_audit_failure
         BEFORE INSERT ON milestone_audit_log
         BEGIN
             SELECT RAISE(FAIL, 'forced milestone audit failure');
         END;",
    )
    .expect("failed to install the forced milestone-audit-failure trigger");
}

fn remove_milestone_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch("DROP TRIGGER IF EXISTS force_milestone_audit_failure;")
        .expect("failed to remove the forced milestone-audit-failure trigger");
}

/// Requirement 1: V4 migration creates the Milestone schema.
#[test]
fn req_m1_v4_migration_creates_milestone_schema() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    for table in ["milestones", "milestone_audit_log"] {
        let exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(exists, "[req_m1] FAIL - missing table {table}");
    }
    println!("[req_m1] PASS - V4 migration created milestones + milestone_audit_log");
}

/// Requirement 2: a fresh V1->V2->V3->V4 database works end to end - a
/// Goal, a Project linked to it, a GoalRelation, and a Milestone on the
/// Project all created together in one migration run.
#[test]
fn req_m2_fresh_v1_v2_v3_v4_database_works_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal_a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project linked to A", Some(goal_a.id)),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(goal_a.id, goal_b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "First milestone", 0),
    ) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.project_id, project.id);
            assert_eq!(value.status, MilestoneStatus::Pending);
        }
        other => panic!("[req_m2] FAIL - expected Success, got {other:?}"),
    }
    println!("[req_m2] PASS - fresh V1->V2->V3->V4 database supports Goal, Project, GoalRelation, and Milestone together");
}

/// Requirement 3 (migration compatibility): an existing database
/// populated only through V3 (real Goal/Project/GoalRelation rows - the
/// actual shape of an existing user's database before this update)
/// upgrades cleanly to V4 - Goal/Project/GoalRelation data unchanged, the
/// milestones table appears, and a new Milestone can reference the
/// pre-existing Project.
#[test]
fn req_m3_v3_populated_database_upgrades_cleanly_to_v4() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone3.db");

    let (goal_id, project_id, relation_id);
    {
        let mut conn = bootstrap_db_at_version(&path, &key, 3).unwrap();
        let has_milestones_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='milestones'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(
            !has_milestones_table,
            "[req_m3] FAIL - a V3-only database already has the milestones table"
        );

        let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Pre-V4 goal")) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        goal_id = goal.id;
        let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Pre-V4 goal B"))
        {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        let project = match create_project(
            &mut conn,
            ui_explicit(),
            mk_create_project_payload("Pre-V4 project", Some(goal_id)),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        project_id = project.id;
        let relation = match link_goals(
            &mut conn,
            ui_explicit(),
            mk_link_payload(goal_id, goal_b.id, GoalRelationType::Supports),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        relation_id = relation.id;
    }

    {
        let mut conn = bootstrap_db_at(&path, &key).unwrap();

        let goal_after = get_goal(&conn, goal_id).unwrap();
        assert_eq!(
            goal_after.as_ref().map(|g| g.title.as_str()),
            Some("Pre-V4 goal"),
            "[req_m3] FAIL - Goal data did not survive the V3 -> V4 upgrade"
        );
        let project_after = get_project(&conn, project_id).unwrap();
        assert_eq!(
            project_after.as_ref().map(|p| p.id),
            Some(project_id),
            "[req_m3] FAIL - Project data did not survive the V3 -> V4 upgrade"
        );
        let relation_after = get_goal_relation(&conn, relation_id).unwrap();
        assert!(
            relation_after.is_some(),
            "[req_m3] FAIL - GoalRelation data did not survive the V3 -> V4 upgrade"
        );

        let has_milestones_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='milestones'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(
            has_milestones_table,
            "[req_m3] FAIL - milestones table missing after upgrade"
        );

        match add_milestone(
            &mut conn,
            ui_explicit(),
            mk_add_milestone_payload(project_id, "Post-V4 milestone", 0),
        ) {
            ActionOutcome::Success { .. } => println!(
                "[req_m3] PASS - V3 data survived the upgrade to V4, and a new AddMilestone works against the pre-existing Project"
            ),
            other => panic!(
                "[req_m3] FAIL - expected AddMilestone to succeed after upgrade, got {other:?}"
            ),
        }
    }
}

/// Requirements 4, 5, 6: a valid AddMilestone succeeds, defaults to
/// PENDING, and survives a full connection close/reopen.
#[test]
fn req_m4_5_6_valid_add_milestone_defaults_pending_and_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone4.db");

    let (project_id, milestone_id);
    {
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let project = match create_project(
            &mut conn,
            ui_explicit(),
            mk_create_project_payload("Van conversion", None),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        project_id = project.id;

        match add_milestone(
            &mut conn,
            ui_explicit(),
            mk_add_milestone_payload(project_id, "  Buy the van  ", 0),
        ) {
            ActionOutcome::Success {
                value,
                confirmation_class,
            } => {
                assert_eq!(
                    value.title, "Buy the van",
                    "[req_m4] FAIL - title was not trimmed"
                );
                assert_eq!(
                    value.status,
                    MilestoneStatus::Pending,
                    "[req_m5] FAIL - a new Milestone must default to PENDING"
                );
                assert_eq!(confirmation_class, ConfirmationClass::ExplicitAdd);
                milestone_id = value.id;
                println!(
                    "[req_m4/5] PASS - AddMilestone succeeded and defaulted to PENDING: {value:?}"
                );
            }
            other => panic!("[req_m4] FAIL - expected Success, got {other:?}"),
        }
    }

    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        let fetched = get_milestone(&conn, milestone_id).unwrap();
        assert!(
            fetched.is_some(),
            "[req_m6] FAIL - milestone not found after reopen"
        );
        assert_eq!(fetched.unwrap().title, "Buy the van");
        println!("[req_m6] PASS - milestone survived a full connection close/reopen");
    }
}

/// Requirement 7: a nonexistent `project_id` is rejected before any
/// write - zero milestone rows written.
#[test]
fn req_m7_nonexistent_project_id_rejected_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone7.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_project_id = crate::domain::ProjectId::new();
    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(fake_project_id, "Dangling reference", 0),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(
                errors.contains(&ValidationError::ReferencedProjectNotFound),
                "[req_m7] FAIL - expected ReferencedProjectNotFound, got {errors:?}"
            );
        }
        other => panic!("[req_m7] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        milestone_row_count(&conn),
        0,
        "[req_m7] FAIL - a milestone row was written despite the dangling project_id"
    );
    println!(
        "[req_m7] PASS - nonexistent project_id rejected before persistence, zero rows written"
    );
}

/// Requirement 8: a duplicate `(project_id, order_index)` within the SAME
/// Project is rejected before any write - resolves the M2 Project Child
/// Action Freeze report's open question.
#[test]
fn req_m8_duplicate_order_index_same_project_rejected_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone8.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "First at slot 0", 0),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Second at slot 0", 0),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(
                errors.contains(&ValidationError::DuplicateMilestoneOrderIndex),
                "[req_m8] FAIL - expected DuplicateMilestoneOrderIndex, got {errors:?}"
            );
        }
        other => panic!("[req_m8] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        milestone_row_count(&conn),
        1,
        "[req_m8] FAIL - expected exactly 1 row (the duplicate must not have been written)"
    );
    println!("[req_m8] PASS - duplicate order_index within the same Project rejected before persistence, still exactly 1 row");
}

/// Requirement 9: the SAME order_index reused by a DIFFERENT Project
/// succeeds - the resolved uniqueness rule is scoped to
/// (project_id, order_index) together, not order_index alone.
#[test]
fn req_m9_same_order_index_different_project_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone9.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project_a = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project A", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let project_b = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project B", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project_a.id, "A's slot 0", 0),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_m9] FAIL - expected Success for Project A, got {other:?}"),
    }
    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project_b.id, "B's slot 0", 0),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_m9] FAIL - expected Success for Project B, got {other:?}"),
    }

    assert_eq!(milestone_row_count(&conn), 2);
    println!("[req_m9] PASS - two different Projects both reusing order_index=0 coexist as 2 distinct rows");
}

/// Requirements 10, 11, 12: UpdateMilestone's `title` and `target_date`
/// (both `Set` and `Clear`) patch semantics all succeed and persist.
#[test]
fn req_m10_11_12_update_milestone_title_and_target_date_patch_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone10.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Original title", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert!(created.target_date.is_none());

    // Requirement 10: title update.
    let renamed = match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: created.id,
            title: Some("  Renamed  ".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_m10] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(renamed.title, "Renamed");
    println!("[req_m10] PASS - UpdateMilestone title succeeded and was trimmed");

    // Requirement 11: target_date Set.
    let now = chrono::Utc::now();
    let with_date = match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: created.id,
            target_date: FieldPatch::Set(now),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_m11] FAIL - expected Success, got {other:?}"),
    };
    assert!(with_date.target_date.is_some());
    println!("[req_m11] PASS - UpdateMilestone target_date Set succeeded");

    // Requirement 12: target_date Clear -> SQL NULL, re-verified via a
    // fresh read.
    let cleared = match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: created.id,
            target_date: FieldPatch::Clear,
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_m12] FAIL - expected Success, got {other:?}"),
    };
    assert!(
        cleared.target_date.is_none(),
        "[req_m12] FAIL - Clear did not null target_date"
    );
    let refetched = get_milestone(&conn, created.id).unwrap().unwrap();
    assert!(
        refetched.target_date.is_none(),
        "[req_m12] FAIL - Clear did not persist as SQL NULL"
    );
    println!("[req_m12] PASS - UpdateMilestone target_date Clear persisted as NULL");
}

/// Requirements 13 & 14: PENDING -> DONE and DONE -> PENDING both succeed
/// - no terminal-state rule.
#[test]
fn req_m13_14_status_pending_done_both_directions_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone13.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Reversible status", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(created.status, MilestoneStatus::Pending);

    let done = match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: created.id,
            status: Some(MilestoneStatus::Done),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_m13] FAIL - PENDING -> DONE should succeed, got {other:?}"),
    };
    assert_eq!(done.status, MilestoneStatus::Done);
    println!("[req_m13] PASS - PENDING -> DONE succeeded");

    let reopened = match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: created.id,
            status: Some(MilestoneStatus::Pending),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!(
            "[req_m14] FAIL - DONE -> PENDING should succeed - no terminal-state rule is frozen, got {other:?}"
        ),
    };
    assert_eq!(reopened.status, MilestoneStatus::Pending);
    println!("[req_m14] PASS - DONE -> PENDING succeeded - PENDING/DONE are freely reversible peer states");
}

/// Requirements 15 & 16: an `order_index` update succeeds when the
/// target slot is free, and is rejected with zero mutation when the
/// target slot is occupied by a DIFFERENT Milestone in the same Project.
#[test]
fn req_m15_16_order_index_update_free_succeeds_occupied_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone15.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let first = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Slot 0", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let second = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Slot 1", 1),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    // Requirement 15: move `first` to a genuinely free slot (2).
    let moved = match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: first.id,
            order_index: Some(2),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_m15] FAIL - moving to a free slot should succeed, got {other:?}"),
    };
    assert_eq!(moved.order_index, 2);
    println!("[req_m15] PASS - order_index update to a free slot succeeded");

    // Requirement 16: try to move `second` (currently at 1) onto
    // `first`'s now-current slot (2) - occupied by a DIFFERENT Milestone.
    let before = get_milestone(&conn, second.id).unwrap().unwrap();
    match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: second.id,
            order_index: Some(2),
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::DuplicateMilestoneOrderIndex));
        }
        other => panic!("[req_m16] FAIL - expected ValidationFailed, got {other:?}"),
    }
    let after = get_milestone(&conn, second.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_m16] FAIL - milestone changed despite the rejected order_index move"
    );
    println!("[req_m16] PASS - order_index move onto another Milestone's occupied slot rejected, zero mutation");
}

/// Requirement 17: an UpdateMilestone touching no fields is rejected.
#[test]
fn req_m17_update_milestone_no_fields_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone17.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "No-op target", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: created.id,
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
        }
        other => panic!("[req_m17] FAIL - expected ValidationFailed, got {other:?}"),
    }
    println!("[req_m17] PASS - no-op UpdateMilestone rejected with NoFieldsToUpdate");
}

/// Requirement 18: UpdateMilestone on a nonexistent id returns structured
/// NotFound.
#[test]
fn req_m18_update_nonexistent_milestone_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone18.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_id = crate::domain::MilestoneId::new();
    match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: fake_id,
            title: Some("Doesn't matter".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::NotFound => {
            println!(
                "[req_m18] PASS - UpdateMilestone on a nonexistent id returned structured NotFound"
            )
        }
        other => panic!("[req_m18] FAIL - expected NotFound, got {other:?}"),
    }
}

/// Requirement 19: AddMilestone's audit row carries the required
/// actor/provenance/confirmation-class metadata.
#[test]
fn req_m19_add_milestone_audit_metadata_correct() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone19.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Audited milestone", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let (action_type, actor, provenance, confirmation_class, previous_snapshot): (
        String,
        String,
        String,
        String,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT action_type, actor, provenance, confirmation_class, previous_snapshot FROM milestone_audit_log WHERE milestone_id = ?1",
            [created.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();

    assert_eq!(action_type, "AddMilestone");
    assert_eq!(actor, "UI");
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "EXPLICIT_ADD");
    assert_eq!(
        previous_snapshot, None,
        "[req_m19] FAIL - AddMilestone's audit row must have no previous_snapshot"
    );
    println!(
        "[req_m19] PASS - AddMilestone audit row carries action_type={action_type}, actor={actor}, provenance={provenance}, confirmation_class={confirmation_class}, previous_snapshot=None"
    );
}

/// Requirement 20: UpdateMilestone's audit row contains a real previous
/// snapshot (the pre-update Milestone state).
#[test]
fn req_m20_update_milestone_audit_contains_previous_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone20.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Pre-update title", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: created.id,
            title: Some("Post-update title".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let previous_snapshot: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM milestone_audit_log WHERE milestone_id = ?1 AND action_type = 'UpdateMilestone'",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let snapshot = previous_snapshot
        .expect("[req_m20] FAIL - UpdateMilestone audit row must have a previous_snapshot");
    assert!(
        snapshot.contains("Pre-update title"),
        "[req_m20] FAIL - previous_snapshot does not contain the pre-update title, got {snapshot}"
    );
    println!("[req_m20] PASS - UpdateMilestone audit row's previous_snapshot contains the pre-update state: {snapshot}");
}

/// Requirement 21: AddMilestone's milestone-insert + audit-insert are one
/// atomic transaction - same forced-failure proof as Goal/Project/
/// GoalRelation hardening.
#[test]
fn req_m21_add_milestone_audit_failure_rolls_back_milestone_insert() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone21.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    install_milestone_audit_failure_trigger(&conn);

    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Should never persist", 0),
    ) {
        ActionOutcome::StorageError(e) => {
            assert_eq!(e.kind(), CoreErrorKind::Storage);
            println!(
                "[req_m21] PASS - forced audit failure surfaced as a typed StorageError: {e:?}"
            );
        }
        other => panic!("[req_m21] FAIL - expected StorageError, got {other:?}"),
    }

    assert_eq!(
        milestone_row_count(&conn),
        0,
        "[req_m21] FAIL - a milestone row survived a rolled-back transaction"
    );
    assert_eq!(
        milestone_audit_row_count(&conn),
        0,
        "[req_m21] FAIL - an audit row survived a rolled-back transaction"
    );

    remove_milestone_audit_failure_trigger(&conn);

    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Now it works", 0),
    ) {
        ActionOutcome::Success { .. } => {
            println!("[req_m21] PASS - connection remains usable after the rollback")
        }
        other => panic!("[req_m21] FAIL - connection unusable after rollback: {other:?}"),
    }
}

/// Requirement 22: UpdateMilestone's milestone-update + audit-insert are
/// one atomic transaction.
#[test]
fn req_m22_update_milestone_audit_failure_rolls_back_milestone_update() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone22.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Stable", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_milestone(&conn, created.id).unwrap().unwrap();

    install_milestone_audit_failure_trigger(&conn);

    match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: created.id,
            title: Some("Should not persist".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::StorageError(e) => {
            assert_eq!(e.kind(), CoreErrorKind::Storage);
            println!(
                "[req_m22] PASS - forced audit failure surfaced as a typed StorageError: {e:?}"
            );
        }
        other => panic!("[req_m22] FAIL - expected StorageError, got {other:?}"),
    }

    remove_milestone_audit_failure_trigger(&conn);

    let after = get_milestone(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_m22] FAIL - milestone mutated despite a rolled-back transaction"
    );
    let audit_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM milestone_audit_log WHERE milestone_id = ?1",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        audit_count, 1,
        "[req_m22] FAIL - expected exactly 1 audit row (the original AddMilestone), got {audit_count}"
    );
    println!(
        "[req_m22] PASS - milestone left identical to its pre-update state, no orphan audit row"
    );
}

/// Requirements 23 & 24: Inferred AddMilestone blocks before write, then
/// a confirmed resubmission succeeds.
#[test]
fn req_m23_24_inferred_add_milestone_requires_confirmation_then_confirmed_resubmission_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone23.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let payload = mk_add_milestone_payload(project.id, "AI-proposed milestone", 0);
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    assert!(!meta.confirmed);

    match add_milestone(&mut conn, meta, payload.clone()) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!("[req_m23] FAIL - expected ConfirmationRequired, got {other:?}"),
    }
    assert_eq!(
        milestone_row_count(&conn),
        0,
        "[req_m23] FAIL - a milestone was written before confirmation"
    );
    println!(
        "[req_m23] PASS - Inferred AddMilestone blocked with ConfirmationRequired, zero rows written"
    );

    let mut confirmed_meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    confirmed_meta.confirmed = true;
    match add_milestone(&mut conn, confirmed_meta, payload) {
        ActionOutcome::Success {
            confirmation_class, ..
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
            assert_eq!(milestone_row_count(&conn), 1);
            println!("[req_m24] PASS - resubmission with confirmed=true persisted the milestone");
        }
        other => {
            panic!("[req_m24] FAIL - expected Success on confirmed resubmission, got {other:?}")
        }
    }
}

/// Requirement 25: Inferred UpdateMilestone blocks before write, zero
/// mutation.
#[test]
fn req_m25_inferred_update_milestone_requires_confirmation_and_does_not_mutate() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone25.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Stable title", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_milestone(&conn, created.id).unwrap().unwrap();

    let payload = UpdateMilestonePayload {
        id: created.id,
        title: Some("AI-proposed rename".to_string()),
        ..Default::default()
    };
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    assert!(!meta.confirmed);

    match update_milestone(&mut conn, meta, payload) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!("[req_m25] FAIL - expected ConfirmationRequired, got {other:?}"),
    }
    let after = get_milestone(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_m25] FAIL - milestone mutated before confirmation"
    );
    println!(
        "[req_m25] PASS - Inferred UpdateMilestone blocked with ConfirmationRequired, zero mutation"
    );
}

/// Requirement 26: a direct, raw SQL insert naming a nonexistent
/// `project_id` is rejected by SQLite's own FK enforcement - bypasses
/// `AddMilestone` entirely, raw SQL kept local to this test.
#[test]
fn req_m26_raw_sql_invalid_project_fk_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone26.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_project_id = crate::domain::ProjectId::new().to_string();
    let milestone_id = crate::domain::MilestoneId::new().to_string();
    let result = conn.execute(
        "INSERT INTO milestones (id, project_id, title, target_date, status, order_index)
         VALUES (?1, ?2, 'Bypasses AddMilestone validation', NULL, 'PENDING', 0)",
        rusqlite::params![milestone_id, fake_project_id],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!(
                "[req_m26] PASS - raw SQL insert with a nonexistent project_id rejected by FK: {err:?}"
            );
        }
        other => panic!("[req_m26] FAIL - expected an SQLite FK constraint error, got {other:?}"),
    }
    assert_eq!(milestone_row_count(&conn), 0);
}

/// Requirement 27: a direct, raw SQL duplicate `(project_id,
/// order_index)` insert is rejected by the `UNIQUE` constraint.
#[test]
fn req_m27_raw_sql_duplicate_order_index_rejected_by_unique() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone27.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Slot 0", 0),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let milestone_id = crate::domain::MilestoneId::new().to_string();
    let result = conn.execute(
        "INSERT INTO milestones (id, project_id, title, target_date, status, order_index)
         VALUES (?1, ?2, 'Duplicate slot', NULL, 'PENDING', 0)",
        rusqlite::params![milestone_id, project.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!("[req_m27] PASS - raw SQL duplicate (project_id, order_index) insert rejected by UNIQUE: {err:?}");
        }
        other => {
            panic!("[req_m27] FAIL - expected an SQLite UNIQUE constraint error, got {other:?}")
        }
    }
    assert_eq!(milestone_row_count(&conn), 1);
}

/// Requirement 28: a direct, raw SQL insert with an invalid `status` text
/// is rejected by the `CHECK` constraint.
#[test]
fn req_m28_raw_sql_invalid_status_rejected_by_check() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone28.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let milestone_id = crate::domain::MilestoneId::new().to_string();
    let result = conn.execute(
        "INSERT INTO milestones (id, project_id, title, target_date, status, order_index)
         VALUES (?1, ?2, 'Invalid status', NULL, 'IN_PROGRESS', 0)",
        rusqlite::params![milestone_id, project.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!(
                "[req_m28] PASS - raw SQL insert with an invalid status rejected by CHECK: {err:?}"
            );
        }
        other => {
            panic!("[req_m28] FAIL - expected an SQLite CHECK constraint error, got {other:?}")
        }
    }
    assert_eq!(milestone_row_count(&conn), 0);
}

/// Requirement 29: a direct, raw SQL insert with an empty (post-trim)
/// title is rejected by the `CHECK` constraint.
#[test]
fn req_m29_raw_sql_empty_title_rejected_by_check() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone29.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let milestone_id = crate::domain::MilestoneId::new().to_string();
    let result = conn.execute(
        "INSERT INTO milestones (id, project_id, title, target_date, status, order_index)
         VALUES (?1, ?2, '   ', NULL, 'PENDING', 0)",
        rusqlite::params![milestone_id, project.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!(
                "[req_m29] PASS - raw SQL insert with a whitespace-only title rejected by CHECK: {err:?}"
            );
        }
        other => {
            panic!("[req_m29] FAIL - expected an SQLite CHECK constraint error, got {other:?}")
        }
    }
    assert_eq!(milestone_row_count(&conn), 0);
}

/// Requirement 30: a direct, raw SQL hard delete of a Project referenced
/// by a Milestone is rejected by `ON DELETE RESTRICT`.
#[test]
fn req_m30_raw_hard_delete_of_referenced_project_is_restricted() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone30.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Referenced project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Keeps the project alive", 0),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let result = conn.execute(
        "DELETE FROM projects WHERE id = ?1",
        rusqlite::params![project.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!(
                "[req_m30] PASS - raw DELETE of a Project referenced by a Milestone rejected by ON DELETE RESTRICT: {err:?}"
            );
        }
        other => panic!(
            "[req_m30] FAIL - expected an SQLite FK constraint error blocking the delete, got {other:?}"
        ),
    }

    let project_still_exists = get_project(&conn, project.id).unwrap();
    assert!(
        project_still_exists.is_some(),
        "[req_m30] FAIL - Project was deleted despite the RESTRICT constraint"
    );
    assert_eq!(
        milestone_row_count(&conn),
        1,
        "[req_m30] FAIL - milestone row disappeared despite the delete being rejected"
    );
    println!("[req_m30] PASS - Project remains, and the milestone remains - no dangling milestone exists");
}

/// Requirement 31: `get_milestones_for_project` returns milestones
/// ordered deterministically by `order_index ASC`.
#[test]
fn req_m31_get_milestones_for_project_returns_order_index_ascending() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "milestone31.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    // Deliberately inserted out of order.
    let third = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Third", 2),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let first = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "First", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let second = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Second", 1),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let milestones = get_milestones_for_project(&conn, project.id).unwrap();
    assert_eq!(milestones.len(), 3);
    assert_eq!(
        milestones.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![first.id, second.id, third.id],
        "[req_m31] FAIL - expected order_index ASC ordering (First, Second, Third)"
    );
    println!("[req_m31] PASS - get_milestones_for_project returned milestones ordered by order_index ASC, independent of insertion order");
}

// =========================================================================
// M2 Task vertical slice (2026-09-17) - new evidence below. Evidence for
// `docs/reports/R012_M2_TASK_VERTICAL_SLICE_2026-09-17.md`. The fifth
// real M2 domain entity, and the first with two independent nullable
// parent references - `tasks.project_id`/`tasks.goal_id`, both
// `ON DELETE RESTRICT`.
// =========================================================================

fn mk_add_task_payload(
    title: &str,
    due_at: Option<chrono::DateTime<chrono::Utc>>,
    project_id: Option<crate::domain::ProjectId>,
    goal_id: Option<crate::domain::GoalId>,
) -> AddTaskPayload {
    AddTaskPayload {
        title: title.to_string(),
        due_at,
        project_id,
        goal_id,
    }
}

fn task_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
        .unwrap()
}

fn task_audit_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM task_audit_log", [], |r| r.get(0))
        .unwrap()
}

/// Same forced-failure mechanism as every other entity's audit-failure
/// trigger, targeting `task_audit_log`.
fn install_task_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER force_task_audit_failure
         BEFORE INSERT ON task_audit_log
         BEGIN
             SELECT RAISE(FAIL, 'forced task audit failure');
         END;",
    )
    .expect("failed to install the forced task-audit-failure trigger");
}

fn remove_task_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch("DROP TRIGGER IF EXISTS force_task_audit_failure;")
        .expect("failed to remove the forced task-audit-failure trigger");
}

/// Requirement 1: V5 migration creates the Task schema.
#[test]
fn req_t1_v5_migration_creates_task_schema() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    for table in ["tasks", "task_audit_log"] {
        let exists: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(exists, "[req_t1] FAIL - missing table {table}");
    }
    println!("[req_t1] PASS - V5 migration created tasks + task_audit_log");
}

/// Requirement 2: a fresh V1->V2->V3->V4->V5 database works end to end -
/// a Goal, a Project linked to it, a GoalRelation, a Milestone, and a
/// Task all created together in one migration run.
#[test]
fn req_t2_fresh_v1_v2_v3_v4_v5_database_works_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal_a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project linked to A", Some(goal_a.id)),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match link_goals(
        &mut conn,
        ui_explicit(),
        mk_link_payload(goal_a.id, goal_b.id, GoalRelationType::Supports),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }
    match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "First milestone", 0),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("First task", None, Some(project.id), Some(goal_a.id)),
    ) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.status, TaskStatus::Todo);
        }
        other => panic!("[req_t2] FAIL - expected Success, got {other:?}"),
    }
    println!("[req_t2] PASS - fresh V1->V2->V3->V4->V5 database supports Goal, Project, GoalRelation, Milestone, and Task together");
}

/// Requirement 3 (migration compatibility): an existing database
/// populated only through V4 (real Goal/Project/GoalRelation/Milestone
/// rows - the actual shape of an existing user's database before this
/// update) upgrades cleanly to V5 - all prior data unchanged, the tasks
/// table appears, and new Tasks can reference the pre-existing Project,
/// the pre-existing Goal, both, or neither.
#[test]
fn req_t3_v4_populated_database_upgrades_cleanly_to_v5() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task3.db");

    let (goal_id, project_id, relation_id, milestone_id);
    {
        let mut conn = bootstrap_db_at_version(&path, &key, 4).unwrap();
        let has_tasks_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='tasks'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(
            !has_tasks_table,
            "[req_t3] FAIL - a V4-only database already has the tasks table"
        );

        let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Pre-V5 goal")) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        goal_id = goal.id;
        let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Pre-V5 goal B"))
        {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        let project = match create_project(
            &mut conn,
            ui_explicit(),
            mk_create_project_payload("Pre-V5 project", Some(goal_id)),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        project_id = project.id;
        let relation = match link_goals(
            &mut conn,
            ui_explicit(),
            mk_link_payload(goal_id, goal_b.id, GoalRelationType::Supports),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        relation_id = relation.id;
        let milestone = match add_milestone(
            &mut conn,
            ui_explicit(),
            mk_add_milestone_payload(project_id, "Pre-V5 milestone", 0),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        milestone_id = milestone.id;
    }

    {
        let mut conn = bootstrap_db_at(&path, &key).unwrap();

        let goal_after = get_goal(&conn, goal_id).unwrap();
        assert_eq!(
            goal_after.as_ref().map(|g| g.title.as_str()),
            Some("Pre-V5 goal"),
            "[req_t3] FAIL - Goal data did not survive the V4 -> V5 upgrade"
        );
        let project_after = get_project(&conn, project_id).unwrap();
        assert_eq!(
            project_after.as_ref().map(|p| p.id),
            Some(project_id),
            "[req_t3] FAIL - Project data did not survive the V4 -> V5 upgrade"
        );
        let relation_after = get_goal_relation(&conn, relation_id).unwrap();
        assert!(
            relation_after.is_some(),
            "[req_t3] FAIL - GoalRelation data did not survive the V4 -> V5 upgrade"
        );
        let milestone_after = get_milestone(&conn, milestone_id).unwrap();
        assert!(
            milestone_after.is_some(),
            "[req_t3] FAIL - Milestone data did not survive the V4 -> V5 upgrade"
        );

        let has_tasks_table: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='tasks'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap();
        assert!(
            has_tasks_table,
            "[req_t3] FAIL - tasks table missing after upgrade"
        );

        // Tasks referencing: the pre-existing Project, the pre-existing
        // Goal, both, and neither - all four ownership modes against
        // upgraded, pre-existing data.
        for (title, proj, goal) in [
            ("Post-V5 standalone", None, None),
            ("Post-V5 project-only", Some(project_id), None),
            ("Post-V5 goal-only", None, Some(goal_id)),
            ("Post-V5 both", Some(project_id), Some(goal_id)),
        ] {
            match add_task(
                &mut conn,
                ui_explicit(),
                mk_add_task_payload(title, None, proj, goal),
            ) {
                ActionOutcome::Success { .. } => {}
                other => panic!(
                    "[req_t3] FAIL - AddTask({title}) should succeed after upgrade, got {other:?}"
                ),
            }
        }
        println!("[req_t3] PASS - V4 data survived the upgrade to V5, and new Tasks work in all four ownership modes against pre-existing Project/Goal");
    }
}

/// Requirements 4, 5, 6, 7, 8, 9: AddTask succeeds in all four ownership
/// modes (standalone, Project-only, Goal-only, both), every new Task
/// defaults to `TODO`, and `recurring_rule_id` defaults to `NULL`
/// (verified via a direct raw-SQL read, not just the in-memory value).
#[test]
fn req_t4_5_6_7_8_9_add_task_all_ownership_modes_default_todo_and_null_recurring_rule() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task4.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let cases: [(
        &str,
        Option<crate::domain::ProjectId>,
        Option<crate::domain::GoalId>,
    ); 4] = [
        ("Standalone", None, None),
        ("Project-only", Some(project.id), None),
        ("Goal-only", None, Some(goal.id)),
        ("Project+Goal", Some(project.id), Some(goal.id)),
    ];

    for (title, proj, g) in cases {
        let created = match add_task(
            &mut conn,
            ui_explicit(),
            mk_add_task_payload(title, None, proj, g),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("[req_t4-7] FAIL - AddTask({title}) should succeed, got {other:?}"),
        };
        assert_eq!(
            created.project_id, proj,
            "[req_t4-7] FAIL - project_id mismatch for {title}"
        );
        assert_eq!(
            created.goal_id, g,
            "[req_t4-7] FAIL - goal_id mismatch for {title}"
        );
        assert_eq!(
            created.status,
            TaskStatus::Todo,
            "[req_t8] FAIL - new Task must default to TODO"
        );
        assert_eq!(
            created.recurring_rule_id, None,
            "[req_t9] FAIL - new Task must default recurring_rule_id to None"
        );

        let raw_recurring_rule_id: Option<String> = conn
            .query_row(
                "SELECT recurring_rule_id FROM tasks WHERE id = ?1",
                rusqlite::params![created.id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            raw_recurring_rule_id, None,
            "[req_t9] FAIL - recurring_rule_id must be persisted as SQL NULL for {title}"
        );
    }
    println!("[req_t4-9] PASS - AddTask succeeded in all four ownership modes, every new Task defaulted to TODO with recurring_rule_id = NULL");
}

/// Requirement 10: a Task survives a full connection close/reopen.
#[test]
fn req_t10_task_survives_close_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task10.db");

    let task_id;
    {
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let created = match add_task(
            &mut conn,
            ui_explicit(),
            mk_add_task_payload("  Persisted task  ", None, None, None),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("[req_t10] FAIL - expected Success, got {other:?}"),
        };
        assert_eq!(
            created.title, "Persisted task",
            "[req_t10] FAIL - title was not trimmed"
        );
        task_id = created.id;
    }

    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        let fetched = get_task(&conn, task_id).unwrap();
        assert!(
            fetched.is_some(),
            "[req_t10] FAIL - task not found after reopen"
        );
        assert_eq!(fetched.unwrap().title, "Persisted task");
        println!("[req_t10] PASS - task survived a full connection close/reopen");
    }
}

/// Requirement 11: a nonexistent `project_id` is rejected before any
/// write - zero task rows written.
#[test]
fn req_t11_nonexistent_project_id_rejected_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task11.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_project_id = crate::domain::ProjectId::new();
    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload(
            "Dangling project reference",
            None,
            Some(fake_project_id),
            None,
        ),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(
                errors.contains(&ValidationError::ReferencedProjectNotFound),
                "[req_t11] FAIL - expected ReferencedProjectNotFound, got {errors:?}"
            );
        }
        other => panic!("[req_t11] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        task_row_count(&conn),
        0,
        "[req_t11] FAIL - a task row was written despite the dangling project_id"
    );
    println!(
        "[req_t11] PASS - nonexistent project_id rejected before persistence, zero rows written"
    );
}

/// Requirement 12: a nonexistent `goal_id` is rejected before any write
/// - zero task rows written.
#[test]
fn req_t12_nonexistent_goal_id_rejected_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task12.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_goal_id = crate::domain::GoalId::new();
    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Dangling goal reference", None, None, Some(fake_goal_id)),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(
                errors.contains(&ValidationError::ReferencedGoalNotFound),
                "[req_t12] FAIL - expected ReferencedGoalNotFound, got {errors:?}"
            );
        }
        other => panic!("[req_t12] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        task_row_count(&conn),
        0,
        "[req_t12] FAIL - a task row was written despite the dangling goal_id"
    );
    println!("[req_t12] PASS - nonexistent goal_id rejected before persistence, zero rows written");
}

// =========================================================================
// M2 Task parent-validation hardening (2026-09-17) - new evidence below.
// Evidence for
// `docs/reports/R013_M2_TASK_PARENT_VALIDATION_HARDENING_2026-09-17.md`.
// Proves `add_task`/`update_task` check `project_id`/`goal_id`
// independently and aggregate BOTH errors (stable Project-then-Goal
// order) when both are invalid, rather than short-circuiting on the
// first missing reference (the bug this pass fixes).
// =========================================================================

/// Requirement A: AddTask with BOTH `project_id` and `goal_id`
/// nonexistent returns BOTH validation errors, in stable
/// `ReferencedProjectNotFound`-then-`ReferencedGoalNotFound` order, with
/// zero writes to either `tasks` or `task_audit_log`.
#[test]
fn req_tpv_a_add_task_both_parents_invalid_returns_both_errors_in_stable_order() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "taskpv_a.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_project_id = crate::domain::ProjectId::new();
    let fake_goal_id = crate::domain::GoalId::new();
    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload(
            "Both parents dangling",
            None,
            Some(fake_project_id),
            Some(fake_goal_id),
        ),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert_eq!(
                errors,
                vec![
                    ValidationError::ReferencedProjectNotFound,
                    ValidationError::ReferencedGoalNotFound,
                ],
                "[req_tpv_a] FAIL - expected both errors in stable Project-then-Goal order, got {errors:?}"
            );
        }
        other => panic!("[req_tpv_a] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        task_row_count(&conn),
        0,
        "[req_tpv_a] FAIL - a task row was written despite both dangling references"
    );
    assert_eq!(
        task_audit_row_count(&conn),
        0,
        "[req_tpv_a] FAIL - an audit row was written despite both dangling references"
    );
    println!("[req_tpv_a] PASS - AddTask with both parents invalid returned both errors in stable order, zero writes");
}

/// Requirement B: AddTask with a valid `project_id` but an invalid
/// `goal_id` still runs the Goal check independently and returns
/// `ReferencedGoalNotFound`, zero writes.
#[test]
fn req_tpv_b_add_task_project_valid_goal_invalid_returns_goal_error() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "taskpv_b.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Valid project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let fake_goal_id = crate::domain::GoalId::new();

    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload(
            "Valid project, dangling goal",
            None,
            Some(project.id),
            Some(fake_goal_id),
        ),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert_eq!(
                errors,
                vec![ValidationError::ReferencedGoalNotFound],
                "[req_tpv_b] FAIL - expected only ReferencedGoalNotFound, got {errors:?}"
            );
        }
        other => panic!("[req_tpv_b] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        task_row_count(&conn),
        0,
        "[req_tpv_b] FAIL - a task row was written"
    );
    println!("[req_tpv_b] PASS - Goal validation still ran independently of the valid Project, zero writes");
}

/// Requirement C: AddTask with an invalid `project_id` but a valid
/// `goal_id` still runs the Goal check (it must not be skipped just
/// because Project failed first) and returns only
/// `ReferencedProjectNotFound`, zero writes.
#[test]
fn req_tpv_c_add_task_project_invalid_goal_valid_returns_only_project_error() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "taskpv_c.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Valid goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let fake_project_id = crate::domain::ProjectId::new();

    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload(
            "Dangling project, valid goal",
            None,
            Some(fake_project_id),
            Some(goal.id),
        ),
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert_eq!(
                errors,
                vec![ValidationError::ReferencedProjectNotFound],
                "[req_tpv_c] FAIL - expected only ReferencedProjectNotFound, got {errors:?}"
            );
        }
        other => panic!("[req_tpv_c] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        task_row_count(&conn),
        0,
        "[req_tpv_c] FAIL - a task row was written"
    );
    println!("[req_tpv_c] PASS - Goal lookup still ran even though Project was invalid, only the Project error was returned, zero writes");
}

/// Requirement D: UpdateTask with BOTH `project_id`/`goal_id` set to
/// nonexistent targets returns BOTH errors in stable order, and leaves
/// the Task - including its timestamps - byte/logically identical, with
/// no new audit row.
#[test]
fn req_tpv_d_update_task_both_parents_invalid_set_returns_both_errors_in_stable_order() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "taskpv_d.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Stable task", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_task(&conn, created.id).unwrap().unwrap();
    let audit_count_before = task_audit_row_count(&conn);

    let fake_project_id = crate::domain::ProjectId::new();
    let fake_goal_id = crate::domain::GoalId::new();
    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            project_id: FieldPatch::Set(fake_project_id),
            goal_id: FieldPatch::Set(fake_goal_id),
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert_eq!(
                errors,
                vec![
                    ValidationError::ReferencedProjectNotFound,
                    ValidationError::ReferencedGoalNotFound,
                ],
                "[req_tpv_d] FAIL - expected both errors in stable Project-then-Goal order, got {errors:?}"
            );
        }
        other => panic!("[req_tpv_d] FAIL - expected ValidationFailed, got {other:?}"),
    }

    let after = get_task(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_tpv_d] FAIL - task changed despite both parents being invalid"
    );
    assert_eq!(
        after.updated_at, before.updated_at,
        "[req_tpv_d] FAIL - updated_at changed despite the rejected update"
    );
    assert_eq!(
        task_audit_row_count(&conn),
        audit_count_before,
        "[req_tpv_d] FAIL - an audit row was written despite the rejected update"
    );
    println!("[req_tpv_d] PASS - UpdateTask with both Set targets invalid returned both errors in stable order, zero mutation, no new audit row");
}

/// Requirement E: UpdateTask with a valid `project_id` Set but an
/// invalid `goal_id` Set still checks Goal independently and returns
/// only `ReferencedGoalNotFound`, zero mutation.
#[test]
fn req_tpv_e_update_task_project_valid_goal_invalid_returns_goal_error() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "taskpv_e.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Valid project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Stable task", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_task(&conn, created.id).unwrap().unwrap();

    let fake_goal_id = crate::domain::GoalId::new();
    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            project_id: FieldPatch::Set(project.id),
            goal_id: FieldPatch::Set(fake_goal_id),
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert_eq!(
                errors,
                vec![ValidationError::ReferencedGoalNotFound],
                "[req_tpv_e] FAIL - expected only ReferencedGoalNotFound, got {errors:?}"
            );
        }
        other => panic!("[req_tpv_e] FAIL - expected ValidationFailed, got {other:?}"),
    }
    let after = get_task(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_tpv_e] FAIL - task changed despite the invalid goal_id"
    );
    println!("[req_tpv_e] PASS - Goal checked independently of the valid Project, zero mutation");
}

/// Requirement F: UpdateTask with an invalid `project_id` Set but a
/// valid `goal_id` Set still performs both applicable lookups and
/// returns only `ReferencedProjectNotFound`, zero mutation.
#[test]
fn req_tpv_f_update_task_project_invalid_goal_valid_returns_only_project_error() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "taskpv_f.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Valid goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Stable task", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_task(&conn, created.id).unwrap().unwrap();

    let fake_project_id = crate::domain::ProjectId::new();
    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            project_id: FieldPatch::Set(fake_project_id),
            goal_id: FieldPatch::Set(goal.id),
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert_eq!(
                errors,
                vec![ValidationError::ReferencedProjectNotFound],
                "[req_tpv_f] FAIL - expected only ReferencedProjectNotFound, got {errors:?}"
            );
        }
        other => panic!("[req_tpv_f] FAIL - expected ValidationFailed, got {other:?}"),
    }
    let after = get_task(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_tpv_f] FAIL - task changed despite the invalid project_id"
    );
    println!("[req_tpv_f] PASS - both applicable lookups ran, only the Project error was returned, zero mutation");
}

/// Requirements 13, 14-21: both valid parents persist correctly, and
/// every `UpdateTask` patch-semantics case succeeds and persists -
/// title, `due_at` Set/Clear, `project_id` Set/Clear, `goal_id`
/// Set/Clear, and both parents cleared together in one update.
#[test]
fn req_t13_14_to_21_update_task_full_patch_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task13.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    // Requirement 13: both valid parents persist correctly.
    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Original title", None, Some(project.id), Some(goal.id)),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(created.project_id, Some(project.id));
    assert_eq!(created.goal_id, Some(goal.id));
    println!("[req_t13] PASS - Task created with both valid parents persisted correctly");

    // Requirement 14: title update.
    let renamed = match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            title: Some("  Renamed  ".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_t14] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(renamed.title, "Renamed");
    println!("[req_t14] PASS - UpdateTask title succeeded and was trimmed");

    // Requirement 15: due_at Set.
    let now = chrono::Utc::now();
    let with_due = match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            due_at: FieldPatch::Set(now),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_t15] FAIL - expected Success, got {other:?}"),
    };
    assert!(with_due.due_at.is_some());
    println!("[req_t15] PASS - UpdateTask due_at Set succeeded");

    // Requirement 16: due_at Clear -> SQL NULL.
    let cleared_due = match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            due_at: FieldPatch::Clear,
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_t16] FAIL - expected Success, got {other:?}"),
    };
    assert!(cleared_due.due_at.is_none());
    let refetched = get_task(&conn, created.id).unwrap().unwrap();
    assert!(
        refetched.due_at.is_none(),
        "[req_t16] FAIL - due_at Clear did not persist as NULL"
    );
    println!("[req_t16] PASS - UpdateTask due_at Clear persisted as NULL");

    // Requirement 17: project_id Set (relink to a second Project).
    let project_b = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project B", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let relinked_project = match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            project_id: FieldPatch::Set(project_b.id),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_t17] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(relinked_project.project_id, Some(project_b.id));
    println!("[req_t17] PASS - UpdateTask project_id Set succeeded");

    // Requirement 18: project_id Clear -> SQL NULL.
    let cleared_project = match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            project_id: FieldPatch::Clear,
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_t18] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(cleared_project.project_id, None);
    println!("[req_t18] PASS - UpdateTask project_id Clear succeeded");

    // Requirement 19: goal_id Set (relink to a second Goal).
    let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let relinked_goal = match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            goal_id: FieldPatch::Set(goal_b.id),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_t19] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(relinked_goal.goal_id, Some(goal_b.id));
    println!("[req_t19] PASS - UpdateTask goal_id Set succeeded");

    // Requirement 20: goal_id Clear -> SQL NULL.
    let cleared_goal = match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            goal_id: FieldPatch::Clear,
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_t20] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(cleared_goal.goal_id, None);
    println!("[req_t20] PASS - UpdateTask goal_id Clear succeeded");

    // Requirement 21: both parents cleared together in one update -
    // re-link both first so there is something real to clear.
    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            project_id: FieldPatch::Set(project.id),
            goal_id: FieldPatch::Set(goal.id),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }
    let both_cleared = match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            project_id: FieldPatch::Clear,
            goal_id: FieldPatch::Clear,
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_t21] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(both_cleared.project_id, None);
    assert_eq!(both_cleared.goal_id, None);
    println!(
        "[req_t21] PASS - both parents cleared together in one UpdateTask - Task is now standalone"
    );
}

/// Requirements 22, 23: a Task's Project can be relinked, and its Goal
/// can be relinked, to a different valid target.
#[test]
fn req_t22_23_project_and_goal_can_both_be_relinked() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task22.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project_a = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project A", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let project_b = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project B", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let goal_a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Relinkable", None, Some(project_a.id), Some(goal_a.id)),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let relinked = match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            project_id: FieldPatch::Set(project_b.id),
            goal_id: FieldPatch::Set(goal_b.id),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_t22-23] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(relinked.project_id, Some(project_b.id));
    assert_eq!(relinked.goal_id, Some(goal_b.id));
    println!("[req_t22-23] PASS - both Project and Goal successfully relinked to different valid targets");
}

/// Requirements 24, 25: relinking to a nonexistent Project (or Goal)
/// causes zero mutation.
#[test]
fn req_t24_25_invalid_relink_causes_zero_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task24.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Stable task", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_task(&conn, created.id).unwrap().unwrap();

    let fake_project_id = crate::domain::ProjectId::new();
    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            project_id: FieldPatch::Set(fake_project_id),
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::ReferencedProjectNotFound));
        }
        other => panic!("[req_t24] FAIL - expected ValidationFailed, got {other:?}"),
    }
    let after_project_attempt = get_task(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after_project_attempt,
        "[req_t24] FAIL - task changed despite invalid project relink"
    );
    println!("[req_t24] PASS - invalid Project relink rejected, zero mutation");

    let fake_goal_id = crate::domain::GoalId::new();
    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            goal_id: FieldPatch::Set(fake_goal_id),
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::ReferencedGoalNotFound));
        }
        other => panic!("[req_t25] FAIL - expected ValidationFailed, got {other:?}"),
    }
    let after_goal_attempt = get_task(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after_goal_attempt,
        "[req_t25] FAIL - task changed despite invalid goal relink"
    );
    println!("[req_t25] PASS - invalid Goal relink rejected, zero mutation");
}

/// Requirement 26: an UpdateTask touching no fields is rejected.
#[test]
fn req_t26_update_task_no_fields_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task26.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("No-op target", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
        }
        other => panic!("[req_t26] FAIL - expected ValidationFailed, got {other:?}"),
    }
    println!("[req_t26] PASS - no-op UpdateTask rejected with NoFieldsToUpdate");
}

/// Requirement 27: UpdateTask on a nonexistent id returns structured
/// NotFound.
#[test]
fn req_t27_update_nonexistent_task_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task27.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_id = crate::domain::TaskId::new();
    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: fake_id,
            title: Some("Doesn't matter".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::NotFound => {
            println!("[req_t27] PASS - UpdateTask on a nonexistent id returned structured NotFound")
        }
        other => panic!("[req_t27] FAIL - expected NotFound, got {other:?}"),
    }
}

/// Requirements 28-32: representative TaskStatus transitions all
/// succeed against a real DB, demonstrating no terminal-state rule -
/// TODO -> IN_PROGRESS, IN_PROGRESS -> DONE, DONE -> TODO (reopen),
/// TODO -> CANCELLED, CANCELLED -> TODO (reopen).
#[test]
fn req_t28_to_32_representative_status_transitions_all_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task28.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Transitions", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(created.status, TaskStatus::Todo);

    let transitions = [
        (TaskStatus::InProgress, "req_t28", "TODO -> IN_PROGRESS"),
        (TaskStatus::Done, "req_t29", "IN_PROGRESS -> DONE"),
        (TaskStatus::Todo, "req_t30", "DONE -> TODO"),
        (TaskStatus::Cancelled, "req_t31", "TODO -> CANCELLED"),
        (TaskStatus::Todo, "req_t32", "CANCELLED -> TODO"),
    ];

    for (target, tag, label) in transitions {
        let updated = match update_task(
            &mut conn,
            ui_explicit(),
            UpdateTaskPayload {
                id: created.id,
                status: Some(target),
                ..Default::default()
            },
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("[{tag}] FAIL - {label} should succeed, got {other:?}"),
        };
        assert_eq!(
            updated.status, target,
            "[{tag}] FAIL - status did not update for {label}"
        );
        println!("[{tag}] PASS - {label} succeeded");
    }
}

/// Requirements 33, 34, 35: `created_at` is preserved across a
/// successful update, `updated_at` advances on a successful update, and
/// a rejected update leaves both timestamps unchanged.
#[test]
fn req_t33_34_35_timestamp_behavior_proof() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task33.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Timestamped", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let original_created_at = created.created_at;
    let original_updated_at = created.updated_at;
    assert_eq!(
        original_created_at, original_updated_at,
        "[req_t33/34] FAIL - a freshly created Task should have created_at == updated_at"
    );

    // Requirement 35: a rejected update (no fields touched) leaves both
    // timestamps unchanged.
    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            ..Default::default()
        },
    ) {
        ActionOutcome::ValidationFailed(_) => {}
        other => panic!("setup failed: {other:?}"),
    }
    let after_rejected = get_task(&conn, created.id).unwrap().unwrap();
    assert_eq!(after_rejected.created_at, original_created_at);
    assert_eq!(
        after_rejected.updated_at, original_updated_at,
        "[req_t35] FAIL - a rejected update must not change updated_at"
    );
    println!("[req_t35] PASS - rejected update left both timestamps unchanged");

    // Requirements 33, 34: a successful update preserves created_at and
    // advances updated_at.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let updated = match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            title: Some("Timestamped, renamed".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_t33/34] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(
        updated.created_at, original_created_at,
        "[req_t33] FAIL - created_at must never change"
    );
    assert!(
        updated.updated_at > original_updated_at,
        "[req_t34] FAIL - updated_at must advance on a successful update"
    );
    println!("[req_t33/34] PASS - created_at preserved, updated_at advanced on successful update");
}

/// Requirement 36: AddTask's audit row carries the required
/// actor/provenance/confirmation-class metadata.
#[test]
fn req_t36_add_task_audit_metadata_correct() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task36.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Audited task", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let (action_type, actor, provenance, confirmation_class, previous_snapshot): (
        String,
        String,
        String,
        String,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT action_type, actor, provenance, confirmation_class, previous_snapshot FROM task_audit_log WHERE task_id = ?1",
            [created.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();

    assert_eq!(action_type, "AddTask");
    assert_eq!(actor, "UI");
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "EXPLICIT_ADD");
    assert_eq!(
        previous_snapshot, None,
        "[req_t36] FAIL - AddTask's audit row must have no previous_snapshot"
    );
    println!("[req_t36] PASS - AddTask audit row carries action_type={action_type}, actor={actor}, provenance={provenance}, confirmation_class={confirmation_class}, previous_snapshot=None");
}

/// Requirement 37: UpdateTask's audit row contains a real previous
/// snapshot (the pre-update Task state).
#[test]
fn req_t37_update_task_audit_contains_previous_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task37.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Pre-update title", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            title: Some("Post-update title".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let previous_snapshot: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM task_audit_log WHERE task_id = ?1 AND action_type = 'UpdateTask'",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let snapshot = previous_snapshot
        .expect("[req_t37] FAIL - UpdateTask audit row must have a previous_snapshot");
    assert!(
        snapshot.contains("Pre-update title"),
        "[req_t37] FAIL - previous_snapshot does not contain the pre-update title, got {snapshot}"
    );
    println!("[req_t37] PASS - UpdateTask audit row's previous_snapshot contains the pre-update state: {snapshot}");
}

/// Requirement 38: AddTask's task-insert + audit-insert are one atomic
/// transaction - same forced-failure proof as every entity above.
#[test]
fn req_t38_add_task_audit_failure_rolls_back_task_insert() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task38.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    install_task_audit_failure_trigger(&conn);

    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Should never persist", None, None, None),
    ) {
        ActionOutcome::StorageError(e) => {
            assert_eq!(e.kind(), CoreErrorKind::Storage);
            println!(
                "[req_t38] PASS - forced audit failure surfaced as a typed StorageError: {e:?}"
            );
        }
        other => panic!("[req_t38] FAIL - expected StorageError, got {other:?}"),
    }

    assert_eq!(
        task_row_count(&conn),
        0,
        "[req_t38] FAIL - a task row survived a rolled-back transaction"
    );
    assert_eq!(
        task_audit_row_count(&conn),
        0,
        "[req_t38] FAIL - an audit row survived a rolled-back transaction"
    );

    remove_task_audit_failure_trigger(&conn);

    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Now it works", None, None, None),
    ) {
        ActionOutcome::Success { .. } => {
            println!("[req_t38] PASS - connection remains usable after the rollback")
        }
        other => panic!("[req_t38] FAIL - connection unusable after rollback: {other:?}"),
    }
}

/// Requirement 39: UpdateTask's task-update + audit-insert are one
/// atomic transaction - `updated_at` must not leak from the rolled-back
/// update either.
#[test]
fn req_t39_update_task_audit_failure_rolls_back_task_update() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task39.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Stable", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_task(&conn, created.id).unwrap().unwrap();

    install_task_audit_failure_trigger(&conn);

    std::thread::sleep(std::time::Duration::from_millis(2));
    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            title: Some("Should not persist".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::StorageError(e) => {
            assert_eq!(e.kind(), CoreErrorKind::Storage);
            println!(
                "[req_t39] PASS - forced audit failure surfaced as a typed StorageError: {e:?}"
            );
        }
        other => panic!("[req_t39] FAIL - expected StorageError, got {other:?}"),
    }

    remove_task_audit_failure_trigger(&conn);

    let after = get_task(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_t39] FAIL - task mutated despite a rolled-back transaction"
    );
    assert_eq!(
        after.updated_at, before.updated_at,
        "[req_t39] FAIL - updated_at leaked from a rolled-back update"
    );
    let audit_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM task_audit_log WHERE task_id = ?1",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        audit_count, 1,
        "[req_t39] FAIL - expected exactly 1 audit row (the original AddTask), got {audit_count}"
    );
    println!("[req_t39] PASS - task left identical to its pre-update state, no orphan audit row, updated_at did not leak");
}

/// Requirements 40, 41: Inferred AddTask blocks before write, then a
/// confirmed resubmission succeeds.
#[test]
fn req_t40_41_inferred_add_task_requires_confirmation_then_confirmed_resubmission_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task40.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let payload = mk_add_task_payload("AI-proposed task", None, None, None);
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    assert!(!meta.confirmed);

    match add_task(&mut conn, meta, payload.clone()) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!("[req_t40] FAIL - expected ConfirmationRequired, got {other:?}"),
    }
    assert_eq!(
        task_row_count(&conn),
        0,
        "[req_t40] FAIL - a task was written before confirmation"
    );
    println!(
        "[req_t40] PASS - Inferred AddTask blocked with ConfirmationRequired, zero rows written"
    );

    let mut confirmed_meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    confirmed_meta.confirmed = true;
    match add_task(&mut conn, confirmed_meta, payload) {
        ActionOutcome::Success {
            confirmation_class, ..
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
            assert_eq!(task_row_count(&conn), 1);
            println!("[req_t41] PASS - resubmission with confirmed=true persisted the task");
        }
        other => {
            panic!("[req_t41] FAIL - expected Success on confirmed resubmission, got {other:?}")
        }
    }
}

/// Requirement 42: Inferred UpdateTask blocks before write, zero
/// mutation.
#[test]
fn req_t42_inferred_update_task_requires_confirmation_and_does_not_mutate() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task42.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Stable title", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let before = get_task(&conn, created.id).unwrap().unwrap();

    let payload = UpdateTaskPayload {
        id: created.id,
        title: Some("AI-proposed rename".to_string()),
        ..Default::default()
    };
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    assert!(!meta.confirmed);

    match update_task(&mut conn, meta, payload) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!("[req_t42] FAIL - expected ConfirmationRequired, got {other:?}"),
    }
    let after = get_task(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_t42] FAIL - task mutated before confirmation"
    );
    println!(
        "[req_t42] PASS - Inferred UpdateTask blocked with ConfirmationRequired, zero mutation"
    );
}

/// Requirement 43: a direct, raw SQL insert naming a nonexistent
/// `project_id` is rejected by SQLite's own FK enforcement.
#[test]
fn req_t43_raw_sql_invalid_project_fk_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task43.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_project_id = crate::domain::ProjectId::new().to_string();
    let task_id = crate::domain::TaskId::new().to_string();
    let now = "2026-09-17T00:00:00Z";
    let result = conn.execute(
        "INSERT INTO tasks (id, title, status, due_at, project_id, goal_id, recurring_rule_id, created_at, updated_at)
         VALUES (?1, 'Bypasses AddTask validation', 'TODO', NULL, ?2, NULL, NULL, ?3, ?3)",
        rusqlite::params![task_id, fake_project_id, now],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!("[req_t43] PASS - raw SQL insert with a nonexistent project_id rejected by FK: {err:?}");
        }
        other => panic!("[req_t43] FAIL - expected an SQLite FK constraint error, got {other:?}"),
    }
    assert_eq!(task_row_count(&conn), 0);
}

/// Requirement 44: same as requirement 43, for `goal_id`.
#[test]
fn req_t44_raw_sql_invalid_goal_fk_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task44.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_goal_id = crate::domain::GoalId::new().to_string();
    let task_id = crate::domain::TaskId::new().to_string();
    let now = "2026-09-17T00:00:00Z";
    let result = conn.execute(
        "INSERT INTO tasks (id, title, status, due_at, project_id, goal_id, recurring_rule_id, created_at, updated_at)
         VALUES (?1, 'Bypasses AddTask validation', 'TODO', NULL, NULL, ?2, NULL, ?3, ?3)",
        rusqlite::params![task_id, fake_goal_id, now],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!("[req_t44] PASS - raw SQL insert with a nonexistent goal_id rejected by FK: {err:?}");
        }
        other => panic!("[req_t44] FAIL - expected an SQLite FK constraint error, got {other:?}"),
    }
    assert_eq!(task_row_count(&conn), 0);
}

/// Requirement 45: a direct, raw SQL insert with an invalid `status`
/// text is rejected by the `CHECK` constraint.
#[test]
fn req_t45_raw_sql_invalid_status_rejected_by_check() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task45.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let task_id = crate::domain::TaskId::new().to_string();
    let now = "2026-09-17T00:00:00Z";
    let result = conn.execute(
        "INSERT INTO tasks (id, title, status, due_at, project_id, goal_id, recurring_rule_id, created_at, updated_at)
         VALUES (?1, 'Invalid status', 'BLOCKED', NULL, NULL, NULL, NULL, ?2, ?2)",
        rusqlite::params![task_id, now],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!(
                "[req_t45] PASS - raw SQL insert with an invalid status rejected by CHECK: {err:?}"
            );
        }
        other => {
            panic!("[req_t45] FAIL - expected an SQLite CHECK constraint error, got {other:?}")
        }
    }
    assert_eq!(task_row_count(&conn), 0);
}

/// Requirement 46: a direct, raw SQL insert with an empty (post-trim)
/// title is rejected by the `CHECK` constraint.
#[test]
fn req_t46_raw_sql_empty_title_rejected_by_check() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task46.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let task_id = crate::domain::TaskId::new().to_string();
    let now = "2026-09-17T00:00:00Z";
    let result = conn.execute(
        "INSERT INTO tasks (id, title, status, due_at, project_id, goal_id, recurring_rule_id, created_at, updated_at)
         VALUES (?1, '   ', 'TODO', NULL, NULL, NULL, NULL, ?2, ?2)",
        rusqlite::params![task_id, now],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!("[req_t46] PASS - raw SQL insert with a whitespace-only title rejected by CHECK: {err:?}");
        }
        other => {
            panic!("[req_t46] FAIL - expected an SQLite CHECK constraint error, got {other:?}")
        }
    }
    assert_eq!(task_row_count(&conn), 0);
}

/// Requirement 47: a direct, raw SQL insert with a non-NULL
/// `recurring_rule_id` is rejected by the temporary
/// `CHECK (recurring_rule_id IS NULL)` constraint - proves the deferred
/// RecurringRule design is genuinely enforced, not just documented.
#[test]
fn req_t47_raw_sql_non_null_recurring_rule_id_rejected_by_check() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task47.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let task_id = crate::domain::TaskId::new().to_string();
    let fake_recurring_rule_id = crate::domain::RecurringRuleId::new().to_string();
    let now = "2026-09-17T00:00:00Z";
    let result = conn.execute(
        "INSERT INTO tasks (id, title, status, due_at, project_id, goal_id, recurring_rule_id, created_at, updated_at)
         VALUES (?1, 'Premature recurrence', 'TODO', NULL, NULL, NULL, ?2, ?3, ?3)",
        rusqlite::params![task_id, fake_recurring_rule_id, now],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "expected a constraint-violation error, got {err:?}"
            );
            println!("[req_t47] PASS - raw SQL insert with a non-NULL recurring_rule_id rejected by the temporary CHECK: {err:?}");
        }
        other => {
            panic!("[req_t47] FAIL - expected an SQLite CHECK constraint error, got {other:?}")
        }
    }
    assert_eq!(task_row_count(&conn), 0);
}

/// Requirement 48: a direct, raw SQL hard delete of a Project referenced
/// by a Task is rejected by `ON DELETE RESTRICT`.
#[test]
fn req_t48_raw_hard_delete_of_referenced_project_is_restricted() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task48.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Referenced project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Keeps the project alive", None, Some(project.id), None),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let result = conn.execute(
        "DELETE FROM projects WHERE id = ?1",
        rusqlite::params![project.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(err.code, rusqlite::ErrorCode::ConstraintViolation, "expected a constraint-violation error, got {err:?}");
            println!("[req_t48] PASS - raw DELETE of a Project referenced by a Task rejected by ON DELETE RESTRICT: {err:?}");
        }
        other => panic!("[req_t48] FAIL - expected an SQLite FK constraint error blocking the delete, got {other:?}"),
    }

    assert!(
        get_project(&conn, project.id).unwrap().is_some(),
        "[req_t48] FAIL - Project was deleted despite the RESTRICT constraint"
    );
    assert_eq!(
        task_row_count(&conn),
        1,
        "[req_t48] FAIL - task row disappeared despite the delete being rejected"
    );
    println!("[req_t48] PASS - Project remains, and the task remains - no dangling task exists");
}

/// Requirement 49: same as requirement 48, for a referenced Goal.
#[test]
fn req_t49_raw_hard_delete_of_referenced_goal_is_restricted() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task49.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let goal = match create_goal(
        &mut conn,
        ui_explicit(),
        mk_create_payload("Referenced goal"),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Keeps the goal alive", None, None, Some(goal.id)),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let result = conn.execute(
        "DELETE FROM goals WHERE id = ?1",
        rusqlite::params![goal.id.to_string()],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(err.code, rusqlite::ErrorCode::ConstraintViolation, "expected a constraint-violation error, got {err:?}");
            println!("[req_t49] PASS - raw DELETE of a Goal referenced by a Task rejected by ON DELETE RESTRICT: {err:?}");
        }
        other => panic!("[req_t49] FAIL - expected an SQLite FK constraint error blocking the delete, got {other:?}"),
    }

    assert!(
        get_goal(&conn, goal.id).unwrap().is_some(),
        "[req_t49] FAIL - Goal was deleted despite the RESTRICT constraint"
    );
    assert_eq!(
        task_row_count(&conn),
        1,
        "[req_t49] FAIL - task row disappeared despite the delete being rejected"
    );
    println!("[req_t49] PASS - Goal remains, and the task remains - no dangling task exists");
}

/// Requirements 50, 51: `get_tasks_for_project`/`get_tasks_for_goal`
/// return only Tasks actually referencing the queried parent.
#[test]
fn req_t50_51_get_tasks_for_project_and_goal_return_only_matching_tasks() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "task50.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project_a = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project A", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let project_b = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project B", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let goal_a = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal A")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let goal_b = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal B")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let task_a_project = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("A's project task", None, Some(project_a.id), None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("B's project task", None, Some(project_b.id), None),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }
    let task_a_goal = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("A's goal task", None, None, Some(goal_a.id)),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("B's goal task", None, None, Some(goal_b.id)),
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let for_project_a = get_tasks_for_project(&conn, project_a.id).unwrap();
    assert_eq!(
        for_project_a.len(),
        1,
        "[req_t50] FAIL - expected exactly 1 task for Project A"
    );
    assert_eq!(for_project_a[0].id, task_a_project.id);
    println!("[req_t50] PASS - get_tasks_for_project returned only the Task referencing Project A");

    let for_goal_a = get_tasks_for_goal(&conn, goal_a.id).unwrap();
    assert_eq!(
        for_goal_a.len(),
        1,
        "[req_t51] FAIL - expected exactly 1 task for Goal A"
    );
    assert_eq!(for_goal_a[0].id, task_a_goal.id);
    println!("[req_t51] PASS - get_tasks_for_goal returned only the Task referencing Goal A");
}

// =========================================================================
// M2 Storage and Audit Safety Hardening (2026-09-18) - new evidence
// below. Evidence for
// `docs/reports/R015_M2_STORAGE_AND_AUDIT_SAFETY_HARDENING_2026-09-18.md`.
//
// Issue A: the master-key/database state machine
// (`bootstrap_db_with_key_lifecycle`/`resolve_master_key`). All tests
// here use the isolated "LifeOS-test" OS-keyring service identity
// (never the real production `security::SERVICE_NAME`/
// `security::MASTER_KEY_ACCOUNT`) with per-test account names, and
// temp-directory database paths - never the real production database
// path or credential. Gracefully reports BLOCKED (not FAIL) if the OS
// credential store is genuinely unavailable in this environment,
// matching the existing `req4` precedent - `--test-threads=1` matters
// here for the same reason it already did for `req4`.
//
// Issue B: mandatory audit `previous_snapshot` serialization
// (`serialize_required_snapshot`) - proves the helper cannot silently
// swallow a serialization failure, and that every applicable Update*
// action's previous_snapshot deserializes back to the exact pre-update
// entity, not merely a title substring.
//
// R016 ADDENDUM (2026-09-18): the `req_kl*` tests in this section can
// return early as "BLOCKED" (still counted as PASS by Cargo) if the real
// OS credential store is unavailable - this is NOT, on its own,
// sufficient proof of the canonical A/B/C/D state machine. That
// deterministic proof (independent of OS keyring availability) now lives
// in the "M2 Key Lifecycle Determinism and Failure Atomicity Hardening
// (R016)" section further below (`req_dkl1`-`req_dkl12`), which also
// fixes a review finding against this section's original cleanup rule:
// `bootstrap_db_with_key_lifecycle` used to delete a freshly-created
// credential on ANY Case-A initialization failure, even one that
// occurred AFTER a database file had already been written - which could
// itself manufacture Case C. `req_kl7`/`req_kl8` below still only prove
// the pre-file-creation failure path; see `req_dkl7` for the
// post-file-creation proof. See R015 §"Addendum (R016)" and the R016
// report for full detail.
// =========================================================================

/// Isolated OS-keyring service identity for every key-lifecycle test
/// below - never the real production `security::SERVICE_NAME`
/// (`"LifeOS"`). Matches the existing `req4` precedent exactly.
const TEST_SERVICE: &str = "LifeOS-test";

/// Requirement 1: DB absent + key absent -> key generated, key stored,
/// DB created/opened successfully (Case A, the normal first-run path).
#[test]
fn req_kl1_db_absent_key_absent_generates_and_stores_key_and_creates_db() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case_a.db");
    let account = "kl1-case-a";
    let _ = security::delete_key(TEST_SERVICE, account);

    assert!(
        !path.exists(),
        "[req_kl1] setup FAIL - db must not exist yet"
    );
    assert_eq!(
        security::get_existing_key(TEST_SERVICE, account).unwrap(),
        None,
        "[req_kl1] setup FAIL - credential must not exist yet"
    );

    match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
        Ok(conn) => {
            drop(conn);
            assert!(path.exists(), "[req_kl1] FAIL - db file was not created");
            let stored = security::get_existing_key(TEST_SERVICE, account).unwrap();
            assert!(stored.is_some(), "[req_kl1] FAIL - key was not stored");
            println!(
                "[req_kl1] PASS - db absent + key absent -> key generated, stored, db created/opened"
            );
        }
        Err(e) => {
            println!("[req_kl1] BLOCKED - could not write to OS credential store: {e}");
        }
    }
    let _ = security::delete_key(TEST_SERVICE, account);
}

/// Requirement 2: DB exists + key exists -> existing key reused, DB
/// opens, credential unchanged (Case B). Never replaced.
#[test]
fn req_kl2_db_exists_key_exists_reuses_key_and_credential_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case_b.db");
    let account = "kl2-case-b";
    let _ = security::delete_key(TEST_SERVICE, account);

    let first_key = match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
        Ok(conn) => {
            drop(conn);
            security::get_existing_key(TEST_SERVICE, account)
                .unwrap()
                .unwrap()
        }
        Err(e) => {
            println!("[req_kl2] BLOCKED - could not write to OS credential store: {e}");
            return;
        }
    };

    match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
        Ok(conn) => drop(conn),
        Err(e) => panic!("[req_kl2] FAIL - expected Success (Case B), got {e:?}"),
    }
    let second_key = security::get_existing_key(TEST_SERVICE, account)
        .unwrap()
        .unwrap();
    assert_eq!(
        first_key, second_key,
        "[req_kl2] FAIL - credential changed on reopen despite the database and credential both already existing"
    );
    println!(
        "[req_kl2] PASS - db exists + key exists -> existing key reused, db opens, credential unchanged"
    );

    let _ = security::delete_key(TEST_SERVICE, account);
}

/// Requirement 3 (CRITICAL): DB exists + key absent -> typed recovery-
/// required failure, NO replacement key stored, DB still exists, DB
/// bytes unchanged (Case C).
#[test]
fn req_kl3_db_exists_key_absent_fails_safely_without_replacement_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case_c.db");
    let account = "kl3-case-c";
    let _ = security::delete_key(TEST_SERVICE, account);

    match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
        Ok(conn) => drop(conn),
        Err(e) => {
            println!("[req_kl3] BLOCKED - could not write to OS credential store: {e}");
            return;
        }
    }
    assert!(path.exists());
    let db_bytes_before = std::fs::read(&path).unwrap();

    // Remove ONLY the credential, leaving the database file exactly as
    // it is - simulates "existing database, missing key" (e.g. a
    // credential store reset, a migration to a new machine without the
    // credential, accidental deletion).
    security::delete_key(TEST_SERVICE, account).expect("cleanup: failed to delete test credential");
    assert_eq!(
        security::get_existing_key(TEST_SERVICE, account).unwrap(),
        None
    );

    match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
        Err(BootstrapError::MasterKeyMissingForExistingDatabase) => {
            println!("[req_kl3] PASS - Case C returned the typed recovery-required error");
        }
        other => {
            panic!("[req_kl3] FAIL - expected MasterKeyMissingForExistingDatabase, got {other:?}")
        }
    }

    assert_eq!(
        security::get_existing_key(TEST_SERVICE, account).unwrap(),
        None,
        "[req_kl3] FAIL - a replacement key was stored despite Case C"
    );
    assert!(
        path.exists(),
        "[req_kl3] FAIL - the existing db disappeared"
    );
    let db_bytes_after = std::fs::read(&path).unwrap();
    assert_eq!(
        db_bytes_before, db_bytes_after,
        "[req_kl3] FAIL - db bytes changed despite Case C returning before any db access"
    );
    println!(
        "[req_kl3] PASS - no replacement key stored, db still exists, db bytes byte-for-byte unchanged"
    );
}

/// Requirement 4: DB absent + key exists -> existing key reused, new DB
/// created, key not replaced (Case D - e.g. after intentional DB
/// deletion, restore preparation, or interrupted setup).
#[test]
fn req_kl4_db_absent_key_exists_reuses_existing_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case_d.db");
    let account = "kl4-case-d";
    let _ = security::delete_key(TEST_SERVICE, account);

    let pre_existing_key = match security::generate_and_store_key(TEST_SERVICE, account) {
        Ok(k) => k,
        Err(e) => {
            println!("[req_kl4] BLOCKED - could not write to OS credential store: {e}");
            return;
        }
    };
    assert!(
        !path.exists(),
        "[req_kl4] setup FAIL - db must not exist yet"
    );

    match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
        Ok(conn) => drop(conn),
        Err(e) => panic!("[req_kl4] FAIL - expected Success (Case D), got {e:?}"),
    }
    assert!(path.exists(), "[req_kl4] FAIL - db was not created");
    let key_after = security::get_existing_key(TEST_SERVICE, account)
        .unwrap()
        .unwrap();
    assert_eq!(
        pre_existing_key, key_after,
        "[req_kl4] FAIL - the pre-existing credential was replaced instead of reused"
    );
    println!(
        "[req_kl4] PASS - db absent + key exists -> existing key reused to create the new db, key not replaced"
    );

    let _ = security::delete_key(TEST_SERVICE, account);
}

/// Requirement 5: the first-run generated key is exactly the key used
/// to create the database - independently re-opens the resulting file
/// with exactly the stored key, proving it is genuinely the encrypting
/// key, not merely "a key was stored somewhere".
#[test]
fn req_kl5_first_run_generated_key_is_exactly_the_key_that_encrypts_the_db() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case_kl5.db");
    let account = "kl5-first-run-key";
    let _ = security::delete_key(TEST_SERVICE, account);

    match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
        Ok(conn) => drop(conn),
        Err(e) => {
            println!("[req_kl5] BLOCKED - could not write to OS credential store: {e}");
            return;
        }
    }
    let stored_key = security::get_existing_key(TEST_SERVICE, account)
        .unwrap()
        .expect("[req_kl5] FAIL - no key was stored");

    let conn = storage::open_encrypted(&path, &stored_key).unwrap();
    storage::verify_key(&conn).expect(
        "[req_kl5] FAIL - the stored key does not decrypt the database it supposedly created",
    );
    println!(
        "[req_kl5] PASS - the first-run generated key is exactly the key that encrypts the created database"
    );

    let _ = security::delete_key(TEST_SERVICE, account);
}

/// Requirement 6: a credential that is PRESENT but WRONG for an
/// existing database still fails normally (a real SQLCipher storage
/// error) - no automatic replacement key, because `resolve_master_key`
/// correctly classifies a present-but-wrong credential as `Reused`
/// (Case B shape - present), never as `FreshlyGenerated`. This is a
/// genuinely different case from Case C's "absent" credential.
#[test]
fn req_kl6_wrong_but_present_key_against_existing_db_fails_without_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case_kl6.db");
    let real_account = "kl6-real-key";
    let wrong_account = "kl6-wrong-key";
    let _ = security::delete_key(TEST_SERVICE, real_account);
    let _ = security::delete_key(TEST_SERVICE, wrong_account);

    match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, real_account) {
        Ok(conn) => drop(conn),
        Err(e) => {
            println!("[req_kl6] BLOCKED - could not write to OS credential store: {e}");
            return;
        }
    }

    let wrong_key = match security::generate_and_store_key(TEST_SERVICE, wrong_account) {
        Ok(k) => k,
        Err(e) => {
            println!("[req_kl6] BLOCKED - could not write to OS credential store: {e}");
            return;
        }
    };

    match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, wrong_account) {
        Err(BootstrapError::Storage(_)) => {
            println!(
                "[req_kl6] PASS - a present-but-wrong credential was rejected by SQLCipher as a normal storage failure"
            );
        }
        other => panic!(
            "[req_kl6] FAIL - expected a Storage error from the wrong-but-present key, got {other:?}"
        ),
    }

    let after = security::get_existing_key(TEST_SERVICE, wrong_account)
        .unwrap()
        .unwrap();
    assert_eq!(
        wrong_key, after,
        "[req_kl6] FAIL - the present-but-wrong credential was replaced instead of left alone"
    );
    println!("[req_kl6] PASS - no automatic replacement key was generated");

    let _ = security::delete_key(TEST_SERVICE, real_account);
    let _ = security::delete_key(TEST_SERVICE, wrong_account);
}

/// Requirement 7: a database-initialization failure AFTER this
/// invocation created a brand-new credential (Case A) cleans up that
/// orphaned credential - forces the failure deterministically and
/// portably by pointing the db path's parent at a plain FILE (so
/// `std::fs::create_dir_all` genuinely fails), rather than relying on
/// any platform-specific permission trick.
#[test]
fn req_kl7_init_failure_after_fresh_credential_created_cleans_up_orphan() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker_file_kl7");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let path = blocker.join("case_kl7.db"); // parent is a FILE - create_dir_all must fail
    let account = "kl7-orphan-cleanup";
    let _ = security::delete_key(TEST_SERVICE, account);

    assert!(!path.exists());
    assert_eq!(
        security::get_existing_key(TEST_SERVICE, account).unwrap(),
        None
    );

    match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
        Err(BootstrapError::Io(_)) => {
            println!(
                "[req_kl7] PASS - forced db initialization failure (Io) after a fresh credential was created"
            );
        }
        Err(BootstrapError::Key(e)) => {
            println!("[req_kl7] BLOCKED - could not write to OS credential store: {e}");
            return;
        }
        other => panic!("[req_kl7] FAIL - expected an Io error, got {other:?}"),
    }

    let leftover = security::get_existing_key(TEST_SERVICE, account).unwrap();
    assert_eq!(
        leftover, None,
        "[req_kl7] FAIL - the freshly-created orphan credential was not cleaned up"
    );
    println!(
        "[req_kl7] PASS - the freshly-created orphan credential was cleaned up after the initialization failure"
    );
}

/// Requirement 8: a database-initialization failure with a PRE-EXISTING
/// credential leaves that credential completely untouched - same
/// forced-failure technique as `req_kl7`, but the credential exists
/// BEFORE the failing call, so `resolve_master_key` resolves it as
/// `Reused`, not `FreshlyGenerated`, and the cleanup branch must never
/// run for it.
#[test]
fn req_kl8_init_failure_with_pre_existing_credential_leaves_it_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let account = "kl8-pre-existing";
    let _ = security::delete_key(TEST_SERVICE, account);

    let pre_existing_key = match security::generate_and_store_key(TEST_SERVICE, account) {
        Ok(k) => k,
        Err(e) => {
            println!("[req_kl8] BLOCKED - could not write to OS credential store: {e}");
            return;
        }
    };

    let blocker = dir.path().join("blocker_file_kl8");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let path = blocker.join("case_kl8.db");

    match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
        Err(BootstrapError::Io(_)) => {
            println!(
                "[req_kl8] PASS - forced db initialization failure with a pre-existing credential"
            );
        }
        other => panic!("[req_kl8] FAIL - expected an Io error, got {other:?}"),
    }

    let after = security::get_existing_key(TEST_SERVICE, account).unwrap();
    assert_eq!(
        after,
        Some(pre_existing_key),
        "[req_kl8] FAIL - the pre-existing credential was deleted or replaced"
    );
    println!("[req_kl8] PASS - pre-existing credential remains untouched after the initialization failure");

    let _ = security::delete_key(TEST_SERVICE, account);
}

/// Requirement 9: repeated successful reopen uses the same master key
/// across multiple bootstraps, not just one reopen.
#[test]
fn req_kl9_repeated_successful_reopen_uses_same_master_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case_kl9.db");
    let account = "kl9-repeated-reopen";
    let _ = security::delete_key(TEST_SERVICE, account);

    let key_first = match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
        Ok(conn) => {
            drop(conn);
            security::get_existing_key(TEST_SERVICE, account)
                .unwrap()
                .unwrap()
        }
        Err(e) => {
            println!("[req_kl9] BLOCKED - could not write to OS credential store: {e}");
            return;
        }
    };

    for i in 0..3 {
        match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
            Ok(conn) => drop(conn),
            Err(e) => panic!("[req_kl9] FAIL - reopen #{i} failed: {e:?}"),
        }
        let key_now = security::get_existing_key(TEST_SERVICE, account)
            .unwrap()
            .unwrap();
        assert_eq!(
            key_first, key_now,
            "[req_kl9] FAIL - master key changed on reopen #{i}"
        );
    }
    println!(
        "[req_kl9] PASS - repeated reopen across 3 additional bootstraps all used the exact same master key"
    );

    let _ = security::delete_key(TEST_SERVICE, account);
}

/// Requirement 10: no test in this file touches the real production
/// credential identity (`security::SERVICE_NAME`/
/// `security::MASTER_KEY_ACCOUNT`) - static source scan proving the
/// only reference anywhere in `src/` is `bootstrap_production_db`'s own
/// call site, not any test.
#[test]
fn req_kl10_no_test_touches_production_credential_identity() {
    // scan_dir (req15, above) only flags matches inside a print!-family
    // call - not useful here, so this test uses its own direct
    // code-reference scan (scan_dir_for_identifiers) instead.
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut real_offenders = Vec::new();
    scan_dir_for_identifiers(
        &src_dir,
        &mut real_offenders,
        &["security::SERVICE_NAME", "security::MASTER_KEY_ACCOUNT"],
    );
    let outside_bootstrap: Vec<_> = real_offenders
        .iter()
        .filter(|line| !line.contains("services") || !line.contains("mod.rs"))
        .collect();
    assert!(
        outside_bootstrap.is_empty(),
        "[req_kl10] FAIL - production credential identity referenced outside bootstrap_production_db: {outside_bootstrap:?}"
    );
    println!(
        "[req_kl10] PASS - security::SERVICE_NAME/MASTER_KEY_ACCOUNT are referenced only by bootstrap_production_db itself ({} total reference(s) in src/); every test in this file uses the isolated \"LifeOS-test\" service",
        real_offenders.len()
    );
}

/// Like `scan_dir`, but for finding genuine Rust *code* references to an
/// identifier path (e.g. `security::SERVICE_NAME`) rather than
/// print-statement interpolation - skips comment lines (`//`/`///`) and
/// occurrences inside a string literal (odd-vs-even `"` count before the
/// match position on that line), so this scan does not flag its own
/// doc comments or its own pattern-array/println! text about the very
/// identifiers it is searching for.
fn scan_dir_for_identifiers(dir: &std::path::Path, offenders: &mut Vec<String>, patterns: &[&str]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_for_identifiers(&path, offenders, patterns);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let Ok(contents) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in contents.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("//") {
                    continue;
                }
                for pat in patterns {
                    if let Some(idx) = line.find(pat) {
                        let before = &line[..idx];
                        let inside_string_literal = before.matches('"').count() % 2 == 1;
                        if !inside_string_literal {
                            offenders.push(format!(
                                "{}:{}: {}",
                                path.display(),
                                i + 1,
                                line.trim()
                            ));
                        }
                    }
                }
            }
        }
    }
}

// --- Issue B: mandatory audit previous_snapshot serialization ----------

/// Test-only type proving `serialize_required_snapshot` does NOT
/// silently swallow a serialization failure (M2 storage/audit safety
/// hardening, 2026-09-18) - deliberately errors on every `Serialize`
/// call, standing in for a hypothetical real domain-struct
/// serialization failure without distorting any actual production
/// struct to create one. The error message embeds an obvious marker
/// (R016) so `req_snap1` can prove structurally - not by assumption
/// about `serde_json`'s current wording - that a custom `Serialize`
/// impl's error text can never reach `CoreError`'s `Display`/`Debug`/
/// diagnostic through `serialize_required_snapshot`.
struct AlwaysFailsToSerialize;

const SNAPSHOT_FAILURE_TEST_MARKER: &str = "SECRET_TEST_VALUE_SHOULD_NEVER_ESCAPE";

impl serde::Serialize for AlwaysFailsToSerialize {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Err(serde::ser::Error::custom(format!(
            "deliberate test-only serialization failure containing {SNAPSHOT_FAILURE_TEST_MARKER}"
        )))
    }
}

/// Proves `serialize_required_snapshot` surfaces a serialization
/// failure as a typed `CoreError` (kind `Internal`), never as a
/// silently-empty/`None` snapshot - AND (R016) proves the underlying
/// serializer's own error text (which could, for some future custom
/// `Serialize` impl, contain sensitive content derived from the value
/// being serialized) never propagates into the returned `CoreError`'s
/// `Display`, `Debug`, or `diagnostic()` - structurally, via a
/// deliberate marker string, not merely by inspecting today's
/// `serde_json` wording.
#[test]
fn req_snap1_serialize_required_snapshot_does_not_swallow_serialization_failure() {
    match serialize_required_snapshot(&AlwaysFailsToSerialize) {
        Err(e) => {
            assert_eq!(
                e.kind(),
                CoreErrorKind::Internal,
                "[req_snap1] FAIL - expected CoreErrorKind::Internal, got {:?}",
                e.kind()
            );
            let display_text = format!("{e}");
            let debug_text = format!("{e:?}");
            assert!(
                !display_text.contains(SNAPSHOT_FAILURE_TEST_MARKER),
                "[req_snap1] FAIL - the deliberate marker leaked into Display: {display_text}"
            );
            assert!(
                !debug_text.contains(SNAPSHOT_FAILURE_TEST_MARKER),
                "[req_snap1] FAIL - the deliberate marker leaked into Debug: {debug_text}"
            );
            assert!(
                !e.diagnostic().contains(SNAPSHOT_FAILURE_TEST_MARKER),
                "[req_snap1] FAIL - the deliberate marker leaked into CoreError::diagnostic(): {}",
                e.diagnostic()
            );
            println!(
                "[req_snap1] PASS - serialize_required_snapshot surfaced the failure as a typed CoreError (not a silently-empty snapshot), and the underlying serializer's own error text (marker: {SNAPSHOT_FAILURE_TEST_MARKER}) does not appear in Display, Debug, or diagnostic(): {e:?}"
            );
        }
        Ok(s) => panic!(
            "[req_snap1] FAIL - expected an Err, got Ok({s:?}) - failure was silently swallowed"
        ),
    }
}

/// UpdateGoal's audit previous_snapshot is non-NULL and deserializes
/// back to the exact PRE-update Goal - a stronger, structured proof
/// than a title substring match.
#[test]
fn req_snap2_update_goal_previous_snapshot_deserializes_to_pre_update_state() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "snap_goal.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_goal(
        &mut conn,
        ui_explicit(),
        mk_create_payload("Pre-update goal title"),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let expected_pre_update = created.clone();

    match update_goal(
        &mut conn,
        ui_explicit(),
        UpdateGoalPayload {
            id: created.id,
            title: Some("Post-update goal title".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let previous_snapshot: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM goal_audit_log WHERE goal_id = ?1 AND action_type = 'UpdateGoal'",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let snapshot_json =
        previous_snapshot.expect("[req_snap2] FAIL - previous_snapshot must not be NULL");
    let deserialized: crate::domain::Goal = serde_json::from_str(&snapshot_json)
        .expect("[req_snap2] FAIL - previous_snapshot did not deserialize to Goal");
    assert_eq!(
        deserialized, expected_pre_update,
        "[req_snap2] FAIL - snapshot does not describe the pre-update Goal"
    );
    assert_eq!(deserialized.title, "Pre-update goal title");
    println!(
        "[req_snap2] PASS - UpdateGoal previous_snapshot deserializes to the exact pre-update Goal"
    );
}

/// UpdateProject's audit previous_snapshot deserializes back to the
/// exact PRE-update Project.
#[test]
fn req_snap3_update_project_previous_snapshot_deserializes_to_pre_update_state() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "snap_project.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Pre-update project title", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let expected_pre_update = created.clone();

    match update_project(
        &mut conn,
        ui_explicit(),
        UpdateProjectPayload {
            id: created.id,
            title: Some("Post-update project title".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let previous_snapshot: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM project_audit_log WHERE project_id = ?1 AND action_type = 'UpdateProject'",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let snapshot_json =
        previous_snapshot.expect("[req_snap3] FAIL - previous_snapshot must not be NULL");
    let deserialized: Project = serde_json::from_str(&snapshot_json)
        .expect("[req_snap3] FAIL - previous_snapshot did not deserialize to Project");
    assert_eq!(
        deserialized, expected_pre_update,
        "[req_snap3] FAIL - snapshot does not describe the pre-update Project"
    );
    println!(
        "[req_snap3] PASS - UpdateProject previous_snapshot deserializes to the exact pre-update Project"
    );
}

/// UpdateProgress's audit previous_snapshot deserializes back to the
/// exact PRE-update Project (the same mandatory-snapshot pattern
/// applies here too - UpdateProgress was in scope for this hardening
/// pass alongside UpdateGoal/UpdateProject/UpdateMilestone/UpdateTask).
#[test]
fn req_snap4_update_progress_previous_snapshot_deserializes_to_pre_update_state() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "snap_progress.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Progress snapshot project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(created.progress, 0);
    let expected_pre_update = created.clone();

    match update_progress(
        &mut conn,
        ui_explicit(),
        UpdateProgressPayload {
            id: created.id,
            progress: 77,
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let previous_snapshot: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM project_audit_log WHERE project_id = ?1 AND action_type = 'UpdateProgress'",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let snapshot_json =
        previous_snapshot.expect("[req_snap4] FAIL - previous_snapshot must not be NULL");
    let deserialized: Project = serde_json::from_str(&snapshot_json)
        .expect("[req_snap4] FAIL - previous_snapshot did not deserialize to Project");
    assert_eq!(
        deserialized, expected_pre_update,
        "[req_snap4] FAIL - snapshot does not describe the pre-update Project"
    );
    assert_eq!(deserialized.progress, 0);
    println!(
        "[req_snap4] PASS - UpdateProgress previous_snapshot deserializes to the exact pre-update Project (progress=0)"
    );
}

/// UpdateMilestone's audit previous_snapshot deserializes back to the
/// exact PRE-update Milestone.
#[test]
fn req_snap5_update_milestone_previous_snapshot_deserializes_to_pre_update_state() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "snap_milestone.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let created = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Pre-update milestone title", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let expected_pre_update = created.clone();

    match update_milestone(
        &mut conn,
        ui_explicit(),
        UpdateMilestonePayload {
            id: created.id,
            title: Some("Post-update milestone title".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let previous_snapshot: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM milestone_audit_log WHERE milestone_id = ?1 AND action_type = 'UpdateMilestone'",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let snapshot_json =
        previous_snapshot.expect("[req_snap5] FAIL - previous_snapshot must not be NULL");
    let deserialized: Milestone = serde_json::from_str(&snapshot_json)
        .expect("[req_snap5] FAIL - previous_snapshot did not deserialize to Milestone");
    assert_eq!(
        deserialized, expected_pre_update,
        "[req_snap5] FAIL - snapshot does not describe the pre-update Milestone"
    );
    println!(
        "[req_snap5] PASS - UpdateMilestone previous_snapshot deserializes to the exact pre-update Milestone"
    );
}

/// UpdateTask's audit previous_snapshot deserializes back to the exact
/// PRE-update Task.
#[test]
fn req_snap6_update_task_previous_snapshot_deserializes_to_pre_update_state() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "snap_task.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Pre-update task title", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let expected_pre_update = created.clone();

    match update_task(
        &mut conn,
        ui_explicit(),
        UpdateTaskPayload {
            id: created.id,
            title: Some("Post-update task title".to_string()),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }

    let previous_snapshot: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM task_audit_log WHERE task_id = ?1 AND action_type = 'UpdateTask'",
            [created.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let snapshot_json =
        previous_snapshot.expect("[req_snap6] FAIL - previous_snapshot must not be NULL");
    let deserialized: Task = serde_json::from_str(&snapshot_json)
        .expect("[req_snap6] FAIL - previous_snapshot did not deserialize to Task");
    assert_eq!(
        deserialized, expected_pre_update,
        "[req_snap6] FAIL - snapshot does not describe the pre-update Task"
    );
    println!(
        "[req_snap6] PASS - UpdateTask previous_snapshot deserializes to the exact pre-update Task"
    );
}

/// Requirement (brief §18): normal production bootstrap path regression
/// - proven against an isolated test environment (temp path + test
/// credential identity), not the real production path/credential.
/// First bootstrap creates the encrypted DB; close; second bootstrap
/// reopens the SAME DB using the SAME key; a record written before the
/// close is still readable after reopen - no plaintext regression (the
/// file still has no `SQLite` plaintext header).
#[test]
fn req_kl_bootstrap_regression_first_and_second_bootstrap_share_state_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bootstrap_regression.db");
    let account = "kl-bootstrap-regression";
    let _ = security::delete_key(TEST_SERVICE, account);

    let goal_id;
    {
        let mut conn = match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
            Ok(conn) => conn,
            Err(e) => {
                println!("[req_kl_bootstrap_regression] BLOCKED - could not write to OS credential store: {e}");
                return;
            }
        };
        let created = match create_goal(
            &mut conn,
            ui_explicit(),
            mk_create_payload("Survives reopen"),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        goal_id = created.id;
    }

    {
        let conn = match bootstrap_db_with_key_lifecycle(&path, TEST_SERVICE, account) {
            Ok(conn) => conn,
            Err(e) => panic!(
                "[req_kl_bootstrap_regression] FAIL - second bootstrap (reopen) failed: {e:?}"
            ),
        };
        let fetched = get_goal(&conn, goal_id).unwrap();
        assert_eq!(
            fetched.map(|g| g.title),
            Some("Survives reopen".to_string()),
            "[req_kl_bootstrap_regression] FAIL - record written before close was not readable after reopen"
        );
    }

    let mut raw = Vec::new();
    fs::File::open(&path)
        .unwrap()
        .read_to_end(&mut raw)
        .unwrap();
    assert!(
        !raw.windows(6).any(|w| w == b"SQLite"),
        "[req_kl_bootstrap_regression] FAIL - file starts with the plaintext SQLite header, not encrypted"
    );
    println!(
        "[req_kl_bootstrap_regression] PASS - first bootstrap created the encrypted db, second bootstrap reopened it with the same key, the record written before close is readable after reopen, no plaintext regression"
    );

    let _ = security::delete_key(TEST_SERVICE, account);
}

// =========================================================================
// M2 Key Lifecycle Determinism and Failure Atomicity Hardening (R016,
// 2026-09-18). Evidence for
// `docs/reports/R016_M2_KEY_LIFECYCLE_DETERMINISM_AND_FAILURE_ATOMICITY_HARDENING_2026-09-18.md`.
//
// The `req_kl*` tests above remain a separate, supplementary,
// environment-dependent smoke/integration suite against the REAL OS
// keyring (still valuable - they prove `KeyringCredentialStore` genuinely
// round-trips through Windows Credential Manager) but are NOT, on their
// own, sufficient proof of the canonical A/B/C/D state machine: a
// "BLOCKED" early-return when the real keyring is unavailable is still
// counted as a PASS by Cargo, even though the state-machine logic was
// never exercised for that run. Every test below instead drives
// `resolve_master_key_with_store`/`bootstrap_db_with_key_lifecycle_with_store`
// directly through `InMemoryCredentialStore` - the SAME production
// state-machine logic `resolve_master_key`/`bootstrap_db_with_key_lifecycle`
// are thin wrappers around, never a parallel reimplementation - so these
// tests always run, deterministically, on every machine, regardless of OS
// credential store availability.
// =========================================================================

use std::cell::RefCell;
use std::collections::HashMap;

/// Deterministic, in-memory `security::CredentialStore` test double
/// (R016) - never touches the real OS credential store. Records every
/// `get`/`set`/`delete` call so tests can assert exactly how many times
/// each happened (e.g. "no unexpected `set()` on Case B/C/D reuse"), and
/// can be configured to fail each operation on demand - deterministically
/// exercising backend-error paths (requirements 10-12) that are
/// impractical to force against the real OS keyring.
#[derive(Default)]
struct InMemoryCredentialStore {
    data: RefCell<HashMap<(String, String), String>>,
    get_calls: RefCell<u32>,
    set_calls: RefCell<u32>,
    delete_calls: RefCell<u32>,
    fail_get: RefCell<bool>,
    fail_set: RefCell<bool>,
    fail_delete: RefCell<bool>,
}

impl InMemoryCredentialStore {
    fn new() -> Self {
        Self::default()
    }

    /// Seeds a credential directly, WITHOUT going through `set()` (so
    /// this does not count toward `set_calls()`) - simulates "a
    /// credential already exists before this bootstrap call" (Cases
    /// B/D).
    fn seeded(service: &str, account: &str, value: &str) -> Self {
        let store = Self::new();
        store.data.borrow_mut().insert(
            (service.to_string(), account.to_string()),
            value.to_string(),
        );
        store
    }

    fn fail_next_get(&self) {
        *self.fail_get.borrow_mut() = true;
    }
    fn fail_next_set(&self) {
        *self.fail_set.borrow_mut() = true;
    }
    fn fail_next_delete(&self) {
        *self.fail_delete.borrow_mut() = true;
    }

    fn set_calls(&self) -> u32 {
        *self.set_calls.borrow()
    }
    fn delete_calls(&self) -> u32 {
        *self.delete_calls.borrow()
    }

    fn raw_get(&self, service: &str, account: &str) -> Option<String> {
        self.data
            .borrow()
            .get(&(service.to_string(), account.to_string()))
            .cloned()
    }
}

impl security::CredentialStore for InMemoryCredentialStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, security::KeyError> {
        *self.get_calls.borrow_mut() += 1;
        if *self.fail_get.borrow() {
            return Err(security::KeyError::Keyring(
                "[test] forced GET backend failure".to_string(),
            ));
        }
        Ok(self.raw_get(service, account))
    }

    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), security::KeyError> {
        *self.set_calls.borrow_mut() += 1;
        if *self.fail_set.borrow() {
            return Err(security::KeyError::Keyring(
                "[test] forced SET backend failure".to_string(),
            ));
        }
        self.data.borrow_mut().insert(
            (service.to_string(), account.to_string()),
            value.to_string(),
        );
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), security::KeyError> {
        *self.delete_calls.borrow_mut() += 1;
        if *self.fail_delete.borrow() {
            return Err(security::KeyError::Keyring(
                "[test] forced DELETE backend failure".to_string(),
            ));
        }
        self.data
            .borrow_mut()
            .remove(&(service.to_string(), account.to_string()));
        Ok(())
    }
}

const DET_SERVICE: &str = "LifeOS-det-test";

/// Requirement 1 (deterministic): DB absent + key absent -> a fresh key
/// is generated, stored, AND is exactly the key the initializer used to
/// create the database (not merely "some key was stored somewhere") -
/// also checks the generated key's format is EXACTLY 64 lowercase
/// hexadecimal characters (`0`-`9`, `a`-`f` only = 256 bits) - not
/// merely `is_ascii_hexdigit()`, which would also accept uppercase
/// `A`-`F` and so would not actually prove lowercase output.
#[test]
fn req_dkl1_db_absent_key_absent_generates_and_stores_key_and_creates_db() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dkl1.db");
    let store = InMemoryCredentialStore::new();
    let account = "dkl1";

    let key_seen_by_initializer: RefCell<Option<String>> = RefCell::new(None);
    let conn = bootstrap_db_with_key_lifecycle_with_store(
        &store,
        &path,
        DET_SERVICE,
        account,
        |p, key_hex| {
            *key_seen_by_initializer.borrow_mut() = Some(key_hex.to_string());
            bootstrap_db_at(p, key_hex)
        },
    )
    .expect("[req_dkl1] FAIL - Case A bootstrap must succeed");
    drop(conn);

    assert!(path.exists(), "[req_dkl1] FAIL - db file was not created");
    let stored = store
        .raw_get(DET_SERVICE, account)
        .expect("[req_dkl1] FAIL - no key was stored");
    assert_eq!(
        stored.len(),
        64,
        "[req_dkl1] FAIL - expected a 64-hex-char (256-bit) key, got length {}",
        stored.len()
    );
    assert!(
        stored
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
        "[req_dkl1] FAIL - stored key is not lowercase hex (0-9, a-f only): {stored}"
    );
    assert_eq!(
        Some(stored.clone()),
        key_seen_by_initializer.into_inner(),
        "[req_dkl1] FAIL - the key stored in the credential store is not the exact key the initializer used to create the db"
    );
    assert_eq!(
        store.set_calls(),
        1,
        "[req_dkl1] FAIL - expected exactly one set() call for a fresh Case A credential"
    );
    println!("[req_dkl1] PASS - db absent + key absent -> a well-formed fresh key was generated, stored, and is exactly the key used to create the db (deterministic, no OS keyring involved)");
}

/// Requirement 2 (deterministic): DB exists + key exists -> the exact
/// existing key is reused across repeated bootstraps, and NO additional
/// `set()` call is ever made once the credential exists.
#[test]
fn req_dkl2_db_exists_key_exists_reuses_exact_key_no_extra_set_calls() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dkl2.db");
    let store = InMemoryCredentialStore::new();
    let account = "dkl2";

    let conn = bootstrap_db_with_key_lifecycle_with_store(
        &store,
        &path,
        DET_SERVICE,
        account,
        bootstrap_db_at,
    )
    .expect("[req_dkl2] FAIL - first (Case A) bootstrap must succeed");
    drop(conn);
    let first_key = store.raw_get(DET_SERVICE, account).unwrap();
    assert_eq!(store.set_calls(), 1);

    let conn = bootstrap_db_with_key_lifecycle_with_store(
        &store,
        &path,
        DET_SERVICE,
        account,
        bootstrap_db_at,
    )
    .expect("[req_dkl2] FAIL - second (Case B) bootstrap must succeed");
    drop(conn);
    let second_key = store.raw_get(DET_SERVICE, account).unwrap();

    assert_eq!(
        first_key, second_key,
        "[req_dkl2] FAIL - key changed on reopen despite db+credential both already existing"
    );
    assert_eq!(
        store.set_calls(),
        1,
        "[req_dkl2] FAIL - Case B reuse must never call set() again"
    );
    println!("[req_dkl2] PASS - db exists + key exists -> exact same key reused, zero additional set() calls");
}

/// Requirement 3 (deterministic, CRITICAL): DB exists + key absent ->
/// typed `MasterKeyMissingForExistingDatabase`, zero key creation
/// (set_calls stays 0 on the fresh store standing in for "credential now
/// absent"), db bytes byte-for-byte unchanged.
#[test]
fn req_dkl3_db_exists_key_absent_fails_safely_zero_key_creation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dkl3.db");
    let account = "dkl3";

    let creating_store = InMemoryCredentialStore::new();
    let conn = bootstrap_db_with_key_lifecycle_with_store(
        &creating_store,
        &path,
        DET_SERVICE,
        account,
        bootstrap_db_at,
    )
    .expect("[req_dkl3] FAIL - setup bootstrap must succeed");
    drop(conn);
    let db_bytes_before = std::fs::read(&path).unwrap();

    // A brand-new, empty store standing in for "the credential is now
    // absent" (e.g. after a credential-store reset) - the db file itself
    // is untouched.
    let empty_store = InMemoryCredentialStore::new();
    match bootstrap_db_with_key_lifecycle_with_store(
        &empty_store,
        &path,
        DET_SERVICE,
        account,
        bootstrap_db_at,
    ) {
        Err(BootstrapError::MasterKeyMissingForExistingDatabase) => {}
        other => {
            panic!("[req_dkl3] FAIL - expected MasterKeyMissingForExistingDatabase, got {other:?}")
        }
    }
    assert_eq!(
        empty_store.set_calls(),
        0,
        "[req_dkl3] FAIL - a replacement key was generated/stored despite Case C"
    );
    let db_bytes_after = std::fs::read(&path).unwrap();
    assert_eq!(
        db_bytes_before, db_bytes_after,
        "[req_dkl3] FAIL - db bytes changed despite Case C returning before any db access"
    );
    println!("[req_dkl3] PASS - db exists + key absent -> typed recovery-required error, zero key creation, db bytes unchanged");
}

/// Requirement 4 (deterministic): DB absent + key exists -> the
/// pre-existing key is reused (seeded directly, not via `set()`), and no
/// `set()` call happens for Case D.
#[test]
fn req_dkl4_db_absent_key_exists_reuses_existing_key_no_set_call() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dkl4.db");
    let account = "dkl4";
    let pre_existing_key = security::generate_key_hex();
    let store = InMemoryCredentialStore::seeded(DET_SERVICE, account, &pre_existing_key);

    assert!(!path.exists());
    let conn = bootstrap_db_with_key_lifecycle_with_store(
        &store,
        &path,
        DET_SERVICE,
        account,
        bootstrap_db_at,
    )
    .expect("[req_dkl4] FAIL - Case D bootstrap must succeed");
    drop(conn);

    assert!(path.exists(), "[req_dkl4] FAIL - db was not created");
    let key_after = store.raw_get(DET_SERVICE, account).unwrap();
    assert_eq!(
        pre_existing_key, key_after,
        "[req_dkl4] FAIL - the pre-existing credential was replaced instead of reused"
    );
    assert_eq!(
        store.set_calls(),
        0,
        "[req_dkl4] FAIL - Case D must never call set() - the credential already existed"
    );
    println!("[req_dkl4] PASS - db absent + key exists -> existing key reused to create the new db, zero set() calls");
}

/// Requirement 5 (deterministic): a credential that is PRESENT but WRONG
/// for an existing db still fails as a normal storage error - no
/// replacement key generated.
#[test]
fn req_dkl5_wrong_present_key_fails_without_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dkl5.db");
    let account = "dkl5";

    let real_store = InMemoryCredentialStore::new();
    let conn = bootstrap_db_with_key_lifecycle_with_store(
        &real_store,
        &path,
        DET_SERVICE,
        account,
        bootstrap_db_at,
    )
    .expect("[req_dkl5] FAIL - setup bootstrap must succeed");
    drop(conn);

    let wrong_key = security::generate_key_hex();
    let wrong_store = InMemoryCredentialStore::seeded(DET_SERVICE, account, &wrong_key);
    match bootstrap_db_with_key_lifecycle_with_store(
        &wrong_store,
        &path,
        DET_SERVICE,
        account,
        bootstrap_db_at,
    ) {
        Err(BootstrapError::Storage(_)) => {}
        other => panic!(
            "[req_dkl5] FAIL - expected a Storage error from the wrong-but-present key, got {other:?}"
        ),
    }
    assert_eq!(
        wrong_store.raw_get(DET_SERVICE, account),
        Some(wrong_key),
        "[req_dkl5] FAIL - the present-but-wrong credential was replaced instead of left alone"
    );
    assert_eq!(
        wrong_store.set_calls(),
        0,
        "[req_dkl5] FAIL - no set() call should happen when a present-but-wrong key is rejected"
    );
    println!("[req_dkl5] PASS - a present-but-wrong key fails normally, with zero replacement-key generation");
}

/// Requirement 6 (deterministic): initialization failure BEFORE any db
/// file is created -> the freshly-generated Case A credential is cleaned
/// up. Forced via the same portable blocker-file technique as `req_kl7`,
/// now with the in-memory store.
#[test]
fn req_dkl6_pre_file_creation_failure_cleans_up_fresh_credential() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker_dkl6");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let path = blocker.join("dkl6.db"); // parent is a FILE - create_dir_all must fail
    let account = "dkl6";
    let store = InMemoryCredentialStore::new();

    assert!(!path.exists());
    match bootstrap_db_with_key_lifecycle_with_store(
        &store,
        &path,
        DET_SERVICE,
        account,
        bootstrap_db_at,
    ) {
        Err(BootstrapError::Io(_)) => {}
        other => panic!("[req_dkl6] FAIL - expected an Io error, got {other:?}"),
    }
    assert!(
        !path.exists(),
        "[req_dkl6] setup invariant FAIL - the db path must still not exist"
    );
    assert_eq!(
        store.raw_get(DET_SERVICE, account),
        None,
        "[req_dkl6] FAIL - the freshly-created orphan credential was not cleaned up"
    );
    assert_eq!(store.delete_calls(), 1);
    println!("[req_dkl6] PASS - pre-file-creation initialization failure cleans up the freshly-created orphan credential");
}

/// Requirement 7 (deterministic, THE R016 FIX): initialization failure
/// AFTER the db file has already been created -> the freshly-generated
/// Case A credential is RETAINED, not deleted - proving LifeOS can no
/// longer manufacture "db exists, credential absent" (Case C) as a side
/// effect of its own cleanup path. Uses a custom initializer closure (not
/// `bootstrap_db_at`, which cannot be made to fail deterministically
/// AFTER writing a file) that writes SYNTHETIC (not real encrypted
/// database) bytes to `path` and then deliberately returns an error.
///
/// What this proves: the DB/key ASSOCIATION is retained and a
/// subsequent bootstrap attempt can retry with the same key. What this
/// does NOT prove: that a genuinely partially-written encrypted database
/// would itself be valid, that SQLCipher could successfully reopen it,
/// or that migrations could successfully resume - actual recovery of a
/// partially-created database is a separate, unproven question outside
/// this test's synthetic-bytes initializer.
#[test]
fn req_dkl7_post_file_creation_failure_retains_fresh_credential() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dkl7.db");
    let account = "dkl7";
    let store = InMemoryCredentialStore::new();

    assert!(!path.exists());
    let result = bootstrap_db_with_key_lifecycle_with_store(
        &store,
        &path,
        DET_SERVICE,
        account,
        |p, _key_hex| {
            // Stands in for bootstrap_db_at creating/writing the db file
            // using the fresh key, THEN failing later (e.g. during
            // verification or migration) - a real FILE now exists at
            // `p`, but its bytes are synthetic placeholder content, NOT
            // an actual encrypted database. This test proves the
            // credential-cleanup decision (retain, not delete) is
            // correct once a file exists; it says nothing about whether
            // a genuinely partially-written encrypted database would
            // itself be recoverable.
            std::fs::write(
                p,
                b"synthetic placeholder bytes, not a real encrypted database",
            )
            .unwrap();
            Err(BootstrapError::Io(std::io::Error::other(
                "[req_dkl7] deliberate forced failure AFTER db file creation",
            )))
        },
    );

    match result {
        Err(BootstrapError::Io(_)) => {}
        other => panic!("[req_dkl7] FAIL - expected the forced Io error, got {other:?}"),
    }
    assert!(
        path.exists(),
        "[req_dkl7] setup invariant FAIL - the db path must now exist"
    );
    let stored = store.raw_get(DET_SERVICE, account);
    assert!(
        stored.is_some(),
        "[req_dkl7] FAIL - the freshly-created credential was deleted despite a db file now existing - this would strand the db as Case C on the next launch"
    );
    assert_eq!(
        store.delete_calls(),
        0,
        "[req_dkl7] FAIL - delete() must never be called once a db file exists at the target path"
    );

    // The resulting state must NOT be re-resolved as Case C - a fresh
    // bootstrap attempt against this same (store, path) must be able to
    // proceed as Case B (both now present), never fail with
    // MasterKeyMissingForExistingDatabase merely because of this cleanup
    // path.
    let resolved = resolve_master_key_with_store(&store, &path, DET_SERVICE, account)
        .expect("[req_dkl7] FAIL - re-resolving must not fail");
    match resolved {
        ResolvedMasterKey::Reused(k) => assert_eq!(Some(k), stored),
        ResolvedMasterKey::FreshlyGenerated(_) => panic!(
            "[req_dkl7] FAIL - a SECOND fresh key was generated - the retained credential was not recognized as already present"
        ),
    }
    println!("[req_dkl7] PASS - post-file-creation initialization failure retains the freshly-created credential; the resulting (db, key) pair correctly re-resolves as Case B, never Case C");
}

/// Requirement 8 (deterministic): initialization failure with a
/// PRE-EXISTING credential leaves it completely untouched, regardless of
/// whether a db file now exists - the cleanup branch never runs at all
/// for `Reused` credentials (Cases B/D).
#[test]
fn req_dkl8_pre_existing_credential_retained_on_init_failure() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker_dkl8");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let path = blocker.join("dkl8.db");
    let account = "dkl8";
    let pre_existing_key = security::generate_key_hex();
    let store = InMemoryCredentialStore::seeded(DET_SERVICE, account, &pre_existing_key);

    match bootstrap_db_with_key_lifecycle_with_store(
        &store,
        &path,
        DET_SERVICE,
        account,
        bootstrap_db_at,
    ) {
        Err(BootstrapError::Io(_)) => {}
        other => panic!("[req_dkl8] FAIL - expected an Io error, got {other:?}"),
    }
    assert_eq!(
        store.raw_get(DET_SERVICE, account),
        Some(pre_existing_key),
        "[req_dkl8] FAIL - the pre-existing credential was deleted or replaced"
    );
    assert_eq!(
        store.delete_calls(),
        0,
        "[req_dkl8] FAIL - delete() must never be called for a pre-existing (Reused) credential"
    );
    println!("[req_dkl8] PASS - pre-existing credential remains completely untouched after an initialization failure");
}

/// Requirement 9 (deterministic): repeated successful reopen uses the
/// exact same master key every time, with no additional set() calls
/// beyond the first.
#[test]
fn req_dkl9_repeated_reopen_uses_same_key_no_extra_set_calls() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dkl9.db");
    let account = "dkl9";
    let store = InMemoryCredentialStore::new();

    let conn = bootstrap_db_with_key_lifecycle_with_store(
        &store,
        &path,
        DET_SERVICE,
        account,
        bootstrap_db_at,
    )
    .expect("[req_dkl9] FAIL - first bootstrap must succeed");
    drop(conn);
    let key_first = store.raw_get(DET_SERVICE, account).unwrap();

    for i in 0..3 {
        let conn = bootstrap_db_with_key_lifecycle_with_store(
            &store,
            &path,
            DET_SERVICE,
            account,
            bootstrap_db_at,
        )
        .unwrap_or_else(|e| panic!("[req_dkl9] FAIL - reopen #{i} failed: {e:?}"));
        drop(conn);
        let key_now = store.raw_get(DET_SERVICE, account).unwrap();
        assert_eq!(
            key_first, key_now,
            "[req_dkl9] FAIL - key changed on reopen #{i}"
        );
    }
    assert_eq!(
        store.set_calls(),
        1,
        "[req_dkl9] FAIL - only the very first Case A bootstrap should ever call set()"
    );
    println!("[req_dkl9] PASS - repeated reopen across 3 additional bootstraps all used the exact same key, with exactly one set() call ever");
}

/// Requirement 10 (deterministic): a credential-backend GET error
/// propagates as a typed `BootstrapError::Key`, and is never
/// misinterpreted as "credential absent" - no key is generated/stored as
/// a result.
#[test]
fn req_dkl10_backend_get_error_is_distinct_from_absence_no_key_generated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dkl10.db");
    let account = "dkl10";
    let store = InMemoryCredentialStore::new();
    store.fail_next_get();

    match resolve_master_key_with_store(&store, &path, DET_SERVICE, account) {
        Err(BootstrapError::Key(_)) => {}
        other => panic!(
            "[req_dkl10] FAIL - a GET backend error must surface as BootstrapError::Key, got {other:?}"
        ),
    }
    assert_eq!(
        store.set_calls(),
        0,
        "[req_dkl10] FAIL - a GET backend error must never be treated as 'absent', which would trigger key generation"
    );
    assert!(
        !path.exists(),
        "[req_dkl10] FAIL - no db initialization should have been attempted"
    );
    println!("[req_dkl10] PASS - a credential-backend GET error is a distinct, typed failure - never interpreted as 'missing', never triggers key generation");
}

/// Requirement 11 (deterministic): a credential-backend SET error during
/// a Case A (first-run) resolution means database initialization never
/// even starts.
#[test]
fn req_dkl11_backend_set_error_on_first_run_prevents_db_initialization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dkl11.db");
    let account = "dkl11";
    let store = InMemoryCredentialStore::new();
    store.fail_next_set();

    let initializer_calls = RefCell::new(0u32);
    let result = bootstrap_db_with_key_lifecycle_with_store(
        &store,
        &path,
        DET_SERVICE,
        account,
        |p, key_hex| {
            *initializer_calls.borrow_mut() += 1;
            bootstrap_db_at(p, key_hex)
        },
    );

    match result {
        Err(BootstrapError::Key(_)) => {}
        other => panic!(
            "[req_dkl11] FAIL - a SET backend error on first run must surface as BootstrapError::Key, got {other:?}"
        ),
    }
    assert_eq!(
        *initializer_calls.borrow(),
        0,
        "[req_dkl11] FAIL - the db initializer must never run if the fresh credential could not be stored"
    );
    assert!(
        !path.exists(),
        "[req_dkl11] FAIL - no db file should have been created"
    );
    println!("[req_dkl11] PASS - a credential-backend SET error on first run prevents db initialization from ever starting");
}

/// Requirement 12 (deterministic): a credential-backend DELETE error
/// during the pre-file-creation cleanup path does not mask the original
/// initialization error - the ORIGINAL error remains what the caller
/// sees. Documents the resulting state: because deletion itself failed,
/// the freshly-generated credential is left behind even though no db
/// file exists (a known, documented, best-effort residual risk - see
/// `bootstrap_db_with_key_lifecycle_with_store`'s doc comment).
#[test]
fn req_dkl12_backend_delete_error_during_cleanup_does_not_mask_original_error() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker_dkl12");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let path = blocker.join("dkl12.db");
    let account = "dkl12";
    let store = InMemoryCredentialStore::new();
    store.fail_next_delete();

    match bootstrap_db_with_key_lifecycle_with_store(
        &store, &path, DET_SERVICE, account, bootstrap_db_at,
    ) {
        Err(BootstrapError::Io(_)) => {
            println!("[req_dkl12] PASS - the ORIGINAL Io initialization error was returned, not a delete-cleanup error");
        }
        other => panic!(
            "[req_dkl12] FAIL - expected the original Io error to survive a failed cleanup delete, got {other:?}"
        ),
    }
    assert_eq!(
        store.delete_calls(),
        1,
        "[req_dkl12] FAIL - a cleanup delete attempt should still have been made"
    );
    // Documented residual state: deletion failed, so the freshly-created
    // credential is still present despite no db file existing at path -
    // a known, best-effort limitation, not silently hidden.
    assert!(
        store.raw_get(DET_SERVICE, account).is_some(),
        "[req_dkl12] FAIL (documentation check) - expected the credential to still be present given the forced delete failure"
    );
    assert!(!path.exists());
    println!("[req_dkl12] PASS - documented residual state after a failed cleanup delete: original init error surfaced unchanged, orphan credential left behind (best-effort cleanup, not silently masked)");
}
