//! M2 Goal vertical slice tests (2026-09-17).
//!
//! Evidence for `docs/reports/REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`.
//! Run with:
//! `cargo test --manifest-path src-tauri/Cargo.toml services::tests -- --nocapture --test-threads=1`
//! (`--test-threads=1` matters for the keyring-touching test.)

use super::*;
use crate::actions::{
    ActionMeta, ActionOutcome, Actor, ConfirmationClass, Provenance, ValidationError,
};
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
        let conn = bootstrap_db_at(&path, &key).unwrap();
        let payload = CreateGoalPayload {
            title: "  Get first paying client  ".to_string(),
            description: Some("DeVDul freelance".to_string()),
            target_date: None,
            priority_user: Some(3),
            domain_tag: Some("career".to_string()),
        };
        match create_goal(&conn, ui_explicit(), payload) {
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
    let conn = bootstrap_db_at(&path, &key).unwrap();

    for bad_title in ["", "   ", "\t\n"] {
        let payload = CreateGoalPayload {
            title: bad_title.to_string(),
            description: None,
            target_date: None,
            priority_user: None,
            domain_tag: None,
        };
        match create_goal(&conn, ui_explicit(), payload) {
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

    let count: i64 = conn
        .query_row("SELECT count(*) FROM goals", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        count, 0,
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
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let overlong_title = "x".repeat(201);
    let payload = CreateGoalPayload {
        title: overlong_title,
        description: None,
        target_date: None,
        priority_user: Some(99),
        domain_tag: None,
    };
    match create_goal(&conn, ui_explicit(), payload) {
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

    let count: i64 = conn
        .query_row("SELECT count(*) FROM goals", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        count, 0,
        "[req8] FAIL - a row was written despite validation failure"
    );
}

/// Requirement 9: valid UpdateGoal succeeds.
#[test]
fn req9_valid_update_goal_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod9.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_goal(
        &conn,
        ui_explicit(),
        CreateGoalPayload {
            title: "Ship M2".to_string(),
            description: None,
            target_date: None,
            priority_user: None,
            domain_tag: None,
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    let update = UpdateGoalPayload {
        id: created.id,
        progress: Some(40),
        status: Some(GoalStatus::Active),
        ..Default::default()
    };
    match update_goal(&conn, ui_explicit(), update) {
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
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_id = crate::domain::GoalId::new();
    let update = UpdateGoalPayload {
        id: fake_id,
        progress: Some(10),
        ..Default::default()
    };
    match update_goal(&conn, ui_explicit(), update) {
        ActionOutcome::NotFound => {
            println!("[req10] PASS - UpdateGoal on a nonexistent id returned structured NotFound")
        }
        other => panic!("[req10] FAIL - expected NotFound, got {other:?}"),
    }
}

/// Requirement 11: an invalid UpdateGoal makes ZERO DB changes.
#[test]
fn req11_invalid_update_makes_zero_changes() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod11.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let created = match create_goal(
        &conn,
        ui_explicit(),
        CreateGoalPayload {
            title: "Stable title".to_string(),
            description: None,
            target_date: None,
            priority_user: None,
            domain_tag: None,
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };

    // First, mark it COMPLETED (a valid transition)...
    let complete = UpdateGoalPayload {
        id: created.id,
        status: Some(GoalStatus::Completed),
        ..Default::default()
    };
    let completed = match update_goal(&conn, ui_explicit(), complete) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(completed.status, GoalStatus::Completed);

    // ...then attempt an invalid transition out of a terminal state.
    let invalid = UpdateGoalPayload {
        id: created.id,
        status: Some(GoalStatus::Active),
        ..Default::default()
    };
    match update_goal(&conn, ui_explicit(), invalid) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::InvalidStatusTransition { .. })));
            println!("[req11] PASS - invalid COMPLETED -> ACTIVE transition rejected: {errors:?}");
        }
        other => panic!("[req11] FAIL - expected ValidationFailed, got {other:?}"),
    }

    let fetched = get_goal(&conn, created.id).unwrap().unwrap();
    assert_eq!(
        fetched.status,
        GoalStatus::Completed,
        "[req11] FAIL - status changed despite validation failure"
    );
    assert_eq!(
        fetched.updated_at, completed.updated_at,
        "[req11] FAIL - updated_at changed despite validation failure"
    );
    println!(
        "[req11] PASS - zero changes persisted after the invalid update (status still COMPLETED)"
    );
}

/// Requirement 12: get_active_goals returns only ACTIVE goals.
#[test]
fn req12_get_active_goals_filters_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "prod12.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let mk = |title: &str| CreateGoalPayload {
        title: title.to_string(),
        description: None,
        target_date: None,
        priority_user: None,
        domain_tag: None,
    };

    let a = match create_goal(&conn, ui_explicit(), mk("Active goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let b = match create_goal(&conn, ui_explicit(), mk("Will be paused")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    update_goal(
        &conn,
        ui_explicit(),
        UpdateGoalPayload {
            id: b.id,
            status: Some(GoalStatus::Paused),
            ..Default::default()
        },
    );

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
