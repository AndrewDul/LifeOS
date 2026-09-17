//! Direct action-layer unit tests (M2 Goal hardening, 2026-09-17).
//!
//! Pure logic, no I/O, no temp files, no keyring - the Goal vertical
//! slice report flagged that `actions` was only exercised indirectly
//! through `services`-level integration tests. These test
//! `validate_create_goal`/`validate_update_goal`/`effective_class`
//! directly against in-memory values.
//!
//! Run with:
//! `cargo test --manifest-path src-tauri/Cargo.toml actions::tests`

use super::*;
use chrono::Utc;

fn base_goal(status: GoalStatus) -> Goal {
    Goal {
        id: GoalId::new(),
        title: "Base goal".to_string(),
        description: None,
        status,
        target_date: None,
        priority_user: None,
        priority_system: None,
        progress: 0,
        domain_tag: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn base_create_payload() -> CreateGoalPayload {
    CreateGoalPayload {
        title: "Valid title".to_string(),
        description: None,
        target_date: None,
        priority_user: None,
        domain_tag: None,
    }
}

// --- CreateGoal validation --------------------------------------------

#[test]
fn create_goal_title_is_trimmed() {
    let payload = CreateGoalPayload {
        title: "  Learn Rust  ".to_string(),
        ..base_create_payload()
    };
    let title = validate_create_goal(&payload).expect("should validate");
    assert_eq!(title, "Learn Rust");
}

#[test]
fn create_goal_rejects_empty_title() {
    for bad in ["", "   ", "\t\n"] {
        let payload = CreateGoalPayload {
            title: bad.to_string(),
            ..base_create_payload()
        };
        let errors = validate_create_goal(&payload).expect_err("empty title must be rejected");
        assert!(
            errors.contains(&ValidationError::EmptyTitle),
            "expected EmptyTitle for {bad:?}, got {errors:?}"
        );
    }
}

#[test]
fn create_goal_rejects_title_too_long() {
    let payload = CreateGoalPayload {
        title: "x".repeat(TITLE_MAX_LEN + 1),
        ..base_create_payload()
    };
    let errors = validate_create_goal(&payload).expect_err("overlong title must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::TitleTooLong { .. })),
        "expected TitleTooLong, got {errors:?}"
    );
}

#[test]
fn create_goal_rejects_priority_out_of_bounds() {
    let payload = CreateGoalPayload {
        priority_user: Some(PRIORITY_MAX + 1),
        ..base_create_payload()
    };
    let errors =
        validate_create_goal(&payload).expect_err("out-of-bounds priority must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::PriorityOutOfBounds { .. })),
        "expected PriorityOutOfBounds, got {errors:?}"
    );
}

// --- Confirmation classification ---------------------------------------

#[test]
fn effective_class_explicit_add_with_explicit_provenance_stays_explicit_add() {
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Explicit),
        ConfirmationClass::ExplicitAdd
    );
}

#[test]
fn effective_class_explicit_add_with_inferred_provenance_becomes_inferred_change() {
    // This is the CreateGoal + Provenance::Inferred case: the type floor
    // (ExplicitAdd) is lower severity than the Inferred provenance floor
    // (InferredChange), so the provenance wins and the action must block
    // until confirmed - see `services::tests` for the end-to-end DB
    // proof that no row is written before that confirmation.
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Inferred),
        ConfirmationClass::InferredChange
    );
    assert!(goal_blocks_until_confirmed(
        ConfirmationClass::InferredChange
    ));
}

#[test]
fn effective_class_important_change_with_explicit_provenance_stays_important_change() {
    assert_eq!(
        effective_class(ConfirmationClass::ImportantChange, Provenance::Explicit),
        ConfirmationClass::ImportantChange
    );
    assert!(!goal_blocks_until_confirmed(
        ConfirmationClass::ImportantChange
    ));
}

// --- UpdateGoal patch validation ---------------------------------------

#[test]
fn update_goal_rejects_when_no_fields_touched() {
    let existing = base_goal(GoalStatus::Active);
    let payload = UpdateGoalPayload {
        id: existing.id,
        ..Default::default()
    };
    let errors =
        validate_update_goal(&existing, &payload).expect_err("no-op update must be rejected");
    assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
}

#[test]
fn update_goal_field_patch_unchanged_alone_is_not_a_touch() {
    let existing = base_goal(GoalStatus::Active);
    let payload = UpdateGoalPayload {
        id: existing.id,
        description: FieldPatch::Unchanged,
        target_date: FieldPatch::Unchanged,
        priority_user: FieldPatch::Unchanged,
        domain_tag: FieldPatch::Unchanged,
        ..Default::default()
    };
    let errors = validate_update_goal(&existing, &payload)
        .expect_err("all-Unchanged update must be rejected as touching nothing");
    assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
}

#[test]
fn update_goal_field_patch_clear_does_not_trigger_length_validation() {
    let existing = base_goal(GoalStatus::Active);
    let payload = UpdateGoalPayload {
        id: existing.id,
        description: FieldPatch::Clear,
        domain_tag: FieldPatch::Clear,
        ..Default::default()
    };
    assert!(
        validate_update_goal(&existing, &payload).is_ok(),
        "Clear should never fail length/bounds validation"
    );
}

#[test]
fn update_goal_field_patch_set_triggers_length_validation() {
    let existing = base_goal(GoalStatus::Active);
    let payload = UpdateGoalPayload {
        id: existing.id,
        description: FieldPatch::Set("x".repeat(DESCRIPTION_MAX_LEN + 1)),
        ..Default::default()
    };
    let errors = validate_update_goal(&existing, &payload)
        .expect_err("overlong Set(description) must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::DescriptionTooLong { .. })),
        "expected DescriptionTooLong, got {errors:?}"
    );
}

#[test]
fn update_goal_field_patch_set_priority_out_of_bounds_is_rejected() {
    let existing = base_goal(GoalStatus::Active);
    let payload = UpdateGoalPayload {
        id: existing.id,
        priority_user: FieldPatch::Set(PRIORITY_MIN - 1),
        ..Default::default()
    };
    let errors = validate_update_goal(&existing, &payload)
        .expect_err("out-of-bounds Set(priority_user) must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::PriorityOutOfBounds { .. })),
        "expected PriorityOutOfBounds, got {errors:?}"
    );
}

// --- Status transitions (M2 hardening correction) ----------------------
//
// The Goal vertical slice invented a rule - COMPLETED/ABANDONED cannot
// transition further - that was never part of the frozen Goal status
// definition. It is removed: every GoalStatus -> GoalStatus transition
// an explicit action requests now passes validation.

#[test]
fn update_goal_status_transition_from_completed_to_active_is_now_valid() {
    let existing = base_goal(GoalStatus::Completed);
    let payload = UpdateGoalPayload {
        id: existing.id,
        status: Some(GoalStatus::Active),
        ..Default::default()
    };
    assert!(
        validate_update_goal(&existing, &payload).is_ok(),
        "reopening a COMPLETED goal must be allowed - no terminal-state rule is frozen"
    );
}

#[test]
fn update_goal_status_transition_from_abandoned_to_paused_is_valid() {
    let existing = base_goal(GoalStatus::Abandoned);
    let payload = UpdateGoalPayload {
        id: existing.id,
        status: Some(GoalStatus::Paused),
        ..Default::default()
    };
    assert!(validate_update_goal(&existing, &payload).is_ok());
}

#[test]
fn update_goal_status_no_op_transition_is_valid() {
    let existing = base_goal(GoalStatus::Active);
    let payload = UpdateGoalPayload {
        id: existing.id,
        status: Some(GoalStatus::Active),
        ..Default::default()
    };
    assert!(validate_update_goal(&existing, &payload).is_ok());
}

// --- FieldPatch semantics itself ----------------------------------------

#[test]
fn field_patch_default_is_unchanged() {
    assert_eq!(FieldPatch::<String>::default(), FieldPatch::Unchanged);
}
