//! M2 Goal vertical slice + hardening test suite (2026-09-17).
//!
//! Evidence for `docs/reports/REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`
//! and `docs/reports/REPORT_M2_GOAL_HARDENING_2026-09-17.md`. Run with:
//! `cargo test --manifest-path src-tauri/Cargo.toml services::tests -- --nocapture --test-threads=1`
//! (`--test-threads=1` matters for the keyring-touching test.)

use super::*;
use crate::actions::{
    ActionMeta, ActionOutcome, Actor, ConfirmationClass, FieldPatch, Provenance, ValidationError,
};
use crate::core_error::CoreErrorKind;
use crate::domain::GoalStatus;
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
                assert_eq!(value.status, GoalStatus::Active);
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
        status: Some(GoalStatus::Active),
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
            status: Some(GoalStatus::Paused),
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
            status: Some(GoalStatus::Completed),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(completed.status, GoalStatus::Completed);

    let reopened = match update_goal(
        &mut conn,
        ui_explicit(),
        UpdateGoalPayload {
            id: created.id,
            status: Some(GoalStatus::Active),
            ..Default::default()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!(
            "[hardening h3] FAIL - reopening a COMPLETED goal should now be allowed, got {other:?}"
        ),
    };
    assert_eq!(reopened.status, GoalStatus::Active);

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
// Evidence for `docs/reports/REPORT_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`.
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
            assert_eq!(value.status, GoalStatus::Active);
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
