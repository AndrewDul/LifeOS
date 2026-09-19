//! M2 Goal vertical slice + hardening test suite (2026-09-17).
//!
//! Evidence for `docs/reports/R005_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`
//! and `docs/reports/R006_M2_GOAL_HARDENING_2026-09-17.md`. Run with:
//! `cargo test --manifest-path src-tauri/Cargo.toml services::tests -- --nocapture --test-threads=1`
//! (`--test-threads=1` matters for the keyring-touching test.)

use super::*;
use crate::actions::{
    ActionMeta, ActionOutcome, Actor, AddMemoryPayload, AddRelationPayload, ConfirmMemoryPayload,
    ConfirmationClass, CorrectMemoryPayload, CorrectRelationPayload, EntityInputRef,
    FactValueInput, FieldPatch, ForgetMemoryPayload, NewEntityData, NewEvidenceData, NewSourceData,
    Provenance, SourceRef, ValidationError,
};
use crate::core_error::CoreErrorKind;
use crate::domain::{
    is_active_relation, DomainRef, EpistemicState, Evidence, EvidenceId, FactValue,
    GoalRelationType, LifecycleStatus, MemoryType, MilestoneStatus, Relation, RelationId, Source,
    SourceId, TaskStatus,
};
use std::fs;
use std::io::Read;
use uuid::Uuid;

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

// =========================================================================
// M2 Personal Knowledge Slice A Foundation (R020, 2026-09-18) - the V7
// eleven-table schema, `ensure_self_entity` bootstrap, narrow
// `get_entity`/`get_fact` reads, and `secure_delete = ON` storage
// hardening. Evidence for
// `docs/reports/R020_M2_PERSONAL_KNOWLEDGE_SLICE_A_FOUNDATION_2026-09-18.md`.
//
// NO AddMemory/CorrectMemory/ConfirmMemory/ForgetMemory exists yet
// (Slice B+) - every test below either proves schema/DB-level
// integrity via deliberate raw SQL (the same "attempt the illegal
// thing, capture the real rejection" style already established
// throughout M2), or exercises `ensure_self_entity`/`get_entity`/
// `get_fact` through test-fixture inserts
// (`storage::personal_knowledge_repo::insert_*`, `#[cfg(test)]`-only),
// never a production Memory action.
// =========================================================================

fn mk_pk_entity(entity_type: &str, canonical_name: &str, sensitivity: Sensitivity) -> Entity {
    let now = Utc::now();
    Entity {
        id: EntityId::new(),
        entity_type: entity_type.to_string(),
        canonical_name: canonical_name.to_string(),
        is_self: false,
        sensitivity,
        domain_ref: None,
        created_at: now,
        updated_at: now,
    }
}

fn mk_pk_source(source_type: &str, sensitivity: Sensitivity) -> Source {
    let now = Utc::now();
    Source {
        id: SourceId::new(),
        source_type: source_type.to_string(),
        label: None,
        reference: None,
        sensitivity,
        captured_at: now,
        created_at: now,
    }
}

fn mk_pk_evidence(
    source_id: SourceId,
    content: Option<&str>,
    sensitivity: Sensitivity,
) -> Evidence {
    Evidence {
        id: EvidenceId::new(),
        source_id,
        content: content.map(|s| s.to_string()),
        locator: if content.is_none() {
            Some("test-locator".to_string())
        } else {
            None
        },
        sensitivity,
        created_at: Utc::now(),
    }
}

fn mk_pk_fact(
    subject_entity_id: EntityId,
    predicate: &str,
    value: FactValue,
    memory_type: MemoryType,
    epistemic_state: EpistemicState,
    sensitivity: Sensitivity,
) -> Fact {
    let now = Utc::now();
    Fact {
        id: FactId::new(),
        subject_entity_id,
        predicate: predicate.to_string(),
        value,
        memory_type,
        epistemic_state,
        confidence: None,
        sensitivity,
        valid_from: now,
        valid_until: None,
        superseded_by_fact_id: None,
        created_at: now,
        updated_at: now,
    }
}

// --- Secure delete (R019 §17, corrected R020 §1; implemented R020 §17-19) --

/// Requirement 1: a freshly-opened encrypted connection has
/// `secure_delete` actively ON.
#[test]
fn req_sd1_secure_delete_on_for_fresh_connection() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "sd1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let value: i64 = conn
        .query_row("PRAGMA secure_delete", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        value, 1,
        "[req_sd1] FAIL - secure_delete was not ON on a fresh connection"
    );
    println!("[req_sd1] PASS - secure_delete = ON on a fresh encrypted connection");
}

/// Requirement 2: `secure_delete` is applied again on every connection,
/// not merely assumed to persist in the database file across a
/// close+reopen.
#[test]
fn req_sd2_secure_delete_on_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "sd2.db");
    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        drop(conn);
    }
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let value: i64 = conn
        .query_row("PRAGMA secure_delete", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        value, 1,
        "[req_sd2] FAIL - secure_delete was not ON after reopen"
    );
    println!("[req_sd2] PASS - secure_delete = ON after close+reopen, proving it is actively applied per-connection, not merely assumed persistent");
}

/// Requirement 3: wrong-key rejection behavior is unchanged by the
/// `secure_delete` hardening.
#[test]
fn req_sd3_wrong_key_behavior_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "sd3.db");
    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        drop(conn);
    }
    let wrong_key = generate_test_key_hex();
    let conn = storage::open_encrypted(&path, &wrong_key).unwrap();
    let result = storage::verify_key(&conn);
    assert!(
        result.is_err(),
        "[req_sd3] FAIL - a wrong key should still fail verify_key after the secure_delete change"
    );
    println!("[req_sd3] PASS - wrong-key rejection is unchanged by the secure_delete hardening");
}

/// Requirement 4: encryption-at-rest is unchanged by the
/// `secure_delete` hardening (no plaintext SQLite header).
#[test]
fn req_sd4_encryption_at_rest_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "sd4.db");
    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        drop(conn);
    }
    let mut raw = Vec::new();
    fs::File::open(&path)
        .unwrap()
        .read_to_end(&mut raw)
        .unwrap();
    assert!(
        !raw.windows(6).any(|w| w == b"SQLite"),
        "[req_sd4] FAIL - file starts with the plaintext SQLite header"
    );
    println!("[req_sd4] PASS - encryption-at-rest unchanged by the secure_delete hardening");
}

/// Requirement 5: `foreign_keys` and `secure_delete` are both ON
/// simultaneously on the same connection.
#[test]
fn req_sd5_foreign_keys_on_simultaneously_with_secure_delete() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "sd5.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    let sd: i64 = conn
        .query_row("PRAGMA secure_delete", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1, "[req_sd5] FAIL - foreign_keys not ON");
    assert_eq!(sd, 1, "[req_sd5] FAIL - secure_delete not ON");
    println!("[req_sd5] PASS - foreign_keys and secure_delete are both ON simultaneously");
}

// --- Journal mode (R019 §17, observation only) --------------------------

/// Observation only (R019 §17/R020 §20) - this test does NOT assert or
/// force a specific mode; it records the actually-measured value for
/// R020's report. No `journal_mode` PRAGMA is set anywhere in this
/// codebase, so this reflects SQLite's/SQLCipher's own compiled
/// default for this exact `rusqlite`+`bundled-sqlcipher` 0.37.0 build.
#[test]
fn req_jm1_measure_current_journal_mode() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "jm1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    println!("[req_jm1] MEASURED - PRAGMA journal_mode = {mode}");
}

// --- SELF bootstrap (R019 §3, corrected/implemented R020 §21-23) --------

/// Requirement A: a fresh bootstrap creates exactly one SELF entity.
#[test]
fn req_self1_fresh_bootstrap_creates_exactly_one_self() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "self1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM entities WHERE is_self = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        count, 1,
        "[req_self1] FAIL - expected exactly one SELF entity after fresh bootstrap"
    );
    println!("[req_self1] PASS - fresh bootstrap creates exactly one SELF entity");
}

/// Requirements B-E: the SELF id survives a close+reopen unchanged.
#[test]
fn req_self2_self_id_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "self2.db");
    let first_id;
    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        first_id = ensure_self_entity(&conn).unwrap().id;
    }
    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        let self_entity = ensure_self_entity(&conn).unwrap();
        assert_eq!(
            self_entity.id, first_id,
            "[req_self2] FAIL - SELF id changed across close+reopen"
        );
    }
    println!("[req_self2] PASS - SELF entity id survives close and reopen unchanged");
}

/// Requirement F: `ensure_self_entity` is idempotent across 3+ repeated
/// calls - same id, still exactly one row.
#[test]
fn req_self3_ensure_self_entity_idempotent_repeated_calls() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "self3.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let first = ensure_self_entity(&conn).unwrap();
    for i in 0..3 {
        let again = ensure_self_entity(&conn).unwrap();
        assert_eq!(
            again.id, first.id,
            "[req_self3] FAIL - repeated call #{i} returned a different SELF id"
        );
    }
    let count: i64 = conn
        .query_row("SELECT count(*) FROM entities WHERE is_self = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        count, 1,
        "[req_self3] FAIL - repeated ensure_self_entity calls created more than one SELF row"
    );
    println!("[req_self3] PASS - ensure_self_entity is idempotent across repeated calls, still exactly one row");
}

/// Requirement G: a raw second `is_self = 1` insert is rejected by the
/// real DB partial unique index.
#[test]
fn req_self4_raw_duplicate_self_insert_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "self4.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let now = Utc::now().to_rfc3339();
    let result = conn.execute(
        "INSERT INTO entities (id, entity_type, canonical_name, is_self, sensitivity, domain_ref_type, domain_ref_id, created_at, updated_at)
         VALUES (?1, 'person', 'DUPLICATE_SELF', 1, 'INTERNAL', NULL, NULL, ?2, ?2)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), now],
    );
    match result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert_eq!(
                err.code,
                rusqlite::ErrorCode::ConstraintViolation,
                "[req_self4] FAIL - expected a constraint-violation error, got {err:?}"
            );
            println!("[req_self4] PASS - a second raw is_self=1 insert is rejected by the real DB unique index: {err:?}");
        }
        other => panic!("[req_self4] FAIL - expected a constraint-violation error, got {other:?}"),
    }
}

/// Requirement H: the canonical SELF row matches the frozen shape
/// exactly.
#[test]
fn req_self5_canonical_self_row_exact_values() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "self5.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let self_entity = ensure_self_entity(&conn).unwrap();
    assert_eq!(self_entity.entity_type, "person");
    assert_eq!(self_entity.canonical_name, "SELF");
    assert!(self_entity.is_self);
    assert_eq!(self_entity.sensitivity, Sensitivity::Internal);
    assert!(self_entity.domain_ref.is_none());
    println!("[req_self5] PASS - canonical SELF row matches the frozen shape exactly: entity_type=person, canonical_name=SELF, is_self=true, sensitivity=INTERNAL, domain_ref=None");
}

// --- Migration tests (R020 §30) ------------------------------------------

/// A fresh V1-through-V7 migration succeeds; all eleven Personal
/// Knowledge tables exist.
#[test]
fn req_mig1_fresh_v1_through_v7_migration_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "mig1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    for table in [
        "entities",
        "sources",
        "evidence",
        "facts",
        "relations",
        "episodes",
        "fact_evidence",
        "relation_evidence",
        "episode_evidence",
        "episode_entities",
        "fact_audit_log",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            exists, 1,
            "[req_mig1] FAIL - table {table} does not exist after fresh migration"
        );
    }
    println!(
        "[req_mig1] PASS - fresh V1-V7 migration succeeds, all 11 Personal Knowledge tables exist"
    );
}

/// A populated V6 database (real Goal/Project/Milestone/Task/
/// GoalRelation rows - the actual shape of an existing user's database
/// before this run) upgrades to V7 non-destructively: old data
/// unchanged, Personal Knowledge tables exist, SELF entity created by
/// the upgrade bootstrap.
#[test]
fn req_mig2_populated_v6_database_upgrades_non_destructively_to_v7() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "mig2.db");

    let goal_id;
    let project_id;
    let milestone_id;
    let task_id;
    let relation_id;
    {
        let mut conn = bootstrap_db_at_version(&path, &key, 6).unwrap();
        let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Pre-V7 goal")) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        goal_id = goal.id;
        let project = match create_project(
            &mut conn,
            ui_explicit(),
            mk_create_project_payload("Pre-V7 project", Some(goal_id)),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        project_id = project.id;
        let milestone = match add_milestone(
            &mut conn,
            ui_explicit(),
            mk_add_milestone_payload(project_id, "Pre-V7 milestone", 0),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        milestone_id = milestone.id;
        let task = match add_task(
            &mut conn,
            ui_explicit(),
            mk_add_task_payload("Pre-V7 task", None, Some(project_id), Some(goal_id)),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        task_id = task.id;
        let goal2 = match create_goal(
            &mut conn,
            ui_explicit(),
            mk_create_payload("Pre-V7 second goal"),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        let relation = match link_goals(
            &mut conn,
            ui_explicit(),
            mk_link_payload(goal_id, goal2.id, GoalRelationType::Supports),
        ) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        relation_id = relation.id;
    }

    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        assert!(
            get_goal(&conn, goal_id).unwrap().is_some(),
            "[req_mig2] FAIL - pre-V7 Goal lost"
        );
        assert!(
            get_project(&conn, project_id).unwrap().is_some(),
            "[req_mig2] FAIL - pre-V7 Project lost"
        );
        assert!(
            get_milestone(&conn, milestone_id).unwrap().is_some(),
            "[req_mig2] FAIL - pre-V7 Milestone lost"
        );
        assert!(
            get_task(&conn, task_id).unwrap().is_some(),
            "[req_mig2] FAIL - pre-V7 Task lost"
        );
        assert!(
            get_goal_relation(&conn, relation_id).unwrap().is_some(),
            "[req_mig2] FAIL - pre-V7 GoalRelation lost"
        );

        let self_count: i64 = conn
            .query_row("SELECT count(*) FROM entities WHERE is_self = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            self_count, 1,
            "[req_mig2] FAIL - SELF entity not created during the V6->V7 upgrade bootstrap"
        );
        println!("[req_mig2] PASS - populated V6 database upgrades to V7 non-destructively: all pre-existing Goal/Project/Milestone/Task/GoalRelation data survives, Personal Knowledge tables exist, SELF entity created");
    }
}

// --- Raw DB defense test matrix (R020 §31) -------------------------------

/// Entity: invalid sensitivity, half-populated domain_ref pair,
/// duplicate non-NULL domain_ref pair (duplicate SELF is covered
/// separately by `req_self4`).
#[test]
fn req_pk_entity_defense_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkentity.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let now = Utc::now().to_rfc3339();

    // invalid sensitivity
    let result = conn.execute(
        "INSERT INTO entities (id, entity_type, canonical_name, is_self, sensitivity, domain_ref_type, domain_ref_id, created_at, updated_at)
         VALUES (?1, 'person', 'Bad Sensitivity', 0, 'NOT_A_REAL_LEVEL', NULL, NULL, ?2, ?2)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), now],
    );
    assert!(
        result.is_err(),
        "[req_pk_entity] FAIL - invalid sensitivity should be rejected"
    );

    // half-populated domain_ref pair (type set, id NULL)
    let result = conn.execute(
        "INSERT INTO entities (id, entity_type, canonical_name, is_self, sensitivity, domain_ref_type, domain_ref_id, created_at, updated_at)
         VALUES (?1, 'project', 'Half Domain Ref', 0, 'INTERNAL', 'Project', NULL, ?2, ?2)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), now],
    );
    assert!(
        result.is_err(),
        "[req_pk_entity] FAIL - half-populated domain_ref pair should be rejected"
    );

    // duplicate non-NULL domain_ref pair
    let shared_domain_id = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO entities (id, entity_type, canonical_name, is_self, sensitivity, domain_ref_type, domain_ref_id, created_at, updated_at)
         VALUES (?1, 'project', 'First Pointer', 0, 'INTERNAL', 'Project', ?2, ?3, ?3)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), shared_domain_id, now],
    )
    .unwrap();
    let result = conn.execute(
        "INSERT INTO entities (id, entity_type, canonical_name, is_self, sensitivity, domain_ref_type, domain_ref_id, created_at, updated_at)
         VALUES (?1, 'project', 'Second Pointer', 0, 'INTERNAL', 'Project', ?2, ?3, ?3)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), shared_domain_id, now],
    );
    assert!(
        result.is_err(),
        "[req_pk_entity] FAIL - duplicate (domain_ref_type, domain_ref_id) should be rejected"
    );

    // invalid is_self boolean representation (post-run verification
    // hardening - only `0`/`1` are valid; the DB CHECK
    // `is_self IN (0, 1)` must reject anything else)
    let result = conn.execute(
        "INSERT INTO entities (id, entity_type, canonical_name, is_self, sensitivity, domain_ref_type, domain_ref_id, created_at, updated_at)
         VALUES (?1, 'person', 'Invalid Boolean', 2, 'INTERNAL', NULL, NULL, ?2, ?2)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), now],
    );
    assert!(
        result.is_err(),
        "[req_pk_entity] FAIL - is_self = 2 should be rejected by the real DB CHECK (is_self IN (0, 1))"
    );

    println!("[req_pk_entity] PASS - invalid sensitivity, half-populated domain_ref, duplicate domain_ref, and an invalid is_self boolean value are all rejected by real DB constraints");
}

/// Source: invalid sensitivity.
#[test]
fn req_pk_source_defense_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pksource.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let now = Utc::now().to_rfc3339();

    let result = conn.execute(
        "INSERT INTO sources (id, source_type, label, reference, sensitivity, captured_at, created_at)
         VALUES (?1, 'USER_INPUT', NULL, NULL, 'NOT_A_REAL_LEVEL', ?2, ?2)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), now],
    );
    assert!(
        result.is_err(),
        "[req_pk_source] FAIL - invalid sensitivity should be rejected"
    );
    println!("[req_pk_source] PASS - invalid sensitivity rejected by the real DB CHECK constraint");
}

/// Evidence: invalid sensitivity, nonexistent source FK, content NULL
/// + locator NULL together.
#[test]
fn req_pk_evidence_defense_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkevidence.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let source = mk_pk_source("USER_INPUT", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_source(&conn, &source).unwrap();
    let now = Utc::now().to_rfc3339();

    let result = conn.execute(
        "INSERT INTO evidence (id, source_id, content, locator, sensitivity, created_at)
         VALUES (?1, ?2, 'some content', NULL, 'NOT_A_REAL_LEVEL', ?3)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), source.id.to_string(), now],
    );
    assert!(
        result.is_err(),
        "[req_pk_evidence] FAIL - invalid sensitivity should be rejected"
    );

    let fake_source_id = SourceId::new().to_string();
    let result = conn.execute(
        "INSERT INTO evidence (id, source_id, content, locator, sensitivity, created_at)
         VALUES (?1, ?2, 'some content', NULL, 'INTERNAL', ?3)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), fake_source_id, now],
    );
    assert!(
        result.is_err(),
        "[req_pk_evidence] FAIL - nonexistent source_id FK should be rejected"
    );

    let result = conn.execute(
        "INSERT INTO evidence (id, source_id, content, locator, sensitivity, created_at)
         VALUES (?1, ?2, NULL, NULL, 'INTERNAL', ?3)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), source.id.to_string(), now],
    );
    assert!(
        result.is_err(),
        "[req_pk_evidence] FAIL - content NULL + locator NULL together should be rejected"
    );

    println!("[req_pk_evidence] PASS - invalid sensitivity, nonexistent source FK, and content+locator both NULL are all rejected");
}

/// Fact: nonexistent subject/object FKs, neither/both semantic object,
/// invalid memory_type/epistemic_state/sensitivity, confidence out of
/// bounds, self-supersession, nonexistent superseded-fact FK.
#[test]
fn req_pk_fact_defense_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkfact.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let subject = mk_pk_entity("person", "Fact Defense Subject", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &subject).unwrap();
    let now = Utc::now().to_rfc3339();

    let base_insert = |id: String,
                       subject_id: String,
                       predicate: &str,
                       value_json: Option<&str>,
                       object_entity_id: Option<String>,
                       memory_type: &str,
                       epistemic_state: &str,
                       confidence: Option<f64>,
                       sensitivity: &str,
                       superseded_by: Option<String>| {
        conn.execute(
            "INSERT INTO facts (id, subject_entity_id, predicate, value_json, object_entity_id, memory_type, epistemic_state, confidence, sensitivity, valid_from, valid_until, superseded_by_fact_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11, ?10, ?10)",
            rusqlite::params![id, subject_id, predicate, value_json, object_entity_id, memory_type, epistemic_state, confidence, sensitivity, now, superseded_by],
        )
    };

    // nonexistent subject FK
    let result = base_insert(
        uuid::Uuid::now_v7().to_string(),
        EntityId::new().to_string(),
        "p",
        Some("1"),
        None,
        "FACT",
        "OBSERVED",
        None,
        "INTERNAL",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - nonexistent subject_entity_id should be rejected"
    );

    // nonexistent object entity FK
    let result = base_insert(
        uuid::Uuid::now_v7().to_string(),
        subject.id.to_string(),
        "p",
        None,
        Some(EntityId::new().to_string()),
        "FACT",
        "OBSERVED",
        None,
        "INTERNAL",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - nonexistent object_entity_id should be rejected"
    );

    // neither value_json nor object_entity_id
    let result = base_insert(
        uuid::Uuid::now_v7().to_string(),
        subject.id.to_string(),
        "p",
        None,
        None,
        "FACT",
        "OBSERVED",
        None,
        "INTERNAL",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - neither value_json nor object_entity_id should be rejected"
    );

    // both value_json and object_entity_id
    let result = base_insert(
        uuid::Uuid::now_v7().to_string(),
        subject.id.to_string(),
        "p",
        Some("1"),
        Some(subject.id.to_string()),
        "FACT",
        "OBSERVED",
        None,
        "INTERNAL",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - both value_json and object_entity_id should be rejected"
    );

    // invalid memory_type
    let result = base_insert(
        uuid::Uuid::now_v7().to_string(),
        subject.id.to_string(),
        "p",
        Some("1"),
        None,
        "NOT_A_REAL_TYPE",
        "OBSERVED",
        None,
        "INTERNAL",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - invalid memory_type should be rejected"
    );

    // invalid epistemic_state
    let result = base_insert(
        uuid::Uuid::now_v7().to_string(),
        subject.id.to_string(),
        "p",
        Some("1"),
        None,
        "FACT",
        "NOT_A_REAL_STATE",
        None,
        "INTERNAL",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - invalid epistemic_state should be rejected"
    );

    // invalid sensitivity
    let result = base_insert(
        uuid::Uuid::now_v7().to_string(),
        subject.id.to_string(),
        "p",
        Some("1"),
        None,
        "FACT",
        "OBSERVED",
        None,
        "NOT_A_REAL_LEVEL",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - invalid sensitivity should be rejected"
    );

    // confidence < 0
    let result = base_insert(
        uuid::Uuid::now_v7().to_string(),
        subject.id.to_string(),
        "p",
        Some("1"),
        None,
        "FACT",
        "OBSERVED",
        Some(-0.1),
        "INTERNAL",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - confidence < 0 should be rejected"
    );

    // confidence > 1
    let result = base_insert(
        uuid::Uuid::now_v7().to_string(),
        subject.id.to_string(),
        "p",
        Some("1"),
        None,
        "FACT",
        "OBSERVED",
        Some(1.1),
        "INTERNAL",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - confidence > 1 should be rejected"
    );

    // self supersession
    let self_superseding_id = uuid::Uuid::now_v7().to_string();
    let result = base_insert(
        self_superseding_id.clone(),
        subject.id.to_string(),
        "p",
        Some("1"),
        None,
        "FACT",
        "OBSERVED",
        None,
        "INTERNAL",
        Some(self_superseding_id),
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - a fact superseding itself should be rejected"
    );

    // nonexistent superseded-fact FK
    let result = base_insert(
        uuid::Uuid::now_v7().to_string(),
        subject.id.to_string(),
        "p",
        Some("1"),
        None,
        "FACT",
        "OBSERVED",
        None,
        "INTERNAL",
        Some(FactId::new().to_string()),
    );
    assert!(
        result.is_err(),
        "[req_pk_fact] FAIL - a nonexistent superseded_by_fact_id should be rejected"
    );

    println!("[req_pk_fact] PASS - all 11 Fact defense-in-depth checks rejected as expected");
}

/// Relation: nonexistent from/to Entity FKs, invalid epistemic_state,
/// invalid sensitivity, confidence bounds, self-edge rejected, and a
/// `relation_type` genuinely outside GoalRelation's 11 closed values is
/// still accepted - proving the vocabulary is genuinely open.
#[test]
fn req_pk_relation_defense_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrelation.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let from = mk_pk_entity("person", "Relation From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &from).unwrap();
    let to = mk_pk_entity("person", "Relation To", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &to).unwrap();
    let now = Utc::now().to_rfc3339();

    let insert = |id: String,
                  from_id: String,
                  to_id: String,
                  relation_type: &str,
                  epistemic_state: &str,
                  confidence: Option<f64>,
                  sensitivity: &str| {
        conn.execute(
            "INSERT INTO relations (id, from_entity_id, to_entity_id, relation_type, note, epistemic_state, confidence, sensitivity, valid_from, valid_until, created_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, NULL, ?8)",
            rusqlite::params![id, from_id, to_id, relation_type, epistemic_state, confidence, sensitivity, now],
        )
    };

    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        EntityId::new().to_string(),
        to.id.to_string(),
        "CUSTOM_TYPE",
        "OBSERVED",
        None,
        "INTERNAL",
    );
    assert!(
        result.is_err(),
        "[req_pk_relation] FAIL - nonexistent from_entity_id should be rejected"
    );

    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        from.id.to_string(),
        EntityId::new().to_string(),
        "CUSTOM_TYPE",
        "OBSERVED",
        None,
        "INTERNAL",
    );
    assert!(
        result.is_err(),
        "[req_pk_relation] FAIL - nonexistent to_entity_id should be rejected"
    );

    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        from.id.to_string(),
        to.id.to_string(),
        "CUSTOM_TYPE",
        "NOT_A_REAL_STATE",
        None,
        "INTERNAL",
    );
    assert!(
        result.is_err(),
        "[req_pk_relation] FAIL - invalid epistemic_state should be rejected"
    );

    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        from.id.to_string(),
        to.id.to_string(),
        "CUSTOM_TYPE",
        "OBSERVED",
        None,
        "NOT_A_REAL_LEVEL",
    );
    assert!(
        result.is_err(),
        "[req_pk_relation] FAIL - invalid sensitivity should be rejected"
    );

    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        from.id.to_string(),
        to.id.to_string(),
        "CUSTOM_TYPE",
        "OBSERVED",
        Some(-0.1),
        "INTERNAL",
    );
    assert!(
        result.is_err(),
        "[req_pk_relation] FAIL - confidence < 0 should be rejected"
    );

    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        from.id.to_string(),
        to.id.to_string(),
        "CUSTOM_TYPE",
        "OBSERVED",
        Some(1.1),
        "INTERNAL",
    );
    assert!(
        result.is_err(),
        "[req_pk_relation] FAIL - confidence > 1 should be rejected"
    );

    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        from.id.to_string(),
        from.id.to_string(),
        "CUSTOM_TYPE",
        "OBSERVED",
        None,
        "INTERNAL",
    );
    assert!(
        result.is_err(),
        "[req_pk_relation] FAIL - a self-edge (from == to) should be rejected"
    );

    // Genuinely open vocabulary: a relation_type far outside
    // GoalRelation's 11 closed values must still be ACCEPTED.
    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        from.id.to_string(),
        to.id.to_string(),
        "THIS_IS_NOT_A_GOALRELATION_VALUE_AT_ALL",
        "OBSERVED",
        None,
        "INTERNAL",
    );
    assert!(
        result.is_ok(),
        "[req_pk_relation] FAIL - a relation_type outside GoalRelation's 11 values must be ACCEPTED (the vocabulary is genuinely open): {result:?}"
    );

    println!("[req_pk_relation] PASS - all 6 Relation defense-in-depth rejections hold, and a relation_type outside GoalRelation's 11 closed values is genuinely accepted");
}

/// Episode: invalid sensitivity, `ended_at < started_at` rejected.
#[test]
fn req_pk_episode_defense_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkepisode.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let now = Utc::now();
    let earlier = (now - chrono::Duration::hours(1)).to_rfc3339();
    let now_str = now.to_rfc3339();

    let result = conn.execute(
        "INSERT INTO episodes (id, title, summary, started_at, ended_at, sensitivity, created_at, updated_at)
         VALUES (?1, NULL, 'summary', ?2, NULL, 'NOT_A_REAL_LEVEL', ?2, ?2)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), now_str],
    );
    assert!(
        result.is_err(),
        "[req_pk_episode] FAIL - invalid sensitivity should be rejected"
    );

    // ended_at (earlier) < started_at (now) - rejected.
    let result = conn.execute(
        "INSERT INTO episodes (id, title, summary, started_at, ended_at, sensitivity, created_at, updated_at)
         VALUES (?1, NULL, 'summary', ?2, ?3, 'INTERNAL', ?2, ?2)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), now_str, earlier],
    );
    assert!(
        result.is_err(),
        "[req_pk_episode] FAIL - ended_at before started_at should be rejected"
    );

    println!("[req_pk_episode] PASS - invalid sensitivity and ended_at-before-started_at are both rejected");
}

/// Join tables: nonexistent referenced IDs rejected, duplicate link
/// rejected by the composite primary key.
#[test]
fn req_pk_join_defense_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkjoin.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let source = mk_pk_source("USER_INPUT", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_source(&conn, &source).unwrap();
    let evidence = mk_pk_evidence(source.id, Some("evidence content"), Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_evidence(&conn, &evidence).unwrap();
    let subject = mk_pk_entity("person", "Join Defense Subject", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &subject).unwrap();
    let fact = mk_pk_fact(
        subject.id,
        "join_defense_predicate",
        FactValue::Scalar("1".to_string()),
        MemoryType::Fact,
        EpistemicState::Observed,
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_fact(&conn, &fact).unwrap();
    let now = Utc::now();

    // nonexistent fact_id
    let result = storage::personal_knowledge_repo::insert_fact_evidence_link(
        &conn,
        FactId::new(),
        evidence.id,
        now,
    );
    assert!(
        result.is_err(),
        "[req_pk_join] FAIL - a nonexistent fact_id in fact_evidence should be rejected"
    );

    // nonexistent evidence_id
    let result = storage::personal_knowledge_repo::insert_fact_evidence_link(
        &conn,
        fact.id,
        EvidenceId::new(),
        now,
    );
    assert!(
        result.is_err(),
        "[req_pk_join] FAIL - a nonexistent evidence_id in fact_evidence should be rejected"
    );

    // valid link, then a duplicate of the exact same link
    storage::personal_knowledge_repo::insert_fact_evidence_link(&conn, fact.id, evidence.id, now)
        .unwrap();
    assert!(
        storage::personal_knowledge_repo::fact_evidence_link_exists(&conn, fact.id, evidence.id)
            .unwrap(),
        "[req_pk_join] FAIL - the valid link was not actually inserted"
    );
    let result = storage::personal_knowledge_repo::insert_fact_evidence_link(
        &conn,
        fact.id,
        evidence.id,
        now,
    );
    assert!(
        result.is_err(),
        "[req_pk_join] FAIL - a duplicate (fact_id, evidence_id) link should be rejected by the composite primary key"
    );

    println!("[req_pk_join] PASS - fact_evidence rejects nonexistent fact_id/evidence_id and duplicate links");
}

// =========================================================================
// Post-run verification hardening (2026-09-18) - closes test/evidence gaps
// left open by R020's own Slice A implementation, found during a dedicated
// review before Slice B: `req_pk_join_defense_matrix` above only exercised
// `fact_evidence`, not the other three join tables; FK/RESTRICT integrity
// was only proven via failed child INSERTs, never a real parent DELETE
// attempt (the exact operation a future `ForgetMemory`'s explicit deletion
// order depends on); the encryption-at-rest regression checked only the
// SQLite header, not a distinctive Personal Knowledge plaintext marker;
// closed-enum mapping completeness and `fact_audit_log`'s own constraints
// (the first table Slice B's `AddMemory` will actually write to) had no
// direct proof. These are verification gaps found by review, not evidence
// of a production defect - every test below passes against the existing,
// unmodified V7 schema and Slice A code.
// =========================================================================

/// Completes join-table coverage: `relation_evidence`/`episode_evidence`/
/// `episode_entities` each get the same nonexistent-left-parent/
/// nonexistent-right-parent/valid-link/duplicate-link matrix
/// `req_pk_join_defense_matrix` above already proved for `fact_evidence`.
/// `Relation`/`Episode` rows are seeded via deliberate raw SQL only - no
/// `insert_relation`/`insert_episode` production function is added,
/// preserving R019 §13's schema-only safety rule.
#[test]
fn req_pk_join_relation_episode_defense_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkjoinre.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    let source = mk_pk_source("USER_INPUT", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_source(&conn, &source).unwrap();
    let evidence = mk_pk_evidence(source.id, Some("shared evidence"), Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_evidence(&conn, &evidence).unwrap();
    let from_entity = mk_pk_entity("person", "Relation Evidence From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &from_entity).unwrap();
    let to_entity = mk_pk_entity("person", "Relation Evidence To", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &to_entity).unwrap();

    // Raw-SQL-seeded Relation and Episode fixtures - deliberately not via
    // a production repo function (none exists, R019 §13).
    let relation_id = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO relations (id, from_entity_id, to_entity_id, relation_type, note, epistemic_state, confidence, sensitivity, valid_from, valid_until, created_at)
         VALUES (?1, ?2, ?3, 'CUSTOM_TYPE', NULL, 'OBSERVED', NULL, 'INTERNAL', ?4, NULL, ?4)",
        rusqlite::params![relation_id, from_entity.id.to_string(), to_entity.id.to_string(), now_str],
    )
    .unwrap();
    let episode_id = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO episodes (id, title, summary, started_at, ended_at, sensitivity, created_at, updated_at)
         VALUES (?1, NULL, 'episode summary', ?2, NULL, 'INTERNAL', ?2, ?2)",
        rusqlite::params![episode_id, now_str],
    )
    .unwrap();

    // --- relation_evidence ---
    let result = conn.execute(
        "INSERT INTO relation_evidence (relation_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            uuid::Uuid::now_v7().to_string(),
            evidence.id.to_string(),
            now_str
        ],
    );
    assert!(
        result.is_err(),
        "[req_pk_join_re] FAIL - nonexistent relation_id in relation_evidence should be rejected"
    );

    let result = conn.execute(
        "INSERT INTO relation_evidence (relation_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![relation_id, uuid::Uuid::now_v7().to_string(), now_str],
    );
    assert!(
        result.is_err(),
        "[req_pk_join_re] FAIL - nonexistent evidence_id in relation_evidence should be rejected"
    );

    conn.execute(
        "INSERT INTO relation_evidence (relation_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![relation_id, evidence.id.to_string(), now_str],
    )
    .unwrap();
    let result = conn.execute(
        "INSERT INTO relation_evidence (relation_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![relation_id, evidence.id.to_string(), now_str],
    );
    assert!(
        result.is_err(),
        "[req_pk_join_re] FAIL - a duplicate (relation_id, evidence_id) link should be rejected"
    );

    // --- episode_evidence ---
    let result = conn.execute(
        "INSERT INTO episode_evidence (episode_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            uuid::Uuid::now_v7().to_string(),
            evidence.id.to_string(),
            now_str
        ],
    );
    assert!(
        result.is_err(),
        "[req_pk_join_re] FAIL - nonexistent episode_id in episode_evidence should be rejected"
    );

    let result = conn.execute(
        "INSERT INTO episode_evidence (episode_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![episode_id, uuid::Uuid::now_v7().to_string(), now_str],
    );
    assert!(
        result.is_err(),
        "[req_pk_join_re] FAIL - nonexistent evidence_id in episode_evidence should be rejected"
    );

    conn.execute(
        "INSERT INTO episode_evidence (episode_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![episode_id, evidence.id.to_string(), now_str],
    )
    .unwrap();
    let result = conn.execute(
        "INSERT INTO episode_evidence (episode_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![episode_id, evidence.id.to_string(), now_str],
    );
    assert!(
        result.is_err(),
        "[req_pk_join_re] FAIL - a duplicate (episode_id, evidence_id) link should be rejected"
    );

    // --- episode_entities ---
    let result = conn.execute(
        "INSERT INTO episode_entities (episode_id, entity_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            uuid::Uuid::now_v7().to_string(),
            from_entity.id.to_string(),
            now_str
        ],
    );
    assert!(
        result.is_err(),
        "[req_pk_join_re] FAIL - nonexistent episode_id in episode_entities should be rejected"
    );

    let result = conn.execute(
        "INSERT INTO episode_entities (episode_id, entity_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![episode_id, uuid::Uuid::now_v7().to_string(), now_str],
    );
    assert!(
        result.is_err(),
        "[req_pk_join_re] FAIL - nonexistent entity_id in episode_entities should be rejected"
    );

    conn.execute(
        "INSERT INTO episode_entities (episode_id, entity_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![episode_id, from_entity.id.to_string(), now_str],
    )
    .unwrap();
    let result = conn.execute(
        "INSERT INTO episode_entities (episode_id, entity_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![episode_id, from_entity.id.to_string(), now_str],
    );
    assert!(
        result.is_err(),
        "[req_pk_join_re] FAIL - a duplicate (episode_id, entity_id) link should be rejected"
    );

    println!("[req_pk_join_re] PASS - relation_evidence/episode_evidence/episode_entities each reject nonexistent left/right parents and duplicate links, matching fact_evidence's already-proven behavior");
}

/// Proves the actual `ON DELETE RESTRICT` behavior (not merely that an
/// invalid child INSERT fails) for the six non-join-table parent-child
/// relationships: `Source -> Evidence`, `Entity -> Fact.subject`,
/// `Entity -> Fact.object`, a Fact's own `superseded_by_fact_id` pointing
/// at another Fact, `Entity -> Relation.from`, `Entity -> Relation.to`.
/// For each: valid parent+child fixture state is created, the referenced
/// parent's real deletion is attempted, the real DB rejects it, and both
/// rows remain intact afterward - directly exercising the same operation
/// a future `ForgetMemory`'s explicit deletion order depends on.
#[test]
fn req_pk_restrict_core_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrestrictcore.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    // Source -> Evidence
    let source = mk_pk_source("USER_INPUT", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_source(&conn, &source).unwrap();
    let evidence = mk_pk_evidence(source.id, Some("restrict content"), Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_evidence(&conn, &evidence).unwrap();
    let result = conn.execute(
        "DELETE FROM sources WHERE id = ?1",
        rusqlite::params![source.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_core] FAIL - deleting a Source still referenced by Evidence should be rejected");
    let source_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sources WHERE id = ?1",
            rusqlite::params![source.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        source_count, 1,
        "[req_pk_restrict_core] FAIL - Source row must remain intact after the rejected delete"
    );

    // Entity -> Fact.subject
    let subject = mk_pk_entity("person", "Restrict Subject", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &subject).unwrap();
    let subject_fact = mk_pk_fact(
        subject.id,
        "restrict_subject_predicate",
        FactValue::Scalar("1".to_string()),
        MemoryType::Fact,
        EpistemicState::Observed,
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_fact(&conn, &subject_fact).unwrap();
    let result = conn.execute(
        "DELETE FROM entities WHERE id = ?1",
        rusqlite::params![subject.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_core] FAIL - deleting an Entity still referenced as Fact.subject_entity_id should be rejected");
    assert!(
        get_entity(&conn, subject.id).unwrap().is_some(),
        "[req_pk_restrict_core] FAIL - subject Entity row must remain intact"
    );

    // Entity -> Fact.object
    let object = mk_pk_entity("vehicle", "Restrict Object", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &object).unwrap();
    let object_fact = mk_pk_fact(
        subject.id,
        "restrict_object_predicate",
        FactValue::EntityRef(object.id),
        MemoryType::Fact,
        EpistemicState::Observed,
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_fact(&conn, &object_fact).unwrap();
    let result = conn.execute(
        "DELETE FROM entities WHERE id = ?1",
        rusqlite::params![object.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_core] FAIL - deleting an Entity still referenced as Fact.object_entity_id should be rejected");
    assert!(
        get_entity(&conn, object.id).unwrap().is_some(),
        "[req_pk_restrict_core] FAIL - object Entity row must remain intact"
    );

    // Fact successor <- predecessor.superseded_by_fact_id
    let successor = mk_pk_fact(
        subject.id,
        "restrict_supersession_predicate",
        FactValue::Scalar("2".to_string()),
        MemoryType::Fact,
        EpistemicState::UserConfirmed,
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_fact(&conn, &successor).unwrap();
    let mut predecessor = mk_pk_fact(
        subject.id,
        "restrict_supersession_predicate",
        FactValue::Scalar("1".to_string()),
        MemoryType::Fact,
        EpistemicState::Stale,
        Sensitivity::Internal,
    );
    predecessor.superseded_by_fact_id = Some(successor.id);
    storage::personal_knowledge_repo::insert_fact(&conn, &predecessor).unwrap();
    let result = conn.execute(
        "DELETE FROM facts WHERE id = ?1",
        rusqlite::params![successor.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_core] FAIL - deleting a Fact still referenced by another Fact's superseded_by_fact_id should be rejected");
    assert!(
        get_fact(&conn, successor.id).unwrap().is_some(),
        "[req_pk_restrict_core] FAIL - successor Fact row must remain intact"
    );
    assert!(
        get_fact(&conn, predecessor.id).unwrap().is_some(),
        "[req_pk_restrict_core] FAIL - predecessor Fact row must remain intact"
    );

    // Entity -> Relation.from / Entity -> Relation.to
    let rel_from = mk_pk_entity("person", "Restrict Relation From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &rel_from).unwrap();
    let rel_to = mk_pk_entity("person", "Restrict Relation To", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &rel_to).unwrap();
    let now_str = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO relations (id, from_entity_id, to_entity_id, relation_type, note, epistemic_state, confidence, sensitivity, valid_from, valid_until, created_at)
         VALUES (?1, ?2, ?3, 'RESTRICT_TEST_TYPE', NULL, 'OBSERVED', NULL, 'INTERNAL', ?4, NULL, ?4)",
        rusqlite::params![uuid::Uuid::now_v7().to_string(), rel_from.id.to_string(), rel_to.id.to_string(), now_str],
    )
    .unwrap();
    let result = conn.execute(
        "DELETE FROM entities WHERE id = ?1",
        rusqlite::params![rel_from.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_core] FAIL - deleting an Entity still referenced as Relation.from_entity_id should be rejected");
    let result = conn.execute(
        "DELETE FROM entities WHERE id = ?1",
        rusqlite::params![rel_to.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_core] FAIL - deleting an Entity still referenced as Relation.to_entity_id should be rejected");
    assert!(get_entity(&conn, rel_from.id).unwrap().is_some());
    assert!(get_entity(&conn, rel_to.id).unwrap().is_some());

    println!("[req_pk_restrict_core] PASS - all 6 core ON DELETE RESTRICT relationships genuinely reject a real parent DELETE, with both parent and child rows left intact");
}

/// Same reasoning as `req_pk_restrict_core_matrix`, for the eight
/// join-table parent relationships: `Fact`/`Evidence -> fact_evidence`,
/// `Relation`/`Evidence -> relation_evidence`, `Episode`/
/// `Evidence -> episode_evidence`, `Episode`/`Entity -> episode_entities`.
#[test]
fn req_pk_restrict_join_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrestrictjoin.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    let source = mk_pk_source("USER_INPUT", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_source(&conn, &source).unwrap();
    let subject = mk_pk_entity("person", "Restrict Join Subject", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &subject).unwrap();
    let fact = mk_pk_fact(
        subject.id,
        "restrict_join_predicate",
        FactValue::Scalar("1".to_string()),
        MemoryType::Fact,
        EpistemicState::Observed,
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_fact(&conn, &fact).unwrap();

    // Fact -> fact_evidence, Evidence -> fact_evidence
    let fe_evidence = mk_pk_evidence(
        source.id,
        Some("fact_evidence restrict content"),
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_evidence(&conn, &fe_evidence).unwrap();
    storage::personal_knowledge_repo::insert_fact_evidence_link(
        &conn,
        fact.id,
        fe_evidence.id,
        now,
    )
    .unwrap();
    let result = conn.execute(
        "DELETE FROM facts WHERE id = ?1",
        rusqlite::params![fact.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_join] FAIL - deleting a Fact still linked via fact_evidence should be rejected");
    let result = conn.execute(
        "DELETE FROM evidence WHERE id = ?1",
        rusqlite::params![fe_evidence.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_join] FAIL - deleting Evidence still linked via fact_evidence should be rejected");

    // Relation -> relation_evidence, Evidence -> relation_evidence
    let rel_from = mk_pk_entity("person", "Restrict Join Rel From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &rel_from).unwrap();
    let rel_to = mk_pk_entity("person", "Restrict Join Rel To", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &rel_to).unwrap();
    let relation_id = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO relations (id, from_entity_id, to_entity_id, relation_type, note, epistemic_state, confidence, sensitivity, valid_from, valid_until, created_at)
         VALUES (?1, ?2, ?3, 'RESTRICT_JOIN_TYPE', NULL, 'OBSERVED', NULL, 'INTERNAL', ?4, NULL, ?4)",
        rusqlite::params![relation_id, rel_from.id.to_string(), rel_to.id.to_string(), now_str],
    )
    .unwrap();
    let re_evidence = mk_pk_evidence(
        source.id,
        Some("relation_evidence restrict content"),
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_evidence(&conn, &re_evidence).unwrap();
    conn.execute(
        "INSERT INTO relation_evidence (relation_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![relation_id, re_evidence.id.to_string(), now_str],
    )
    .unwrap();
    let result = conn.execute(
        "DELETE FROM relations WHERE id = ?1",
        rusqlite::params![relation_id],
    );
    assert!(result.is_err(), "[req_pk_restrict_join] FAIL - deleting a Relation still linked via relation_evidence should be rejected");
    let result = conn.execute(
        "DELETE FROM evidence WHERE id = ?1",
        rusqlite::params![re_evidence.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_join] FAIL - deleting Evidence still linked via relation_evidence should be rejected");

    // Episode -> episode_evidence, Evidence -> episode_evidence
    let episode_id = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO episodes (id, title, summary, started_at, ended_at, sensitivity, created_at, updated_at)
         VALUES (?1, NULL, 'restrict episode summary', ?2, NULL, 'INTERNAL', ?2, ?2)",
        rusqlite::params![episode_id, now_str],
    )
    .unwrap();
    let ee_evidence = mk_pk_evidence(
        source.id,
        Some("episode_evidence restrict content"),
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_evidence(&conn, &ee_evidence).unwrap();
    conn.execute(
        "INSERT INTO episode_evidence (episode_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![episode_id, ee_evidence.id.to_string(), now_str],
    )
    .unwrap();
    let result = conn.execute(
        "DELETE FROM episodes WHERE id = ?1",
        rusqlite::params![episode_id.clone()],
    );
    assert!(result.is_err(), "[req_pk_restrict_join] FAIL - deleting an Episode still linked via episode_evidence should be rejected");
    let result = conn.execute(
        "DELETE FROM evidence WHERE id = ?1",
        rusqlite::params![ee_evidence.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_join] FAIL - deleting Evidence still linked via episode_evidence should be rejected");

    // Episode -> episode_entities, Entity -> episode_entities
    let ee_entity = mk_pk_entity(
        "person",
        "Restrict Join Episode Entity",
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_entity(&conn, &ee_entity).unwrap();
    conn.execute(
        "INSERT INTO episode_entities (episode_id, entity_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![episode_id, ee_entity.id.to_string(), now_str],
    )
    .unwrap();
    let result = conn.execute(
        "DELETE FROM episodes WHERE id = ?1",
        rusqlite::params![episode_id],
    );
    assert!(result.is_err(), "[req_pk_restrict_join] FAIL - deleting an Episode still linked via episode_entities should be rejected");
    let result = conn.execute(
        "DELETE FROM entities WHERE id = ?1",
        rusqlite::params![ee_entity.id.to_string()],
    );
    assert!(result.is_err(), "[req_pk_restrict_join] FAIL - deleting an Entity still linked via episode_entities should be rejected");

    println!("[req_pk_restrict_join] PASS - all 8 join-table ON DELETE RESTRICT relationships genuinely reject a real parent DELETE");
}

/// `fact_audit_log`: the first table Slice B's `AddMemory` will actually
/// write to. Proves its frozen schema before any production code touches
/// it: a valid marker can be inserted; invalid `action_type`/`actor`/
/// `provenance` are each rejected; `previous_snapshot` may be `NULL` or
/// contain real serialized text; and - the property a future
/// `ForgetMemory` structurally depends on (R017 §20) - `fact_id` is
/// genuinely NOT a foreign key, so a syntactically valid but nonexistent
/// `FactId` is accepted, not rejected.
#[test]
fn req_pk_fact_audit_log_defense_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkauditlog.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let now = Utc::now().to_rfc3339();

    let insert = |id: String,
                  fact_id: String,
                  action_type: &str,
                  actor: &str,
                  provenance: &str,
                  previous_snapshot: Option<&str>| {
        conn.execute(
            "INSERT INTO fact_audit_log (id, fact_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'EXPLICIT_ADD', ?6, ?7)",
            rusqlite::params![id, fact_id, action_type, actor, provenance, previous_snapshot, now],
        )
    };

    // fact_id is NOT a foreign key - a syntactically valid but
    // nonexistent FactId must be ACCEPTED, not rejected (the exact
    // property a future ForgetMemory's residual marker depends on).
    let nonexistent_fact_id = FactId::new().to_string();
    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        nonexistent_fact_id.clone(),
        "AddMemory",
        "UI",
        "Explicit",
        None,
    );
    assert!(
        result.is_ok(),
        "[req_pk_audit] FAIL - a valid AddMemory marker with previous_snapshot = NULL and a nonexistent (but syntactically valid) fact_id should be ACCEPTED, since fact_id is deliberately not an FK: {result:?}"
    );

    // previous_snapshot MAY contain real serialized text (e.g. for a
    // future CorrectMemory/ConfirmMemory marker).
    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        nonexistent_fact_id.clone(),
        "CorrectMemory",
        "UI",
        "Explicit",
        Some(r#"{"predicate":"example","value_json":"1"}"#),
    );
    assert!(
        result.is_ok(),
        "[req_pk_audit] FAIL - a valid marker with non-NULL previous_snapshot should be accepted: {result:?}"
    );

    // invalid action_type
    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        nonexistent_fact_id.clone(),
        "NotARealMemoryAction",
        "UI",
        "Explicit",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_audit] FAIL - invalid action_type should be rejected"
    );

    // invalid actor
    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        nonexistent_fact_id.clone(),
        "AddMemory",
        "NotARealActor",
        "Explicit",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_audit] FAIL - invalid actor should be rejected"
    );

    // invalid provenance
    let result = insert(
        uuid::Uuid::now_v7().to_string(),
        nonexistent_fact_id,
        "AddMemory",
        "UI",
        "NotARealProvenance",
        None,
    );
    assert!(
        result.is_err(),
        "[req_pk_audit] FAIL - invalid provenance should be rejected"
    );

    println!("[req_pk_audit] PASS - fact_audit_log accepts valid markers (NULL or real previous_snapshot, and a nonexistent fact_id since it is deliberately not an FK) and rejects invalid action_type/actor/provenance");
}

// --- Enum round-trip completeness (post-run verification hardening) -----

/// All 4 `Sensitivity` values: `as_str()`/`parse()` round-trip exactly,
/// `parse` of an invalid string returns `None`, and the real DB `CHECK`
/// accepts every one of the 4 values (not merely one or two plus
/// rejection of an invalid value, which `req_pk_entity_defense_matrix`
/// already proved).
#[test]
fn req_pkenum1_sensitivity_round_trip_and_db_accepts_all_values() {
    let values = [
        (Sensitivity::Public, "PUBLIC"),
        (Sensitivity::Internal, "INTERNAL"),
        (Sensitivity::Sensitive, "SENSITIVE"),
        (Sensitivity::NeverLeavesDevice, "NEVER_LEAVES_DEVICE"),
    ];
    assert_eq!(
        values.len(),
        4,
        "[req_pkenum1] FAIL - expected exactly 4 Sensitivity values"
    );
    for (variant, expected_str) in values {
        assert_eq!(variant.as_str(), expected_str);
        assert_eq!(Sensitivity::parse(expected_str), Some(variant));
    }
    assert_eq!(Sensitivity::parse("NOT_A_REAL_VALUE"), None);

    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkenum1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    for (i, (_, s)) in values.iter().enumerate() {
        let e = mk_pk_entity(
            "concept",
            &format!("Sensitivity DB test {i}"),
            Sensitivity::parse(s).unwrap(),
        );
        storage::personal_knowledge_repo::insert_entity(&conn, &e).unwrap_or_else(|err| {
            panic!("[req_pkenum1] FAIL - DB rejected valid sensitivity {s}: {err:?}")
        });
    }
    println!(
        "[req_pkenum1] PASS - all 4 Sensitivity values round-trip exactly and are DB-accepted"
    );
}

/// All 11 `MemoryType` values, same proof shape as `req_pkenum1`.
#[test]
fn req_pkenum2_memory_type_round_trip_and_db_accepts_all_values() {
    let values = [
        (MemoryType::Fact, "FACT"),
        (MemoryType::Preference, "PREFERENCE"),
        (MemoryType::Experience, "EXPERIENCE"),
        (MemoryType::Decision, "DECISION"),
        (MemoryType::Observation, "OBSERVATION"),
        (MemoryType::Hypothesis, "HYPOTHESIS"),
        (MemoryType::EffectiveSolution, "EFFECTIVE_SOLUTION"),
        (MemoryType::IneffectiveSolution, "INEFFECTIVE_SOLUTION"),
        (MemoryType::Skill, "SKILL"),
        (MemoryType::Habit, "HABIT"),
        (MemoryType::CurrentState, "CURRENT_STATE"),
    ];
    assert_eq!(
        values.len(),
        11,
        "[req_pkenum2] FAIL - expected exactly 11 MemoryType values"
    );
    for (variant, expected_str) in values {
        assert_eq!(variant.as_str(), expected_str);
        assert_eq!(MemoryType::parse(expected_str), Some(variant));
    }
    assert_eq!(MemoryType::parse("NOT_A_REAL_VALUE"), None);

    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkenum2.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let subject = mk_pk_entity(
        "person",
        "MemoryType DB Test Subject",
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_entity(&conn, &subject).unwrap();
    for (variant, s) in values {
        let f = mk_pk_fact(
            subject.id,
            "memory_type_db_test_predicate",
            FactValue::Scalar("1".to_string()),
            variant,
            EpistemicState::Observed,
            Sensitivity::Internal,
        );
        storage::personal_knowledge_repo::insert_fact(&conn, &f).unwrap_or_else(|err| {
            panic!("[req_pkenum2] FAIL - DB rejected valid memory_type {s}: {err:?}")
        });
    }
    println!(
        "[req_pkenum2] PASS - all 11 MemoryType values round-trip exactly and are DB-accepted"
    );
}

/// All 5 `EpistemicState` values, same proof shape as `req_pkenum1`.
#[test]
fn req_pkenum3_epistemic_state_round_trip_and_db_accepts_all_values() {
    let values = [
        (EpistemicState::UserConfirmed, "USER_CONFIRMED"),
        (EpistemicState::Observed, "OBSERVED"),
        (EpistemicState::Inferred, "INFERRED"),
        (EpistemicState::Disputed, "DISPUTED"),
        (EpistemicState::Stale, "STALE"),
    ];
    assert_eq!(
        values.len(),
        5,
        "[req_pkenum3] FAIL - expected exactly 5 EpistemicState values"
    );
    for (variant, expected_str) in values {
        assert_eq!(variant.as_str(), expected_str);
        assert_eq!(EpistemicState::parse(expected_str), Some(variant));
    }
    assert_eq!(EpistemicState::parse("NOT_A_REAL_VALUE"), None);

    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkenum3.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let subject = mk_pk_entity(
        "person",
        "EpistemicState DB Test Subject",
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_entity(&conn, &subject).unwrap();
    for (variant, s) in values {
        let f = mk_pk_fact(
            subject.id,
            "epistemic_state_db_test_predicate",
            FactValue::Scalar("1".to_string()),
            MemoryType::Fact,
            variant,
            Sensitivity::Internal,
        );
        storage::personal_knowledge_repo::insert_fact(&conn, &f).unwrap_or_else(|err| {
            panic!("[req_pkenum3] FAIL - DB rejected valid epistemic_state {s}: {err:?}")
        });
    }
    println!(
        "[req_pkenum3] PASS - all 5 EpistemicState values round-trip exactly and are DB-accepted"
    );
}

// --- Encryption-at-rest, Personal Knowledge content marker ---------------

/// Strengthens R020's encryption-at-rest regression (which only checked
/// for the absence of the plaintext `SQLite` file header) by inserting a
/// distinctive, real Personal Knowledge value (an `Entity.canonical_name`)
/// and proving that exact marker text is absent from the raw encrypted
/// file bytes too - proving actual Personal Knowledge CONTENT is
/// encrypted, not only the database header. Correct-key reopen still
/// reads the marker back normally; a wrong key still fails. No real user
/// data, no production credential, the raw test key is never printed.
#[test]
fn req_pkenc1_personal_knowledge_content_encrypted_at_rest() {
    const MARKER: &str = "R020_PK_PLAINTEXT_MARKER_SHOULD_NOT_APPEAR";
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkenc1.db");

    let entity_id;
    {
        let conn = bootstrap_db_at(&path, &key).unwrap();
        let entity = mk_pk_entity("concept", MARKER, Sensitivity::Sensitive);
        entity_id = entity.id;
        storage::personal_knowledge_repo::insert_entity(&conn, &entity).unwrap();
    }

    let mut raw = Vec::new();
    fs::File::open(&path)
        .unwrap()
        .read_to_end(&mut raw)
        .unwrap();
    assert!(
        !raw.windows(6).any(|w| w == b"SQLite"),
        "[req_pkenc1] FAIL - file starts with the plaintext SQLite header"
    );
    assert!(
        !raw.windows(MARKER.len()).any(|w| w == MARKER.as_bytes()),
        "[req_pkenc1] FAIL - the distinctive Personal Knowledge marker was found in plaintext in the raw encrypted file"
    );

    // Correct key reopen - the marker reads back normally.
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let fetched = get_entity(&conn, entity_id)
        .unwrap()
        .expect("[req_pkenc1] FAIL - marker entity not found after correct-key reopen");
    assert_eq!(fetched.canonical_name, MARKER);

    // Wrong key still fails (unchanged behavior).
    let wrong_key = generate_test_key_hex();
    let wrong_conn = storage::open_encrypted(&path, &wrong_key).unwrap();
    assert!(
        storage::verify_key(&wrong_conn).is_err(),
        "[req_pkenc1] FAIL - a wrong key should still fail verify_key"
    );

    println!("[req_pkenc1] PASS - a distinctive Personal Knowledge marker (Entity.canonical_name) is absent from the raw encrypted file bytes; correct key reads it back normally, wrong key still fails");
}

// --- Length-bound tests (R020 §32) ---------------------------------------

/// Table-driven proof of all 15 R018-frozen max-length constants: N
/// characters accepted, N+1 rejected, via the shared
/// `domain::validate_personal_knowledge_text_len` helper (R020 §28).
#[test]
fn req_pklen1_all_frozen_length_constants_enforced() {
    let constants: [(&str, usize); 15] = [
        ("ENTITY_TYPE_MAX_LEN", crate::domain::ENTITY_TYPE_MAX_LEN),
        (
            "CANONICAL_NAME_MAX_LEN",
            crate::domain::CANONICAL_NAME_MAX_LEN,
        ),
        (
            "DOMAIN_REF_TYPE_MAX_LEN",
            crate::domain::DOMAIN_REF_TYPE_MAX_LEN,
        ),
        (
            "DOMAIN_REF_ID_MAX_LEN",
            crate::domain::DOMAIN_REF_ID_MAX_LEN,
        ),
        ("PREDICATE_MAX_LEN", crate::domain::PREDICATE_MAX_LEN),
        ("VALUE_JSON_MAX_LEN", crate::domain::VALUE_JSON_MAX_LEN),
        (
            "RELATION_TYPE_MAX_LEN",
            crate::domain::RELATION_TYPE_MAX_LEN,
        ),
        (
            "RELATION_NOTE_MAX_LEN",
            crate::domain::RELATION_NOTE_MAX_LEN,
        ),
        (
            "EPISODE_TITLE_MAX_LEN",
            crate::domain::EPISODE_TITLE_MAX_LEN,
        ),
        (
            "EPISODE_SUMMARY_MAX_LEN",
            crate::domain::EPISODE_SUMMARY_MAX_LEN,
        ),
        ("SOURCE_TYPE_MAX_LEN", crate::domain::SOURCE_TYPE_MAX_LEN),
        ("SOURCE_LABEL_MAX_LEN", crate::domain::SOURCE_LABEL_MAX_LEN),
        (
            "SOURCE_REFERENCE_MAX_LEN",
            crate::domain::SOURCE_REFERENCE_MAX_LEN,
        ),
        (
            "EVIDENCE_CONTENT_MAX_LEN",
            crate::domain::EVIDENCE_CONTENT_MAX_LEN,
        ),
        (
            "EVIDENCE_LOCATOR_MAX_LEN",
            crate::domain::EVIDENCE_LOCATOR_MAX_LEN,
        ),
    ];
    for (name, max_len) in constants {
        let at_max = "a".repeat(max_len);
        let over_max = "a".repeat(max_len + 1);
        assert!(
            crate::domain::validate_personal_knowledge_text_len(&at_max, max_len).is_ok(),
            "[req_pklen1] FAIL - {name} = {max_len}: exactly {max_len} chars should be accepted"
        );
        assert_eq!(
            crate::domain::validate_personal_knowledge_text_len(&over_max, max_len),
            Err(max_len + 1),
            "[req_pklen1] FAIL - {name} = {max_len}: {max_len} + 1 chars should be rejected"
        );
    }
    println!("[req_pklen1] PASS - all 15 frozen length constants accept exactly-at-bound and reject one-over-bound");
}

// --- Read round-trip tests (R020 §33) ------------------------------------

/// `get_entity` round-trips every field: no domain_ref, WITH
/// domain_ref, sensitivity, timestamps, and SELF/non-SELF.
#[test]
fn req_pkrt1_get_entity_round_trip_all_fields() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrt1.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let e1 = mk_pk_entity("vehicle", "Test Car", Sensitivity::Public);
    storage::personal_knowledge_repo::insert_entity(&conn, &e1).unwrap();
    let fetched1 = get_entity(&conn, e1.id)
        .unwrap()
        .expect("[req_pkrt1] FAIL - entity without domain_ref not found");
    assert_eq!(
        fetched1, e1,
        "[req_pkrt1] FAIL - round-trip mismatch for entity without domain_ref"
    );

    let mut e2 = mk_pk_entity("project", "Linked Project Entity", Sensitivity::Sensitive);
    e2.domain_ref = Some(DomainRef {
        domain_ref_type: "Project".to_string(),
        domain_ref_id: crate::domain::ProjectId::new().to_string(),
    });
    storage::personal_knowledge_repo::insert_entity(&conn, &e2).unwrap();
    let fetched2 = get_entity(&conn, e2.id)
        .unwrap()
        .expect("[req_pkrt1] FAIL - entity WITH domain_ref not found");
    assert_eq!(
        fetched2, e2,
        "[req_pkrt1] FAIL - round-trip mismatch for entity WITH domain_ref"
    );

    let self_entity = ensure_self_entity(&conn).unwrap();
    let fetched_self = get_entity(&conn, self_entity.id)
        .unwrap()
        .expect("[req_pkrt1] FAIL - SELF entity not found via get_entity");
    assert_eq!(
        fetched_self, self_entity,
        "[req_pkrt1] FAIL - SELF entity round-trip mismatch"
    );
    assert!(fetched_self.is_self);
    assert!(!fetched1.is_self);

    println!("[req_pkrt1] PASS - get_entity round-trips all fields: no domain_ref, WITH domain_ref, sensitivity, timestamps, SELF and non-SELF");
}

/// `get_fact` round-trips a `Scalar` value Fact, an `EntityRef` value
/// Fact, all enum parsing, nullable confidence, temporal fields,
/// sensitivity, and a valid `superseded_by_fact_id` link.
#[test]
fn req_pkrt2_get_fact_round_trip_scalar_and_entity_ref() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrt2.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let subject = mk_pk_entity("person", "Fact Subject", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &subject).unwrap();
    let object = mk_pk_entity("vehicle", "Fact Object Vehicle", Sensitivity::Public);
    storage::personal_knowledge_repo::insert_entity(&conn, &object).unwrap();

    let mut f1 = mk_pk_fact(
        subject.id,
        "prefers_work_time",
        FactValue::Scalar("\"morning\"".to_string()),
        MemoryType::Preference,
        EpistemicState::UserConfirmed,
        Sensitivity::Internal,
    );
    f1.confidence = Some(0.75);
    storage::personal_knowledge_repo::insert_fact(&conn, &f1).unwrap();
    let fetched1 = get_fact(&conn, f1.id)
        .unwrap()
        .expect("[req_pkrt2] FAIL - scalar-value fact not found");
    assert_eq!(
        fetched1, f1,
        "[req_pkrt2] FAIL - scalar-value fact round-trip mismatch"
    );

    let f2 = mk_pk_fact(
        subject.id,
        "owns_vehicle",
        FactValue::EntityRef(object.id),
        MemoryType::Fact,
        EpistemicState::Inferred,
        Sensitivity::Public,
    );
    storage::personal_knowledge_repo::insert_fact(&conn, &f2).unwrap();
    let fetched2 = get_fact(&conn, f2.id)
        .unwrap()
        .expect("[req_pkrt2] FAIL - entity-ref fact not found");
    assert_eq!(
        fetched2, f2,
        "[req_pkrt2] FAIL - entity-ref fact round-trip mismatch"
    );
    assert!(
        fetched2.confidence.is_none(),
        "[req_pkrt2] FAIL - expected NULL confidence to round-trip as None"
    );

    // A valid superseded_by_fact_id link (test-fixture raw UPDATE - no
    // CorrectMemory exists yet to do this for real, Slice C).
    let successor = mk_pk_fact(
        subject.id,
        "prefers_work_time",
        FactValue::Scalar("\"evening\"".to_string()),
        MemoryType::Preference,
        EpistemicState::UserConfirmed,
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_fact(&conn, &successor).unwrap();
    let valid_until = Utc::now();
    conn.execute(
        "UPDATE facts SET valid_until = ?2, superseded_by_fact_id = ?3, epistemic_state = 'STALE' WHERE id = ?1",
        rusqlite::params![
            f1.id.to_string(),
            valid_until.to_rfc3339(),
            successor.id.to_string()
        ],
    )
    .unwrap();
    let fetched_closed = get_fact(&conn, f1.id)
        .unwrap()
        .expect("[req_pkrt2] FAIL - closed predecessor fact not found");
    assert_eq!(fetched_closed.valid_until, Some(valid_until));
    assert_eq!(fetched_closed.superseded_by_fact_id, Some(successor.id));
    assert_eq!(fetched_closed.epistemic_state, EpistemicState::Stale);

    println!("[req_pkrt2] PASS - get_fact round-trips Scalar and EntityRef values, enum parsing, nullable confidence, temporal fields, sensitivity, and a valid superseded_by_fact_id link");
}

// =========================================================================
// M2 Personal Knowledge Slice B - AddMemory (2026-09-18)
//
// Evidence for
// `docs/reports/R021_M2_PERSONAL_KNOWLEDGE_ADDMEMORY_SLICE_B_2026-09-18.md`.
// The first production Personal Knowledge mutation. Database-dependent
// behavior only - pure `validate_add_memory` structural tests live in
// `actions::tests` instead (same split as every other action).
// =========================================================================

fn mk_am_source_new() -> SourceRef {
    SourceRef::New(NewSourceData {
        source_type: "conversation".to_string(),
        label: None,
        reference: None,
        sensitivity: Sensitivity::Internal,
        captured_at: Utc::now(),
    })
}

fn mk_am_evidence() -> NewEvidenceData {
    NewEvidenceData {
        content: Some("test evidence content".to_string()),
        locator: None,
        sensitivity: Sensitivity::Internal,
    }
}

/// A minimal, always-valid baseline payload - Self subject, scalar JSON
/// value, brand-new Source, Evidence with content only. Every test below
/// overrides only the fields it cares about via struct-update syntax.
fn mk_am_payload() -> AddMemoryPayload {
    AddMemoryPayload {
        subject: EntityInputRef::SelfEntity,
        predicate: "likes".to_string(),
        value: FactValueInput::ScalarJson("\"coffee\"".to_string()),
        memory_type: MemoryType::Preference,
        epistemic_state: EpistemicState::UserConfirmed,
        confidence: None,
        sensitivity: Sensitivity::Internal,
        source: mk_am_source_new(),
        evidence: mk_am_evidence(),
    }
}

fn self_entity_id(conn: &Connection) -> EntityId {
    storage::personal_knowledge_repo::get_self_entity(conn)
        .unwrap()
        .unwrap()
        .id
}

fn entity_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM entities", [], |r| r.get(0))
        .unwrap()
}

fn am_source_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM sources", [], |r| r.get(0))
        .unwrap()
}

fn am_fact_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM facts", [], |r| r.get(0))
        .unwrap()
}

fn am_evidence_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM evidence", [], |r| r.get(0))
        .unwrap()
}

fn am_fact_evidence_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM fact_evidence", [], |r| r.get(0))
        .unwrap()
}

fn am_fact_audit_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM fact_audit_log", [], |r| r.get(0))
        .unwrap()
}

/// `(entities, sources, evidence, facts, fact_evidence, fact_audit_log)`
/// - every table `AddMemory` can possibly mutate. Post-run correctness
/// hardening (2026-09-18): a rejected `AddMemory` call (same-payload or
/// persisted duplicate-domain-ref) must leave every one of these six
/// counts unchanged, not merely the two or three a narrower proof might
/// happen to check.
fn am_full_table_snapshot(conn: &Connection) -> (i64, i64, i64, i64, i64, i64) {
    (
        entity_row_count(conn),
        am_source_row_count(conn),
        am_evidence_row_count(conn),
        am_fact_row_count(conn),
        am_fact_evidence_row_count(conn),
        am_fact_audit_row_count(conn),
    )
}

/// `(relations, episodes, relation_evidence, episode_evidence, episode_entities)`
/// - the five tables `AddMemory` must never write to (schema-only, R019
/// §13). Read with raw SQL, the same "tests reach these tables directly,
/// production never does" precedent `req_pkrt2` and its neighbors
/// already established.
fn relation_episode_side_effect_counts(conn: &Connection) -> (i64, i64, i64, i64, i64) {
    let q = |table: &str| -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };
    (
        q("relations"),
        q("episodes"),
        q("relation_evidence"),
        q("episode_evidence"),
        q("episode_entities"),
    )
}

/// Full-field comparison for a `Source` - there is no production
/// `get_source` (nothing besides existence needs one, `source_exists`
/// covers `AddMemory`'s own needs), so immutability-on-reuse proofs read
/// the row directly, matching the raw-SQL-for-tests-only precedent above.
fn read_source_fields(
    conn: &Connection,
    id: SourceId,
) -> (
    String,
    Option<String>,
    Option<String>,
    String,
    String,
    String,
) {
    conn.query_row(
        "SELECT source_type, label, reference, sensitivity, captured_at, created_at FROM sources WHERE id = ?1",
        rusqlite::params![id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
    )
    .unwrap()
}

/// Same forced-failure mechanism as `install_audit_failure_trigger`/
/// `install_project_audit_failure_trigger`/etc. - `RAISE(FAIL, ...)`
/// aborts only the failing statement, proving `add_memory`'s own
/// `Transaction`-drop rollback, not SQLite doing it unprompted.
fn install_fact_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER force_fact_audit_failure
         BEFORE INSERT ON fact_audit_log
         BEGIN
             SELECT RAISE(FAIL, 'forced fact audit failure');
         END;",
    )
    .expect("failed to install the forced fact-audit-failure trigger");
}

fn remove_fact_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch("DROP TRIGGER IF EXISTS force_fact_audit_failure;")
        .expect("failed to remove the forced fact-audit-failure trigger");
}

/// An earlier-stage forced failure (before the final audit-log write) -
/// proves the rollback also covers a failure that happens partway
/// through the insert sequence, not only the very last step.
fn install_evidence_insert_failure_trigger(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER force_evidence_failure
         BEFORE INSERT ON evidence
         BEGIN
             SELECT RAISE(FAIL, 'forced evidence insert failure');
         END;",
    )
    .expect("failed to install the forced evidence-insert-failure trigger");
}

fn remove_evidence_insert_failure_trigger(conn: &Connection) {
    conn.execute_batch("DROP TRIGGER IF EXISTS force_evidence_failure;")
        .expect("failed to remove the forced evidence-insert-failure trigger");
}

// --- Confirmation gate (R018 §3/R021 §2) --------------------------------

#[test]
fn req_am_conf_a_explicit_unconfirmed_executes_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_conf_a.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    match add_memory(&mut conn, ui_explicit(), mk_am_payload()) {
        ActionOutcome::Success {
            confirmation_class, ..
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::ExplicitAdd);
        }
        other => panic!("[req_am_conf_a] FAIL - expected Success, got {other:?}"),
    }
    assert_eq!(am_fact_row_count(&conn), 1);
}

/// Central safety proof: `Provenance::Inferred` at `AddMemory`'s
/// `ExplicitAdd` floor must block with genuinely zero writes across
/// every table `AddMemory` could otherwise touch - no override code
/// exists for this in `add_memory` (unlike `CorrectMemory`), so this
/// proves the *generic* `effective_class`/`blocks_until_confirmed`
/// machinery alone is sufficient.
#[test]
fn req_am_conf_b_inferred_unconfirmed_blocks_with_zero_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_conf_b.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    match add_memory(&mut conn, meta, mk_am_payload()) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!("[req_am_conf_b] FAIL - expected ConfirmationRequired, got {other:?}"),
    }
    assert_eq!(
        am_fact_row_count(&conn),
        0,
        "[req_am_conf_b] FAIL - zero Fact writes expected"
    );
    assert_eq!(
        entity_row_count(&conn),
        1,
        "[req_am_conf_b] FAIL - only the bootstrap SELF entity should exist"
    );
    assert_eq!(am_source_row_count(&conn), 0);
    assert_eq!(am_evidence_row_count(&conn), 0);
    assert_eq!(am_fact_audit_row_count(&conn), 0);
}

#[test]
fn req_am_conf_c_inferred_confirmed_persists_user_confirmed_regardless_of_payload_state() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_conf_c.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let mut meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    meta.confirmed = true;
    let payload = AddMemoryPayload {
        epistemic_state: EpistemicState::Observed,
        ..mk_am_payload()
    };
    match add_memory(&mut conn, meta, payload) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(
                value.epistemic_state,
                EpistemicState::UserConfirmed,
                "[req_am_conf_c] FAIL - a confirmed Inferred resubmission must persist USER_CONFIRMED, not the payload's originally-proposed state"
            );
        }
        other => panic!("[req_am_conf_c] FAIL - expected Success, got {other:?}"),
    }
}

/// The only path that can persist a deliberate `HYPOTHESIS`+`INFERRED`
/// memory (R018 §5): a direct `Explicit` call persists exactly the
/// caller's requested `epistemic_state`, even though `Inferred` normally
/// implies a not-yet-confirmed model proposal.
#[test]
fn req_am_conf_d_explicit_call_persists_deliberate_hypothesis_inferred_state() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_conf_d.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let mut meta = ui_explicit();
    meta.confirmed = true; // irrelevant for Explicit - included to prove it stays irrelevant
    let payload = AddMemoryPayload {
        memory_type: MemoryType::Hypothesis,
        epistemic_state: EpistemicState::Inferred,
        ..mk_am_payload()
    };
    match add_memory(&mut conn, meta, payload) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.memory_type, MemoryType::Hypothesis);
            assert_eq!(
                value.epistemic_state,
                EpistemicState::Inferred,
                "[req_am_conf_d] FAIL - an explicit call must persist exactly the requested epistemic_state unchanged"
            );
        }
        other => panic!("[req_am_conf_d] FAIL - expected Success, got {other:?}"),
    }
}

/// Post-run correctness hardening (2026-09-18): the confirmation gate
/// must precede ALL database-dependent resolution, not merely block
/// before a write. A structurally valid but database-invalid payload
/// (an `Existing` reference to a nonexistent `EntityId`/`SourceId`, each
/// tried independently) must still return `ConfirmationRequired` for an
/// unconfirmed `Inferred` call - never `ValidationFailed(
/// ReferencedEntityNotFound/ReferencedSourceNotFound)` - proving the
/// database-dependent stage was never reached at all.
#[test]
fn req_am_conf_gate_precedes_db_dependent_validation() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_conf_gate_order.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_entity = EntityId::new();
    let fake_source = SourceId::new();
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);

    let payload_subject = AddMemoryPayload {
        subject: EntityInputRef::Existing(fake_entity),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, meta, payload_subject) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!(
            "[req_am_conf_gate_order] FAIL - expected ConfirmationRequired (gate must precede DB-dependent subject resolution), got {other:?}"
        ),
    }

    let payload_source = AddMemoryPayload {
        source: SourceRef::Existing(fake_source),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, meta, payload_source) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!(
            "[req_am_conf_gate_order] FAIL - expected ConfirmationRequired (gate must precede DB-dependent source resolution), got {other:?}"
        ),
    }

    assert_eq!(
        entity_row_count(&conn),
        1,
        "[req_am_conf_gate_order] FAIL - only the bootstrap SELF entity should exist"
    );
    assert_eq!(am_source_row_count(&conn), 0);
    assert_eq!(am_evidence_row_count(&conn), 0);
    assert_eq!(am_fact_row_count(&conn), 0);
    assert_eq!(am_fact_evidence_row_count(&conn), 0);
    assert_eq!(am_fact_audit_row_count(&conn), 0);
}

// --- Success combination matrix (R021 - 10 required combinations, ------
// grouped into a handful of table-driven tests rather than one test per
// trivial combination) ----------------------------------------------------

/// Combinations 1-2: Self subject + ScalarJson value + New Source, with
/// Evidence provided as content-only and as locator-only.
#[test]
fn req_am_success_self_subject_scalar_value_evidence_content_or_locator() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_succ1.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let self_id = self_entity_id(&conn);

    let p1 = AddMemoryPayload {
        evidence: NewEvidenceData {
            content: Some("user explicitly said it".to_string()),
            locator: None,
            sensitivity: Sensitivity::Internal,
        },
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p1) {
        ActionOutcome::Success { value, .. } => assert_eq!(value.subject_entity_id, self_id),
        other => panic!("[req_am_success1] FAIL content-only evidence, got {other:?}"),
    }

    let p2 = AddMemoryPayload {
        predicate: "mentioned_in".to_string(),
        evidence: NewEvidenceData {
            content: None,
            locator: Some("chat://conversation/42".to_string()),
            sensitivity: Sensitivity::Internal,
        },
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p2) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_am_success1] FAIL locator-only evidence, got {other:?}"),
    }

    assert_eq!(am_fact_row_count(&conn), 2);
    assert_eq!(am_evidence_row_count(&conn), 2);
    assert_eq!(am_fact_evidence_row_count(&conn), 2);
}

/// Combinations 3-4 + 10: Entity-valued Fact with a brand-new object (no
/// `domain_ref`), an Existing object, and a self-edge (subject == object,
/// both `Existing` referencing the same `EntityId`) - explicitly allowed,
/// no invented restriction.
#[test]
fn req_am_success_entity_valued_facts_new_existing_and_self_edge_objects() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_succ2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let self_id = self_entity_id(&conn);

    let p1 = AddMemoryPayload {
        value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
            entity_type: "vehicle".to_string(),
            canonical_name: "My Car".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        })),
        ..mk_am_payload()
    };
    let new_object_id = match add_memory(&mut conn, ui_explicit(), p1) {
        ActionOutcome::Success { value, .. } => match value.value {
            FactValue::EntityRef(id) => id,
            other => panic!("[req_am_success2] FAIL - expected EntityRef, got {other:?}"),
        },
        other => panic!("[req_am_success2] FAIL new object, got {other:?}"),
    };
    assert_ne!(new_object_id, self_id);

    let p2 = AddMemoryPayload {
        predicate: "also_owns".to_string(),
        value: FactValueInput::Entity(EntityInputRef::Existing(new_object_id)),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p2) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_am_success2] FAIL existing object, got {other:?}"),
    }

    let p3 = AddMemoryPayload {
        subject: EntityInputRef::SelfEntity,
        value: FactValueInput::Entity(EntityInputRef::SelfEntity),
        predicate: "reflects_on_self".to_string(),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p3) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.subject_entity_id, self_id);
            assert_eq!(value.value, FactValue::EntityRef(self_id));
        }
        other => panic!("[req_am_success2] FAIL self-edge, got {other:?}"),
    }

    assert_eq!(am_fact_row_count(&conn), 3);
    assert_eq!(
        entity_row_count(&conn),
        2,
        "SELF + the one new vehicle Entity"
    );
}

/// Combinations 5-6: Existing subject, and New subject without a
/// `domain_ref`.
#[test]
fn req_am_success_subject_variants_existing_and_new_without_domain_ref() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_succ3.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let p_new_person = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "person".to_string(),
            canonical_name: "Alex".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        }),
        ..mk_am_payload()
    };
    let alex_id = match add_memory(&mut conn, ui_explicit(), p_new_person) {
        ActionOutcome::Success { value, .. } => value.subject_entity_id,
        other => panic!("[req_am_success3] FAIL new subject, got {other:?}"),
    };

    let p_existing = AddMemoryPayload {
        subject: EntityInputRef::Existing(alex_id),
        predicate: "works_at".to_string(),
        value: FactValueInput::ScalarJson("\"Acme\"".to_string()),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p_existing) {
        ActionOutcome::Success { value, .. } => assert_eq!(value.subject_entity_id, alex_id),
        other => panic!("[req_am_success3] FAIL existing subject, got {other:?}"),
    }

    assert_eq!(am_fact_row_count(&conn), 2);
    assert_eq!(entity_row_count(&conn), 2, "SELF + Alex");
}

/// Combinations 7-9: subject/object backed by an authoritative
/// `domain_ref` on both roles (`Goal`/`Project`, then `Milestone`/`Task`),
/// and a `SourceRef::Existing` reuse.
#[test]
fn req_am_success_domain_ref_backed_new_entities_both_roles_and_existing_source() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_succ4.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal D")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup goal failed: {other:?}"),
    };
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project D", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup project failed: {other:?}"),
    };
    let milestone = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Milestone D", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup milestone failed: {other:?}"),
    };
    let task = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Task D", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup task failed: {other:?}"),
    };

    let p1 = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "goal".to_string(),
            canonical_name: "Goal D Entity".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(DomainRef {
                domain_ref_type: "Goal".to_string(),
                domain_ref_id: goal.id.to_string(),
            }),
        }),
        value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
            entity_type: "project".to_string(),
            canonical_name: "Project D Entity".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(DomainRef {
                domain_ref_type: "Project".to_string(),
                domain_ref_id: project.id.to_string(),
            }),
        })),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p1) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_am_success4] FAIL goal/project new entities, got {other:?}"),
    }

    let existing_source_id: String = conn
        .query_row("SELECT id FROM sources LIMIT 1", [], |r| r.get(0))
        .unwrap();
    let existing_source_id = SourceId::parse(&existing_source_id).unwrap();

    let p2 = AddMemoryPayload {
        predicate: "tracks".to_string(),
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "milestone".to_string(),
            canonical_name: "Milestone D Entity".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(DomainRef {
                domain_ref_type: "Milestone".to_string(),
                domain_ref_id: milestone.id.to_string(),
            }),
        }),
        value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
            entity_type: "task".to_string(),
            canonical_name: "Task D Entity".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(DomainRef {
                domain_ref_type: "Task".to_string(),
                domain_ref_id: task.id.to_string(),
            }),
        })),
        source: SourceRef::Existing(existing_source_id),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p2) {
        ActionOutcome::Success { .. } => {}
        other => panic!(
            "[req_am_success4] FAIL milestone/task new entities + existing source, got {other:?}"
        ),
    }

    assert_eq!(am_fact_row_count(&conn), 2);
    assert_eq!(
        entity_row_count(&conn),
        5,
        "SELF + Goal/Project/Milestone/Task-backed Entities"
    );
    assert_eq!(
        am_source_row_count(&conn),
        1,
        "[req_am_success4] FAIL - the second call must reuse the existing Source, not create a new one"
    );
}

// --- Duplicate domain-ref detection (two layers, R018 §9/R019 §9) ------

#[test]
fn req_am_dup_persisted_duplicate_domain_ref_rejected_across_two_calls_zero_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_dup.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Dup Goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup goal failed: {other:?}"),
    };
    let domain_ref = DomainRef {
        domain_ref_type: "Goal".to_string(),
        domain_ref_id: goal.id.to_string(),
    };

    let p1 = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "goal".to_string(),
            canonical_name: "Goal Entity 1".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(domain_ref.clone()),
        }),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p1) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_am_dup] FAIL first insert, got {other:?}"),
    }
    // Full-table snapshot (post-run correctness hardening, 2026-09-18) -
    // the first call's own writes are the baseline the rejected second
    // call must not disturb in any way.
    let snapshot_after_first = am_full_table_snapshot(&conn);
    let self_after_first = self_entity_id(&conn);

    // A SEPARATE AddMemory call (not the same payload) tries to create
    // ANOTHER New Entity with the same domain_ref - the persisted-
    // duplicate check (find_entity_by_domain_ref), not the same-payload
    // preflight, must catch this.
    let p2 = AddMemoryPayload {
        predicate: "different_predicate".to_string(),
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "goal".to_string(),
            canonical_name: "Goal Entity 2, different name".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(domain_ref),
        }),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p2) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::DuplicateDomainReference));
        }
        other => panic!("[req_am_dup] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(
        am_full_table_snapshot(&conn),
        snapshot_after_first,
        "[req_am_dup] FAIL - the rejected second call must leave every AddMemory-mutated table (entities/sources/evidence/facts/fact_evidence/fact_audit_log) exactly as the first call left them"
    );
    assert_eq!(
        self_entity_id(&conn),
        self_after_first,
        "[req_am_dup] FAIL - SELF must remain exactly the same entity"
    );
}

/// Post-run correctness hardening (2026-09-18): the same-payload
/// duplicate-domain-ref preflight (`actions::validate_add_memory`, pure)
/// is proven directly in `actions::tests` - this proves the full
/// `add_memory` action respects it too, with genuinely zero writes
/// across every mutated table, not only that the pure validator function
/// itself rejects the shape.
#[test]
fn req_am_dup_same_payload_full_zero_write_proof() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_dup_same_payload.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let goal = match create_goal(
        &mut conn,
        ui_explicit(),
        mk_create_payload("Same-Payload Dup Goal"),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup goal failed: {other:?}"),
    };
    let domain_ref = DomainRef {
        domain_ref_type: "Goal".to_string(),
        domain_ref_id: goal.id.to_string(),
    };
    let self_before = self_entity_id(&conn);
    let snapshot_before = am_full_table_snapshot(&conn);

    let payload = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "goal".to_string(),
            canonical_name: "Goal Entity Subject".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(domain_ref.clone()),
        }),
        value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
            entity_type: "goal".to_string(),
            canonical_name: "Goal Entity Object, different name".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(domain_ref),
        })),
        ..mk_am_payload()
    };

    match add_memory(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(
                errors.contains(&ValidationError::DuplicateDomainReference),
                "[req_am_dup_same_payload] FAIL - expected DuplicateDomainReference, got {errors:?}"
            );
        }
        other => {
            panic!("[req_am_dup_same_payload] FAIL - expected ValidationFailed, got {other:?}")
        }
    }

    assert_eq!(
        am_full_table_snapshot(&conn),
        snapshot_before,
        "[req_am_dup_same_payload] FAIL - a same-payload duplicate-domain-ref rejection must leave every AddMemory-mutated table (entities/sources/evidence/facts/fact_evidence/fact_audit_log) completely unchanged"
    );
    assert_eq!(
        self_entity_id(&conn),
        self_before,
        "[req_am_dup_same_payload] FAIL - SELF must remain exactly the same entity"
    );
}

// --- Authoritative domain-ref matrix (R021 §7) --------------------------

#[test]
fn req_am_dref_matrix_supported_types_accepted_invalid_and_unsupported_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_dref.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Goal for dref")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup goal failed: {other:?}"),
    };
    let project = match create_project(
        &mut conn,
        ui_explicit(),
        mk_create_project_payload("Project for dref", None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup project failed: {other:?}"),
    };
    let milestone = match add_milestone(
        &mut conn,
        ui_explicit(),
        mk_add_milestone_payload(project.id, "Milestone for dref", 0),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup milestone failed: {other:?}"),
    };
    let task = match add_task(
        &mut conn,
        ui_explicit(),
        mk_add_task_payload("Task for dref", None, None, None),
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup task failed: {other:?}"),
    };

    let cases: Vec<(&str, String)> = vec![
        ("Goal", goal.id.to_string()),
        ("Project", project.id.to_string()),
        ("Milestone", milestone.id.to_string()),
        ("Task", task.id.to_string()),
    ];

    // Accepted: a real, existing row of each supported type.
    for (type_str, id_str) in &cases {
        let payload = AddMemoryPayload {
            subject: EntityInputRef::New(NewEntityData {
                entity_type: type_str.to_lowercase(),
                canonical_name: format!("{type_str} entity"),
                sensitivity: Sensitivity::Internal,
                domain_ref: Some(DomainRef {
                    domain_ref_type: type_str.to_string(),
                    domain_ref_id: id_str.clone(),
                }),
            }),
            ..mk_am_payload()
        };
        match add_memory(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { .. } => {}
            other => panic!(
                "[req_am_dref] FAIL - existing {type_str} domain_ref should be accepted, got {other:?}"
            ),
        }
    }

    // Rejected: well-formed but nonexistent id, for each supported type.
    for (type_str, _) in &cases {
        let fake_id = EntityId::new().to_string();
        let payload = AddMemoryPayload {
            subject: EntityInputRef::New(NewEntityData {
                entity_type: type_str.to_lowercase(),
                canonical_name: "Nonexistent ref".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: Some(DomainRef {
                    domain_ref_type: type_str.to_string(),
                    domain_ref_id: fake_id,
                }),
            }),
            ..mk_am_payload()
        };
        match add_memory(&mut conn, ui_explicit(), payload) {
            ActionOutcome::ValidationFailed(errors) => assert!(
                errors.contains(&ValidationError::ReferencedDomainObjectNotFound),
                "[req_am_dref] FAIL - {type_str} well-formed-but-nonexistent id should reject with ReferencedDomainObjectNotFound, got {errors:?}"
            ),
            other => panic!(
                "[req_am_dref] FAIL - {type_str} nonexistent id should be ValidationFailed, got {other:?}"
            ),
        }
    }

    // Rejected: syntactically invalid id, for each supported type.
    for (type_str, _) in &cases {
        let payload = AddMemoryPayload {
            subject: EntityInputRef::New(NewEntityData {
                entity_type: type_str.to_lowercase(),
                canonical_name: "Malformed ref".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: Some(DomainRef {
                    domain_ref_type: type_str.to_string(),
                    domain_ref_id: "not-a-uuid".to_string(),
                }),
            }),
            ..mk_am_payload()
        };
        match add_memory(&mut conn, ui_explicit(), payload) {
            ActionOutcome::ValidationFailed(errors) => assert!(
                errors.contains(&ValidationError::ReferencedDomainObjectNotFound),
                "[req_am_dref] FAIL - {type_str} malformed id should reject with ReferencedDomainObjectNotFound, got {errors:?}"
            ),
            other => panic!(
                "[req_am_dref] FAIL - {type_str} malformed id should be ValidationFailed, got {other:?}"
            ),
        }
    }

    // Rejected: unsupported domain_ref_type - does not close the open
    // domain_ref_type vocabulary, only limits what Core validates today.
    let payload = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "vehicle".to_string(),
            canonical_name: "My Car".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(DomainRef {
                domain_ref_type: "Vehicle".to_string(),
                domain_ref_id: "anything".to_string(),
            }),
        }),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::UnsupportedDomainReferenceType));
        }
        other => panic!(
            "[req_am_dref] FAIL - unsupported domain_ref_type should be ValidationFailed, got {other:?}"
        ),
    }

    // Every rejected attempt above must have written nothing at all -
    // only the 4 accepted-case Entities (+ SELF) and their Facts exist.
    assert_eq!(entity_row_count(&conn), 1 + 4);
    assert_eq!(am_fact_row_count(&conn), 4);
}

// --- Existing-reference not-found (R021 §6) -----------------------------

#[test]
fn req_am_not_found_existing_subject_object_source_rejected_zero_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_nf.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fake_entity = EntityId::new();
    let fake_source = SourceId::new();

    let p1 = AddMemoryPayload {
        subject: EntityInputRef::Existing(fake_entity),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p1) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::ReferencedEntityNotFound))
        }
        other => panic!("[req_am_nf] FAIL subject, got {other:?}"),
    }

    let p2 = AddMemoryPayload {
        value: FactValueInput::Entity(EntityInputRef::Existing(fake_entity)),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p2) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::ReferencedEntityNotFound))
        }
        other => panic!("[req_am_nf] FAIL object, got {other:?}"),
    }

    let p3 = AddMemoryPayload {
        source: SourceRef::Existing(fake_source),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), p3) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::ReferencedSourceNotFound))
        }
        other => panic!("[req_am_nf] FAIL source, got {other:?}"),
    }

    assert_eq!(am_fact_row_count(&conn), 0);
    assert_eq!(entity_row_count(&conn), 1, "SELF only");
    assert_eq!(am_source_row_count(&conn), 0);
}

// --- Existing Entity/Source immutability-on-reuse (R021) ----------------

#[test]
fn req_am_existing_entity_and_source_untouched_on_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_immut.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let setup = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "person".to_string(),
            canonical_name: "Jordan".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        }),
        ..mk_am_payload()
    };
    let (jordan_id, source_id) = match add_memory(&mut conn, ui_explicit(), setup) {
        ActionOutcome::Success { value, .. } => {
            let source_id_str: String = conn
                .query_row(
                    "SELECT e.source_id FROM evidence e
                     JOIN fact_evidence fe ON fe.evidence_id = e.id
                     WHERE fe.fact_id = ?1",
                    rusqlite::params![value.id.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
            (
                value.subject_entity_id,
                SourceId::parse(&source_id_str).unwrap(),
            )
        }
        other => panic!("[req_am_immut] FAIL setup, got {other:?}"),
    };

    let entity_before = get_entity(&conn, jordan_id).unwrap().unwrap();
    let source_before = read_source_fields(&conn, source_id);

    let reuse = AddMemoryPayload {
        subject: EntityInputRef::Existing(jordan_id),
        predicate: "lives_in".to_string(),
        value: FactValueInput::ScalarJson("\"Warsaw\"".to_string()),
        source: SourceRef::Existing(source_id),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), reuse) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_am_immut] FAIL reuse call, got {other:?}"),
    }

    let entity_after = get_entity(&conn, jordan_id).unwrap().unwrap();
    let source_after = read_source_fields(&conn, source_id);
    assert_eq!(
        entity_before, entity_after,
        "[req_am_immut] FAIL - Existing Entity was mutated on reuse"
    );
    assert_eq!(
        source_before, source_after,
        "[req_am_immut] FAIL - Existing Source was mutated on reuse"
    );
    assert_eq!(entity_row_count(&conn), 2, "SELF + Jordan");
    assert_eq!(
        am_source_row_count(&conn),
        1,
        "[req_am_immut] FAIL - reuse must not create a second Source row"
    );
}

// --- Full atomic rollback (R021 - one of Slice B's central acceptance --
// tests) ------------------------------------------------------------------

#[test]
fn req_am_rollback_forced_audit_failure_rolls_back_everything() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_rollback1.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    install_fact_audit_failure_trigger(&conn);

    let payload = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "person".to_string(),
            canonical_name: "Rollback Subject".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        }),
        value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
            entity_type: "place".to_string(),
            canonical_name: "Rollback Object".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        })),
        ..mk_am_payload()
    };

    match add_memory(&mut conn, ui_explicit(), payload) {
        ActionOutcome::StorageError(_) => {}
        other => panic!(
            "[req_am_rollback1] FAIL - expected StorageError from the forced audit failure, got {other:?}"
        ),
    }

    remove_fact_audit_failure_trigger(&conn);

    assert_eq!(
        entity_row_count(&conn),
        1,
        "[req_am_rollback1] FAIL - only SELF should survive; the new subject+object Entities must be rolled back"
    );
    assert_eq!(
        am_source_row_count(&conn),
        0,
        "[req_am_rollback1] FAIL - the new Source must be rolled back"
    );
    assert_eq!(
        am_fact_row_count(&conn),
        0,
        "[req_am_rollback1] FAIL - the Fact must be rolled back"
    );
    assert_eq!(
        am_evidence_row_count(&conn),
        0,
        "[req_am_rollback1] FAIL - the Evidence must be rolled back"
    );
    assert_eq!(am_fact_evidence_row_count(&conn), 0);
    assert_eq!(am_fact_audit_row_count(&conn), 0);

    // The connection remains usable afterward - a normal AddMemory call
    // succeeds, and SELF is still exactly the original bootstrap row.
    match add_memory(&mut conn, ui_explicit(), mk_am_payload()) {
        ActionOutcome::Success { .. } => {}
        other => panic!(
            "[req_am_rollback1] FAIL - connection should remain usable after rollback, got {other:?}"
        ),
    }
    let self_entity = storage::personal_knowledge_repo::get_self_entity(&conn)
        .unwrap()
        .unwrap();
    assert!(self_entity.is_self);
}

/// An earlier-stage failure (before the final `fact_audit_log` write) -
/// the same rollback guarantee must hold no matter which insert in the
/// sequence fails.
#[test]
fn req_am_rollback_earlier_stage_forced_evidence_failure_rolls_back_new_entity_and_source() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_rollback2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    install_evidence_insert_failure_trigger(&conn);

    let payload = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "person".to_string(),
            canonical_name: "Earlier Rollback Subject".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        }),
        ..mk_am_payload()
    };

    match add_memory(&mut conn, ui_explicit(), payload) {
        ActionOutcome::StorageError(_) => {}
        other => panic!("[req_am_rollback2] FAIL - expected StorageError, got {other:?}"),
    }

    remove_evidence_insert_failure_trigger(&conn);

    assert_eq!(
        entity_row_count(&conn),
        1,
        "the new subject Entity must be rolled back"
    );
    assert_eq!(
        am_source_row_count(&conn),
        0,
        "the new Source must be rolled back"
    );
    assert_eq!(
        am_fact_row_count(&conn),
        0,
        "the Fact insert happens before Evidence in the sequence but must still roll back"
    );
    assert_eq!(am_evidence_row_count(&conn), 0);
}

// --- Audit correctness (R021) --------------------------------------------

#[test]
fn req_am_audit_row_fields_correct_and_previous_snapshot_always_null() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_audit1.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    // A deliberately known requested_at, not Utc::now(), so occurred_at
    // can be proven equal to it exactly rather than merely "recent".
    let requested_at: chrono::DateTime<chrono::Utc> = "2026-09-18T12:34:56Z"
        .parse()
        .expect("valid RFC3339 literal");
    let meta = ActionMeta {
        actor: Actor::Ui,
        provenance: Provenance::Explicit,
        requested_at,
        confirmed: false,
    };

    let fact = match add_memory(&mut conn, meta, mk_am_payload()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let (action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at): (
        String,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at FROM fact_audit_log WHERE fact_id = ?1",
            rusqlite::params![fact.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap();
    assert_eq!(action_type, "AddMemory");
    assert_eq!(actor, "UI");
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "EXPLICIT_ADD");
    assert!(
        previous_snapshot.is_none(),
        "[req_am_audit1] FAIL - previous_snapshot must always be NULL for AddMemory (a create, never an update)"
    );
    assert_eq!(
        occurred_at,
        requested_at.to_rfc3339(),
        "[req_am_audit1] FAIL - fact_audit_log.occurred_at must equal ActionMeta.requested_at exactly, using the project's canonical RFC3339 representation"
    );
}

/// Confirmation and provenance are distinct concepts: confirming an
/// Inferred action must never silently rewrite the audit row's
/// `provenance` to `Explicit`, even though `epistemic_state` becomes
/// `USER_CONFIRMED`.
#[test]
fn req_am_audit_provenance_not_rewritten_when_inferred_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_audit2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let mut meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    meta.confirmed = true;
    let payload = AddMemoryPayload {
        epistemic_state: EpistemicState::Observed,
        ..mk_am_payload()
    };
    let fact = match add_memory(&mut conn, meta, payload) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    assert_eq!(fact.epistemic_state, EpistemicState::UserConfirmed);

    let provenance: String = conn
        .query_row(
            "SELECT provenance FROM fact_audit_log WHERE fact_id = ?1",
            rusqlite::params![fact.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        provenance, "Inferred",
        "[req_am_audit2] FAIL - confirming an inferred action must never silently rewrite provenance to Explicit"
    );
    let actor: String = conn
        .query_row(
            "SELECT actor FROM fact_audit_log WHERE fact_id = ?1",
            rusqlite::params![fact.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(actor, "CloudLLM");
}

// --- Zero side effects on Relation/Episode tables (schema-only, R019 §13) -

#[test]
fn req_am_no_side_effects_on_relation_episode_tables() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_no_side.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    match add_memory(&mut conn, ui_explicit(), mk_am_payload()) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }
    assert_eq!(
        relation_episode_side_effect_counts(&conn),
        (0, 0, 0, 0, 0),
        "[req_am_no_side] FAIL - AddMemory must never write to relations/episodes/relation_evidence/episode_evidence/episode_entities"
    );
}

// --- No implicit dedup/correction/alias-merge (R021 §16) ----------------

#[test]
fn req_am_no_implicit_dedup_two_identical_calls_create_two_facts() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_no_dedup.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    for _ in 0..2 {
        match add_memory(&mut conn, ui_explicit(), mk_am_payload()) {
            ActionOutcome::Success { .. } => {}
            other => panic!("[req_am_no_dedup] FAIL, got {other:?}"),
        }
    }
    assert_eq!(
        am_fact_row_count(&conn),
        2,
        "[req_am_no_dedup] FAIL - two explicit, identical AddMemory calls must create two distinct Facts - no implicit dedup/correction/auto-STALE/similarity-merge"
    );
}

#[test]
fn req_am_no_entity_alias_merge_same_canonical_name_creates_two_entities() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "am_no_alias.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    for i in 0..2 {
        let payload = AddMemoryPayload {
            predicate: format!("predicate_{i}"),
            subject: EntityInputRef::New(NewEntityData {
                entity_type: "person".to_string(),
                canonical_name: "Sam".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
            }),
            ..mk_am_payload()
        };
        match add_memory(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { .. } => {}
            other => panic!("[req_am_no_alias] FAIL, got {other:?}"),
        }
    }
    assert_eq!(
        entity_row_count(&conn),
        3,
        "[req_am_no_alias] FAIL - two New Entities sharing a canonical_name (no domain_ref) must not be merged - SELF + 2 distinct Sam Entities expected"
    );
}

// =========================================================================
// M2 Personal Knowledge Slice C - CorrectMemory (2026-09-18)
//
// Evidence for
// `docs/reports/R022_M2_PERSONAL_KNOWLEDGE_CORRECTMEMORY_SLICE_C_2026-09-18.md`.
// The second production Personal Knowledge mutation - "the retained
// semantic knowledge is wrong/outdated; replace it while preserving
// history", never an in-place semantic edit. Database-dependent
// behavior only - pure `validate_correct_memory` structural tests live
// in `actions::tests` instead (same split as `AddMemory`).
// =========================================================================

/// A minimal, always-valid baseline `CorrectMemoryPayload` targeting a
/// given predecessor - ScalarJson value, brand-new Source, Evidence
/// with content only. Every test below overrides only the fields it
/// cares about via struct-update syntax.
fn mk_cm_payload(predecessor_fact_id: FactId) -> CorrectMemoryPayload {
    CorrectMemoryPayload {
        predecessor_fact_id,
        value: FactValueInput::ScalarJson("\"tea, not coffee\"".to_string()),
        memory_type: None,
        confidence: FieldPatch::Unchanged,
        sensitivity: None,
        source: mk_am_source_new(),
        evidence: NewEvidenceData {
            content: Some("correction evidence".to_string()),
            locator: None,
            sensitivity: Sensitivity::Internal,
        },
    }
}

/// Creates a fresh, active predecessor `Fact` via `add_memory` (Self
/// subject, ScalarJson value, New Source/Evidence) - the standard
/// fixture every `CorrectMemory` test starts from.
fn mk_cm_predecessor(conn: &mut Connection) -> Fact {
    match add_memory(conn, ui_explicit(), mk_am_payload()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[cm predecessor setup] FAIL - add_memory failed: {other:?}"),
    }
}

// --- Confirmation test matrix (R022 §21) --------------------------------

#[test]
fn req_cm_conf_a_explicit_unconfirmed_executes_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_conf_a.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);

    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(predecessor.id)) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::ImportantChange);
            assert_eq!(value.epistemic_state, EpistemicState::UserConfirmed);
        }
        other => panic!("[req_cm_conf_a] FAIL - expected Success, got {other:?}"),
    }
}

/// The Personal-Knowledge inferred-confirmation overlay (R018 §6):
/// unlike `AddMemory`, `CorrectMemory`'s `ImportantChange` floor is NOT
/// dominated by `InferredChange` in the generic ladder, so
/// `services::memory_action_requires_confirmation` is what forces the
/// block here. Uses a NONEXISTENT `predecessor_fact_id` to prove the
/// gate precedes the predecessor lookup entirely - the correct outcome
/// is `ConfirmationRequired`, never `NotFound`.
#[test]
fn req_cm_conf_b_inferred_unconfirmed_blocks_before_predecessor_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_conf_b.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let nonexistent = FactId::new();
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    match correct_memory(&mut conn, meta, mk_cm_payload(nonexistent)) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::ImportantChange);
        }
        other => panic!(
            "[req_cm_conf_b] FAIL - expected ConfirmationRequired (gate must precede predecessor lookup), got {other:?}"
        ),
    }
    assert_eq!(
        am_full_table_snapshot(&conn),
        (1, 0, 0, 0, 0, 0),
        "[req_cm_conf_b] FAIL - zero writes, only the bootstrap SELF entity should exist"
    );
}

#[test]
fn req_cm_conf_c_inferred_confirmed_executes_and_audit_provenance_remains_inferred() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_conf_c.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);

    let mut meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    meta.confirmed = true;
    let successor = match correct_memory(&mut conn, meta, mk_cm_payload(predecessor.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_conf_c] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(successor.epistemic_state, EpistemicState::UserConfirmed);

    let provenance: String = conn
        .query_row(
            "SELECT provenance FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'CorrectMemory'",
            rusqlite::params![predecessor.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        provenance, "Inferred",
        "[req_cm_conf_c] FAIL - confirming an inferred correction must never silently rewrite provenance to Explicit"
    );
}

/// Pure structural validation still precedes the confirmation gate -
/// mirrors the corrected R021 `AddMemory` safety boundary exactly.
#[test]
fn req_cm_conf_d_structurally_invalid_payload_returns_validation_failed_before_gate() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_conf_d.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    let payload = CorrectMemoryPayload {
        value: FactValueInput::ScalarJson("{not valid json".to_string()),
        ..mk_cm_payload(FactId::new())
    };
    match correct_memory(&mut conn, meta, payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::InvalidValueJson));
        }
        other => panic!("[req_cm_conf_d] FAIL - expected ValidationFailed, got {other:?}"),
    }
}

// --- Active predecessor matrix (R022 §22) -------------------------------

/// Proves the full `is_active_fact` two-part predicate, not merely
/// `valid_until`: (A) a normal active predecessor is accepted; (B) an
/// already-corrected (now `Stale`) predecessor is rejected; (C) a raw
/// fixture with `epistemic_state = STALE` but `valid_until = NULL` is
/// rejected (the deliberate inconsistent-state case); (D) a raw fixture
/// with `epistemic_state != STALE` but `valid_until != NULL` is
/// rejected.
#[test]
fn req_cm_active_predecessor_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_active.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let active = mk_cm_predecessor(&mut conn);
    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(active.id)) {
        ActionOutcome::Success { .. } => {}
        other => {
            panic!("[req_cm_active] FAIL - A: active predecessor should be accepted, got {other:?}")
        }
    }

    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(active.id)) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::FactNotActive));
        }
        other => panic!(
            "[req_cm_active] FAIL - B: already-corrected predecessor should be rejected, got {other:?}"
        ),
    }

    let fixture_c = mk_cm_predecessor(&mut conn);
    conn.execute(
        "UPDATE facts SET epistemic_state = 'STALE' WHERE id = ?1",
        rusqlite::params![fixture_c.id.to_string()],
    )
    .unwrap();
    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(fixture_c.id)) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::FactNotActive));
        }
        other => panic!(
            "[req_cm_active] FAIL - C: STALE-with-NULL-valid_until predecessor should be rejected, got {other:?}"
        ),
    }

    let fixture_d = mk_cm_predecessor(&mut conn);
    conn.execute(
        "UPDATE facts SET valid_until = ?2 WHERE id = ?1",
        rusqlite::params![fixture_d.id.to_string(), Utc::now().to_rfc3339()],
    )
    .unwrap();
    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(fixture_d.id)) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::FactNotActive));
        }
        other => panic!(
            "[req_cm_active] FAIL - D: valid_until-set predecessor should be rejected, got {other:?}"
        ),
    }

    // Zero writes from any rejected attempt (B/C/D) - three predecessor
    // Facts (active/fixture_c/fixture_d) plus exactly one successor
    // (from A's success).
    assert_eq!(am_fact_row_count(&conn), 4);
}

// --- Subject/predicate immutability (R022 §10) --------------------------

#[test]
fn req_cm_subject_predicate_inherited_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_inherit_sp.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);

    let successor = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(predecessor.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_inherit_sp] FAIL, got {other:?}"),
    };
    assert_eq!(successor.subject_entity_id, predecessor.subject_entity_id);
    assert_eq!(successor.predicate, predecessor.predicate);
}

// --- FactValueInput success matrix (R022 §8) ----------------------------

/// Correction successor value supports ScalarJson, Entity(SelfEntity),
/// Entity(Existing), Entity(New without domain_ref), and Entity(New with
/// a valid supported domain_ref) - each needs its OWN fresh predecessor,
/// since a predecessor becomes inactive after being corrected once.
#[test]
fn req_cm_success_value_variants_scalar_self_existing_new_domain_ref() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_value_variants.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let self_id = self_entity_id(&conn);

    let p1 = mk_cm_predecessor(&mut conn);
    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(p1.id)) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(
                value.value,
                FactValue::Scalar("\"tea, not coffee\"".to_string())
            );
        }
        other => panic!("[req_cm_value] FAIL ScalarJson, got {other:?}"),
    }

    let p2 = mk_cm_predecessor(&mut conn);
    match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::SelfEntity),
            ..mk_cm_payload(p2.id)
        },
    ) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.value, FactValue::EntityRef(self_id));
        }
        other => panic!("[req_cm_value] FAIL Entity(SelfEntity), got {other:?}"),
    }

    let p3 = mk_cm_predecessor(&mut conn);
    match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::Existing(self_id)),
            ..mk_cm_payload(p3.id)
        },
    ) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.value, FactValue::EntityRef(self_id));
        }
        other => panic!("[req_cm_value] FAIL Entity(Existing), got {other:?}"),
    }

    let p4 = mk_cm_predecessor(&mut conn);
    let new_object_id = match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
                entity_type: "vehicle".to_string(),
                canonical_name: "Correction Car".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
            })),
            ..mk_cm_payload(p4.id)
        },
    ) {
        ActionOutcome::Success { value, .. } => match value.value {
            FactValue::EntityRef(id) => id,
            other => panic!("[req_cm_value] FAIL - expected EntityRef, got {other:?}"),
        },
        other => panic!("[req_cm_value] FAIL Entity(New no domain_ref), got {other:?}"),
    };
    assert_ne!(new_object_id, self_id);

    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("CM Value Goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup goal failed: {other:?}"),
    };
    let p5 = mk_cm_predecessor(&mut conn);
    match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
                entity_type: "goal".to_string(),
                canonical_name: "CM Value Goal Entity".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: Some(DomainRef {
                    domain_ref_type: "Goal".to_string(),
                    domain_ref_id: goal.id.to_string(),
                }),
            })),
            ..mk_cm_payload(p5.id)
        },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_cm_value] FAIL Entity(New with domain_ref), got {other:?}"),
    }
}

/// All the ways a correction can be rejected before any write: malformed
/// JSON, overlong JSON, a missing `Existing` Entity, a nonexistent
/// authoritative domain object, and a persisted-duplicate domain_ref -
/// reusing ONE predecessor throughout (every attempt is rejected, so the
/// predecessor is never consumed) and proving it remains completely
/// unchanged at the end.
#[test]
fn req_cm_rejects_malformed_value_and_missing_references_zero_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_reject.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);
    let predecessor_before = predecessor.clone();
    let snapshot_before_first_four = am_full_table_snapshot(&conn);

    match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            value: FactValueInput::ScalarJson("{not valid".to_string()),
            ..mk_cm_payload(predecessor.id)
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::InvalidValueJson))
        }
        other => panic!("[req_cm_reject] FAIL malformed JSON, got {other:?}"),
    }

    let overlong = format!("\"{}\"", "x".repeat(crate::domain::VALUE_JSON_MAX_LEN));
    match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            value: FactValueInput::ScalarJson(overlong),
            ..mk_cm_payload(predecessor.id)
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => assert!(errors.iter().any(|e| matches!(
            e,
            ValidationError::PersonalKnowledgeFieldTooLong {
                field: "value_json",
                ..
            }
        ))),
        other => panic!("[req_cm_reject] FAIL overlong JSON, got {other:?}"),
    }

    match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::Existing(EntityId::new())),
            ..mk_cm_payload(predecessor.id)
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::ReferencedEntityNotFound))
        }
        other => panic!("[req_cm_reject] FAIL missing existing entity, got {other:?}"),
    }

    match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
                entity_type: "goal".to_string(),
                canonical_name: "Bad Goal Ref".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: Some(DomainRef {
                    domain_ref_type: "Goal".to_string(),
                    domain_ref_id: EntityId::new().to_string(),
                }),
            })),
            ..mk_cm_payload(predecessor.id)
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::ReferencedDomainObjectNotFound))
        }
        other => panic!("[req_cm_reject] FAIL nonexistent domain object, got {other:?}"),
    }

    assert_eq!(
        am_full_table_snapshot(&conn),
        snapshot_before_first_four,
        "[req_cm_reject] FAIL - the four rejected attempts above must leave every table unchanged"
    );

    // Persisted-duplicate domain_ref needs a real, already-persisted
    // Entity owning the domain_ref first - a legitimate setup write,
    // snapshotted separately so it is not mistaken for a rejected call's
    // side effect.
    let goal2 = match create_goal(&mut conn, ui_explicit(), mk_create_payload("CM Dup Goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup goal failed: {other:?}"),
    };
    let domain_ref = DomainRef {
        domain_ref_type: "Goal".to_string(),
        domain_ref_id: goal2.id.to_string(),
    };
    let seed = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "goal".to_string(),
            canonical_name: "Existing Goal Entity".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(domain_ref.clone()),
        }),
        ..mk_am_payload()
    };
    match add_memory(&mut conn, ui_explicit(), seed) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup seed AddMemory failed: {other:?}"),
    }
    let snapshot_before_dup = am_full_table_snapshot(&conn);

    match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
                entity_type: "goal".to_string(),
                canonical_name: "Duplicate Goal Entity".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: Some(domain_ref),
            })),
            ..mk_cm_payload(predecessor.id)
        },
    ) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::DuplicateDomainReference))
        }
        other => panic!("[req_cm_reject] FAIL persisted duplicate domain_ref, got {other:?}"),
    }

    assert_eq!(
        am_full_table_snapshot(&conn),
        snapshot_before_dup,
        "[req_cm_reject] FAIL - the persisted-duplicate rejection must leave every table unchanged"
    );

    let predecessor_after = get_fact(&conn, predecessor.id).unwrap().unwrap();
    assert_eq!(
        predecessor_before, predecessor_after,
        "[req_cm_reject] FAIL - the predecessor must remain byte-for-byte unchanged after every rejected attempt"
    );
}

// --- memory_type/sensitivity/confidence inherit-vs-override (R022 §23) -

#[test]
fn req_cm_memory_type_sensitivity_confidence_inherit_and_override() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_inherit_override.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let p1_payload = AddMemoryPayload {
        memory_type: MemoryType::Observation,
        sensitivity: Sensitivity::Sensitive,
        confidence: Some(0.4),
        ..mk_am_payload()
    };
    let predecessor1 = match add_memory(&mut conn, ui_explicit(), p1_payload) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup predecessor1 failed: {other:?}"),
    };
    let successor1 = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(predecessor1.id))
    {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_inherit_override] FAIL inherit case, got {other:?}"),
    };
    assert_eq!(
        successor1.memory_type,
        MemoryType::Observation,
        "memory_type should inherit when None"
    );
    assert_eq!(
        successor1.sensitivity,
        Sensitivity::Sensitive,
        "sensitivity should inherit when None"
    );
    assert_eq!(
        successor1.confidence,
        Some(0.4),
        "confidence should inherit under Unchanged"
    );
    let predecessor1_after = get_fact(&conn, predecessor1.id).unwrap().unwrap();
    assert_eq!(predecessor1_after.memory_type, MemoryType::Observation);
    assert_eq!(predecessor1_after.sensitivity, Sensitivity::Sensitive);
    assert_eq!(predecessor1_after.confidence, Some(0.4));

    let p2_payload = AddMemoryPayload {
        memory_type: MemoryType::Observation,
        sensitivity: Sensitivity::Sensitive,
        confidence: Some(0.4),
        ..mk_am_payload()
    };
    let predecessor2 = match add_memory(&mut conn, ui_explicit(), p2_payload) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup predecessor2 failed: {other:?}"),
    };
    let override_payload = CorrectMemoryPayload {
        memory_type: Some(MemoryType::EffectiveSolution),
        sensitivity: Some(Sensitivity::Public),
        confidence: FieldPatch::Set(0.9),
        ..mk_cm_payload(predecessor2.id)
    };
    let successor2 = match correct_memory(&mut conn, ui_explicit(), override_payload) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_inherit_override] FAIL override case, got {other:?}"),
    };
    assert_eq!(successor2.memory_type, MemoryType::EffectiveSolution);
    assert_eq!(successor2.sensitivity, Sensitivity::Public);
    assert_eq!(successor2.confidence, Some(0.9));
    let predecessor2_after = get_fact(&conn, predecessor2.id).unwrap().unwrap();
    assert_eq!(
        predecessor2_after.memory_type,
        MemoryType::Observation,
        "predecessor memory_type never changes even when successor overrides"
    );
    assert_eq!(
        predecessor2_after.sensitivity,
        Sensitivity::Sensitive,
        "predecessor sensitivity never changes even when successor overrides"
    );
    assert_eq!(
        predecessor2_after.confidence,
        Some(0.4),
        "predecessor confidence never changes even when successor overrides"
    );
}

// --- confidence FieldPatch matrix (R022 §23/§35) ------------------------

#[test]
fn req_cm_confidence_fieldpatch_matrix_unchanged_set_clear() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_confidence_patch.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    fn make_predecessor(conn: &mut Connection) -> Fact {
        let payload = AddMemoryPayload {
            confidence: Some(0.4),
            ..mk_am_payload()
        };
        match add_memory(conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup predecessor failed: {other:?}"),
        }
    }

    let p_unchanged = make_predecessor(&mut conn);
    let s_unchanged = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(p_unchanged.id))
    {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_conf_patch] FAIL Unchanged, got {other:?}"),
    };
    assert_eq!(s_unchanged.confidence, Some(0.4));
    assert_eq!(
        get_fact(&conn, p_unchanged.id).unwrap().unwrap().confidence,
        Some(0.4)
    );

    let p_set = make_predecessor(&mut conn);
    let s_set = match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            confidence: FieldPatch::Set(0.8),
            ..mk_cm_payload(p_set.id)
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_conf_patch] FAIL Set, got {other:?}"),
    };
    assert_eq!(s_set.confidence, Some(0.8));
    assert_eq!(
        get_fact(&conn, p_set.id).unwrap().unwrap().confidence,
        Some(0.4),
        "predecessor confidence must remain untouched"
    );

    let p_clear = make_predecessor(&mut conn);
    let s_clear = match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            confidence: FieldPatch::Clear,
            ..mk_cm_payload(p_clear.id)
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_conf_patch] FAIL Clear, got {other:?}"),
    };
    assert_eq!(s_clear.confidence, None);
    assert_eq!(
        get_fact(&conn, p_clear.id).unwrap().unwrap().confidence,
        Some(0.4),
        "predecessor confidence must remain untouched"
    );
}

// --- Successor/predecessor full-field proof + no-gap/no-overlap --------
// (R022 §13-15)

#[test]
fn req_cm_successor_and_predecessor_full_field_proof_no_gap_no_overlap() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_full_field.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let t1: chrono::DateTime<chrono::Utc> = "2026-09-18T10:00:00Z".parse().unwrap();
    let meta1 = ActionMeta {
        actor: Actor::Ui,
        provenance: Provenance::Explicit,
        requested_at: t1,
        confirmed: false,
    };
    let predecessor = match add_memory(&mut conn, meta1, mk_am_payload()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let predecessor_before = predecessor.clone();

    let t2: chrono::DateTime<chrono::Utc> = "2026-09-18T11:30:00Z".parse().unwrap();
    let meta2 = ActionMeta {
        actor: Actor::Ui,
        provenance: Provenance::Explicit,
        requested_at: t2,
        confirmed: false,
    };
    let successor = match correct_memory(&mut conn, meta2, mk_cm_payload(predecessor.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_full_field] FAIL, got {other:?}"),
    };

    let predecessor_after = get_fact(&conn, predecessor.id).unwrap().unwrap();

    assert_eq!(predecessor_after.id, predecessor_before.id);
    assert_eq!(
        predecessor_after.subject_entity_id,
        predecessor_before.subject_entity_id
    );
    assert_eq!(predecessor_after.predicate, predecessor_before.predicate);
    assert_eq!(predecessor_after.value, predecessor_before.value);
    assert_eq!(
        predecessor_after.memory_type,
        predecessor_before.memory_type
    );
    assert_eq!(predecessor_after.confidence, predecessor_before.confidence);
    assert_eq!(
        predecessor_after.sensitivity,
        predecessor_before.sensitivity
    );
    assert_eq!(predecessor_after.valid_from, predecessor_before.valid_from);
    assert_eq!(predecessor_after.created_at, predecessor_before.created_at);

    assert_ne!(
        predecessor_after.valid_until,
        predecessor_before.valid_until
    );
    assert_ne!(
        predecessor_after.superseded_by_fact_id,
        predecessor_before.superseded_by_fact_id
    );
    assert_ne!(
        predecessor_after.epistemic_state,
        predecessor_before.epistemic_state
    );
    assert_ne!(predecessor_after.updated_at, predecessor_before.updated_at);

    assert_eq!(predecessor_after.valid_until, Some(t2));
    assert_eq!(predecessor_after.superseded_by_fact_id, Some(successor.id));
    assert_eq!(predecessor_after.epistemic_state, EpistemicState::Stale);
    assert_eq!(predecessor_after.updated_at, t2);

    assert_eq!(predecessor_after.valid_until, Some(successor.valid_from));
    assert_eq!(successor.valid_from, t2);
    assert_eq!(successor.superseded_by_fact_id, None);
}

// --- Correction chain A -> B -> C (R022 §16/§24) ------------------------

#[test]
fn req_cm_correction_chain_a_to_b_to_c() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_chain.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let t0: chrono::DateTime<chrono::Utc> = "2026-09-18T09:00:00Z".parse().unwrap();
    let meta0 = ActionMeta {
        actor: Actor::Ui,
        provenance: Provenance::Explicit,
        requested_at: t0,
        confirmed: false,
    };
    let a = match add_memory(&mut conn, meta0, mk_am_payload()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup A failed: {other:?}"),
    };

    let t1: chrono::DateTime<chrono::Utc> = "2026-09-18T10:00:00Z".parse().unwrap();
    let meta1 = ActionMeta {
        actor: Actor::Ui,
        provenance: Provenance::Explicit,
        requested_at: t1,
        confirmed: false,
    };
    let b = match correct_memory(&mut conn, meta1, mk_cm_payload(a.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_chain] FAIL A->B, got {other:?}"),
    };

    let t2: chrono::DateTime<chrono::Utc> = "2026-09-18T11:00:00Z".parse().unwrap();
    let meta2 = ActionMeta {
        actor: Actor::Ui,
        provenance: Provenance::Explicit,
        requested_at: t2,
        confirmed: false,
    };
    let c = match correct_memory(&mut conn, meta2, mk_cm_payload(b.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_chain] FAIL B->C, got {other:?}"),
    };

    let a_final = get_fact(&conn, a.id).unwrap().unwrap();
    let b_final = get_fact(&conn, b.id).unwrap().unwrap();
    let c_final = get_fact(&conn, c.id).unwrap().unwrap();

    assert_eq!(a_final.epistemic_state, EpistemicState::Stale);
    assert_eq!(a_final.valid_until, Some(t1));
    assert_eq!(a_final.superseded_by_fact_id, Some(b.id));

    assert_eq!(b_final.epistemic_state, EpistemicState::Stale);
    assert_eq!(b_final.valid_from, t1);
    assert_eq!(b_final.valid_until, Some(t2));
    assert_eq!(b_final.superseded_by_fact_id, Some(c.id));

    assert_eq!(c_final.epistemic_state, EpistemicState::UserConfirmed);
    assert_eq!(c_final.valid_from, t2);
    assert_eq!(c_final.valid_until, None);
    assert_eq!(c_final.superseded_by_fact_id, None);

    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(a.id)) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::FactNotActive))
        }
        other => {
            panic!("[req_cm_chain] FAIL - correcting A again should be rejected, got {other:?}")
        }
    }
    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(b.id)) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::FactNotActive))
        }
        other => {
            panic!("[req_cm_chain] FAIL - correcting B again should be rejected, got {other:?}")
        }
    }
}

// --- Contradiction is not correction (R022 §25) -------------------------

#[test]
fn req_cm_sibling_fact_with_same_subject_predicate_remains_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_sibling.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fact1 = match add_memory(&mut conn, ui_explicit(), mk_am_payload()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup fact1 failed: {other:?}"),
    };
    let fact2 = match add_memory(&mut conn, ui_explicit(), mk_am_payload()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup fact2 failed: {other:?}"),
    };
    assert_eq!(fact1.subject_entity_id, fact2.subject_entity_id);
    assert_eq!(fact1.predicate, fact2.predicate);
    assert_ne!(fact1.id, fact2.id);
    let fact2_before = fact2.clone();

    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(fact1.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_cm_sibling] FAIL, got {other:?}"),
    }

    let fact2_after = get_fact(&conn, fact2.id).unwrap().unwrap();
    assert_eq!(
        fact2_before, fact2_after,
        "[req_cm_sibling] FAIL - a sibling Fact sharing subject+predicate must remain byte-for-byte unchanged"
    );
}

// --- Mandatory previous_snapshot (R022 §17) -----------------------------

#[test]
fn req_cm_previous_snapshot_mandatory_and_deserializes_exactly_to_pre_correction_predecessor() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_snapshot.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);
    let predecessor_pre_correction = predecessor.clone();

    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(predecessor.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_cm_snapshot] FAIL, got {other:?}"),
    }

    let previous_snapshot: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'CorrectMemory'",
            rusqlite::params![predecessor.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let snapshot_json = previous_snapshot
        .expect("[req_cm_snapshot] FAIL - previous_snapshot must be non-NULL for CorrectMemory");
    let deserialized: Fact =
        serde_json::from_str(&snapshot_json).expect("snapshot must deserialize as a Fact");
    assert_eq!(
        deserialized, predecessor_pre_correction,
        "[req_cm_snapshot] FAIL - snapshot must deserialize exactly to the predecessor's PRE-correction state"
    );
}

// --- Audit fact_id rationale (R022 §18/§27/§28) -------------------------

#[test]
fn req_cm_audit_fact_id_is_predecessor_id_with_correct_fields() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_audit.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);
    let predecessor_pre_correction = predecessor.clone();

    // A deliberately known actor/provenance/requested_at (post-run audit
    // hardening, 2026-09-18) - not ui_explicit()'s implicit Actor::Ui, so
    // `actor` and `occurred_at` can be proven equal to exact expected
    // values rather than merely present.
    let requested_at: chrono::DateTime<chrono::Utc> = "2026-09-18T15:20:00Z"
        .parse()
        .expect("valid RFC3339 literal");
    let meta = ActionMeta {
        actor: Actor::CloudLlm,
        provenance: Provenance::Explicit,
        requested_at,
        confirmed: false,
    };

    let successor = match correct_memory(&mut conn, meta, mk_cm_payload(predecessor.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_audit] FAIL, got {other:?}"),
    };

    let (fact_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at): (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT fact_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at
             FROM fact_audit_log WHERE action_type = 'CorrectMemory'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap();
    assert_eq!(
        fact_id,
        predecessor.id.to_string(),
        "[req_cm_audit] FAIL - audit fact_id must be the PREDECESSOR id, not the successor's"
    );
    assert_ne!(fact_id, successor.id.to_string());
    assert_eq!(action_type, "CorrectMemory");
    assert_eq!(
        actor, "CloudLLM",
        "[req_cm_audit] FAIL - audit actor must match the exact ActionMeta.actor supplied"
    );
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "IMPORTANT_CHANGE");
    assert_eq!(
        occurred_at,
        requested_at.to_rfc3339(),
        "[req_cm_audit] FAIL - fact_audit_log.occurred_at must equal ActionMeta.requested_at exactly"
    );
    let snapshot_json = previous_snapshot
        .expect("[req_cm_audit] FAIL - previous_snapshot must be non-NULL for CorrectMemory");
    let deserialized: Fact =
        serde_json::from_str(&snapshot_json).expect("snapshot must deserialize as a Fact");
    assert_eq!(
        deserialized, predecessor_pre_correction,
        "[req_cm_audit] FAIL - previous_snapshot must deserialize exactly to the predecessor's PRE-correction state"
    );
}

// --- Full atomic rollback (R022 - central acceptance proof, §30) -------

#[test]
fn req_cm_rollback_forced_audit_failure_rolls_back_everything() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_rollback1.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);
    let predecessor_before = predecessor.clone();
    let snapshot_before = am_full_table_snapshot(&conn);

    install_fact_audit_failure_trigger(&conn);

    let payload = CorrectMemoryPayload {
        value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
            entity_type: "place".to_string(),
            canonical_name: "Rollback CM Object".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        })),
        ..mk_cm_payload(predecessor.id)
    };
    match correct_memory(&mut conn, ui_explicit(), payload) {
        ActionOutcome::StorageError(_) => {}
        other => panic!("[req_cm_rollback1] FAIL - expected StorageError, got {other:?}"),
    }

    remove_fact_audit_failure_trigger(&conn);

    assert_eq!(
        am_full_table_snapshot(&conn),
        snapshot_before,
        "[req_cm_rollback1] FAIL - every AddMemory/CorrectMemory-mutated table must be exactly as before the failed correction (new object Entity/Source/successor Fact/Evidence/link/audit all absent)"
    );
    let predecessor_after = get_fact(&conn, predecessor.id).unwrap().unwrap();
    assert_eq!(
        predecessor_after, predecessor_before,
        "[req_cm_rollback1] FAIL - the predecessor update must also roll back completely"
    );
    assert!(
        is_active_fact(&predecessor_after),
        "[req_cm_rollback1] FAIL - predecessor must still be active after rollback"
    );

    match add_memory(&mut conn, ui_explicit(), mk_am_payload()) {
        ActionOutcome::Success { .. } => {}
        other => panic!(
            "[req_cm_rollback1] FAIL - connection should remain usable after rollback, got {other:?}"
        ),
    }
}

/// An earlier-stage failure (before the predecessor-update/audit steps)
/// - the same rollback guarantee must hold regardless of which insert in
/// the sequence fails.
#[test]
fn req_cm_rollback_earlier_stage_forced_evidence_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_rollback2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);
    let predecessor_before = predecessor.clone();
    let snapshot_before = am_full_table_snapshot(&conn);

    install_evidence_insert_failure_trigger(&conn);
    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(predecessor.id)) {
        ActionOutcome::StorageError(_) => {}
        other => panic!("[req_cm_rollback2] FAIL - expected StorageError, got {other:?}"),
    }
    remove_evidence_insert_failure_trigger(&conn);

    assert_eq!(am_full_table_snapshot(&conn), snapshot_before);
    let predecessor_after = get_fact(&conn, predecessor.id).unwrap().unwrap();
    assert_eq!(
        predecessor_after, predecessor_before,
        "[req_cm_rollback2] FAIL - predecessor must remain unchanged even when failure happens before its own update step"
    );
}

/// Post-run temporal-integrity hardening (2026-09-18): forces
/// `update_fact_as_superseded`'s own new runtime guard to fail for
/// real, inside a genuine `correct_memory` call, and proves the whole
/// transaction - including the trigger's own side effect - rolls back.
/// A test-only trigger (no production failure flag) closes the exact
/// predecessor row immediately after the Evidence insert - the step
/// right before `update_fact_as_superseded` runs in the real
/// transaction order - so by the time the guarded `UPDATE ... WHERE
/// valid_until IS NULL AND epistemic_state <> 'STALE'` executes, it
/// matches zero rows and returns the new typed error.
#[test]
fn req_cm_rollback_predecessor_update_guard_rejects_and_rolls_back() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_rollback3.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);
    let predecessor_before = predecessor.clone();
    let snapshot_before = am_full_table_snapshot(&conn);

    conn.execute_batch(&format!(
        "CREATE TRIGGER force_close_predecessor_before_update
         AFTER INSERT ON evidence
         BEGIN
             UPDATE facts SET valid_until = '2000-01-01T00:00:00+00:00', epistemic_state = 'STALE'
             WHERE id = '{}';
         END;",
        predecessor.id
    ))
    .unwrap();

    let payload = CorrectMemoryPayload {
        value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
            entity_type: "place".to_string(),
            canonical_name: "Guard Rollback Object".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        })),
        ..mk_cm_payload(predecessor.id)
    };
    match correct_memory(&mut conn, ui_explicit(), payload) {
        ActionOutcome::StorageError(_) => {}
        other => panic!(
            "[req_cm_rollback3] FAIL - expected StorageError from the predecessor-update guard, got {other:?}"
        ),
    }

    conn.execute_batch("DROP TRIGGER IF EXISTS force_close_predecessor_before_update;")
        .unwrap();

    assert_eq!(
        am_full_table_snapshot(&conn),
        snapshot_before,
        "[req_cm_rollback3] FAIL - the whole transaction, including the trigger's own side effect, must roll back"
    );
    let predecessor_after = get_fact(&conn, predecessor.id).unwrap().unwrap();
    assert_eq!(
        predecessor_after, predecessor_before,
        "[req_cm_rollback3] FAIL - predecessor must remain completely unchanged after rollback"
    );
}

// --- update_fact_as_superseded direct repo-level proof (post-run -------
// temporal-integrity hardening, 2026-09-18) - proves RELEASE-build
// behavior (a real runtime `Result`, never a `debug_assert!` that
// compiles out), calling the repo function directly rather than only
// through `correct_memory`.

#[test]
fn req_cm_repo_update_fact_as_superseded_active_predecessor_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_repo_update_a.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);
    // superseded_by_fact_id is a real FK (`REFERENCES facts(id)`) - a
    // real successor row must exist first, exactly as `correct_memory`
    // always inserts one before calling this function.
    let successor = mk_cm_predecessor(&mut conn);
    let ts = Utc::now();

    let result = storage::personal_knowledge_repo::update_fact_as_superseded(
        &conn,
        predecessor.id,
        successor.id,
        ts,
    );
    assert!(
        result.is_ok(),
        "[req_cm_repo_update_a] FAIL - active predecessor update should succeed, got {result:?}"
    );

    let after = get_fact(&conn, predecessor.id).unwrap().unwrap();
    assert_eq!(after.valid_until, Some(ts));
    assert_eq!(after.superseded_by_fact_id, Some(successor.id));
    assert_eq!(after.epistemic_state, EpistemicState::Stale);
    assert_eq!(after.updated_at, ts);
}

#[test]
fn req_cm_repo_update_fact_as_superseded_stale_predecessor_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_repo_update_b.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);
    conn.execute(
        "UPDATE facts SET epistemic_state = 'STALE' WHERE id = ?1",
        rusqlite::params![predecessor.id.to_string()],
    )
    .unwrap();
    let before = get_fact(&conn, predecessor.id).unwrap().unwrap();

    let result = storage::personal_knowledge_repo::update_fact_as_superseded(
        &conn,
        predecessor.id,
        FactId::new(),
        Utc::now(),
    );
    assert!(
        result.is_err(),
        "[req_cm_repo_update_b] FAIL - STALE predecessor update should be rejected at runtime, got {result:?}"
    );

    let after = get_fact(&conn, predecessor.id).unwrap().unwrap();
    assert_eq!(
        before, after,
        "[req_cm_repo_update_b] FAIL - no field should change on a rejected update"
    );
}

#[test]
fn req_cm_repo_update_fact_as_superseded_closed_predecessor_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_repo_update_c.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);
    conn.execute(
        "UPDATE facts SET valid_until = ?2 WHERE id = ?1",
        rusqlite::params![predecessor.id.to_string(), Utc::now().to_rfc3339()],
    )
    .unwrap();
    let before = get_fact(&conn, predecessor.id).unwrap().unwrap();

    let result = storage::personal_knowledge_repo::update_fact_as_superseded(
        &conn,
        predecessor.id,
        FactId::new(),
        Utc::now(),
    );
    assert!(
        result.is_err(),
        "[req_cm_repo_update_c] FAIL - closed (valid_until set) predecessor update should be rejected, got {result:?}"
    );

    let after = get_fact(&conn, predecessor.id).unwrap().unwrap();
    assert_eq!(before, after);
}

#[test]
fn req_cm_repo_update_fact_as_superseded_nonexistent_fact_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_repo_update_d.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let result = storage::personal_knowledge_repo::update_fact_as_superseded(
        &conn,
        FactId::new(),
        FactId::new(),
        Utc::now(),
    );
    assert!(
        result.is_err(),
        "[req_cm_repo_update_d] FAIL - nonexistent predecessor update should be rejected, not silently Ok, got {result:?}"
    );
}

// --- Existing Entity/Source reuse immutability (R022 §32) --------------

#[test]
fn req_cm_existing_entity_and_source_untouched_on_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_immut.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let setup = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "person".to_string(),
            canonical_name: "CM Reuse Person".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        }),
        ..mk_am_payload()
    };
    let (reusable_entity_id, reusable_source_id) = match add_memory(&mut conn, ui_explicit(), setup)
    {
        ActionOutcome::Success { value, .. } => {
            let source_id_str: String = conn
                .query_row(
                    "SELECT e.source_id FROM evidence e
                     JOIN fact_evidence fe ON fe.evidence_id = e.id
                     WHERE fe.fact_id = ?1",
                    rusqlite::params![value.id.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
            (
                value.subject_entity_id,
                SourceId::parse(&source_id_str).unwrap(),
            )
        }
        other => panic!("setup failed: {other:?}"),
    };

    let entity_before = get_entity(&conn, reusable_entity_id).unwrap().unwrap();
    let source_before = read_source_fields(&conn, reusable_source_id);

    let predecessor = mk_cm_predecessor(&mut conn);
    let reuse_payload = CorrectMemoryPayload {
        value: FactValueInput::Entity(EntityInputRef::Existing(reusable_entity_id)),
        source: SourceRef::Existing(reusable_source_id),
        ..mk_cm_payload(predecessor.id)
    };
    match correct_memory(&mut conn, ui_explicit(), reuse_payload) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_cm_immut] FAIL, got {other:?}"),
    }

    let entity_after = get_entity(&conn, reusable_entity_id).unwrap().unwrap();
    let source_after = read_source_fields(&conn, reusable_source_id);
    assert_eq!(
        entity_before, entity_after,
        "[req_cm_immut] FAIL - Existing Entity was mutated on reuse"
    );
    assert_eq!(
        source_before, source_after,
        "[req_cm_immut] FAIL - Existing Source was mutated on reuse"
    );
}

// --- Predecessor Evidence/Source stay linked; successor gets new -------
// Evidence (R022 §9/§39)

#[test]
fn req_cm_predecessor_evidence_and_source_remain_linked_successor_gets_new_evidence() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_evidence.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);

    let predecessor_evidence_id: String = conn
        .query_row(
            "SELECT evidence_id FROM fact_evidence WHERE fact_id = ?1",
            rusqlite::params![predecessor.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();

    let successor = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(predecessor.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_evidence] FAIL, got {other:?}"),
    };

    let predecessor_evidence_id_after: String = conn
        .query_row(
            "SELECT evidence_id FROM fact_evidence WHERE fact_id = ?1",
            rusqlite::params![predecessor.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        predecessor_evidence_id, predecessor_evidence_id_after,
        "[req_cm_evidence] FAIL - predecessor's original Evidence link must remain untouched"
    );

    let successor_evidence_id: String = conn
        .query_row(
            "SELECT evidence_id FROM fact_evidence WHERE fact_id = ?1",
            rusqlite::params![successor.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(
        predecessor_evidence_id, successor_evidence_id,
        "[req_cm_evidence] FAIL - successor must receive a brand-new Evidence row, never share the predecessor's"
    );

    assert_eq!(am_evidence_row_count(&conn), 2);
    assert_eq!(am_fact_evidence_row_count(&conn), 2);
}

// --- Read-side persistence proofs (R022 §40) ----------------------------

#[test]
fn req_cm_read_side_persisted_state_via_get_fact() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_read_side.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);

    let successor = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(predecessor.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cm_read_side] FAIL, got {other:?}"),
    };

    let predecessor_read = get_fact(&conn, predecessor.id)
        .unwrap()
        .expect("predecessor must still be readable");
    assert_eq!(predecessor_read.epistemic_state, EpistemicState::Stale);
    assert!(predecessor_read.valid_until.is_some());

    let successor_read = get_fact(&conn, successor.id)
        .unwrap()
        .expect("successor must be readable");
    assert_eq!(
        successor_read.epistemic_state,
        EpistemicState::UserConfirmed
    );
    assert!(successor_read.valid_until.is_none());
    assert_eq!(
        successor_read, successor,
        "get_fact-read successor must match the in-memory returned struct exactly"
    );
}

// --- No Relation/Episode side effects (R022 §44) ------------------------

#[test]
fn req_cm_no_relation_episode_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cm_no_side.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let predecessor = mk_cm_predecessor(&mut conn);
    match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(predecessor.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }
    assert_eq!(
        relation_episode_side_effect_counts(&conn),
        (0, 0, 0, 0, 0),
        "[req_cm_no_side] FAIL - CorrectMemory must never write to relations/episodes/relation_evidence/episode_evidence/episode_entities"
    );
}

// =========================================================================
// M2 Personal Knowledge Slice D - ConfirmMemory (2026-09-18)
//
// Evidence for
// `docs/reports/R023_M2_PERSONAL_KNOWLEDGE_CONFIRMMEMORY_SLICE_D_2026-09-18.md`.
// The third production Personal Knowledge mutation - a pure epistemic
// confirmation of one existing Fact in place, never a new/successor
// Fact. Database-dependent behavior only - pure `validate_confirm_
// memory` is trivial and tested directly in `actions::tests` instead.
// =========================================================================

fn mk_cfm_payload(fact_id: FactId) -> ConfirmMemoryPayload {
    ConfirmMemoryPayload { fact_id }
}

/// Creates a fresh, active target `Fact` with a caller-chosen
/// `epistemic_state`, via `add_memory` with `Provenance::Explicit` (an
/// explicit call persists exactly the requested `epistemic_state`
/// unchanged, per R018 §5 - the standard fixture every `ConfirmMemory`
/// test starts from).
fn mk_cfm_target(conn: &mut Connection, epistemic_state: EpistemicState) -> Fact {
    let payload = AddMemoryPayload {
        epistemic_state,
        ..mk_am_payload()
    };
    match add_memory(conn, ui_explicit(), payload) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[cfm target setup] FAIL - add_memory failed: {other:?}"),
    }
}

// --- Confirmation matrix (R023 §19/§24) ---------------------------------

#[test]
fn req_cfm_conf_a_explicit_unconfirmed_executes_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_conf_a.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_cfm_target(&mut conn, EpistemicState::Inferred);

    match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(target.id)) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::ExplicitAdd);
            assert_eq!(value.epistemic_state, EpistemicState::UserConfirmed);
            assert_eq!(value.id, target.id);
        }
        other => panic!("[req_cfm_conf_a] FAIL - expected Success, got {other:?}"),
    }
}

/// The generic `effective_class`/`blocks_until_confirmed` ladder alone
/// already blocks an inferred, unconfirmed proposal at `ConfirmMemory`'s
/// `ExplicitAdd` floor - no Personal-Knowledge-specific overlay exists
/// for it (unlike `CorrectMemory`'s `ImportantChange` floor). Uses a
/// NONEXISTENT `FactId` to prove the gate precedes the target lookup
/// entirely.
#[test]
fn req_cfm_conf_b_inferred_unconfirmed_blocks_before_fact_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_conf_b.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let nonexistent = FactId::new();
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    match confirm_memory(&mut conn, meta, mk_cfm_payload(nonexistent)) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
        }
        other => panic!(
            "[req_cfm_conf_b] FAIL - expected ConfirmationRequired (gate must precede the Fact lookup), got {other:?}"
        ),
    }
    assert_eq!(
        am_full_table_snapshot(&conn),
        (1, 0, 0, 0, 0, 0),
        "[req_cfm_conf_b] FAIL - zero writes, only the bootstrap SELF entity should exist"
    );
}

#[test]
fn req_cfm_conf_c_inferred_confirmed_executes_and_audit_provenance_remains_inferred() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_conf_c.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_cfm_target(&mut conn, EpistemicState::Observed);

    let mut meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    meta.confirmed = true;
    let confirmed = match confirm_memory(&mut conn, meta, mk_cfm_payload(target.id)) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
            value
        }
        other => panic!("[req_cfm_conf_c] FAIL - expected Success, got {other:?}"),
    };
    assert_eq!(confirmed.epistemic_state, EpistemicState::UserConfirmed);

    let provenance: String = conn
        .query_row(
            "SELECT provenance FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'ConfirmMemory'",
            rusqlite::params![target.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        provenance, "Inferred",
        "[req_cfm_conf_c] FAIL - confirming an inferred proposal must never silently rewrite provenance to Explicit"
    );
}

#[test]
fn req_cfm_conf_d_explicit_missing_fact_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_conf_d.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(FactId::new())) {
        ActionOutcome::NotFound => {}
        other => panic!("[req_cfm_conf_d] FAIL - expected NotFound, got {other:?}"),
    }
}

// --- State transition matrix (R023 §7/§8/§9/§20/§25) --------------------

/// `INFERRED`/`OBSERVED`/`DISPUTED` are all confirmable; already-
/// `USER_CONFIRMED` is rejected as a no-op; `STALE` and a closed
/// active-looking raw fixture are both rejected via `is_active_fact`.
#[test]
fn req_cfm_state_transition_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_transition.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    for state in [
        EpistemicState::Inferred,
        EpistemicState::Observed,
        EpistemicState::Disputed,
    ] {
        let target = mk_cfm_target(&mut conn, state);
        match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(target.id)) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(
                    value.epistemic_state,
                    EpistemicState::UserConfirmed,
                    "{state:?} should confirm to UserConfirmed"
                );
                assert_eq!(value.id, target.id, "no new Fact id should be produced");
            }
            other => panic!("[req_cfm_transition] FAIL for {state:?}, got {other:?}"),
        }
    }

    let already_confirmed = mk_cfm_target(&mut conn, EpistemicState::UserConfirmed);
    match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(already_confirmed.id)) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::FactAlreadyUserConfirmed));
        }
        other => panic!(
            "[req_cfm_transition] FAIL - already-confirmed should be rejected as a no-op, got {other:?}"
        ),
    }

    let stale_target = mk_cfm_target(&mut conn, EpistemicState::Inferred);
    conn.execute(
        "UPDATE facts SET epistemic_state = 'STALE' WHERE id = ?1",
        rusqlite::params![stale_target.id.to_string()],
    )
    .unwrap();
    match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(stale_target.id)) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::FactNotActive));
        }
        other => panic!("[req_cfm_transition] FAIL - STALE should be rejected, got {other:?}"),
    }

    let closed_target = mk_cfm_target(&mut conn, EpistemicState::Observed);
    conn.execute(
        "UPDATE facts SET valid_until = ?2 WHERE id = ?1",
        rusqlite::params![closed_target.id.to_string(), Utc::now().to_rfc3339()],
    )
    .unwrap();
    match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(closed_target.id)) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::FactNotActive));
        }
        other => panic!(
            "[req_cfm_transition] FAIL - closed active-looking fixture should be rejected, got {other:?}"
        ),
    }
}

// --- Identity / no temporal successor (R023 §21) ------------------------

#[test]
fn req_cfm_identity_no_temporal_successor() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_identity.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_cfm_target(&mut conn, EpistemicState::Inferred);
    let facts_before = am_fact_row_count(&conn);

    let confirmed = match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(target.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cfm_identity] FAIL, got {other:?}"),
    };

    assert_eq!(confirmed.id, target.id);
    assert_eq!(
        am_fact_row_count(&conn),
        facts_before,
        "[req_cfm_identity] FAIL - no successor Fact should be created"
    );
    assert_eq!(confirmed.valid_from, target.valid_from);
    assert_eq!(confirmed.valid_until, target.valid_until);
    assert_eq!(
        confirmed.superseded_by_fact_id,
        target.superseded_by_fact_id
    );
}

// --- Mutation contract full-field proof (R023 §10) ----------------------

#[test]
fn req_cfm_mutation_contract_full_field_proof() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_mutation.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_cfm_target(&mut conn, EpistemicState::Disputed);
    let target_before = target.clone();

    let confirmed = match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(target.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cfm_mutation] FAIL, got {other:?}"),
    };

    let read_back = get_fact(&conn, target.id).unwrap().unwrap();
    assert_eq!(read_back, confirmed);

    assert_eq!(confirmed.id, target_before.id);
    assert_eq!(confirmed.subject_entity_id, target_before.subject_entity_id);
    assert_eq!(confirmed.predicate, target_before.predicate);
    assert_eq!(confirmed.value, target_before.value);
    assert_eq!(confirmed.memory_type, target_before.memory_type);
    assert_eq!(confirmed.confidence, target_before.confidence);
    assert_eq!(confirmed.sensitivity, target_before.sensitivity);
    assert_eq!(confirmed.valid_from, target_before.valid_from);
    assert_eq!(confirmed.valid_until, target_before.valid_until);
    assert_eq!(
        confirmed.superseded_by_fact_id,
        target_before.superseded_by_fact_id
    );
    assert_eq!(confirmed.created_at, target_before.created_at);

    assert_ne!(confirmed.epistemic_state, target_before.epistemic_state);
    assert_ne!(confirmed.updated_at, target_before.updated_at);
    assert_eq!(confirmed.epistemic_state, EpistemicState::UserConfirmed);
}

// --- No Entity/Source/Evidence side effects (R023 §22) ------------------

#[test]
fn req_cfm_no_entity_source_evidence_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_no_side.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_cfm_target(&mut conn, EpistemicState::Inferred);
    let snapshot_before = am_full_table_snapshot(&conn);

    match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(target.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_cfm_no_side] FAIL, got {other:?}"),
    }

    let snapshot_after = am_full_table_snapshot(&conn);
    assert_eq!(snapshot_after.0, snapshot_before.0, "entities unchanged");
    assert_eq!(snapshot_after.1, snapshot_before.1, "sources unchanged");
    assert_eq!(snapshot_after.2, snapshot_before.2, "evidence unchanged");
    assert_eq!(
        snapshot_after.3, snapshot_before.3,
        "facts row count unchanged - in-place mutation, no successor"
    );
    assert_eq!(
        snapshot_after.4, snapshot_before.4,
        "fact_evidence unchanged"
    );
    assert_eq!(
        snapshot_after.5,
        snapshot_before.5 + 1,
        "exactly one new audit row"
    );
}

// --- No Relation/Episode side effects (R023 §23) ------------------------

#[test]
fn req_cfm_no_relation_episode_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_no_rel_side.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_cfm_target(&mut conn, EpistemicState::Inferred);
    match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(target.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup failed: {other:?}"),
    }
    assert_eq!(
        relation_episode_side_effect_counts(&conn),
        (0, 0, 0, 0, 0),
        "[req_cfm_no_rel_side] FAIL - ConfirmMemory must never write to relations/episodes/relation_evidence/episode_evidence/episode_entities"
    );
}

// --- Audit fields, mandatory snapshot exactness, timestamp proof -------
// (R023 §14/§19/§29/§30)

#[test]
fn req_cfm_audit_fields_and_previous_snapshot_exactness() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_audit.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_cfm_target(&mut conn, EpistemicState::Inferred);
    let target_pre_confirmation = target.clone();

    let requested_at: chrono::DateTime<chrono::Utc> = "2026-09-18T21:15:00Z"
        .parse()
        .expect("valid RFC3339 literal");
    let meta = ActionMeta {
        actor: Actor::CloudLlm,
        provenance: Provenance::Explicit,
        requested_at,
        confirmed: false,
    };
    match confirm_memory(&mut conn, meta, mk_cfm_payload(target.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_cfm_audit] FAIL, got {other:?}"),
    }

    let (
        fact_id,
        action_type,
        actor,
        provenance,
        confirmation_class,
        previous_snapshot,
        occurred_at,
    ): (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT fact_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at
             FROM fact_audit_log WHERE action_type = 'ConfirmMemory'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap();

    assert_eq!(fact_id, target.id.to_string());
    assert_eq!(action_type, "ConfirmMemory");
    assert_eq!(
        actor, "CloudLLM",
        "[req_cfm_audit] FAIL - audit actor must match the exact ActionMeta.actor supplied"
    );
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "EXPLICIT_ADD");
    assert_eq!(
        occurred_at,
        requested_at.to_rfc3339(),
        "[req_cfm_audit] FAIL - occurred_at must equal ActionMeta.requested_at exactly"
    );

    let snapshot_json = previous_snapshot
        .expect("[req_cfm_audit] FAIL - previous_snapshot must be non-NULL for ConfirmMemory");
    let deserialized: Fact =
        serde_json::from_str(&snapshot_json).expect("snapshot must deserialize as a Fact");
    assert_eq!(
        deserialized, target_pre_confirmation,
        "[req_cfm_audit] FAIL - previous_snapshot must deserialize exactly to the PRE-confirmation Fact"
    );

    let current = get_fact(&conn, target.id).unwrap().unwrap();
    assert_eq!(current.epistemic_state, EpistemicState::UserConfirmed);
    assert_eq!(
        current.updated_at, requested_at,
        "[req_cfm_audit] FAIL - updated_at must equal ActionMeta.requested_at exactly"
    );
}

// --- DISPUTED sibling proof (R023 §9/§31) -------------------------------

/// R018's critical clarification: confirming one `DISPUTED` Fact never
/// touches a sibling Fact sharing the same subject+predicate - no
/// contradiction detection, no global "resolution" of a contradiction
/// set. `ConfirmMemory` confirms exactly one assertion.
#[test]
fn req_cfm_disputed_sibling_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_sibling.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let fact_a = mk_cfm_target(&mut conn, EpistemicState::Disputed);
    let fact_b_payload = AddMemoryPayload {
        subject: EntityInputRef::Existing(fact_a.subject_entity_id),
        predicate: fact_a.predicate.clone(),
        value: FactValueInput::ScalarJson("\"a different value\"".to_string()),
        epistemic_state: EpistemicState::Disputed,
        ..mk_am_payload()
    };
    let fact_b = match add_memory(&mut conn, ui_explicit(), fact_b_payload) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup fact_b failed: {other:?}"),
    };
    assert_eq!(fact_a.subject_entity_id, fact_b.subject_entity_id);
    assert_eq!(fact_a.predicate, fact_b.predicate);
    assert_ne!(fact_a.value, fact_b.value);
    let fact_b_before = fact_b.clone();

    match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(fact_a.id)) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.epistemic_state, EpistemicState::UserConfirmed);
        }
        other => panic!("[req_cfm_sibling] FAIL, got {other:?}"),
    }

    let fact_b_after = get_fact(&conn, fact_b.id).unwrap().unwrap();
    assert_eq!(
        fact_b_before, fact_b_after,
        "[req_cfm_sibling] FAIL - a sibling DISPUTED Fact sharing subject+predicate must remain byte-for-byte unchanged - ConfirmMemory does not globally resolve a contradiction"
    );
}

// --- Full atomic rollback (R023 - central acceptance proof, §17/§21) ---

/// Mandatory central proof: forces the `fact_audit_log` insert to fail
/// (the same reused trigger `AddMemory`/`CorrectMemory` already use)
/// inside a real `confirm_memory` call, and proves the Fact update
/// rolls back completely - no partial confirmation.
#[test]
fn req_cfm_rollback_forced_audit_failure_rolls_back_fact_state() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_rollback1.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_cfm_target(&mut conn, EpistemicState::Inferred);
    let target_before = target.clone();

    install_fact_audit_failure_trigger(&conn);
    match confirm_memory(&mut conn, ui_explicit(), mk_cfm_payload(target.id)) {
        ActionOutcome::StorageError(_) => {}
        other => panic!("[req_cfm_rollback1] FAIL - expected StorageError, got {other:?}"),
    }
    remove_fact_audit_failure_trigger(&conn);

    let target_after = get_fact(&conn, target.id).unwrap().unwrap();
    assert_eq!(
        target_after, target_before,
        "[req_cfm_rollback1] FAIL - Fact must remain exactly unchanged after rollback, including epistemic_state and updated_at"
    );

    let audit_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'ConfirmMemory'",
            rusqlite::params![target.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        audit_count, 0,
        "[req_cfm_rollback1] FAIL - no ConfirmMemory audit row should exist"
    );

    match add_memory(&mut conn, ui_explicit(), mk_am_payload()) {
        ActionOutcome::Success { .. } => {}
        other => panic!(
            "[req_cfm_rollback1] FAIL - connection should remain usable after rollback, got {other:?}"
        ),
    }
}

// --- Guarded-update direct repo-level test matrix (R023 §11/§12/§18) ---
//
// `confirm_fact_epistemic_state`'s own `Result`-returning behavior,
// proven directly (not only through `confirm_memory`) - matches R022's
// `update_fact_as_superseded` repo-level proof precedent exactly. A
// genuine service-level "guard fires mid-transaction" race proof (R022
// §22's style) is not attempted here: `ConfirmMemory`'s transaction has
// only two steps (the guarded update, then the audit insert) with no
// earlier insert to hook a same-transaction trigger onto before the
// guarded update runs, unlike `CorrectMemory`'s six-step transaction -
// there is no legitimate seam for it. The forced-audit-failure rollback
// test above remains this action's mandatory central rollback proof.
#[test]
fn req_cfm_repo_confirm_fact_epistemic_state_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cfm_repo_matrix.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    for state in [
        EpistemicState::Inferred,
        EpistemicState::Observed,
        EpistemicState::Disputed,
    ] {
        let target = mk_cfm_target(&mut conn, state);
        let result = storage::personal_knowledge_repo::confirm_fact_epistemic_state(
            &conn,
            target.id,
            Utc::now(),
        );
        assert!(
            result.is_ok(),
            "[req_cfm_repo_matrix] FAIL - {state:?} should succeed, got {result:?}"
        );
        let after = get_fact(&conn, target.id).unwrap().unwrap();
        assert_eq!(after.epistemic_state, EpistemicState::UserConfirmed);
    }

    let already_confirmed = mk_cfm_target(&mut conn, EpistemicState::UserConfirmed);
    let before = get_fact(&conn, already_confirmed.id).unwrap().unwrap();
    let result = storage::personal_knowledge_repo::confirm_fact_epistemic_state(
        &conn,
        already_confirmed.id,
        Utc::now(),
    );
    assert!(
        result.is_err(),
        "[req_cfm_repo_matrix] FAIL - USER_CONFIRMED should be rejected, got {result:?}"
    );
    assert_eq!(
        get_fact(&conn, already_confirmed.id).unwrap().unwrap(),
        before
    );

    let stale_target = mk_cfm_target(&mut conn, EpistemicState::Inferred);
    conn.execute(
        "UPDATE facts SET epistemic_state = 'STALE' WHERE id = ?1",
        rusqlite::params![stale_target.id.to_string()],
    )
    .unwrap();
    let before = get_fact(&conn, stale_target.id).unwrap().unwrap();
    let result = storage::personal_knowledge_repo::confirm_fact_epistemic_state(
        &conn,
        stale_target.id,
        Utc::now(),
    );
    assert!(
        result.is_err(),
        "[req_cfm_repo_matrix] FAIL - STALE should be rejected, got {result:?}"
    );
    assert_eq!(get_fact(&conn, stale_target.id).unwrap().unwrap(), before);

    let closed_target = mk_cfm_target(&mut conn, EpistemicState::Observed);
    conn.execute(
        "UPDATE facts SET valid_until = ?2 WHERE id = ?1",
        rusqlite::params![closed_target.id.to_string(), Utc::now().to_rfc3339()],
    )
    .unwrap();
    let before = get_fact(&conn, closed_target.id).unwrap().unwrap();
    let result = storage::personal_knowledge_repo::confirm_fact_epistemic_state(
        &conn,
        closed_target.id,
        Utc::now(),
    );
    assert!(
        result.is_err(),
        "[req_cfm_repo_matrix] FAIL - closed active-looking fixture should be rejected, got {result:?}"
    );
    assert_eq!(get_fact(&conn, closed_target.id).unwrap().unwrap(), before);

    let result = storage::personal_knowledge_repo::confirm_fact_epistemic_state(
        &conn,
        FactId::new(),
        Utc::now(),
    );
    assert!(
        result.is_err(),
        "[req_cfm_repo_matrix] FAIL - missing FactId should be rejected, not silently Ok, got {result:?}"
    );
}

// =========================================================================
// M2 Personal Knowledge Slice E - ForgetMemory (2026-09-18)
//
// Evidence for
// `docs/reports/R024_M2_PERSONAL_KNOWLEDGE_FORGETMEMORY_SLICE_E_2026-09-18.md`.
// The fourth production Personal Knowledge mutation, and the first
// hard-DELETE action - R019's final 14-step privacy-safe algorithm.
// Database-dependent behavior only - the trivial pure `validate_forget_
// memory` is tested directly in `actions::tests` instead.
// =========================================================================

fn mk_fm_payload(fact_id: FactId) -> ForgetMemoryPayload {
    ForgetMemoryPayload { fact_id }
}

/// `ui_explicit()` with `confirmed = true` - `Delete` blocks EVERY
/// unconfirmed call, `Explicit` included, so most success-path
/// `ForgetMemory` tests need this rather than plain `ui_explicit()`.
fn confirmed_explicit() -> ActionMeta {
    let mut meta = ui_explicit();
    meta.confirmed = true;
    meta
}

/// Creates a fresh, standalone active Fact via `add_memory` - the
/// standard fixture most `ForgetMemory` tests start from.
fn mk_fm_target(conn: &mut Connection) -> Fact {
    match add_memory(conn, ui_explicit(), mk_am_payload()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[fm target setup] FAIL - add_memory failed: {other:?}"),
    }
}

/// Creates a brand-new, non-SELF Entity via a scratch `AddMemory` call
/// (`Entity(New(..))` as the value) and returns its id - the Fact that
/// created it is otherwise irrelevant to the caller.
fn mk_fm_second_entity(conn: &mut Connection) -> EntityId {
    let payload = AddMemoryPayload {
        value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
            entity_type: "person".to_string(),
            canonical_name: "FM Second Entity".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        })),
        ..mk_am_payload()
    };
    match add_memory(conn, ui_explicit(), payload) {
        ActionOutcome::Success { value, .. } => match value.value {
            FactValue::EntityRef(id) => id,
            other => panic!("[fm second entity setup] FAIL - expected EntityRef, got {other:?}"),
        },
        other => panic!("[fm second entity setup] FAIL - add_memory failed: {other:?}"),
    }
}

/// Raw fixture - no production write path creates `relations` rows
/// (R019 §13, unchanged by this slice). `from_entity_id`/`to_entity_id`
/// must differ (`CHECK (from_entity_id <> to_entity_id)`).
fn insert_raw_relation(
    conn: &Connection,
    from_entity_id: EntityId,
    to_entity_id: EntityId,
) -> String {
    let id = uuid::Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO relations (id, from_entity_id, to_entity_id, relation_type, epistemic_state, sensitivity, valid_from, created_at)
         VALUES (?1, ?2, ?3, 'test_relation', 'OBSERVED', 'INTERNAL', ?4, ?4)",
        rusqlite::params![id, from_entity_id.to_string(), to_entity_id.to_string(), now],
    )
    .unwrap();
    id
}

/// Raw fixture - no production write path creates `episodes` rows
/// (R019 §13, unchanged by this slice).
fn insert_raw_episode(conn: &Connection) -> String {
    let id = uuid::Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO episodes (id, summary, started_at, sensitivity, created_at, updated_at)
         VALUES (?1, 'test episode', ?2, 'INTERNAL', ?2, ?2)",
        rusqlite::params![id, now],
    )
    .unwrap();
    id
}

/// Forces the FINAL `facts` `DELETE` (step 11) to fail, proving the
/// earlier steps in the same transaction (audit scrub, inbound-
/// supersession clearing, Evidence/Source deletion) all roll back too.
/// A test-only trigger, no production failure flag.
fn install_fact_delete_failure_trigger(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER force_fact_delete_failure
         BEFORE DELETE ON facts
         BEGIN
             SELECT RAISE(FAIL, 'forced fact delete failure');
         END;",
    )
    .expect("failed to install the forced fact-delete-failure trigger");
}

fn remove_fact_delete_failure_trigger(conn: &Connection) {
    conn.execute_batch("DROP TRIGGER IF EXISTS force_fact_delete_failure;")
        .expect("failed to remove the forced fact-delete-failure trigger");
}

// --- Confirmation matrix (R024 §27) -------------------------------------

/// `Delete` blocks EVERY unconfirmed call, `Explicit` included - unlike
/// every other Memory action, there is no "the call itself is the
/// confirmation" path for a hard delete. A nonexistent `FactId` proves
/// the target is never fetched before consent.
#[test]
fn req_fm_conf_a_explicit_unconfirmed_blocks_before_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_conf_a.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    match forget_memory(&mut conn, ui_explicit(), mk_fm_payload(FactId::new())) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::Delete);
        }
        other => {
            panic!("[req_fm_conf_a] FAIL - expected ConfirmationRequired(Delete), got {other:?}")
        }
    }
    assert_eq!(
        am_full_table_snapshot(&conn),
        (1, 0, 0, 0, 0, 0),
        "[req_fm_conf_a] FAIL - zero writes, only the bootstrap SELF entity should exist"
    );
}

#[test]
fn req_fm_conf_b_explicit_confirmed_missing_fact_returns_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_conf_b.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    match forget_memory(
        &mut conn,
        confirmed_explicit(),
        mk_fm_payload(FactId::new()),
    ) {
        ActionOutcome::NotFound => {}
        other => panic!("[req_fm_conf_b] FAIL - expected NotFound, got {other:?}"),
    }
}

#[test]
fn req_fm_conf_c_explicit_confirmed_existing_fact_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_conf_c.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_fm_target(&mut conn);

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target.id)) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(
                value, target.id,
                "[req_fm_conf_c] FAIL - success value must be only the FactId"
            );
            assert_eq!(confirmation_class, ConfirmationClass::Delete);
        }
        other => panic!("[req_fm_conf_c] FAIL, got {other:?}"),
    }
    assert!(get_fact(&conn, target.id).unwrap().is_none());
}

#[test]
fn req_fm_conf_d_inferred_unconfirmed_blocks_before_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_conf_d.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    match forget_memory(&mut conn, meta, mk_fm_payload(FactId::new())) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::Delete);
        }
        other => panic!("[req_fm_conf_d] FAIL, got {other:?}"),
    }
    assert_eq!(am_full_table_snapshot(&conn), (1, 0, 0, 0, 0, 0));
}

#[test]
fn req_fm_conf_e_inferred_confirmed_succeeds_audit_provenance_inferred() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_conf_e.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_fm_target(&mut conn);

    let mut meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    meta.confirmed = true;
    match forget_memory(&mut conn, meta, mk_fm_payload(target.id)) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(value, target.id);
            assert_eq!(confirmation_class, ConfirmationClass::Delete);
        }
        other => panic!("[req_fm_conf_e] FAIL, got {other:?}"),
    }
    let provenance: String = conn
        .query_row(
            "SELECT provenance FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'ForgetMemory'",
            rusqlite::params![target.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        provenance, "Inferred",
        "[req_fm_conf_e] FAIL - confirming an inferred proposal must never silently rewrite provenance to Explicit"
    );
}

// --- Target eligibility: active AND historical (R024 §6) --------------

#[test]
fn req_fm_target_eligibility_active_and_historical() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_eligibility.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    for state in [
        EpistemicState::UserConfirmed,
        EpistemicState::Inferred,
        EpistemicState::Observed,
        EpistemicState::Disputed,
    ] {
        let payload = AddMemoryPayload {
            epistemic_state: state,
            ..mk_am_payload()
        };
        let target = match add_memory(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed for {state:?}: {other:?}"),
        };
        match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target.id)) {
            ActionOutcome::Success { value, .. } => assert_eq!(value, target.id),
            other => panic!("[req_fm_eligibility] FAIL for active {state:?}, got {other:?}"),
        }
        assert!(get_fact(&conn, target.id).unwrap().is_none());
    }

    let stale_target = mk_fm_target(&mut conn);
    conn.execute(
        "UPDATE facts SET epistemic_state = 'STALE', valid_until = ?2 WHERE id = ?1",
        rusqlite::params![stale_target.id.to_string(), Utc::now().to_rfc3339()],
    )
    .unwrap();
    match forget_memory(
        &mut conn,
        confirmed_explicit(),
        mk_fm_payload(stale_target.id),
    ) {
        ActionOutcome::Success { value, .. } => assert_eq!(value, stale_target.id),
        other => panic!("[req_fm_eligibility] FAIL for STALE/historical, got {other:?}"),
    }
    assert!(get_fact(&conn, stale_target.id).unwrap().is_none());
}

// --- Audit marker fields, no target snapshot (R024 §15/§16) ------------

#[test]
fn req_fm_audit_marker_fields_and_no_target_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_audit.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_fm_target(&mut conn);

    let requested_at: chrono::DateTime<chrono::Utc> = "2026-09-18T22:00:00Z"
        .parse()
        .expect("valid RFC3339 literal");
    let meta = ActionMeta {
        actor: Actor::CloudLlm,
        provenance: Provenance::Explicit,
        requested_at,
        confirmed: true,
    };
    match forget_memory(&mut conn, meta, mk_fm_payload(target.id)) {
        ActionOutcome::Success { value, .. } => assert_eq!(value, target.id),
        other => panic!("[req_fm_audit] FAIL, got {other:?}"),
    }

    let (
        fact_id,
        action_type,
        actor,
        provenance,
        confirmation_class,
        previous_snapshot,
        occurred_at,
    ): (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT fact_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at
             FROM fact_audit_log WHERE action_type = 'ForgetMemory'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap();

    assert_eq!(fact_id, target.id.to_string());
    assert_eq!(action_type, "ForgetMemory");
    assert_eq!(actor, "CloudLLM");
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "DELETE");
    assert!(
        previous_snapshot.is_none(),
        "[req_fm_audit] FAIL - ForgetMemory's own audit marker must never carry a snapshot"
    );
    assert_eq!(occurred_at, requested_at.to_rfc3339());

    assert!(get_fact(&conn, target.id).unwrap().is_none());
}

// --- Hard Fact deletion proof (R024 §14) --------------------------------

#[test]
fn req_fm_hard_delete_proof() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_hard_delete.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_fm_target(&mut conn);
    let facts_before = am_fact_row_count(&conn);

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_hard_delete] FAIL, got {other:?}"),
    }

    assert!(get_fact(&conn, target.id).unwrap().is_none());
    let raw_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM facts WHERE id = ?1",
            rusqlite::params![target.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(raw_count, 0);
    assert_eq!(am_fact_row_count(&conn), facts_before - 1);
}

// --- Supersession-chain test matrix (R024 §9/§22) -----------------------

#[test]
fn req_fm_supersession_chain_forgetting_middle_fact() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_chain_middle.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let a = mk_fm_target(&mut conn);
    let b = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(a.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup A->B failed: {other:?}"),
    };
    let c = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(b.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup B->C failed: {other:?}"),
    };

    let a_before = get_fact(&conn, a.id).unwrap().unwrap();
    let c_before = get_fact(&conn, c.id).unwrap().unwrap();

    let t_forget: chrono::DateTime<chrono::Utc> = "2026-09-18T11:00:00Z".parse().unwrap();
    let forget_meta = ActionMeta {
        actor: Actor::Ui,
        provenance: Provenance::Explicit,
        requested_at: t_forget,
        confirmed: true,
    };
    match forget_memory(&mut conn, forget_meta, mk_fm_payload(b.id)) {
        ActionOutcome::Success { value, .. } => assert_eq!(value, b.id),
        other => panic!("[req_fm_chain_middle] FAIL forgetting B, got {other:?}"),
    }

    assert!(get_fact(&conn, b.id).unwrap().is_none(), "B must be gone");
    let a_after = get_fact(&conn, a.id).unwrap().unwrap();
    assert_eq!(
        a_after.superseded_by_fact_id, None,
        "[req_fm_chain_middle] FAIL - A's superseded_by must be cleared, never rewired to C"
    );
    assert_eq!(
        a_after.valid_until, a_before.valid_until,
        "A.valid_until must remain unchanged"
    );
    assert_eq!(
        a_after.epistemic_state, a_before.epistemic_state,
        "A.epistemic_state must remain unchanged"
    );
    assert_eq!(
        a_after.updated_at, t_forget,
        "A.updated_at must equal the forget call's requested_at"
    );
    assert_eq!(a_after.subject_entity_id, a_before.subject_entity_id);
    assert_eq!(a_after.predicate, a_before.predicate);
    assert_eq!(a_after.value, a_before.value);
    assert_eq!(a_after.valid_from, a_before.valid_from);
    assert_eq!(a_after.created_at, a_before.created_at);

    let c_after = get_fact(&conn, c.id).unwrap().unwrap();
    assert_eq!(
        c_after, c_before,
        "[req_fm_chain_middle] FAIL - C must remain byte-for-byte unchanged"
    );
}

#[test]
fn req_fm_forgetting_predecessor_leaves_successor_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_chain_predecessor.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = mk_fm_target(&mut conn);
    let b = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(a.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup A->B failed: {other:?}"),
    };
    let b_before = get_fact(&conn, b.id).unwrap().unwrap();

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(a.id)) {
        ActionOutcome::Success { value, .. } => assert_eq!(value, a.id),
        other => panic!("[req_fm_chain_predecessor] FAIL, got {other:?}"),
    }

    assert!(get_fact(&conn, a.id).unwrap().is_none());
    let b_after = get_fact(&conn, b.id).unwrap().unwrap();
    assert_eq!(
        b_before, b_after,
        "[req_fm_chain_predecessor] FAIL - forgetting a predecessor must leave the successor byte-for-byte unchanged"
    );
}

#[test]
fn req_fm_forgetting_terminal_fact_clears_immediate_predecessor_only() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_chain_terminal.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let a = mk_fm_target(&mut conn);
    let b = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(a.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup A->B failed: {other:?}"),
    };
    let c = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(b.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup B->C failed: {other:?}"),
    };
    let a_before = get_fact(&conn, a.id).unwrap().unwrap();

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(c.id)) {
        ActionOutcome::Success { value, .. } => assert_eq!(value, c.id),
        other => panic!("[req_fm_chain_terminal] FAIL, got {other:?}"),
    }

    assert!(get_fact(&conn, c.id).unwrap().is_none());
    let b_after = get_fact(&conn, b.id).unwrap().unwrap();
    assert_eq!(
        b_after.superseded_by_fact_id, None,
        "[req_fm_chain_terminal] FAIL - B's superseded_by must be cleared"
    );
    assert_eq!(
        b_after.epistemic_state,
        EpistemicState::Stale,
        "B remains STALE"
    );
    assert!(b_after.valid_until.is_some(), "B remains closed/historical");
    let a_after = get_fact(&conn, a.id).unwrap().unwrap();
    assert_eq!(
        a_before, a_after,
        "[req_fm_chain_terminal] FAIL - A (further up the chain) must remain completely unchanged"
    );
}

// --- Multi/zero-Evidence support (R024 §10) -----------------------------

#[test]
fn req_fm_zero_evidence_fixture_forgotten_safely() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_zero_evidence.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let target = mk_fm_target(&mut conn);
    conn.execute(
        "DELETE FROM fact_evidence WHERE fact_id = ?1",
        rusqlite::params![target.id.to_string()],
    )
    .unwrap();

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target.id)) {
        ActionOutcome::Success { value, .. } => assert_eq!(value, target.id),
        other => panic!("[req_fm_zero_evidence] FAIL, got {other:?}"),
    }
    assert!(get_fact(&conn, target.id).unwrap().is_none());
}

// --- Evidence privacy-first deletion (R024 §10, brief §29/§30) ---------

/// Raw fixtures (no production write path exists for `relations`/
/// `episodes`) proving Evidence privacy-first deletion: an Evidence row
/// shared with a Relation and an Episode is still always deleted when
/// its Fact is forgotten, but the Relation/Episode OBJECTS themselves
/// survive - they merely lose that Evidence's support.
#[test]
fn req_fm_relation_episode_shared_evidence_privacy_proof() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_rel_ep_evidence.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let self_id = self_entity_id(&conn);
    let second_entity = mk_fm_second_entity(&mut conn);

    let target = mk_fm_target(&mut conn);
    let evidence_id: String = conn
        .query_row(
            "SELECT evidence_id FROM fact_evidence WHERE fact_id = ?1",
            rusqlite::params![target.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();

    let relation_id = insert_raw_relation(&conn, self_id, second_entity);
    let episode_id = insert_raw_episode(&conn);
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO relation_evidence (relation_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![relation_id, evidence_id, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO episode_evidence (episode_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![episode_id, evidence_id, now],
    )
    .unwrap();

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target.id)) {
        ActionOutcome::Success { value, .. } => assert_eq!(value, target.id),
        other => panic!("[req_fm_rel_ep_evidence] FAIL, got {other:?}"),
    }

    assert!(get_fact(&conn, target.id).unwrap().is_none());
    let evidence_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM evidence WHERE id = ?1",
            rusqlite::params![evidence_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        evidence_count, 0,
        "[req_fm_rel_ep_evidence] FAIL - shared Evidence must be deleted (privacy-first)"
    );
    for (table, column) in [
        ("fact_evidence", "evidence_id"),
        ("relation_evidence", "evidence_id"),
        ("episode_evidence", "evidence_id"),
    ] {
        let count: i64 = conn
            .query_row(
                &format!("SELECT count(*) FROM {table} WHERE {column} = ?1"),
                rusqlite::params![evidence_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 0,
            "[req_fm_rel_ep_evidence] FAIL - {table} link must be gone"
        );
    }

    let relation_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM relations WHERE id = ?1",
            rusqlite::params![relation_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        relation_count, 1,
        "[req_fm_rel_ep_evidence] FAIL - the Relation row itself must survive"
    );
    let episode_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM episodes WHERE id = ?1",
            rusqlite::params![episode_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        episode_count, 1,
        "[req_fm_rel_ep_evidence] FAIL - the Episode row itself must survive"
    );
}

/// A deliberate raw fixture - two Facts (T and retained K) both linking
/// to the SAME Evidence row (production never creates this shape;
/// `AddMemory`/`CorrectMemory` always create a fresh Evidence per
/// Fact). Forgetting T must still delete the shared Evidence - privacy
/// of the forgotten Evidence outranks preserving it for K, even though
/// K may now have zero Evidence links as a result. K itself (the Fact)
/// must survive.
#[test]
fn req_fm_other_fact_shared_evidence_deleted_but_other_fact_survives() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_shared_evidence.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let target = mk_fm_target(&mut conn);
    let evidence_id: String = conn
        .query_row(
            "SELECT evidence_id FROM fact_evidence WHERE fact_id = ?1",
            rusqlite::params![target.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();

    let k = mk_fm_target(&mut conn);
    // Replace K's own separately-created Evidence link with the SHARED
    // one, reproducing exactly "K may now have zero Evidence links."
    conn.execute(
        "DELETE FROM fact_evidence WHERE fact_id = ?1",
        rusqlite::params![k.id.to_string()],
    )
    .unwrap();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO fact_evidence (fact_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![k.id.to_string(), evidence_id, now],
    )
    .unwrap();

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target.id)) {
        ActionOutcome::Success { value, .. } => assert_eq!(value, target.id),
        other => panic!("[req_fm_shared_evidence] FAIL, got {other:?}"),
    }

    assert!(get_fact(&conn, target.id).unwrap().is_none());
    let evidence_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM evidence WHERE id = ?1",
            rusqlite::params![evidence_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        evidence_count, 0,
        "[req_fm_shared_evidence] FAIL - the shared Evidence must still be deleted"
    );

    let k_after = get_fact(&conn, k.id).unwrap();
    assert!(
        k_after.is_some(),
        "[req_fm_shared_evidence] FAIL - the other retained Fact K must survive"
    );
    let k_evidence_links: i64 = conn
        .query_row(
            "SELECT count(*) FROM fact_evidence WHERE fact_id = ?1",
            rusqlite::params![k.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        k_evidence_links, 0,
        "K now has zero Evidence links in this fixture - acceptable, privacy of forgotten Evidence outranks shared-Evidence preservation"
    );
}

// --- Source orphan/shared lifecycle (R024 §11, brief §17) --------------

#[test]
fn req_fm_source_matrix_orphan_and_shared() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_source_matrix.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    // A: unique Source becomes orphan and is deleted.
    let target_a = mk_fm_target(&mut conn);
    let source_a: String = conn
        .query_row(
            "SELECT e.source_id FROM evidence e JOIN fact_evidence fe ON fe.evidence_id = e.id WHERE fe.fact_id = ?1",
            rusqlite::params![target_a.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_a.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_source_matrix] FAIL case A, got {other:?}"),
    }
    let source_a_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sources WHERE id = ?1",
            rusqlite::params![source_a],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source_a_count, 0, "case A: orphaned Source must be deleted");

    // B: target's Evidence shares a Source with another retained Fact's Evidence.
    let target_b = mk_fm_target(&mut conn);
    let source_b: String = conn
        .query_row(
            "SELECT e.source_id FROM evidence e JOIN fact_evidence fe ON fe.evidence_id = e.id WHERE fe.fact_id = ?1",
            rusqlite::params![target_b.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let source_b_id = SourceId::parse(&source_b).unwrap();
    let retained_payload = AddMemoryPayload {
        source: SourceRef::Existing(source_b_id),
        ..mk_am_payload()
    };
    let retained = match add_memory(&mut conn, ui_explicit(), retained_payload) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup retained Fact failed: {other:?}"),
    };
    let retained_evidence_before: String = conn
        .query_row(
            "SELECT evidence_id FROM fact_evidence WHERE fact_id = ?1",
            rusqlite::params![retained.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_b.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_source_matrix] FAIL case B, got {other:?}"),
    }
    let source_b_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sources WHERE id = ?1",
            rusqlite::params![source_b],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        source_b_count, 1,
        "case B: shared Source must remain byte-for-byte"
    );
    assert!(
        get_fact(&conn, retained.id).unwrap().is_some(),
        "case B: the retained Fact must survive"
    );
    let retained_evidence_after: String = conn
        .query_row(
            "SELECT evidence_id FROM fact_evidence WHERE fact_id = ?1",
            rusqlite::params![retained.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        retained_evidence_before, retained_evidence_after,
        "case B: retained Fact's own Evidence link must remain"
    );

    // C: target has TWO Evidence rows sharing the SAME orphan Source.
    let target_c = mk_fm_target(&mut conn);
    let source_c_id: String = conn
        .query_row(
            "SELECT e.source_id FROM evidence e JOIN fact_evidence fe ON fe.evidence_id = e.id WHERE fe.fact_id = ?1",
            rusqlite::params![target_c.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let extra_evidence_id = uuid::Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO evidence (id, source_id, content, locator, sensitivity, created_at) VALUES (?1, ?2, 'second evidence', NULL, 'INTERNAL', ?3)",
        rusqlite::params![extra_evidence_id, source_c_id, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fact_evidence (fact_id, evidence_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![target_c.id.to_string(), extra_evidence_id, now],
    )
    .unwrap();

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_c.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_source_matrix] FAIL case C, got {other:?}"),
    }
    let source_c_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sources WHERE id = ?1",
            rusqlite::params![source_c_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        source_c_count, 0,
        "case C: the shared orphan Source must be deleted exactly once, safely"
    );
}

// --- Entity orphan/shared lifecycle, self/orphan cases (R024 §12/§13, --
// brief §20 A-D) -----------------------------------------------------------

#[test]
fn req_fm_entity_retention_matrix_self_and_orphan_cases() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_entity_matrix_1.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let self_id = self_entity_id(&conn);

    // A: SELF subject only - SELF survives.
    let target_a = mk_fm_target(&mut conn);
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_a.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_entity_matrix_1] FAIL case A, got {other:?}"),
    }
    assert!(
        get_entity(&conn, self_id).unwrap().is_some(),
        "case A: SELF must survive"
    );

    // B: non-SELF subject used only by T - deleted.
    let target_b = match add_memory(
        &mut conn,
        ui_explicit(),
        AddMemoryPayload {
            subject: EntityInputRef::New(NewEntityData {
                entity_type: "person".to_string(),
                canonical_name: "Case B Subject".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
            }),
            ..mk_am_payload()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup case B failed: {other:?}"),
    };
    let subject_b = target_b.subject_entity_id;
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_b.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_entity_matrix_1] FAIL case B, got {other:?}"),
    }
    assert!(
        get_entity(&conn, subject_b).unwrap().is_none(),
        "case B: orphaned non-SELF subject Entity must be deleted"
    );

    // C: non-SELF object used only by T - deleted.
    let target_c = match add_memory(
        &mut conn,
        ui_explicit(),
        AddMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
                entity_type: "place".to_string(),
                canonical_name: "Case C Object".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
            })),
            ..mk_am_payload()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup case C failed: {other:?}"),
    };
    let object_c = match target_c.value {
        FactValue::EntityRef(id) => id,
        other => panic!("expected EntityRef, got {other:?}"),
    };
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_c.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_entity_matrix_1] FAIL case C, got {other:?}"),
    }
    assert!(
        get_entity(&conn, object_c).unwrap().is_none(),
        "case C: orphaned non-SELF object Entity must be deleted"
    );

    // D: raw fixture - the SAME non-SELF Entity is both subject and
    // object of T, with T as its ONLY reference. No current production
    // path creates a brand-new Entity as both subject and object of one
    // Fact in a single call - this proves the service's own
    // deduplication/orphan-check logic against that shape directly.
    let entity_d_id = uuid::Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO entities (id, entity_type, canonical_name, is_self, sensitivity, created_at, updated_at)
         VALUES (?1, 'person', 'Case D Entity', 0, 'INTERNAL', ?2, ?2)",
        rusqlite::params![entity_d_id, now],
    )
    .unwrap();
    let fact_d_id = uuid::Uuid::now_v7().to_string();
    conn.execute(
        "INSERT INTO facts (id, subject_entity_id, predicate, value_json, object_entity_id, memory_type, epistemic_state, sensitivity, valid_from, created_at, updated_at)
         VALUES (?1, ?2, 'self_edge_predicate', NULL, ?2, 'FACT', 'OBSERVED', 'INTERNAL', ?3, ?3, ?3)",
        rusqlite::params![fact_d_id, entity_d_id, now],
    )
    .unwrap();
    let target_d_id = FactId::parse(&fact_d_id).unwrap();
    let entity_d = EntityId::parse(&entity_d_id).unwrap();

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_d_id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_entity_matrix_1] FAIL case D, got {other:?}"),
    }
    assert!(
        get_entity(&conn, entity_d).unwrap().is_none(),
        "case D: self-edge Entity with T as its only reference must be deleted (deduplicated, no double-delete failure)"
    );
}

// --- Entity orphan/shared lifecycle, shared-reference retention cases -
// (R024 §12/§13, brief §20 E-J) ---------------------------------------------

#[test]
fn req_fm_entity_retention_matrix_shared_reference_cases() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_entity_matrix_2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let self_id = self_entity_id(&conn);

    // E: candidate Entity also referenced as another Fact's subject - retained.
    let target_e = match add_memory(
        &mut conn,
        ui_explicit(),
        AddMemoryPayload {
            subject: EntityInputRef::New(NewEntityData {
                entity_type: "person".to_string(),
                canonical_name: "Case E Entity".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
            }),
            ..mk_am_payload()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup case E failed: {other:?}"),
    };
    let entity_e = target_e.subject_entity_id;
    let sibling_e = match add_memory(
        &mut conn,
        ui_explicit(),
        AddMemoryPayload {
            subject: EntityInputRef::Existing(entity_e),
            predicate: "case_e_sibling_predicate".to_string(),
            ..mk_am_payload()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup case E sibling failed: {other:?}"),
    };
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_e.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_entity_matrix_2] FAIL case E, got {other:?}"),
    }
    assert!(
        get_entity(&conn, entity_e).unwrap().is_some(),
        "case E: Entity referenced by another Fact's subject must be retained"
    );
    assert!(get_fact(&conn, sibling_e.id).unwrap().is_some());

    // F: candidate Entity also referenced as another Fact's object - retained.
    let target_f = match add_memory(
        &mut conn,
        ui_explicit(),
        AddMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
                entity_type: "place".to_string(),
                canonical_name: "Case F Entity".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
            })),
            ..mk_am_payload()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup case F failed: {other:?}"),
    };
    let entity_f = match target_f.value {
        FactValue::EntityRef(id) => id,
        other => panic!("expected EntityRef, got {other:?}"),
    };
    let _sibling_f = match add_memory(
        &mut conn,
        ui_explicit(),
        AddMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::Existing(entity_f)),
            predicate: "case_f_sibling_predicate".to_string(),
            ..mk_am_payload()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup case F sibling failed: {other:?}"),
    };
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_f.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_entity_matrix_2] FAIL case F, got {other:?}"),
    }
    assert!(
        get_entity(&conn, entity_f).unwrap().is_some(),
        "case F: Entity referenced by another Fact's object must be retained"
    );

    // G/H: candidate Entity referenced by Relation.from / Relation.to - retained.
    let target_g = match add_memory(
        &mut conn,
        ui_explicit(),
        AddMemoryPayload {
            subject: EntityInputRef::New(NewEntityData {
                entity_type: "person".to_string(),
                canonical_name: "Case G Entity".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
            }),
            ..mk_am_payload()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup case G failed: {other:?}"),
    };
    let entity_g = target_g.subject_entity_id;
    let _relation_g = insert_raw_relation(&conn, entity_g, self_id);
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_g.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_entity_matrix_2] FAIL case G, got {other:?}"),
    }
    assert!(
        get_entity(&conn, entity_g).unwrap().is_some(),
        "case G: Entity referenced by Relation.from must be retained"
    );

    let target_h = match add_memory(
        &mut conn,
        ui_explicit(),
        AddMemoryPayload {
            subject: EntityInputRef::New(NewEntityData {
                entity_type: "person".to_string(),
                canonical_name: "Case H Entity".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
            }),
            ..mk_am_payload()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup case H failed: {other:?}"),
    };
    let entity_h = target_h.subject_entity_id;
    let _relation_h = insert_raw_relation(&conn, self_id, entity_h);
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_h.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_entity_matrix_2] FAIL case H, got {other:?}"),
    }
    assert!(
        get_entity(&conn, entity_h).unwrap().is_some(),
        "case H: Entity referenced by Relation.to must be retained"
    );

    // I: candidate Entity referenced by episode_entities - retained.
    let target_i = match add_memory(
        &mut conn,
        ui_explicit(),
        AddMemoryPayload {
            subject: EntityInputRef::New(NewEntityData {
                entity_type: "person".to_string(),
                canonical_name: "Case I Entity".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
            }),
            ..mk_am_payload()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup case I failed: {other:?}"),
    };
    let entity_i = target_i.subject_entity_id;
    let episode_i = insert_raw_episode(&conn);
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO episode_entities (episode_id, entity_id, created_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![episode_i, entity_i.to_string(), now],
    )
    .unwrap();
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_i.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_entity_matrix_2] FAIL case I, got {other:?}"),
    }
    assert!(
        get_entity(&conn, entity_i).unwrap().is_some(),
        "case I: Entity referenced by episode_entities must be retained"
    );

    // J: candidate Entity has a domain_ref but zero PK references after
    // T's deletion - deleted anyway; the authoritative Goal row survives.
    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("Case J Goal")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup goal failed: {other:?}"),
    };
    let target_j = match add_memory(
        &mut conn,
        ui_explicit(),
        AddMemoryPayload {
            subject: EntityInputRef::New(NewEntityData {
                entity_type: "goal".to_string(),
                canonical_name: "Case J Entity".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: Some(DomainRef {
                    domain_ref_type: "Goal".to_string(),
                    domain_ref_id: goal.id.to_string(),
                }),
            }),
            ..mk_am_payload()
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup case J failed: {other:?}"),
    };
    let entity_j = target_j.subject_entity_id;
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target_j.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_entity_matrix_2] FAIL case J, got {other:?}"),
    }
    assert!(
        get_entity(&conn, entity_j).unwrap().is_none(),
        "case J: domain_ref alone must NOT retain an otherwise-orphaned Entity"
    );
    assert!(
        get_goal(&conn, goal.id).unwrap().is_some(),
        "case J: the authoritative Goal row itself must remain completely untouched"
    );
}

// --- Audit snapshot scrubbing (R024 §8, brief §22-25) -------------------

#[test]
fn req_fm_snapshot_scrub_confirmmemory_audit() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_scrub_cfm.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let payload = AddMemoryPayload {
        epistemic_state: EpistemicState::Inferred,
        ..mk_am_payload()
    };
    let target = match add_memory(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup AddMemory failed: {other:?}"),
    };
    match confirm_memory(
        &mut conn,
        ui_explicit(),
        ConfirmMemoryPayload { fact_id: target.id },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup ConfirmMemory failed: {other:?}"),
    }

    let snapshot_before: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'ConfirmMemory'",
            rusqlite::params![target.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let snapshot_before =
        snapshot_before.expect("ConfirmMemory audit snapshot must be non-NULL before forgetting");
    let _: Fact = serde_json::from_str(&snapshot_before)
        .expect("the pre-scrub snapshot must contain serialized semantic Fact data");

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_scrub_cfm] FAIL, got {other:?}"),
    }

    let confirm_snapshot_after: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'ConfirmMemory'",
            rusqlite::params![target.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        confirm_snapshot_after.is_none(),
        "[req_fm_scrub_cfm] FAIL - the old ConfirmMemory audit snapshot must be scrubbed to NULL, while the row itself still exists"
    );

    let forget_snapshot: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'ForgetMemory'",
            rusqlite::params![target.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        forget_snapshot.is_none(),
        "[req_fm_scrub_cfm] FAIL - ForgetMemory's own audit marker must have NULL snapshot"
    );
}

#[test]
fn req_fm_snapshot_scrub_correctmemory_audit() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_scrub_cm.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let a = mk_fm_target(&mut conn);
    let b = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(a.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup A->B failed: {other:?}"),
    };
    let _c = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(b.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup B->C failed: {other:?}"),
    };

    let snapshot_before: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'CorrectMemory'",
            rusqlite::params![b.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        snapshot_before.is_some(),
        "B's own CorrectMemory audit row (from correcting B->C) must have a non-NULL snapshot before forgetting"
    );

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(b.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_scrub_cm] FAIL, got {other:?}"),
    }

    assert!(get_fact(&conn, b.id).unwrap().is_none(), "B must be gone");

    let snapshot_after: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'CorrectMemory'",
            rusqlite::params![b.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        snapshot_after.is_none(),
        "[req_fm_scrub_cm] FAIL - B's own CorrectMemory audit snapshot must be scrubbed to NULL"
    );

    let forget_snapshot: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'ForgetMemory'",
            rusqlite::params![b.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(forget_snapshot.is_none());
}

/// `ForgetMemory` scrubs ONLY `previous_snapshot` - every other audit
/// column (`id`/`action_type`/`actor`/`provenance`/`confirmation_class`/
/// `occurred_at`) survives untouched, and the row itself is never
/// deleted; it becomes payload-free structural history.
#[test]
fn req_fm_old_audit_structural_metadata_survives_scrub() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_scrub_metadata.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let payload = AddMemoryPayload {
        epistemic_state: EpistemicState::Inferred,
        ..mk_am_payload()
    };
    let target = match add_memory(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    match confirm_memory(
        &mut conn,
        ui_explicit(),
        ConfirmMemoryPayload { fact_id: target.id },
    ) {
        ActionOutcome::Success { .. } => {}
        other => panic!("setup ConfirmMemory failed: {other:?}"),
    }

    let before: (String, String, String, String, String, String) = conn
        .query_row(
            "SELECT id, action_type, actor, provenance, confirmation_class, occurred_at FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'ConfirmMemory'",
            rusqlite::params![target.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap();

    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(target.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_fm_scrub_metadata] FAIL, got {other:?}"),
    }

    let after: (String, String, String, String, String, String) = conn
        .query_row(
            "SELECT id, action_type, actor, provenance, confirmation_class, occurred_at FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'ConfirmMemory'",
            rusqlite::params![target.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap();

    assert_eq!(
        before, after,
        "[req_fm_scrub_metadata] FAIL - every audit column besides previous_snapshot must survive scrubbing exactly"
    );
}

// --- Full atomic rollback (R024 - central acceptance proof, §20) -------

/// MANDATORY central proof: a rich pre-call state - target Fact T
/// (historical/`Stale`, itself the predecessor of a further correction
/// so it carries its OWN payload-bearing `CorrectMemory` audit
/// snapshot), an inbound supersession predecessor P, T's own Evidence/
/// Source, and a candidate orphan object Entity. Forces the FINAL
/// `ForgetMemory` audit insert (step 13) to fail and proves every
/// earlier step (5-12) in the same call rolls back completely.
#[test]
fn req_fm_rollback_forced_final_audit_failure_restores_everything() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_rollback1.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let p = mk_fm_target(&mut conn);
    let t = match correct_memory(
        &mut conn,
        ui_explicit(),
        CorrectMemoryPayload {
            value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
                entity_type: "place".to_string(),
                canonical_name: "Rollback Orphan Candidate".to_string(),
                sensitivity: Sensitivity::Internal,
                domain_ref: None,
            })),
            ..mk_cm_payload(p.id)
        },
    ) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup P->T failed: {other:?}"),
    };
    let orphan_candidate_entity = match t.value {
        FactValue::EntityRef(id) => id,
        other => panic!("expected EntityRef, got {other:?}"),
    };
    let _u = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(t.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup T->U failed: {other:?}"),
    };

    let p_before = get_fact(&conn, p.id).unwrap().unwrap();
    let t_before = get_fact(&conn, t.id).unwrap().unwrap();
    let t_evidence_id: String = conn
        .query_row(
            "SELECT evidence_id FROM fact_evidence WHERE fact_id = ?1",
            rusqlite::params![t.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let t_source_id: String = conn
        .query_row(
            "SELECT source_id FROM evidence WHERE id = ?1",
            rusqlite::params![t_evidence_id],
            |r| r.get(0),
        )
        .unwrap();
    let correct_memory_snapshot_before: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'CorrectMemory'",
            rusqlite::params![t.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        correct_memory_snapshot_before.is_some(),
        "T's own CorrectMemory audit snapshot must be non-NULL before forgetting"
    );
    let entity_before = get_entity(&conn, orphan_candidate_entity).unwrap();
    assert!(entity_before.is_some());

    let snapshot_before = am_full_table_snapshot(&conn);
    let audit_count_before = am_fact_audit_row_count(&conn);

    install_fact_audit_failure_trigger(&conn);
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(t.id)) {
        ActionOutcome::StorageError(_) => {}
        other => panic!("[req_fm_rollback1] FAIL - expected StorageError, got {other:?}"),
    }
    remove_fact_audit_failure_trigger(&conn);

    assert_eq!(
        am_full_table_snapshot(&conn),
        snapshot_before,
        "[req_fm_rollback1] FAIL - every table must be exactly as before the failed forget"
    );
    assert_eq!(am_fact_audit_row_count(&conn), audit_count_before);

    let t_after = get_fact(&conn, t.id).unwrap();
    assert_eq!(
        t_after,
        Some(t_before),
        "[req_fm_rollback1] FAIL - target Fact T must still exist, byte-for-byte unchanged"
    );

    let p_after = get_fact(&conn, p.id).unwrap().unwrap();
    assert_eq!(
        p_after, p_before,
        "[req_fm_rollback1] FAIL - inbound predecessor P's superseded_by must be restored"
    );

    let evidence_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM evidence WHERE id = ?1",
            rusqlite::params![t_evidence_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        evidence_count, 1,
        "[req_fm_rollback1] FAIL - T's Evidence must still exist"
    );
    let fact_evidence_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM fact_evidence WHERE fact_id = ?1 AND evidence_id = ?2",
            rusqlite::params![t.id.to_string(), t_evidence_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        fact_evidence_count, 1,
        "[req_fm_rollback1] FAIL - the fact_evidence join row must be restored"
    );
    let source_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sources WHERE id = ?1",
            rusqlite::params![t_source_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        source_count, 1,
        "[req_fm_rollback1] FAIL - T's Source must still exist"
    );

    let entity_after = get_entity(&conn, orphan_candidate_entity).unwrap();
    assert_eq!(
        entity_after, entity_before,
        "[req_fm_rollback1] FAIL - the candidate orphan Entity must still exist"
    );

    let correct_memory_snapshot_after: Option<String> = conn
        .query_row(
            "SELECT previous_snapshot FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'CorrectMemory'",
            rusqlite::params![t.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        correct_memory_snapshot_after, correct_memory_snapshot_before,
        "[req_fm_rollback1] FAIL - T's CorrectMemory audit snapshot must be restored (scrub rolled back)"
    );

    let forget_marker_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM fact_audit_log WHERE fact_id = ?1 AND action_type = 'ForgetMemory'",
            rusqlite::params![t.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        forget_marker_count, 0,
        "[req_fm_rollback1] FAIL - no ForgetMemory audit marker must exist"
    );

    match add_memory(&mut conn, ui_explicit(), mk_am_payload()) {
        ActionOutcome::Success { .. } => {}
        other => panic!(
            "[req_fm_rollback1] FAIL - connection should remain usable after rollback, got {other:?}"
        ),
    }
}

/// A mid-transaction failure (forcing the target `facts` `DELETE`
/// itself, step 11, to fail) - proves the EARLIER steps in the same
/// call (audit scrub, inbound-supersession clearing) also roll back,
/// not only a final-step failure.
#[test]
fn req_fm_rollback_mid_step_forced_fact_delete_failure() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_rollback2.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let p = mk_fm_target(&mut conn);
    let t = match correct_memory(&mut conn, ui_explicit(), mk_cm_payload(p.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup P->T failed: {other:?}"),
    };

    let p_before = get_fact(&conn, p.id).unwrap().unwrap();
    let t_before = get_fact(&conn, t.id).unwrap().unwrap();
    let snapshot_before = am_full_table_snapshot(&conn);

    install_fact_delete_failure_trigger(&conn);
    match forget_memory(&mut conn, confirmed_explicit(), mk_fm_payload(t.id)) {
        ActionOutcome::StorageError(_) => {}
        other => panic!("[req_fm_rollback2] FAIL - expected StorageError, got {other:?}"),
    }
    remove_fact_delete_failure_trigger(&conn);

    assert_eq!(
        am_full_table_snapshot(&conn),
        snapshot_before,
        "[req_fm_rollback2] FAIL - every table must be restored, including the already-performed audit scrub and inbound-supersession clearing that happened before the later DELETE failed"
    );
    let p_after = get_fact(&conn, p.id).unwrap().unwrap();
    assert_eq!(
        p_after, p_before,
        "[req_fm_rollback2] FAIL - P's superseded_by_fact_id clearing (an earlier step) must roll back when a later step fails"
    );
    let t_after = get_fact(&conn, t.id).unwrap();
    assert_eq!(
        t_after,
        Some(t_before),
        "[req_fm_rollback2] FAIL - T must still exist"
    );
}

// --- Post-run transaction-isolation hardening (2026-09-18) --------------

/// Direct proof that `ForgetMemory` opens its transaction with
/// `TransactionBehavior::Immediate`, not the default `Deferred` every
/// other action's plain `conn.transaction()` uses - a `Deferred`
/// transaction would let a concurrent writer change the exact rows
/// `ForgetMemory` is about to read/delete before it ever acquires a
/// write lock. Two real connections to the SAME encrypted database
/// file: Connection A opens `ForgetMemory`'s transaction via the exact
/// production helper; Connection B (a zero busy timeout, so the
/// outcome is immediate and deterministic - no sleeps) attempts a
/// harmless write and must be rejected with `SQLITE_BUSY`/`LOCKED`
/// while A's transaction is open, then must succeed normally once A's
/// transaction is dropped (rolled back).
#[test]
fn req_fm_uses_immediate_transaction_blocking_concurrent_writer() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_immediate_lock.db");
    let mut conn_a = bootstrap_db_at(&path, &key).unwrap();
    let conn_b = bootstrap_db_at(&path, &key).unwrap();
    conn_b
        .busy_timeout(std::time::Duration::from_millis(0))
        .unwrap();

    let tx_a = begin_forget_transaction(&mut conn_a).unwrap();

    let harmless_write = || {
        conn_b.execute(
            "INSERT INTO sources (id, source_type, sensitivity, captured_at, created_at)
             VALUES (?1, 'conversation', 'INTERNAL', ?2, ?2)",
            rusqlite::params![uuid::Uuid::now_v7().to_string(), Utc::now().to_rfc3339()],
        )
    };

    let blocked_result = harmless_write();
    match &blocked_result {
        Err(rusqlite::Error::SqliteFailure(err, _)) => {
            assert!(
                matches!(
                    err.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ),
                "[req_fm_immediate_lock] FAIL - expected SQLITE_BUSY/LOCKED, got {err:?}"
            );
        }
        other => panic!(
            "[req_fm_immediate_lock] FAIL - a concurrent writer must be blocked while ForgetMemory's IMMEDIATE transaction holds the write lock, got {other:?}"
        ),
    }

    drop(tx_a);

    let unblocked_result = harmless_write();
    assert!(
        unblocked_result.is_ok(),
        "[req_fm_immediate_lock] FAIL - after A's transaction is dropped/rolled back, B should be able to write normally, got {unblocked_result:?}"
    );
}

/// Direct repo-level proof for `delete_fact_forget_target`, matching
/// the standard already established for `CorrectMemory`/`ConfirmMemory`'s
/// own guarded updates: an existing Fact deletes successfully; a
/// missing `FactId` returns a real `Err`, never a silent `Ok`.
#[test]
fn req_fm_repo_delete_fact_forget_target_direct_proof() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "fm_repo_delete_direct.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let target = mk_fm_target(&mut conn);
    // This narrow function only deletes the `facts` row itself - the
    // caller (`forget_memory`) is responsible for clearing
    // `fact_evidence` first; remove it directly here so this isolated
    // proof does not trip the real `ON DELETE RESTRICT` FK.
    conn.execute(
        "DELETE FROM fact_evidence WHERE fact_id = ?1",
        rusqlite::params![target.id.to_string()],
    )
    .unwrap();
    let result = storage::personal_knowledge_repo::delete_fact_forget_target(&conn, target.id);
    assert!(
        result.is_ok(),
        "[req_fm_repo_delete_direct] FAIL - existing Fact delete should succeed, got {result:?}"
    );
    assert!(get_fact(&conn, target.id).unwrap().is_none());

    let missing_result =
        storage::personal_knowledge_repo::delete_fact_forget_target(&conn, FactId::new());
    assert!(
        missing_result.is_err(),
        "[req_fm_repo_delete_direct] FAIL - missing FactId delete should be a real Err, not silently Ok, got {missing_result:?}"
    );
}

// =========================================================================
// M2 Personal Knowledge Relation Foundation and AddRelation (R026,
// 2026-09-18)
//
// Evidence for
// `docs/reports/R026_M2_PERSONAL_KNOWLEDGE_RELATION_FOUNDATION_AND_ADDRELATION_2026-09-18.md`.
// V8 migration proofs, Relation read-side proofs, and the first
// production Relation mutation (`AddRelation`). Database-dependent
// behavior only - pure `validate_add_relation` structural tests live in
// `actions::tests` instead, same split as every other action.
// =========================================================================

fn mk_pk_relation(
    from_entity_id: EntityId,
    to_entity_id: EntityId,
    relation_type: &str,
    epistemic_state: EpistemicState,
    sensitivity: Sensitivity,
) -> Relation {
    let now = Utc::now();
    Relation {
        id: RelationId::new(),
        from_entity_id,
        to_entity_id,
        relation_type: relation_type.to_string(),
        note: None,
        epistemic_state,
        confidence: None,
        sensitivity,
        valid_from: now,
        valid_until: None,
        created_at: now,
    }
}

fn relation_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM relations", [], |r| r.get(0))
        .unwrap()
}

fn relation_evidence_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM relation_evidence", [], |r| r.get(0))
        .unwrap()
}

fn relation_audit_row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM relation_audit_log", [], |r| r.get(0))
        .unwrap()
}

/// `(entities, sources, evidence, relations, relation_evidence,
/// relation_audit_log)` - every table `AddRelation` can possibly mutate,
/// same reasoning as `am_full_table_snapshot`.
fn ar_full_table_snapshot(conn: &Connection) -> (i64, i64, i64, i64, i64, i64) {
    (
        entity_row_count(conn),
        am_source_row_count(conn),
        am_evidence_row_count(conn),
        relation_row_count(conn),
        relation_evidence_row_count(conn),
        relation_audit_row_count(conn),
    )
}

/// Same forced-failure mechanism as `install_fact_audit_failure_trigger` -
/// `RAISE(FAIL, ...)` aborts only the failing statement, proving
/// `add_relation`'s own `Transaction`-drop rollback, not SQLite doing it
/// unprompted.
fn install_relation_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER force_relation_audit_failure
         BEFORE INSERT ON relation_audit_log
         BEGIN
             SELECT RAISE(FAIL, 'forced relation audit failure');
         END;",
    )
    .expect("failed to install the forced relation-audit-failure trigger");
}

fn remove_relation_audit_failure_trigger(conn: &Connection) {
    conn.execute_batch("DROP TRIGGER IF EXISTS force_relation_audit_failure;")
        .expect("failed to remove the forced relation-audit-failure trigger");
}

fn mk_ar_to_new(entity_type: &str, canonical_name: &str) -> EntityInputRef {
    EntityInputRef::New(NewEntityData {
        entity_type: entity_type.to_string(),
        canonical_name: canonical_name.to_string(),
        sensitivity: Sensitivity::Internal,
        domain_ref: None,
    })
}

/// A minimal, always-valid baseline payload - Self `from`, brand-new
/// `to` Entity, open `relation_type`, brand-new Source, Evidence with
/// content only. Every test below overrides only the fields it cares
/// about via struct-update syntax, same convention as `mk_am_payload`.
fn mk_ar_payload() -> AddRelationPayload {
    AddRelationPayload {
        from: EntityInputRef::SelfEntity,
        to: mk_ar_to_new("project", "Relation Target Entity"),
        relation_type: "works_on".to_string(),
        note: None,
        epistemic_state: EpistemicState::UserConfirmed,
        confidence: None,
        sensitivity: Sensitivity::Internal,
        source: mk_am_source_new(),
        evidence: mk_am_evidence(),
    }
}

// --- V8 migration proofs (R026 §2 A-M) -----------------------------------

/// A. Fresh V1-through-V8 migration succeeds. C. `relation_audit_log`
/// exists with exactly the frozen 8 columns, in order.
#[test]
fn req_mig3_fresh_v1_through_v8_migration_succeeds_relation_audit_log_exists() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "mig3.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'relation_audit_log'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        exists, 1,
        "[req_mig3] FAIL - relation_audit_log table missing after fresh migration"
    );

    let mut cols: Vec<String> = Vec::new();
    let mut stmt = conn
        .prepare("PRAGMA table_info(relation_audit_log)")
        .unwrap();
    let mut rows = stmt.query([]).unwrap();
    while let Some(row) = rows.next().unwrap() {
        let name: String = row.get(1).unwrap();
        cols.push(name);
    }
    assert_eq!(
        cols,
        vec![
            "id",
            "relation_id",
            "action_type",
            "actor",
            "provenance",
            "confirmation_class",
            "previous_snapshot",
            "occurred_at",
        ],
        "[req_mig3] FAIL - relation_audit_log columns do not match the frozen R025 shape exactly"
    );
    println!("[req_mig3] PASS - fresh V1-V8 migration succeeds; relation_audit_log exists with exactly the 8 frozen columns");
}

/// B. A populated V7 database (a real pre-existing Fact/Entity, the
/// actual shape of an existing user's database before this run)
/// upgrades to V8 non-destructively: old data unchanged,
/// `relation_audit_log` exists, and the upgraded connection is genuinely
/// usable for `AddRelation` (not merely schema-present) - folds in the
/// encrypted-reopen proof (R026 §44).
#[test]
fn req_mig4_populated_v7_database_upgrades_non_destructively_to_v8() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "mig4.db");

    let fact_id;
    {
        let mut conn = bootstrap_db_at_version(&path, &key, 7).unwrap();
        ensure_self_entity(&conn).unwrap();
        let fact = match add_memory(&mut conn, ui_explicit(), mk_am_payload()) {
            ActionOutcome::Success { value, .. } => value,
            other => panic!("setup failed: {other:?}"),
        };
        fact_id = fact.id;
    }

    {
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        assert!(
            get_fact(&conn, fact_id).unwrap().is_some(),
            "[req_mig4] FAIL - pre-V8 Fact lost during V7->V8 upgrade"
        );
        let exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'relation_audit_log'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            exists, 1,
            "[req_mig4] FAIL - relation_audit_log missing after V7->V8 upgrade"
        );

        match add_relation(&mut conn, ui_explicit(), mk_ar_payload()) {
            ActionOutcome::Success { .. } => {}
            other => {
                panic!("[req_mig4] FAIL - AddRelation failed on the upgraded V7->V8 DB: {other:?}")
            }
        }
        println!("[req_mig4] PASS - populated V7 database upgrades to V8 non-destructively: pre-existing Fact survives, relation_audit_log exists, AddRelation works on the upgraded DB");
    }
}

/// D/E. `action_type` CHECK accepts all 4 frozen values, rejects an
/// unknown one.
#[test]
fn req_mig5_relation_audit_log_action_type_check_accepts_valid_rejects_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "mig5.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let fake_relation_id = RelationId::new();

    for action_type in [
        "AddRelation",
        "CorrectRelation",
        "ConfirmRelation",
        "ForgetRelation",
    ] {
        conn.execute(
            "INSERT INTO relation_audit_log (id, relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
             VALUES (?1, ?2, ?3, 'UI', 'Explicit', 'EXPLICIT_ADD', NULL, ?4)",
            rusqlite::params![
                Uuid::now_v7().to_string(),
                fake_relation_id.to_string(),
                action_type,
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap_or_else(|e| panic!("[req_mig5] FAIL - valid action_type {action_type} rejected: {e:?}"));
    }

    let result = conn.execute(
        "INSERT INTO relation_audit_log (id, relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, 'NotARealAction', 'UI', 'Explicit', 'EXPLICIT_ADD', NULL, ?3)",
        rusqlite::params![
            Uuid::now_v7().to_string(),
            fake_relation_id.to_string(),
            Utc::now().to_rfc3339()
        ],
    );
    assert!(
        result.is_err(),
        "[req_mig5] FAIL - invalid action_type was NOT rejected by the CHECK constraint"
    );
    println!("[req_mig5] PASS - relation_audit_log.action_type CHECK accepts all 4 frozen values, rejects an unknown one");
}

/// F/G/H/I/J. `actor`/`provenance` CHECKs accept every frozen value and
/// reject an unknown one; `previous_snapshot` accepts both NULL and
/// non-NULL text.
#[test]
fn req_mig6_relation_audit_log_actor_provenance_previous_snapshot_checks() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "mig6.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let fake_relation_id = RelationId::new();

    for actor in ["UI", "CloudLLM", "NeXa"] {
        conn.execute(
            "INSERT INTO relation_audit_log (id, relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
             VALUES (?1, ?2, 'AddRelation', ?3, 'Explicit', 'EXPLICIT_ADD', NULL, ?4)",
            rusqlite::params![
                Uuid::now_v7().to_string(),
                fake_relation_id.to_string(),
                actor,
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap_or_else(|e| panic!("[req_mig6] FAIL - valid actor {actor} rejected: {e:?}"));
    }
    let bad_actor = conn.execute(
        "INSERT INTO relation_audit_log (id, relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, 'AddRelation', 'NotAnActor', 'Explicit', 'EXPLICIT_ADD', NULL, ?3)",
        rusqlite::params![
            Uuid::now_v7().to_string(),
            fake_relation_id.to_string(),
            Utc::now().to_rfc3339()
        ],
    );
    assert!(
        bad_actor.is_err(),
        "[req_mig6] FAIL - invalid actor was NOT rejected"
    );

    for provenance in ["Explicit", "Inferred"] {
        conn.execute(
            "INSERT INTO relation_audit_log (id, relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
             VALUES (?1, ?2, 'AddRelation', 'UI', ?3, 'EXPLICIT_ADD', NULL, ?4)",
            rusqlite::params![
                Uuid::now_v7().to_string(),
                fake_relation_id.to_string(),
                provenance,
                Utc::now().to_rfc3339()
            ],
        )
        .unwrap_or_else(|e| panic!("[req_mig6] FAIL - valid provenance {provenance} rejected: {e:?}"));
    }
    let bad_provenance = conn.execute(
        "INSERT INTO relation_audit_log (id, relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, 'AddRelation', 'UI', 'NotAProvenance', 'EXPLICIT_ADD', NULL, ?3)",
        rusqlite::params![
            Uuid::now_v7().to_string(),
            fake_relation_id.to_string(),
            Utc::now().to_rfc3339()
        ],
    );
    assert!(
        bad_provenance.is_err(),
        "[req_mig6] FAIL - invalid provenance was NOT rejected"
    );

    conn.execute(
        "INSERT INTO relation_audit_log (id, relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, 'CorrectRelation', 'UI', 'Explicit', 'IMPORTANT_CHANGE', ?3, ?4)",
        rusqlite::params![
            Uuid::now_v7().to_string(),
            fake_relation_id.to_string(),
            "{\"some\":\"snapshot\"}",
            Utc::now().to_rfc3339()
        ],
    )
    .unwrap();

    println!("[req_mig6] PASS - relation_audit_log actor/provenance CHECK constraints accept every frozen value and reject an unknown one; previous_snapshot accepts NULL and non-NULL text");
}

/// K. `relation_id` accepts a syntactically valid, nonexistent
/// `RelationId` - directly proving there is NO foreign key.
#[test]
fn req_mig7_relation_audit_log_relation_id_is_not_a_foreign_key() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "mig7.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let nonexistent = RelationId::new();
    let result = conn.execute(
        "INSERT INTO relation_audit_log (id, relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at)
         VALUES (?1, ?2, 'AddRelation', 'UI', 'Explicit', 'EXPLICIT_ADD', NULL, ?3)",
        rusqlite::params![
            Uuid::now_v7().to_string(),
            nonexistent.to_string(),
            Utc::now().to_rfc3339()
        ],
    );
    assert!(
        result.is_ok(),
        "[req_mig7] FAIL - relation_id rejected a syntactically valid nonexistent RelationId - it must NOT be a foreign key: {result:?}"
    );
    assert_eq!(
        relation_row_count(&conn),
        0,
        "[req_mig7] FAIL - no relations row should exist for this test"
    );
    println!("[req_mig7] PASS - relation_audit_log.relation_id accepts a nonexistent RelationId, proving it is deliberately NOT a foreign key");
}

/// L. `relations` itself gained NO columns from V8 - byte-for-byte the
/// frozen V7 shape, no `updated_at`, no `superseded_by_relation_id`.
#[test]
fn req_mig8_relations_table_not_altered_by_v8() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "mig8.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let mut cols: Vec<String> = Vec::new();
    let mut stmt = conn.prepare("PRAGMA table_info(relations)").unwrap();
    let mut rows = stmt.query([]).unwrap();
    while let Some(row) = rows.next().unwrap() {
        let name: String = row.get(1).unwrap();
        cols.push(name);
    }
    assert_eq!(
        cols,
        vec![
            "id",
            "from_entity_id",
            "to_entity_id",
            "relation_type",
            "note",
            "epistemic_state",
            "confidence",
            "sensitivity",
            "valid_from",
            "valid_until",
            "created_at",
        ],
        "[req_mig8] FAIL - relations columns changed by V8 - must remain exactly the frozen V7 shape"
    );
    println!("[req_mig8] PASS - relations table is byte-for-byte unaltered by V8: no updated_at, no superseded_by_relation_id");
}

/// M. An existing V7-shaped raw `Relation` fixture row remains fully
/// schema-compatible after the V8 migration.
#[test]
fn req_mig9_existing_v7_relation_raw_fixtures_remain_compatible_after_v8() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "mig9.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let from = mk_pk_entity("person", "Fixture From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &from).unwrap();
    let to = mk_pk_entity("project", "Fixture To", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &to).unwrap();

    let relation = mk_pk_relation(
        from.id,
        to.id,
        "collaborates_with",
        EpistemicState::Observed,
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_relation(&conn, &relation).unwrap();

    let fetched = get_relation(&conn, relation.id)
        .unwrap()
        .expect("[req_mig9] FAIL - fixture Relation not found after V8 migration");
    assert_eq!(
        fetched, relation,
        "[req_mig9] FAIL - V7 Relation fixture round-trip mismatch after V8 migration"
    );
    println!("[req_mig9] PASS - a raw V7-shaped Relation fixture row remains fully schema-compatible after the V8 migration");
}

// --- Relation read-side proofs (R026 §6) ---------------------------------

/// `get_relation` round-trips every field exactly.
#[test]
fn req_pkrt3_get_relation_round_trip_all_fields() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrt3.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let from = mk_pk_entity("person", "Relation From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &from).unwrap();
    let to = mk_pk_entity("project", "Relation To", Sensitivity::Public);
    storage::personal_knowledge_repo::insert_entity(&conn, &to).unwrap();

    let mut relation = mk_pk_relation(
        from.id,
        to.id,
        "works_on",
        EpistemicState::UserConfirmed,
        Sensitivity::Internal,
    );
    relation.note = Some("full round trip note".to_string());
    relation.confidence = Some(0.85);
    storage::personal_knowledge_repo::insert_relation(&conn, &relation).unwrap();

    let fetched = get_relation(&conn, relation.id)
        .unwrap()
        .expect("[req_pkrt3] FAIL - relation not found");
    assert_eq!(
        fetched, relation,
        "[req_pkrt3] FAIL - full-field round-trip mismatch"
    );
    println!("[req_pkrt3] PASS - get_relation round-trips every field exactly");
}

/// `note`/`confidence`/`valid_until` all round-trip as `None` when NULL.
#[test]
fn req_pkrt4_get_relation_nullable_fields_none() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrt4.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    let from = mk_pk_entity("person", "Nullable From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &from).unwrap();
    let to = mk_pk_entity("project", "Nullable To", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &to).unwrap();

    let relation = mk_pk_relation(
        from.id,
        to.id,
        "nullable_test",
        EpistemicState::Observed,
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_relation(&conn, &relation).unwrap();

    let fetched = get_relation(&conn, relation.id).unwrap().unwrap();
    assert!(
        fetched.note.is_none(),
        "[req_pkrt4] FAIL - note should be None"
    );
    assert!(
        fetched.confidence.is_none(),
        "[req_pkrt4] FAIL - confidence should be None"
    );
    assert!(
        fetched.valid_until.is_none(),
        "[req_pkrt4] FAIL - valid_until should be None"
    );
    println!("[req_pkrt4] PASS - note/confidence/valid_until all round-trip as None when NULL");
}

/// All 5 `EpistemicState` values parse correctly for `Relation`.
#[test]
fn req_pkrt5_get_relation_all_epistemic_states_parse() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrt5.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let from = mk_pk_entity("person", "ES From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &from).unwrap();

    for state in [
        EpistemicState::UserConfirmed,
        EpistemicState::Observed,
        EpistemicState::Inferred,
        EpistemicState::Disputed,
        EpistemicState::Stale,
    ] {
        let to = mk_pk_entity(
            "project",
            &format!("ES To {state:?}"),
            Sensitivity::Internal,
        );
        storage::personal_knowledge_repo::insert_entity(&conn, &to).unwrap();
        let relation = mk_pk_relation(
            from.id,
            to.id,
            "epistemic_test",
            state,
            Sensitivity::Internal,
        );
        storage::personal_knowledge_repo::insert_relation(&conn, &relation).unwrap();
        let fetched = get_relation(&conn, relation.id).unwrap().unwrap();
        assert_eq!(
            fetched.epistemic_state, state,
            "[req_pkrt5] FAIL - {state:?} did not round-trip"
        );
    }
    println!("[req_pkrt5] PASS - all 5 EpistemicState values parse correctly for Relation");
}

/// All 4 `Sensitivity` values parse correctly for `Relation`.
#[test]
fn req_pkrt6_get_relation_all_sensitivity_values_parse() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrt6.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let from = mk_pk_entity("person", "Sens From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &from).unwrap();

    for sens in [
        Sensitivity::Public,
        Sensitivity::Internal,
        Sensitivity::Sensitive,
        Sensitivity::NeverLeavesDevice,
    ] {
        let to = mk_pk_entity(
            "project",
            &format!("Sens To {sens:?}"),
            Sensitivity::Internal,
        );
        storage::personal_knowledge_repo::insert_entity(&conn, &to).unwrap();
        let relation = mk_pk_relation(
            from.id,
            to.id,
            "sensitivity_test",
            EpistemicState::Observed,
            sens,
        );
        storage::personal_knowledge_repo::insert_relation(&conn, &relation).unwrap();
        let fetched = get_relation(&conn, relation.id).unwrap().unwrap();
        assert_eq!(
            fetched.sensitivity, sens,
            "[req_pkrt6] FAIL - {sens:?} did not round-trip"
        );
    }
    println!("[req_pkrt6] PASS - all 4 Sensitivity values parse correctly for Relation");
}

/// An arbitrary open-vocabulary `relation_type` round-trips exactly - no
/// enum conversion, never confused with `GoalRelationType`.
#[test]
fn req_pkrt7_get_relation_open_relation_type_parses() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrt7.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    let from = mk_pk_entity("person", "Type From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &from).unwrap();
    let to = mk_pk_entity("project", "Type To", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &to).unwrap();

    let relation = mk_pk_relation(
        from.id,
        to.id,
        "THIS_IS_NOT_A_GOALRELATION_VALUE",
        EpistemicState::Observed,
        Sensitivity::Internal,
    );
    storage::personal_knowledge_repo::insert_relation(&conn, &relation).unwrap();
    let fetched = get_relation(&conn, relation.id).unwrap().unwrap();
    assert_eq!(fetched.relation_type, "THIS_IS_NOT_A_GOALRELATION_VALUE");
    println!("[req_pkrt7] PASS - an arbitrary open-vocabulary relation_type round-trips exactly, no enum conversion");
}

/// A nonexistent id returns `None`, never an error.
#[test]
fn req_pkrt8_get_relation_nonexistent_id_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "pkrt8.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();
    assert!(get_relation(&conn, RelationId::new()).unwrap().is_none());
    println!("[req_pkrt8] PASS - get_relation returns None for a nonexistent id");
}

/// `is_active_relation` A-D matrix - pure logic, no I/O, mirroring
/// `is_active_fact` exactly.
#[test]
fn req_iar_is_active_relation_matrix() {
    let from = EntityId::new();
    let to = EntityId::new();
    let mk = |state: EpistemicState, valid_until: Option<chrono::DateTime<Utc>>| Relation {
        id: RelationId::new(),
        from_entity_id: from,
        to_entity_id: to,
        relation_type: "test".to_string(),
        note: None,
        epistemic_state: state,
        confidence: None,
        sensitivity: Sensitivity::Internal,
        valid_from: Utc::now(),
        valid_until,
        created_at: Utc::now(),
    };

    assert!(
        is_active_relation(&mk(EpistemicState::UserConfirmed, None)),
        "[req_iar_a] FAIL - valid_until None + non-STALE must be active"
    );
    assert!(
        !is_active_relation(&mk(EpistemicState::UserConfirmed, Some(Utc::now()))),
        "[req_iar_b] FAIL - valid_until Some + non-STALE must be inactive"
    );
    assert!(
        !is_active_relation(&mk(EpistemicState::Stale, None)),
        "[req_iar_c] FAIL - valid_until None + STALE must be inactive"
    );
    assert!(
        !is_active_relation(&mk(EpistemicState::Stale, Some(Utc::now()))),
        "[req_iar_d] FAIL - valid_until Some + STALE must be inactive"
    );
    println!("[req_iar] PASS - is_active_relation matrix A-D all correct, mirroring is_active_fact exactly");
}

// --- AddRelation: confirmation gate/matrix (R026 §12-14/§39) ------------

#[test]
fn req_ar_conf_a_explicit_unconfirmed_executes_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_conf_a.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    match add_relation(&mut conn, ui_explicit(), mk_ar_payload()) {
        ActionOutcome::Success {
            confirmation_class, ..
        } => assert_eq!(confirmation_class, ConfirmationClass::ExplicitAdd),
        other => panic!("[req_ar_conf_a] FAIL - expected Success, got {other:?}"),
    }
    assert_eq!(relation_row_count(&conn), 1);
}

/// Central safety proof: `Provenance::Inferred` at `AddRelation`'s
/// `ExplicitAdd` floor must block with genuinely zero writes across
/// every table it could otherwise touch - no override code exists for
/// this (unlike `CorrectRelation`, not implemented this run), proving
/// the *generic* `effective_class`/`blocks_until_confirmed` machinery
/// alone is sufficient, matching `AddMemory`.
#[test]
fn req_ar_conf_b_inferred_unconfirmed_blocks_with_zero_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_conf_b.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    match add_relation(&mut conn, meta, mk_ar_payload()) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange)
        }
        other => panic!("[req_ar_conf_b] FAIL - expected ConfirmationRequired, got {other:?}"),
    }
    assert_eq!(
        ar_full_table_snapshot(&conn),
        (1, 0, 0, 0, 0, 0),
        "[req_ar_conf_b] FAIL - zero writes expected across every Relation-family table (only the bootstrap SELF entity should exist)"
    );
}

#[test]
fn req_ar_conf_c_inferred_confirmed_persists_user_confirmed_and_audit_provenance_inferred() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_conf_c.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let mut meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    meta.confirmed = true;
    let payload = AddRelationPayload {
        epistemic_state: EpistemicState::Observed,
        ..mk_ar_payload()
    };
    let relation_id;
    match add_relation(&mut conn, meta, payload) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(
                value.epistemic_state,
                EpistemicState::UserConfirmed,
                "[req_ar_conf_c] FAIL - a confirmed Inferred resubmission must persist USER_CONFIRMED, not the payload's originally-proposed state"
            );
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange);
            relation_id = value.id;
        }
        other => panic!("[req_ar_conf_c] FAIL - expected Success, got {other:?}"),
    }
    let provenance: String = conn
        .query_row(
            "SELECT provenance FROM relation_audit_log WHERE relation_id = ?1",
            rusqlite::params![relation_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        provenance, "Inferred",
        "[req_ar_conf_c] FAIL - audit provenance must remain Inferred, never silently rewritten to Explicit"
    );
}

#[test]
fn req_ar_conf_d_structurally_invalid_inferred_unconfirmed_returns_validation_failed_before_gate() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_conf_d.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    let payload = AddRelationPayload {
        relation_type: "   ".to_string(),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, meta, payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::PersonalKnowledgeFieldEmpty {
                field: "relation_type"
            }));
        }
        other => panic!(
            "[req_ar_conf_d] FAIL - expected ValidationFailed BEFORE the confirmation gate, got {other:?}"
        ),
    }
}

/// Post-run correctness boundary (mirrors `req_am_conf_gate_precedes_db_dependent_validation`
/// exactly): a structurally valid but database-invalid payload (an
/// `Existing` reference to a nonexistent `EntityId`/`SourceId`, each
/// tried independently) must still return `ConfirmationRequired` for an
/// unconfirmed `Inferred` call - never `ValidationFailed` - proving the
/// database-dependent resolution stage was never reached.
#[test]
fn req_ar_conf_gate_precedes_db_dependent_validation() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_conf_gate_order.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);

    let payload_from = AddRelationPayload {
        from: EntityInputRef::Existing(EntityId::new()),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, meta, payload_from) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange)
        }
        other => panic!(
            "[req_ar_conf_gate_order] FAIL - gate must precede DB-dependent from-resolution, got {other:?}"
        ),
    }

    let payload_to = AddRelationPayload {
        to: EntityInputRef::Existing(EntityId::new()),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, meta, payload_to) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange)
        }
        other => panic!(
            "[req_ar_conf_gate_order] FAIL - gate must precede DB-dependent to-resolution, got {other:?}"
        ),
    }

    let payload_source = AddRelationPayload {
        source: SourceRef::Existing(SourceId::new()),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, meta, payload_source) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::InferredChange)
        }
        other => panic!(
            "[req_ar_conf_gate_order] FAIL - gate must precede DB-dependent source-resolution, got {other:?}"
        ),
    }

    assert_eq!(
        ar_full_table_snapshot(&conn),
        (1, 0, 0, 0, 0, 0),
        "[req_ar_conf_gate_order] FAIL - zero writes expected"
    );
}

/// The only path that can persist a deliberate non-`USER_CONFIRMED`
/// state (mirrors `AddMemory`'s `HYPOTHESIS`+`INFERRED` precedent): a
/// direct `Explicit` call persists exactly the caller's requested
/// `epistemic_state`.
#[test]
fn req_ar_explicit_persists_payload_epistemic_state_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_explicit_state.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let payload = AddRelationPayload {
        epistemic_state: EpistemicState::Disputed,
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { value, .. } => assert_eq!(
            value.epistemic_state,
            EpistemicState::Disputed,
            "[req_ar_explicit_state] FAIL - explicit call must persist exactly the requested epistemic_state"
        ),
        other => panic!("[req_ar_explicit_state] FAIL - expected Success, got {other:?}"),
    }
}

// --- AddRelation: endpoint matrix (R026 §15/§19 A-K) ---------------------

/// A-F: every Self/Existing/New from/to combination succeeds correctly.
#[test]
fn req_ar_endpoint_matrix_success_combinations() {
    // A: Self -> Existing
    {
        let dir = tempfile::tempdir().unwrap();
        let (path, key) = temp_db(&dir, "ar_ep_a.db");
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let existing = mk_pk_entity("project", "EP-A Existing", Sensitivity::Internal);
        storage::personal_knowledge_repo::insert_entity(&conn, &existing).unwrap();
        let self_id = self_entity_id(&conn);
        let payload = AddRelationPayload {
            from: EntityInputRef::SelfEntity,
            to: EntityInputRef::Existing(existing.id),
            ..mk_ar_payload()
        };
        match add_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.from_entity_id, self_id);
                assert_eq!(value.to_entity_id, existing.id);
            }
            other => panic!("[req_ar_ep_a] FAIL - expected Success, got {other:?}"),
        }
    }
    // B: Existing -> Self
    {
        let dir = tempfile::tempdir().unwrap();
        let (path, key) = temp_db(&dir, "ar_ep_b.db");
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let existing = mk_pk_entity("person", "EP-B Existing", Sensitivity::Internal);
        storage::personal_knowledge_repo::insert_entity(&conn, &existing).unwrap();
        let self_id = self_entity_id(&conn);
        let payload = AddRelationPayload {
            from: EntityInputRef::Existing(existing.id),
            to: EntityInputRef::SelfEntity,
            ..mk_ar_payload()
        };
        match add_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.from_entity_id, existing.id);
                assert_eq!(value.to_entity_id, self_id);
            }
            other => panic!("[req_ar_ep_b] FAIL - expected Success, got {other:?}"),
        }
    }
    // C: Existing -> Existing (different)
    {
        let dir = tempfile::tempdir().unwrap();
        let (path, key) = temp_db(&dir, "ar_ep_c.db");
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let e1 = mk_pk_entity("person", "EP-C From", Sensitivity::Internal);
        storage::personal_knowledge_repo::insert_entity(&conn, &e1).unwrap();
        let e2 = mk_pk_entity("project", "EP-C To", Sensitivity::Internal);
        storage::personal_knowledge_repo::insert_entity(&conn, &e2).unwrap();
        let payload = AddRelationPayload {
            from: EntityInputRef::Existing(e1.id),
            to: EntityInputRef::Existing(e2.id),
            ..mk_ar_payload()
        };
        match add_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.from_entity_id, e1.id);
                assert_eq!(value.to_entity_id, e2.id);
            }
            other => panic!("[req_ar_ep_c] FAIL - expected Success, got {other:?}"),
        }
    }
    // D: New -> Existing
    {
        let dir = tempfile::tempdir().unwrap();
        let (path, key) = temp_db(&dir, "ar_ep_d.db");
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let existing = mk_pk_entity("project", "EP-D Existing", Sensitivity::Internal);
        storage::personal_knowledge_repo::insert_entity(&conn, &existing).unwrap();
        let payload = AddRelationPayload {
            from: mk_ar_to_new("person", "EP-D New From"),
            to: EntityInputRef::Existing(existing.id),
            ..mk_ar_payload()
        };
        match add_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.to_entity_id, existing.id);
                assert!(get_entity(&conn, value.from_entity_id).unwrap().is_some());
            }
            other => panic!("[req_ar_ep_d] FAIL - expected Success, got {other:?}"),
        }
    }
    // E: Existing -> New
    {
        let dir = tempfile::tempdir().unwrap();
        let (path, key) = temp_db(&dir, "ar_ep_e.db");
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let existing = mk_pk_entity("person", "EP-E Existing", Sensitivity::Internal);
        storage::personal_knowledge_repo::insert_entity(&conn, &existing).unwrap();
        let payload = AddRelationPayload {
            from: EntityInputRef::Existing(existing.id),
            to: mk_ar_to_new("project", "EP-E New To"),
            ..mk_ar_payload()
        };
        match add_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.from_entity_id, existing.id);
                assert!(get_entity(&conn, value.to_entity_id).unwrap().is_some());
            }
            other => panic!("[req_ar_ep_e] FAIL - expected Success, got {other:?}"),
        }
    }
    // F: New -> New (different Entities)
    {
        let dir = tempfile::tempdir().unwrap();
        let (path, key) = temp_db(&dir, "ar_ep_f.db");
        let mut conn = bootstrap_db_at(&path, &key).unwrap();
        let payload = AddRelationPayload {
            from: mk_ar_to_new("person", "EP-F New From"),
            to: mk_ar_to_new("project", "EP-F New To"),
            ..mk_ar_payload()
        };
        match add_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => {
                assert_ne!(value.from_entity_id, value.to_entity_id);
                assert!(get_entity(&conn, value.from_entity_id).unwrap().is_some());
                assert!(get_entity(&conn, value.to_entity_id).unwrap().is_some());
            }
            other => panic!("[req_ar_ep_f] FAIL - expected Success, got {other:?}"),
        }
    }
    println!("[req_ar_endpoint_matrix_success] PASS - A-F: Self/Existing/New in every from/to combination succeed correctly");
}

/// G: Self -> Self rejected.
#[test]
fn req_ar_endpoint_g_self_to_self_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_ep_g.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let payload = AddRelationPayload {
        from: EntityInputRef::SelfEntity,
        to: EntityInputRef::SelfEntity,
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::SelfPersonalKnowledgeRelation))
        }
        other => panic!(
            "[req_ar_ep_g] FAIL - expected ValidationFailed(SelfPersonalKnowledgeRelation), got {other:?}"
        ),
    }
    assert_eq!(relation_row_count(&conn), 0);
}

/// H: Existing(X) -> Existing(X) rejected.
#[test]
fn req_ar_endpoint_h_existing_x_to_existing_x_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_ep_h.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let x = mk_pk_entity("person", "EP-H X", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &x).unwrap();
    let payload = AddRelationPayload {
        from: EntityInputRef::Existing(x.id),
        to: EntityInputRef::Existing(x.id),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::SelfPersonalKnowledgeRelation))
        }
        other => panic!(
            "[req_ar_ep_h] FAIL - expected ValidationFailed(SelfPersonalKnowledgeRelation), got {other:?}"
        ),
    }
    assert_eq!(relation_row_count(&conn), 0);
}

/// I: `SelfEntity` + `Existing(actual SELF id)` - the pure structural
/// fast path in `validate_add_relation` cannot catch this (it never
/// resolves `SelfEntity` to a concrete id); only the AUTHORITATIVE check
/// in `services::add_relation`, after resolution, catches it.
#[test]
fn req_ar_endpoint_i_self_to_existing_self_id_rejected_after_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_ep_i.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let self_id = self_entity_id(&conn);
    let payload = AddRelationPayload {
        from: EntityInputRef::SelfEntity,
        to: EntityInputRef::Existing(self_id),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::SelfPersonalKnowledgeRelation))
        }
        other => panic!(
            "[req_ar_ep_i] FAIL - expected ValidationFailed(SelfPersonalKnowledgeRelation), got {other:?}"
        ),
    }
    assert_eq!(relation_row_count(&conn), 0);
}

/// J: missing `from` Existing reference.
#[test]
fn req_ar_endpoint_j_missing_from_existing_returns_validation_failed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_ep_j.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let payload = AddRelationPayload {
        from: EntityInputRef::Existing(EntityId::new()),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::ReferencedEntityNotFound))
        }
        other => panic!(
            "[req_ar_ep_j] FAIL - expected ValidationFailed(ReferencedEntityNotFound), got {other:?}"
        ),
    }
    assert_eq!(relation_row_count(&conn), 0);
}

/// K: missing `to` Existing reference.
#[test]
fn req_ar_endpoint_k_missing_to_existing_returns_validation_failed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_ep_k.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let payload = AddRelationPayload {
        to: EntityInputRef::Existing(EntityId::new()),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::ReferencedEntityNotFound))
        }
        other => panic!(
            "[req_ar_ep_k] FAIL - expected ValidationFailed(ReferencedEntityNotFound), got {other:?}"
        ),
    }
    assert_eq!(relation_row_count(&conn), 0);
}

// --- AddRelation: duplicate domain_ref (R026 §11/§17 A-D) ---------------

/// A/D: same-payload duplicate - both `from`/`to` are `New` with the
/// SAME `domain_ref` - rejected by the PURE, no-SQL same-payload
/// preflight check.
#[test]
fn req_ar_dup_domainref_same_payload_both_new_same_ref_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_dup_a.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("AR dup goal a")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let domain_ref = DomainRef {
        domain_ref_type: "Goal".to_string(),
        domain_ref_id: goal.id.to_string(),
    };
    let from = EntityInputRef::New(NewEntityData {
        entity_type: "goal".to_string(),
        canonical_name: "AR Dup From".to_string(),
        sensitivity: Sensitivity::Internal,
        domain_ref: Some(domain_ref.clone()),
    });
    let to = EntityInputRef::New(NewEntityData {
        entity_type: "goal".to_string(),
        canonical_name: "AR Dup To".to_string(),
        sensitivity: Sensitivity::Internal,
        domain_ref: Some(domain_ref),
    });
    let payload = AddRelationPayload {
        from,
        to,
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::DuplicateDomainReference))
        }
        other => panic!(
            "[req_ar_dup_a] FAIL - expected ValidationFailed(DuplicateDomainReference), got {other:?}"
        ),
    }
    assert_eq!(relation_row_count(&conn), 0);
    assert_eq!(
        entity_row_count(&conn),
        1,
        "[req_ar_dup_a] FAIL - only SELF should exist, zero writes"
    );
}

/// B: persisted duplicate on `from`.
#[test]
fn req_ar_dup_domainref_persisted_on_from_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_dup_b.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("AR dup goal b")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let domain_ref = DomainRef {
        domain_ref_type: "Goal".to_string(),
        domain_ref_id: goal.id.to_string(),
    };
    let mut existing_entity = mk_pk_entity("goal", "AR Dup B Existing", Sensitivity::Internal);
    existing_entity.domain_ref = Some(domain_ref.clone());
    storage::personal_knowledge_repo::insert_entity(&conn, &existing_entity).unwrap();

    let from = EntityInputRef::New(NewEntityData {
        entity_type: "goal".to_string(),
        canonical_name: "AR Dup B New".to_string(),
        sensitivity: Sensitivity::Internal,
        domain_ref: Some(domain_ref),
    });
    let payload = AddRelationPayload {
        from,
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::DuplicateDomainReference))
        }
        other => panic!(
            "[req_ar_dup_b] FAIL - expected ValidationFailed(DuplicateDomainReference), got {other:?}"
        ),
    }
    assert_eq!(relation_row_count(&conn), 0);
}

/// C: persisted duplicate on `to`.
#[test]
fn req_ar_dup_domainref_persisted_on_to_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_dup_c.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let goal = match create_goal(&mut conn, ui_explicit(), mk_create_payload("AR dup goal c")) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("setup failed: {other:?}"),
    };
    let domain_ref = DomainRef {
        domain_ref_type: "Goal".to_string(),
        domain_ref_id: goal.id.to_string(),
    };
    let mut existing_entity = mk_pk_entity("goal", "AR Dup C Existing", Sensitivity::Internal);
    existing_entity.domain_ref = Some(domain_ref.clone());
    storage::personal_knowledge_repo::insert_entity(&conn, &existing_entity).unwrap();

    let to = EntityInputRef::New(NewEntityData {
        entity_type: "goal".to_string(),
        canonical_name: "AR Dup C New".to_string(),
        sensitivity: Sensitivity::Internal,
        domain_ref: Some(domain_ref),
    });
    let payload = AddRelationPayload {
        to,
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::DuplicateDomainReference))
        }
        other => panic!(
            "[req_ar_dup_c] FAIL - expected ValidationFailed(DuplicateDomainReference), got {other:?}"
        ),
    }
    assert_eq!(relation_row_count(&conn), 0);
}

// --- AddRelation: no duplicate-edge dedup (R026 §20) ---------------------

/// R025 froze NO Relation deduplication - two successful AddRelation
/// calls with identical (from, relation_type, to) create two distinct
/// Relations with distinct RelationIds; both rows survive.
#[test]
fn req_ar_no_dedup_two_identical_edges_both_survive_distinct_ids() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_no_dedup.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let to_entity = mk_pk_entity("project", "No Dedup Target", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &to_entity).unwrap();
    let payload = AddRelationPayload {
        from: EntityInputRef::SelfEntity,
        to: EntityInputRef::Existing(to_entity.id),
        relation_type: "works_on".to_string(),
        ..mk_ar_payload()
    };

    let id1 = match add_relation(&mut conn, ui_explicit(), payload.clone()) {
        ActionOutcome::Success { value, .. } => value.id,
        other => panic!("[req_ar_no_dedup] FAIL - first call: expected Success, got {other:?}"),
    };
    let id2 = match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { value, .. } => value.id,
        other => panic!("[req_ar_no_dedup] FAIL - second call: expected Success, got {other:?}"),
    };
    assert_ne!(
        id1, id2,
        "[req_ar_no_dedup] FAIL - two independent AddRelation calls must create distinct RelationIds"
    );
    assert_eq!(
        relation_row_count(&conn),
        2,
        "[req_ar_no_dedup] FAIL - both identical-looking Relations must survive, no implicit dedup/merge/auto-STALE"
    );
}

// --- AddRelation: relation_type policy (R026 §16/§36) --------------------

#[test]
fn req_ar_relation_type_open_value_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_rt_open.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let payload = AddRelationPayload {
        relation_type: "THIS_IS_NOT_A_GOALRELATION_VALUE".to_string(),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.relation_type, "THIS_IS_NOT_A_GOALRELATION_VALUE")
        }
        other => panic!("[req_ar_rt_open] FAIL - expected Success, got {other:?}"),
    }
}

#[test]
fn req_ar_relation_type_blank_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_rt_blank.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let payload = AddRelationPayload {
        relation_type: "   ".to_string(),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(
                errors.contains(&ValidationError::PersonalKnowledgeFieldEmpty {
                    field: "relation_type"
                })
            )
        }
        other => panic!("[req_ar_rt_blank] FAIL - expected ValidationFailed, got {other:?}"),
    }
    assert_eq!(relation_row_count(&conn), 0);
}

#[test]
fn req_ar_relation_type_length_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_rt_len.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let exact = "x".repeat(crate::domain::RELATION_TYPE_MAX_LEN);
    let payload_exact = AddRelationPayload {
        relation_type: exact.clone(),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload_exact) {
        ActionOutcome::Success { value, .. } => assert_eq!(value.relation_type, exact),
        other => {
            panic!("[req_ar_rt_len] FAIL - exact-length relation_type rejected, got {other:?}")
        }
    }

    let too_long = "x".repeat(crate::domain::RELATION_TYPE_MAX_LEN + 1);
    let payload_too_long = AddRelationPayload {
        relation_type: too_long,
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload_too_long) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.iter().any(|e| matches!(
                e,
                ValidationError::PersonalKnowledgeFieldTooLong { field, .. } if *field == "relation_type"
            )))
        }
        other => {
            panic!("[req_ar_rt_len] FAIL - N+1-length relation_type NOT rejected, got {other:?}")
        }
    }
}

// --- AddRelation: note policy (R026 §16/§37) -----------------------------

#[test]
fn req_ar_note_none_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_note_none.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let payload = AddRelationPayload {
        note: None,
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { value, .. } => assert!(value.note.is_none()),
        other => panic!("[req_ar_note_none] FAIL - expected Success, got {other:?}"),
    }
}

/// R025 §12's frozen policy: no trim, no semantic normalization -
/// empty/whitespace-only note is accepted and preserved exactly.
#[test]
fn req_ar_note_empty_and_whitespace_preserved_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_note_ws.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let payload_empty = AddRelationPayload {
        note: Some("".to_string()),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload_empty) {
        ActionOutcome::Success { value, .. } => assert_eq!(
            value.note,
            Some("".to_string()),
            "[req_ar_note_ws] FAIL - empty note must be preserved exactly, no trim-to-None"
        ),
        other => panic!("[req_ar_note_ws] FAIL - expected Success for empty note, got {other:?}"),
    }

    let payload_ws = AddRelationPayload {
        note: Some("   ".to_string()),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload_ws) {
        ActionOutcome::Success { value, .. } => assert_eq!(
            value.note,
            Some("   ".to_string()),
            "[req_ar_note_ws] FAIL - whitespace-only note must be preserved exactly"
        ),
        other => {
            panic!("[req_ar_note_ws] FAIL - expected Success for whitespace note, got {other:?}")
        }
    }
}

#[test]
fn req_ar_note_length_bounds() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_note_len.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let exact = "n".repeat(crate::domain::RELATION_NOTE_MAX_LEN);
    let payload_exact = AddRelationPayload {
        note: Some(exact.clone()),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload_exact) {
        ActionOutcome::Success { value, .. } => assert_eq!(value.note, Some(exact)),
        other => panic!("[req_ar_note_len] FAIL - exact-length note rejected, got {other:?}"),
    }

    let too_long = "n".repeat(crate::domain::RELATION_NOTE_MAX_LEN + 1);
    let payload_too_long = AddRelationPayload {
        note: Some(too_long),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload_too_long) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.iter().any(|e| matches!(
                e,
                ValidationError::PersonalKnowledgeFieldTooLong { field, .. } if *field == "note"
            )))
        }
        other => panic!("[req_ar_note_len] FAIL - N+1-length note NOT rejected, got {other:?}"),
    }
}

// --- AddRelation: confidence matrix (R026 §38) ---------------------------

#[test]
fn req_ar_confidence_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_confidence.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    for accepted in [None, Some(0.0f32), Some(1.0f32), Some(0.5f32)] {
        let payload = AddRelationPayload {
            confidence: accepted,
            ..mk_ar_payload()
        };
        match add_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => assert_eq!(value.confidence, accepted),
            other => {
                panic!("[req_ar_confidence] FAIL - {accepted:?} should be accepted, got {other:?}")
            }
        }
    }

    for rejected in [
        -0.01f32,
        1.01f32,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ] {
        let payload = AddRelationPayload {
            confidence: Some(rejected),
            ..mk_ar_payload()
        };
        match add_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::ValidationFailed(errors) => assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::ConfidenceOutOfBounds { .. }))),
            other => {
                panic!("[req_ar_confidence] FAIL - {rejected:?} should be rejected, got {other:?}")
            }
        }
    }
}

// --- AddRelation: Source/Evidence lifecycle (R026 §21/§22/§35) ----------

#[test]
fn req_ar_source_existing_reused_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_source_existing.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let source = mk_pk_source("document", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_source(&conn, &source).unwrap();
    let before = read_source_fields(&conn, source.id);

    let payload = AddRelationPayload {
        source: SourceRef::Existing(source.id),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_ar_source_existing] FAIL - expected Success, got {other:?}"),
    }
    assert_eq!(
        before,
        read_source_fields(&conn, source.id),
        "[req_ar_source_existing] FAIL - Existing Source must never be mutated"
    );
    assert_eq!(
        am_source_row_count(&conn),
        1,
        "[req_ar_source_existing] FAIL - no new Source should be created"
    );
}

/// Evidence is always fresh (never reused across two `AddRelation`
/// calls sharing the same Source), and the resulting `relation_evidence`
/// link is exact: `relation_id`/`evidence_id`/`created_at` correct,
/// exactly one link per Relation, Evidence row exists.
#[test]
fn req_ar_evidence_always_fresh_and_relation_evidence_link_defense() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_evidence_fresh.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let source = mk_pk_source("document", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_source(&conn, &source).unwrap();

    let payload = AddRelationPayload {
        source: SourceRef::Existing(source.id),
        ..mk_ar_payload()
    };
    let relation1 = match add_relation(&mut conn, ui_explicit(), payload.clone()) {
        ActionOutcome::Success { value, .. } => value,
        other => {
            panic!("[req_ar_evidence_fresh] FAIL - first call: expected Success, got {other:?}")
        }
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { .. } => {}
        other => {
            panic!("[req_ar_evidence_fresh] FAIL - second call: expected Success, got {other:?}")
        }
    }

    assert_eq!(
        am_evidence_row_count(&conn),
        2,
        "[req_ar_evidence_fresh] FAIL - each AddRelation call must create its own fresh Evidence, never reuse"
    );

    let (rel_id, ev_id, created_at): (String, String, String) = conn
        .query_row(
            "SELECT relation_id, evidence_id, created_at FROM relation_evidence WHERE relation_id = ?1",
            rusqlite::params![relation1.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(rel_id, relation1.id.to_string());
    let link_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM relation_evidence WHERE relation_id = ?1",
            rusqlite::params![relation1.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        link_count, 1,
        "[req_ar_evidence_fresh] FAIL - exactly one relation_evidence link expected per Relation"
    );
    let evidence_row_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM evidence WHERE id = ?1",
            rusqlite::params![ev_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(evidence_row_exists, 1);
    assert_eq!(created_at, relation1.created_at.to_rfc3339());
    println!("[req_ar_evidence_fresh] PASS - Evidence is always fresh; relation_evidence link is exact and unique per Relation");
}

// --- AddRelation: timestamps and audit exactness (R026 §23/§25/§26) -----

#[test]
fn req_ar_timestamps_exact_from_fixed_requested_at() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_timestamps.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let fixed = chrono::DateTime::parse_from_rfc3339("2026-01-15T10:30:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let meta = ActionMeta {
        actor: Actor::Ui,
        provenance: Provenance::Explicit,
        requested_at: fixed,
        confirmed: false,
    };
    match add_relation(&mut conn, meta, mk_ar_payload()) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(
                value.valid_from, fixed,
                "[req_ar_timestamps] FAIL - valid_from must equal meta.requested_at exactly"
            );
            assert_eq!(
                value.created_at, fixed,
                "[req_ar_timestamps] FAIL - created_at must equal meta.requested_at exactly"
            );
            assert!(
                value.valid_until.is_none(),
                "[req_ar_timestamps] FAIL - valid_until must always start None"
            );
        }
        other => panic!("[req_ar_timestamps] FAIL - expected Success, got {other:?}"),
    }
}

#[test]
fn req_ar_audit_exact_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_audit_explicit.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let fixed = chrono::DateTime::parse_from_rfc3339("2026-02-01T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let meta = ActionMeta {
        actor: Actor::CloudLlm,
        provenance: Provenance::Explicit,
        requested_at: fixed,
        confirmed: false,
    };
    let relation = match add_relation(&mut conn, meta, mk_ar_payload()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_ar_audit_explicit] FAIL - expected Success, got {other:?}"),
    };
    let (relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at): (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at FROM relation_audit_log WHERE relation_id = ?1",
            rusqlite::params![relation.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap();
    assert_eq!(relation_id, relation.id.to_string());
    assert_eq!(action_type, "AddRelation");
    assert_eq!(actor, "CloudLLM");
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "EXPLICIT_ADD");
    assert_eq!(previous_snapshot, None);
    assert_eq!(occurred_at, fixed.to_rfc3339());
}

#[test]
fn req_ar_audit_exact_inferred_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_audit_inferred.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let fixed = chrono::DateTime::parse_from_rfc3339("2026-02-02T09:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let meta = ActionMeta {
        actor: Actor::CloudLlm,
        provenance: Provenance::Inferred,
        requested_at: fixed,
        confirmed: true,
    };
    let relation = match add_relation(&mut conn, meta, mk_ar_payload()) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.epistemic_state, EpistemicState::UserConfirmed);
            value
        }
        other => panic!("[req_ar_audit_inferred] FAIL - expected Success, got {other:?}"),
    };
    let (provenance, confirmation_class): (String, String) = conn
        .query_row(
            "SELECT provenance, confirmation_class FROM relation_audit_log WHERE relation_id = ?1",
            rusqlite::params![relation.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(provenance, "Inferred");
    assert_eq!(confirmation_class, "INFERRED_CHANGE");
}

// --- AddRelation: transaction atomicity / rollback (R026 §27-29) --------

/// MANDATORY: the richest possible `AddRelation` (New `from`, New `to`,
/// New Source, fresh Evidence, Relation, `relation_evidence`), with the
/// FINAL audit-log insert forced to fail - every newly-created row must
/// roll back, SELF must survive untouched, and the connection must
/// remain usable afterward.
#[test]
fn req_ar_forced_final_audit_failure_rolls_back_everything() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_rollback_final.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = ar_full_table_snapshot(&conn);

    install_relation_audit_failure_trigger(&conn);
    let payload = AddRelationPayload {
        from: mk_ar_to_new("person", "Rollback New From"),
        to: mk_ar_to_new("project", "Rollback New To"),
        source: mk_am_source_new(),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::StorageError(_) => {}
        other => panic!("[req_ar_rollback_final] FAIL - expected StorageError, got {other:?}"),
    }
    remove_relation_audit_failure_trigger(&conn);

    let after = ar_full_table_snapshot(&conn);
    assert_eq!(
        before, after,
        "[req_ar_rollback_final] FAIL - forced final-audit failure must roll back EVERY newly-created row (Entities/Source/Relation/Evidence/relation_evidence/audit)"
    );
    assert!(
        storage::personal_knowledge_repo::get_self_entity(&conn)
            .unwrap()
            .is_some(),
        "[req_ar_rollback_final] FAIL - SELF must still exist"
    );

    match add_relation(&mut conn, ui_explicit(), mk_ar_payload()) {
        ActionOutcome::Success { .. } => {}
        other => {
            panic!("[req_ar_rollback_final] FAIL - connection unusable after rollback: {other:?}")
        }
    }
}

/// An earlier-stage forced failure (Evidence insert, before the final
/// audit write) must also roll back the already-created Entities/
/// Source/Relation in the same transaction, not only the last step.
#[test]
fn req_ar_earlier_stage_evidence_failure_rolls_back_entities_source_relation() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_rollback_earlier.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = ar_full_table_snapshot(&conn);

    install_evidence_insert_failure_trigger(&conn);
    let payload = AddRelationPayload {
        from: mk_ar_to_new("person", "Earlier Rollback From"),
        to: mk_ar_to_new("project", "Earlier Rollback To"),
        source: mk_am_source_new(),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::StorageError(_) => {}
        other => panic!("[req_ar_rollback_earlier] FAIL - expected StorageError, got {other:?}"),
    }
    remove_evidence_insert_failure_trigger(&conn);

    let after = ar_full_table_snapshot(&conn);
    assert_eq!(
        before, after,
        "[req_ar_rollback_earlier] FAIL - an earlier-stage forced failure (Evidence insert) must also roll back the already-created Entities/Source/Relation in the same transaction"
    );
}

// --- AddRelation: immutability of reused rows (R026 §30/§31) ------------

#[test]
fn req_ar_existing_entity_and_source_immutable_after_success() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_immutable.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let entity = mk_pk_entity("person", "Immutable Entity", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &entity).unwrap();
    let source = mk_pk_source("document", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_source(&conn, &source).unwrap();
    let entity_before = get_entity(&conn, entity.id).unwrap().unwrap();
    let source_before = read_source_fields(&conn, source.id);

    let payload = AddRelationPayload {
        from: EntityInputRef::Existing(entity.id),
        source: SourceRef::Existing(source.id),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_ar_immutable] FAIL - expected Success, got {other:?}"),
    }
    assert_eq!(
        get_entity(&conn, entity.id).unwrap().unwrap(),
        entity_before,
        "[req_ar_immutable] FAIL - Existing Entity must never be mutated"
    );
    assert_eq!(
        read_source_fields(&conn, source.id),
        source_before,
        "[req_ar_immutable] FAIL - Existing Source must never be mutated"
    );
}

#[test]
fn req_ar_self_entity_immutable_after_use_as_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_self_immutable.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let self_before = storage::personal_knowledge_repo::get_self_entity(&conn)
        .unwrap()
        .unwrap();

    match add_relation(&mut conn, ui_explicit(), mk_ar_payload()) {
        ActionOutcome::Success { value, .. } => assert_eq!(value.from_entity_id, self_before.id),
        other => panic!("[req_ar_self_immutable] FAIL - expected Success, got {other:?}"),
    }
    let self_after = storage::personal_knowledge_repo::get_self_entity(&conn)
        .unwrap()
        .unwrap();
    assert_eq!(
        self_after, self_before,
        "[req_ar_self_immutable] FAIL - SELF must be byte-for-byte unchanged after use as a Relation endpoint"
    );
}

// --- AddRelation: zero Fact/Episode side effects, no temporal/inverse
// side effects (R026 §34/§42/§43) ----------------------------------------

#[test]
fn req_ar_zero_fact_episode_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_zero_side_effects.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = relation_episode_side_effect_counts(&conn);
    let before_facts = am_fact_row_count(&conn);
    let before_fact_evidence = am_fact_evidence_row_count(&conn);

    match add_relation(&mut conn, ui_explicit(), mk_ar_payload()) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_ar_zero_side_effects] FAIL - expected Success, got {other:?}"),
    }

    let after = relation_episode_side_effect_counts(&conn);
    // `relations`/`relation_evidence` (indices 0/2) are EXPECTED to
    // change - that is AddRelation's whole point.
    assert_eq!(
        after.1, before.1,
        "[req_ar_zero_side_effects] FAIL - episodes must be untouched"
    );
    assert_eq!(
        after.3, before.3,
        "[req_ar_zero_side_effects] FAIL - episode_evidence must be untouched"
    );
    assert_eq!(
        after.4, before.4,
        "[req_ar_zero_side_effects] FAIL - episode_entities must be untouched"
    );
    assert_eq!(
        am_fact_row_count(&conn),
        before_facts,
        "[req_ar_zero_side_effects] FAIL - facts must be untouched"
    );
    assert_eq!(
        am_fact_evidence_row_count(&conn),
        before_fact_evidence,
        "[req_ar_zero_side_effects] FAIL - fact_evidence must be untouched"
    );
}

#[test]
fn req_ar_no_temporal_side_effects_on_pre_existing_relation() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_no_temporal_side_effects.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let to_entity = mk_pk_entity("project", "Temporal Target", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &to_entity).unwrap();
    let payload = AddRelationPayload {
        from: EntityInputRef::SelfEntity,
        to: EntityInputRef::Existing(to_entity.id),
        relation_type: "works_on".to_string(),
        ..mk_ar_payload()
    };

    let first = match add_relation(&mut conn, ui_explicit(), payload.clone()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_ar_no_temporal] FAIL - first call: expected Success, got {other:?}"),
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_ar_no_temporal] FAIL - second call: expected Success, got {other:?}"),
    }

    let first_after = get_relation(&conn, first.id).unwrap().unwrap();
    assert_eq!(
        first_after, first,
        "[req_ar_no_temporal] FAIL - a second AddRelation must never close/supersede/mark STALE a pre-existing identical Relation"
    );
}

#[test]
fn req_ar_no_inverse_or_symmetric_auto_creation() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_no_inverse.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let to_entity = mk_pk_entity("project", "Inverse Target", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &to_entity).unwrap();
    let self_id = self_entity_id(&conn);
    let payload = AddRelationPayload {
        from: EntityInputRef::SelfEntity,
        to: EntityInputRef::Existing(to_entity.id),
        relation_type: "works_on".to_string(),
        ..mk_ar_payload()
    };
    match add_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_ar_no_inverse] FAIL - expected Success, got {other:?}"),
    }
    assert_eq!(
        relation_row_count(&conn),
        1,
        "[req_ar_no_inverse] FAIL - exactly one edge expected, no automatic inverse/symmetric counterpart"
    );
    let inverse_exists: i64 = conn
        .query_row(
            "SELECT count(*) FROM relations WHERE from_entity_id = ?1 AND to_entity_id = ?2",
            rusqlite::params![to_entity.id.to_string(), self_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        inverse_exists, 0,
        "[req_ar_no_inverse] FAIL - no inverse edge should exist"
    );
}

// --- AddRelation: RelationId generation and output equality (R026 §40/§41) --

#[test]
fn req_ar_relation_id_distinct_and_output_matches_get_relation_readback() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "ar_id_readback.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    let r1 = match add_relation(&mut conn, ui_explicit(), mk_ar_payload()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_ar_id_readback] FAIL - first call: expected Success, got {other:?}"),
    };
    let r2 = match add_relation(&mut conn, ui_explicit(), mk_ar_payload()) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_ar_id_readback] FAIL - second call: expected Success, got {other:?}"),
    };
    assert_ne!(
        r1.id, r2.id,
        "[req_ar_id_readback] FAIL - two AddRelation calls must produce distinct RelationIds"
    );

    let readback1 = get_relation(&conn, r1.id)
        .unwrap()
        .expect("[req_ar_id_readback] FAIL - r1 not found via get_relation");
    assert_eq!(
        readback1, r1,
        "[req_ar_id_readback] FAIL - the returned Relation must exactly equal get_relation's read-back, no partial/in-memory-only representation"
    );
}

// =========================================================================
// M2 Personal Knowledge CorrectRelation (R027, 2026-09-19)
//
// Evidence for
// `docs/reports/R027_M2_PERSONAL_KNOWLEDGE_CORRECTRELATION_2026-09-19.md`.
// The second production Relation mutation - an IN-PLACE metadata
// correction of `note`/`confidence`/`sensitivity`, never a successor
// row, never a change to the frozen `(from_entity_id, relation_type,
// to_entity_id)` identity triple. Database-dependent behavior only -
// pure `validate_correct_relation` structural tests live in
// `actions::tests` instead, same split as every other action.
// =========================================================================

/// Inserts a fresh, active Relation with a known starting
/// `note`/`confidence`/`sensitivity` and the requested `epistemic_state`
/// - the baseline fixture every `CorrectRelation` test corrects.
fn mk_cr_active_relation(conn: &Connection, epistemic_state: EpistemicState) -> Relation {
    let from = mk_pk_entity("person", "CR From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(conn, &from).unwrap();
    let to = mk_pk_entity("project", "CR To", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(conn, &to).unwrap();
    let mut relation = mk_pk_relation(
        from.id,
        to.id,
        "works_on",
        epistemic_state,
        Sensitivity::Internal,
    );
    relation.note = Some("original note".to_string());
    relation.confidence = Some(0.4);
    storage::personal_knowledge_repo::insert_relation(conn, &relation).unwrap();
    relation
}

/// Raw-SQL-only mutation (no production action can currently produce an
/// inactive Relation - `ForgetRelation` is not implemented this run) -
/// same precedent as the Fact-family's own pre-`CorrectMemory` raw
/// fixture mutations.
fn raw_set_relation_valid_until_and_state(
    conn: &Connection,
    relation_id: RelationId,
    valid_until: Option<chrono::DateTime<Utc>>,
    epistemic_state: EpistemicState,
) {
    conn.execute(
        "UPDATE relations SET valid_until = ?2, epistemic_state = ?3 WHERE id = ?1",
        rusqlite::params![
            relation_id.to_string(),
            valid_until.map(|d| d.to_rfc3339()),
            epistemic_state.as_str()
        ],
    )
    .unwrap();
}

/// A minimal, always-valid baseline payload - `note` `Set`, everything
/// else `Unchanged`/`None`. Every test below overrides only the fields
/// it cares about via struct-update syntax, same convention as
/// `mk_ar_payload`.
fn mk_cr_payload(relation_id: RelationId) -> CorrectRelationPayload {
    CorrectRelationPayload {
        relation_id,
        note: FieldPatch::Set("corrected note".to_string()),
        confidence: FieldPatch::Unchanged,
        sensitivity: None,
    }
}

// --- Direct guarded-update matrix (R027 §17-19 A-F) ----------------------

#[test]
fn req_cr_repo_guarded_update_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_repo_matrix.db");
    let conn = bootstrap_db_at(&path, &key).unwrap();

    // A: active USER_CONFIRMED -> success exactly one.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let result = storage::personal_knowledge_repo::update_relation_metadata_if_active(
            &conn,
            relation.id,
            Some("updated a"),
            Some(0.9),
            Sensitivity::Public,
        );
        assert!(result.is_ok(), "[req_cr_repo_a] FAIL - {result:?}");
        let after = get_relation(&conn, relation.id).unwrap().unwrap();
        assert_eq!(after.note, Some("updated a".to_string()));
        assert_eq!(after.confidence, Some(0.9));
        assert_eq!(after.sensitivity, Sensitivity::Public);
    }

    // B: active INFERRED -> success.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::Inferred);
        let result = storage::personal_knowledge_repo::update_relation_metadata_if_active(
            &conn,
            relation.id,
            Some("updated b"),
            None,
            Sensitivity::Sensitive,
        );
        assert!(result.is_ok(), "[req_cr_repo_b] FAIL - {result:?}");
    }

    // C: STALE + valid_until NULL -> Err, zero mutation.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        raw_set_relation_valid_until_and_state(&conn, relation.id, None, EpistemicState::Stale);
        let before = get_relation(&conn, relation.id).unwrap().unwrap();
        let result = storage::personal_knowledge_repo::update_relation_metadata_if_active(
            &conn,
            relation.id,
            Some("should not apply"),
            Some(0.1),
            Sensitivity::Public,
        );
        assert!(
            result.is_err(),
            "[req_cr_repo_c] FAIL - expected Err for STALE target"
        );
        assert_eq!(
            get_relation(&conn, relation.id).unwrap().unwrap(),
            before,
            "[req_cr_repo_c] FAIL - zero mutation expected"
        );
    }

    // D: non-STALE + valid_until Some -> Err, zero mutation.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        raw_set_relation_valid_until_and_state(
            &conn,
            relation.id,
            Some(Utc::now()),
            EpistemicState::UserConfirmed,
        );
        let before = get_relation(&conn, relation.id).unwrap().unwrap();
        let result = storage::personal_knowledge_repo::update_relation_metadata_if_active(
            &conn,
            relation.id,
            Some("should not apply"),
            Some(0.1),
            Sensitivity::Public,
        );
        assert!(
            result.is_err(),
            "[req_cr_repo_d] FAIL - expected Err for closed target"
        );
        assert_eq!(get_relation(&conn, relation.id).unwrap().unwrap(), before);
    }

    // E: STALE + valid_until Some -> Err, zero mutation.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        raw_set_relation_valid_until_and_state(
            &conn,
            relation.id,
            Some(Utc::now()),
            EpistemicState::Stale,
        );
        let before = get_relation(&conn, relation.id).unwrap().unwrap();
        let result = storage::personal_knowledge_repo::update_relation_metadata_if_active(
            &conn,
            relation.id,
            Some("should not apply"),
            Some(0.1),
            Sensitivity::Public,
        );
        assert!(
            result.is_err(),
            "[req_cr_repo_e] FAIL - expected Err for stale+closed target"
        );
        assert_eq!(get_relation(&conn, relation.id).unwrap().unwrap(), before);
    }

    // F: nonexistent RelationId -> Err, never silent Ok.
    {
        let result = storage::personal_knowledge_repo::update_relation_metadata_if_active(
            &conn,
            RelationId::new(),
            Some("x"),
            None,
            Sensitivity::Public,
        );
        assert!(
            result.is_err(),
            "[req_cr_repo_f] FAIL - nonexistent id must be a real Err, never silent Ok"
        );
    }
}

// --- Confirmation matrix / gate-before-lookup (R027 §7-11/§34) ----------

#[test]
fn req_cr_conf_a_explicit_unconfirmed_executes_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_conf_a.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
    match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(relation.id)) {
        ActionOutcome::Success {
            confirmation_class, ..
        } => assert_eq!(confirmation_class, ConfirmationClass::ImportantChange),
        other => panic!("[req_cr_conf_a] FAIL - expected Success, got {other:?}"),
    }
}

#[test]
fn req_cr_conf_b_explicit_confirmed_true_succeeds_same_class() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_conf_b.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
    let mut meta = ui_explicit();
    meta.confirmed = true;
    match correct_relation(&mut conn, meta, mk_cr_payload(relation.id)) {
        ActionOutcome::Success {
            confirmation_class, ..
        } => assert_eq!(confirmation_class, ConfirmationClass::ImportantChange),
        other => panic!("[req_cr_conf_b] FAIL - expected Success, got {other:?}"),
    }
}

/// CRITICAL/MANDATORY (R027 §8/§11): `ImportantChange` (severity 3)
/// already dominates `InferredChange` (severity 2) in the generic
/// `effective_class` ladder, so the PK-specific
/// `memory_action_requires_confirmation` overlay is what actually
/// blocks here - reused completely unmodified from the Fact family.
/// Using a NONEXISTENT `RelationId` proves the gate runs BEFORE the
/// target is ever looked up: the result must be `ConfirmationRequired`,
/// never `NotFound`.
#[test]
fn req_cr_conf_c_inferred_unconfirmed_blocks_gate_precedes_relation_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_conf_c.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    let payload = mk_cr_payload(RelationId::new());
    match correct_relation(&mut conn, meta, payload) {
        ActionOutcome::ConfirmationRequired { confirmation_class } => {
            assert_eq!(confirmation_class, ConfirmationClass::ImportantChange)
        }
        other => panic!(
            "[req_cr_conf_c] FAIL - expected ConfirmationRequired (gate must precede Relation lookup, not NotFound), got {other:?}"
        ),
    }
    assert_eq!(
        relation_audit_row_count(&conn),
        0,
        "[req_cr_conf_c] FAIL - zero audit writes expected"
    );
}

#[test]
fn req_cr_conf_d_inferred_confirmed_succeeds_audit_provenance_inferred_epistemic_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_conf_d.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let relation = mk_cr_active_relation(&conn, EpistemicState::Disputed);
    let mut meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    meta.confirmed = true;
    match correct_relation(&mut conn, meta, mk_cr_payload(relation.id)) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::ImportantChange);
            assert_eq!(
                value.epistemic_state,
                EpistemicState::Disputed,
                "[req_cr_conf_d] FAIL - CorrectRelation must never touch epistemic_state"
            );
        }
        other => panic!("[req_cr_conf_d] FAIL - expected Success, got {other:?}"),
    }
    let provenance: String = conn
        .query_row(
            "SELECT provenance FROM relation_audit_log WHERE relation_id = ?1 AND action_type = 'CorrectRelation'",
            rusqlite::params![relation.id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        provenance, "Inferred",
        "[req_cr_conf_d] FAIL - audit provenance must remain Inferred, never rewritten to Explicit"
    );
}

/// The structural no-fields-to-correct rule (R027 §4/§32) precedes BOTH
/// confirmation and DB access - using `Inferred`+unconfirmed with a
/// nonexistent `RelationId` proves this: the result must be
/// `ValidationFailed(NoFieldsToUpdate)`, never `ConfirmationRequired`
/// or `NotFound`.
#[test]
fn req_cr_conf_e_structural_no_op_rejected_before_gate_and_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_conf_e.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let meta = ActionMeta::now(Actor::CloudLlm, Provenance::Inferred);
    let payload = CorrectRelationPayload {
        relation_id: RelationId::new(),
        note: FieldPatch::Unchanged,
        confidence: FieldPatch::Unchanged,
        sensitivity: None,
    };
    match correct_relation(&mut conn, meta, payload) {
        ActionOutcome::ValidationFailed(errors) => {
            assert!(errors.contains(&ValidationError::NoFieldsToUpdate))
        }
        other => panic!(
            "[req_cr_conf_e] FAIL - expected ValidationFailed(NoFieldsToUpdate) BEFORE confirmation/lookup, got {other:?}"
        ),
    }
    assert_eq!(relation_audit_row_count(&conn), 0);
}

#[test]
fn req_cr_missing_target_returns_not_found_after_gate_satisfied() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_missing.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(RelationId::new())) {
        ActionOutcome::NotFound => {}
        other => panic!("[req_cr_missing] FAIL - expected NotFound, got {other:?}"),
    }
    assert_eq!(relation_audit_row_count(&conn), 0);
}

// --- Active/inactive Relation matrix (R027 §12/§13/§35) ------------------

#[test]
fn req_cr_active_matrix_correctable_epistemic_states_unchanged_after() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_active_matrix.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    for state in [
        EpistemicState::UserConfirmed,
        EpistemicState::Observed,
        EpistemicState::Inferred,
        EpistemicState::Disputed,
    ] {
        let relation = mk_cr_active_relation(&conn, state);
        match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(relation.id)) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(
                    value.epistemic_state, state,
                    "[req_cr_active_matrix] FAIL - {state:?} must remain unchanged after correction"
                );
            }
            other => panic!(
                "[req_cr_active_matrix] FAIL - {state:?} should be correctable, got {other:?}"
            ),
        }
    }
}

#[test]
fn req_cr_inactive_matrix_rejected_zero_writes() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_inactive_matrix.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    // STALE + NULL valid_until
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        raw_set_relation_valid_until_and_state(&conn, relation.id, None, EpistemicState::Stale);
        let before = get_relation(&conn, relation.id).unwrap().unwrap();
        match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(relation.id)) {
            ActionOutcome::ValidationFailed(errors) => {
                assert!(errors.contains(&ValidationError::RelationNotActive))
            }
            other => panic!("[req_cr_inactive_a] FAIL - expected RelationNotActive, got {other:?}"),
        }
        assert_eq!(get_relation(&conn, relation.id).unwrap().unwrap(), before);
    }

    // non-STALE + Some(valid_until)
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::Observed);
        raw_set_relation_valid_until_and_state(
            &conn,
            relation.id,
            Some(Utc::now()),
            EpistemicState::Observed,
        );
        let before = get_relation(&conn, relation.id).unwrap().unwrap();
        match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(relation.id)) {
            ActionOutcome::ValidationFailed(errors) => {
                assert!(errors.contains(&ValidationError::RelationNotActive))
            }
            other => panic!("[req_cr_inactive_b] FAIL - expected RelationNotActive, got {other:?}"),
        }
        assert_eq!(get_relation(&conn, relation.id).unwrap().unwrap(), before);
    }

    // STALE + Some(valid_until)
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        raw_set_relation_valid_until_and_state(
            &conn,
            relation.id,
            Some(Utc::now()),
            EpistemicState::Stale,
        );
        let before = get_relation(&conn, relation.id).unwrap().unwrap();
        match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(relation.id)) {
            ActionOutcome::ValidationFailed(errors) => {
                assert!(errors.contains(&ValidationError::RelationNotActive))
            }
            other => panic!("[req_cr_inactive_c] FAIL - expected RelationNotActive, got {other:?}"),
        }
        assert_eq!(get_relation(&conn, relation.id).unwrap().unwrap(), before);
    }

    assert_eq!(
        relation_audit_row_count(&conn),
        0,
        "[req_cr_inactive_matrix] FAIL - zero audit writes expected for every inactive rejection"
    );
}

// --- Identity/epistemic/temporal immutability (R027 §36-38) -------------

#[test]
fn req_cr_identity_fields_immutable() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_identity.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
    match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(before.id)) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.id, before.id);
            assert_eq!(value.from_entity_id, before.from_entity_id);
            assert_eq!(value.to_entity_id, before.to_entity_id);
            assert_eq!(value.relation_type, before.relation_type);
        }
        other => panic!("[req_cr_identity] FAIL - expected Success, got {other:?}"),
    }
}

#[test]
fn req_cr_temporal_fields_immutable() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_temporal.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
    let fixed = chrono::DateTime::parse_from_rfc3339("2026-03-01T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let meta = ActionMeta {
        actor: Actor::Ui,
        provenance: Provenance::Explicit,
        requested_at: fixed,
        confirmed: false,
    };
    match correct_relation(&mut conn, meta, mk_cr_payload(before.id)) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.valid_from, before.valid_from);
            assert_eq!(value.valid_until, before.valid_until);
            assert_eq!(value.created_at, before.created_at);
        }
        other => panic!("[req_cr_temporal] FAIL - expected Success, got {other:?}"),
    }
}

// --- Note patch matrix (R027 §28) ----------------------------------------

#[test]
fn req_cr_note_patch_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_note_matrix.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    // A: Unchanged -> exact old note retained.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let payload = CorrectRelationPayload {
            relation_id: relation.id,
            note: FieldPatch::Unchanged,
            confidence: FieldPatch::Set(0.7),
            sensitivity: None,
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => assert_eq!(value.note, relation.note),
            other => panic!("[req_cr_note_a] FAIL - expected Success, got {other:?}"),
        }
    }

    // B: Set("new") -> exact new string stored.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let payload = CorrectRelationPayload {
            note: FieldPatch::Set("new".to_string()),
            ..mk_cr_payload(relation.id)
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.note, Some("new".to_string()))
            }
            other => panic!("[req_cr_note_b] FAIL - expected Success, got {other:?}"),
        }
    }

    // C: Clear -> None.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let payload = CorrectRelationPayload {
            note: FieldPatch::Clear,
            ..mk_cr_payload(relation.id)
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => assert!(value.note.is_none()),
            other => panic!("[req_cr_note_c] FAIL - expected Success, got {other:?}"),
        }
    }

    // D: Set("") -> empty string stored, NOT NULL.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let payload = CorrectRelationPayload {
            note: FieldPatch::Set(String::new()),
            ..mk_cr_payload(relation.id)
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.note, Some(String::new()))
            }
            other => panic!("[req_cr_note_d] FAIL - expected Success, got {other:?}"),
        }
    }

    // E: Set("   ") -> whitespace stored exactly.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let payload = CorrectRelationPayload {
            note: FieldPatch::Set("   ".to_string()),
            ..mk_cr_payload(relation.id)
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.note, Some("   ".to_string()))
            }
            other => panic!("[req_cr_note_e] FAIL - expected Success, got {other:?}"),
        }
    }

    // F: exact RELATION_NOTE_MAX_LEN -> success.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let exact = "n".repeat(crate::domain::RELATION_NOTE_MAX_LEN);
        let payload = CorrectRelationPayload {
            note: FieldPatch::Set(exact.clone()),
            ..mk_cr_payload(relation.id)
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => assert_eq!(value.note, Some(exact)),
            other => panic!("[req_cr_note_f] FAIL - expected Success, got {other:?}"),
        }
    }

    // G: N+1 -> ValidationFailed before DB access.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let too_long = "n".repeat(crate::domain::RELATION_NOTE_MAX_LEN + 1);
        let payload = CorrectRelationPayload {
            note: FieldPatch::Set(too_long),
            ..mk_cr_payload(relation.id)
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::ValidationFailed(errors) => {
                assert!(errors.iter().any(|e| matches!(
                    e,
                    ValidationError::PersonalKnowledgeFieldTooLong { field, .. } if *field == "note"
                )))
            }
            other => panic!("[req_cr_note_g] FAIL - expected ValidationFailed, got {other:?}"),
        }
        let after = get_relation(&conn, relation.id).unwrap().unwrap();
        assert_eq!(
            after, relation,
            "[req_cr_note_g] FAIL - zero mutation expected"
        );
    }
}

// --- Confidence patch matrix (R027 §29) ----------------------------------

#[test]
fn req_cr_confidence_patch_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_confidence_matrix.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    // A: Unchanged -> old confidence retained.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(relation.id)) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.confidence, relation.confidence)
            }
            other => panic!("[req_cr_confidence_a] FAIL - expected Success, got {other:?}"),
        }
    }

    // B/C/D: Set(0.0)/Set(1.0)/Set(0.5).
    for accepted in [0.0f32, 1.0f32, 0.5f32] {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let payload = CorrectRelationPayload {
            confidence: FieldPatch::Set(accepted),
            ..mk_cr_payload(relation.id)
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.confidence, Some(accepted))
            }
            other => panic!(
                "[req_cr_confidence_bcd] FAIL - {accepted} should be accepted, got {other:?}"
            ),
        }
    }

    // E: Clear -> None.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let payload = CorrectRelationPayload {
            confidence: FieldPatch::Clear,
            ..mk_cr_payload(relation.id)
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => assert!(value.confidence.is_none()),
            other => panic!("[req_cr_confidence_e] FAIL - expected Success, got {other:?}"),
        }
    }

    // F: invalid values -> structural ValidationFailed, zero mutation.
    for rejected in [
        -0.01f32,
        1.01f32,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ] {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let payload = CorrectRelationPayload {
            confidence: FieldPatch::Set(rejected),
            ..mk_cr_payload(relation.id)
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::ValidationFailed(errors) => assert!(errors
                .iter()
                .any(|e| matches!(e, ValidationError::ConfidenceOutOfBounds { .. }))),
            other => panic!(
                "[req_cr_confidence_f] FAIL - {rejected:?} should be rejected, got {other:?}"
            ),
        }
        let after = get_relation(&conn, relation.id).unwrap().unwrap();
        assert_eq!(
            after, relation,
            "[req_cr_confidence_f] FAIL - zero mutation expected"
        );
    }
}

// --- Sensitivity patch matrix (R027 §30) ---------------------------------

#[test]
fn req_cr_sensitivity_patch_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_sensitivity_matrix.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();

    // A: None -> old sensitivity retained.
    {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(relation.id)) {
            ActionOutcome::Success { value, .. } => {
                assert_eq!(value.sensitivity, relation.sensitivity)
            }
            other => panic!("[req_cr_sensitivity_a] FAIL - expected Success, got {other:?}"),
        }
    }

    // B-E: Some(each of the 4 frozen values).
    for sens in [
        Sensitivity::Public,
        Sensitivity::Internal,
        Sensitivity::Sensitive,
        Sensitivity::NeverLeavesDevice,
    ] {
        let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
        let payload = CorrectRelationPayload {
            sensitivity: Some(sens),
            ..mk_cr_payload(relation.id)
        };
        match correct_relation(&mut conn, ui_explicit(), payload) {
            ActionOutcome::Success { value, .. } => assert_eq!(value.sensitivity, sens),
            other => {
                panic!("[req_cr_sensitivity_bcde] FAIL - {sens:?} should succeed, got {other:?}")
            }
        }
    }
}

// --- Combined patch and same-value resupply (R027 §31/§33) --------------

#[test]
fn req_cr_combined_patch_all_three_fields_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_combined.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = mk_cr_active_relation(&conn, EpistemicState::Observed);
    let payload = CorrectRelationPayload {
        relation_id: before.id,
        note: FieldPatch::Set("combined note".to_string()),
        confidence: FieldPatch::Clear,
        sensitivity: Some(Sensitivity::Sensitive),
    };
    match correct_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.note, Some("combined note".to_string()));
            assert!(value.confidence.is_none());
            assert_eq!(value.sensitivity, Sensitivity::Sensitive);
            // Every immutable field unchanged.
            assert_eq!(value.id, before.id);
            assert_eq!(value.from_entity_id, before.from_entity_id);
            assert_eq!(value.to_entity_id, before.to_entity_id);
            assert_eq!(value.relation_type, before.relation_type);
            assert_eq!(value.epistemic_state, before.epistemic_state);
            assert_eq!(value.valid_from, before.valid_from);
            assert_eq!(value.valid_until, before.valid_until);
            assert_eq!(value.created_at, before.created_at);
        }
        other => panic!("[req_cr_combined] FAIL - expected Success, got {other:?}"),
    }
}

/// Explicitly resupplying the Relation's CURRENT value still counts as
/// a requested correction - never coerced into `NoFieldsToUpdate`, and
/// never optimized away by a database-dependent same-value check.
#[test]
fn req_cr_same_value_resupply_still_succeeds_and_writes_audit() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_same_value.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let from = mk_pk_entity("person", "Same Value From", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &from).unwrap();
    let to = mk_pk_entity("project", "Same Value To", Sensitivity::Internal);
    storage::personal_knowledge_repo::insert_entity(&conn, &to).unwrap();
    let mut relation = mk_pk_relation(
        from.id,
        to.id,
        "works_on",
        EpistemicState::UserConfirmed,
        Sensitivity::Internal,
    );
    relation.note = Some("same".to_string());
    relation.confidence = Some(0.5);
    storage::personal_knowledge_repo::insert_relation(&conn, &relation).unwrap();

    let payload = CorrectRelationPayload {
        relation_id: relation.id,
        note: FieldPatch::Set("same".to_string()),
        confidence: FieldPatch::Unchanged,
        sensitivity: None,
    };
    match correct_relation(&mut conn, ui_explicit(), payload) {
        ActionOutcome::Success { value, .. } => {
            assert_eq!(value.note, Some("same".to_string()));
            assert_eq!(value.confidence, Some(0.5));
        }
        other => panic!(
            "[req_cr_same_value] FAIL - explicit resupply must still succeed, not become NoFieldsToUpdate, got {other:?}"
        ),
    }
    assert_eq!(
        relation_audit_row_count(&conn),
        1,
        "[req_cr_same_value] FAIL - an audit row must be written even for a same-value resupply"
    );
}

// --- Mandatory previous_snapshot / audit exactness (R027 §20-25) --------

#[test]
fn req_cr_snapshot_deserializes_exactly_to_pre_correction_relation() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_snapshot.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
    let target_id;
    match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(before.id)) {
        ActionOutcome::Success { value, .. } => target_id = value.id,
        other => panic!("[req_cr_snapshot] FAIL - expected Success, got {other:?}"),
    }
    let snapshot_json: String = conn
        .query_row(
            "SELECT previous_snapshot FROM relation_audit_log WHERE relation_id = ?1 AND action_type = 'CorrectRelation'",
            rusqlite::params![target_id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    let deserialized: Relation = serde_json::from_str(&snapshot_json).unwrap();
    assert_eq!(
        deserialized, before,
        "[req_cr_snapshot] FAIL - previous_snapshot must deserialize to EXACTLY the PRE-correction Relation, not merely contain the note or be non-NULL"
    );
}

#[test]
fn req_cr_audit_exact_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_audit_explicit.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
    let fixed = chrono::DateTime::parse_from_rfc3339("2026-03-02T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let meta = ActionMeta {
        actor: Actor::CloudLlm,
        provenance: Provenance::Explicit,
        requested_at: fixed,
        confirmed: false,
    };
    match correct_relation(&mut conn, meta, mk_cr_payload(before.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_cr_audit_explicit] FAIL - expected Success, got {other:?}"),
    }
    let (relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at): (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT relation_id, action_type, actor, provenance, confirmation_class, previous_snapshot, occurred_at FROM relation_audit_log WHERE relation_id = ?1 AND action_type = 'CorrectRelation'",
            rusqlite::params![before.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .unwrap();
    assert_eq!(relation_id, before.id.to_string());
    assert_eq!(action_type, "CorrectRelation");
    assert_eq!(actor, "CloudLLM");
    assert_eq!(provenance, "Explicit");
    assert_eq!(confirmation_class, "IMPORTANT_CHANGE");
    let snapshot: Relation = serde_json::from_str(&previous_snapshot.unwrap()).unwrap();
    assert_eq!(snapshot, before);
    assert_eq!(occurred_at, fixed.to_rfc3339());
}

#[test]
fn req_cr_audit_exact_inferred_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_audit_inferred.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = mk_cr_active_relation(&conn, EpistemicState::Inferred);
    let fixed = chrono::DateTime::parse_from_rfc3339("2026-03-03T11:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let meta = ActionMeta {
        actor: Actor::CloudLlm,
        provenance: Provenance::Inferred,
        requested_at: fixed,
        confirmed: true,
    };
    match correct_relation(&mut conn, meta, mk_cr_payload(before.id)) {
        ActionOutcome::Success {
            value,
            confirmation_class,
        } => {
            assert_eq!(confirmation_class, ConfirmationClass::ImportantChange);
            assert_eq!(value.epistemic_state, EpistemicState::Inferred);
        }
        other => panic!("[req_cr_audit_inferred] FAIL - expected Success, got {other:?}"),
    }
    let (provenance, confirmation_class): (String, String) = conn
        .query_row(
            "SELECT provenance, confirmation_class FROM relation_audit_log WHERE relation_id = ?1 AND action_type = 'CorrectRelation'",
            rusqlite::params![before.id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(provenance, "Inferred");
    assert_eq!(confirmation_class, "IMPORTANT_CHANGE");
}

// --- Transaction atomicity / rollback (R027 §26/§27) ---------------------

/// MANDATORY: force the `relation_audit_log` insert to fail - the
/// guarded metadata `UPDATE` must roll back too, the target Relation
/// must be EXACTLY the pre-correction state, no `CorrectRelation` audit
/// row exists, and the connection must remain usable afterward.
#[test]
fn req_cr_forced_audit_failure_rolls_back_metadata_update() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_rollback.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);

    install_relation_audit_failure_trigger(&conn);
    match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(before.id)) {
        ActionOutcome::StorageError(_) => {}
        other => panic!("[req_cr_rollback] FAIL - expected StorageError, got {other:?}"),
    }
    remove_relation_audit_failure_trigger(&conn);

    let after = get_relation(&conn, before.id).unwrap().unwrap();
    assert_eq!(
        after, before,
        "[req_cr_rollback] FAIL - forced audit failure must roll back the metadata UPDATE too"
    );
    assert_eq!(
        relation_audit_row_count(&conn),
        0,
        "[req_cr_rollback] FAIL - no CorrectRelation audit row should exist"
    );

    // Connection remains usable.
    match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(before.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_cr_rollback] FAIL - connection unusable after rollback: {other:?}"),
    }
}

// --- Entity/Source/Evidence and Fact/Episode non-interference (R027 §39/§40) --

#[test]
fn req_cr_entity_source_evidence_non_interference() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_non_interference.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before_entities = entity_row_count(&conn);
    let before_sources = am_source_row_count(&conn);
    let before_evidence = am_evidence_row_count(&conn);
    let before_relation_evidence = relation_evidence_row_count(&conn);

    let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
    match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(relation.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_cr_non_interference] FAIL - expected Success, got {other:?}"),
    }

    assert_eq!(
        entity_row_count(&conn),
        before_entities + 2,
        "[req_cr_non_interference] FAIL - only the two fixture Entities created for this test should exist, CorrectRelation itself creates none"
    );
    assert_eq!(am_source_row_count(&conn), before_sources);
    assert_eq!(am_evidence_row_count(&conn), before_evidence);
    assert_eq!(relation_evidence_row_count(&conn), before_relation_evidence);
}

#[test]
fn req_cr_fact_episode_non_interference() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_fact_episode.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before = relation_episode_side_effect_counts(&conn);
    let before_facts = am_fact_row_count(&conn);
    let before_fact_evidence = am_fact_evidence_row_count(&conn);

    let relation = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
    match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(relation.id)) {
        ActionOutcome::Success { .. } => {}
        other => panic!("[req_cr_fact_episode] FAIL - expected Success, got {other:?}"),
    }

    let after = relation_episode_side_effect_counts(&conn);
    assert_eq!(
        after.1, before.1,
        "[req_cr_fact_episode] FAIL - episodes must be untouched"
    );
    assert_eq!(
        after.2, before.2,
        "[req_cr_fact_episode] FAIL - relation_evidence must be untouched (CorrectRelation touches no Evidence)"
    );
    assert_eq!(
        after.3, before.3,
        "[req_cr_fact_episode] FAIL - episode_evidence must be untouched"
    );
    assert_eq!(
        after.4, before.4,
        "[req_cr_fact_episode] FAIL - episode_entities must be untouched"
    );
    assert_eq!(
        am_fact_row_count(&conn),
        before_facts,
        "[req_cr_fact_episode] FAIL - facts must be untouched"
    );
    assert_eq!(
        am_fact_evidence_row_count(&conn),
        before_fact_evidence,
        "[req_cr_fact_episode] FAIL - fact_evidence must be untouched"
    );
}

// --- Row-count/id/output proofs (R027 §14/§41/§42) -----------------------

#[test]
fn req_cr_no_new_relation_row_same_id_and_output_matches_readback() {
    let dir = tempfile::tempdir().unwrap();
    let (path, key) = temp_db(&dir, "cr_row_count.db");
    let mut conn = bootstrap_db_at(&path, &key).unwrap();
    let before_count = relation_row_count(&conn);
    let target = mk_cr_active_relation(&conn, EpistemicState::UserConfirmed);
    assert_eq!(relation_row_count(&conn), before_count + 1);

    let returned = match correct_relation(&mut conn, ui_explicit(), mk_cr_payload(target.id)) {
        ActionOutcome::Success { value, .. } => value,
        other => panic!("[req_cr_row_count] FAIL - expected Success, got {other:?}"),
    };

    assert_eq!(
        relation_row_count(&conn),
        before_count + 1,
        "[req_cr_row_count] FAIL - CorrectRelation must never create a new relations row"
    );
    assert_eq!(
        returned.id, target.id,
        "[req_cr_row_count] FAIL - same RelationId expected"
    );

    let readback = get_relation(&conn, target.id)
        .unwrap()
        .expect("[req_cr_row_count] FAIL - target not found via get_relation");
    assert_eq!(
        returned, readback,
        "[req_cr_row_count] FAIL - returned Relation must exactly equal get_relation's read-back, no stale in-memory result"
    );
}
