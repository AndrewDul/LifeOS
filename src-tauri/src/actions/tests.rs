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

fn base_goal(status: LifecycleStatus) -> Goal {
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
    assert!(blocks_until_confirmed(ConfirmationClass::InferredChange));
}

#[test]
fn effective_class_important_change_with_explicit_provenance_stays_important_change() {
    assert_eq!(
        effective_class(ConfirmationClass::ImportantChange, Provenance::Explicit),
        ConfirmationClass::ImportantChange
    );
    assert!(!blocks_until_confirmed(ConfirmationClass::ImportantChange));
}

// --- UpdateGoal patch validation ---------------------------------------

#[test]
fn update_goal_rejects_when_no_fields_touched() {
    let existing = base_goal(LifecycleStatus::Active);
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
    let existing = base_goal(LifecycleStatus::Active);
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
    let existing = base_goal(LifecycleStatus::Active);
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
    let existing = base_goal(LifecycleStatus::Active);
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
    let existing = base_goal(LifecycleStatus::Active);
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
// definition. It is removed: every LifecycleStatus -> LifecycleStatus transition
// an explicit action requests now passes validation.

#[test]
fn update_goal_status_transition_from_completed_to_active_is_now_valid() {
    let existing = base_goal(LifecycleStatus::Completed);
    let payload = UpdateGoalPayload {
        id: existing.id,
        status: Some(LifecycleStatus::Active),
        ..Default::default()
    };
    assert!(
        validate_update_goal(&existing, &payload).is_ok(),
        "reopening a COMPLETED goal must be allowed - no terminal-state rule is frozen"
    );
}

#[test]
fn update_goal_status_transition_from_abandoned_to_paused_is_valid() {
    let existing = base_goal(LifecycleStatus::Abandoned);
    let payload = UpdateGoalPayload {
        id: existing.id,
        status: Some(LifecycleStatus::Paused),
        ..Default::default()
    };
    assert!(validate_update_goal(&existing, &payload).is_ok());
}

#[test]
fn update_goal_status_no_op_transition_is_valid() {
    let existing = base_goal(LifecycleStatus::Active);
    let payload = UpdateGoalPayload {
        id: existing.id,
        status: Some(LifecycleStatus::Active),
        ..Default::default()
    };
    assert!(validate_update_goal(&existing, &payload).is_ok());
}

// --- FieldPatch semantics itself ----------------------------------------

#[test]
fn field_patch_default_is_unchanged() {
    assert_eq!(FieldPatch::<String>::default(), FieldPatch::Unchanged);
}

// =========================================================================
// Project (M2 Project vertical slice, 2026-09-17) - direct unit tests.
// Pure `validate_create_project`/`validate_update_progress`/
// `effective_class` calls, no DB, no I/O - mirrors the Goal tests above.
// The `goal_id` reference check is NOT exercised here (it needs a
// database read and lives in `services::create_project`) - see
// `services::tests` for that.
// =========================================================================

fn base_create_project_payload() -> CreateProjectPayload {
    CreateProjectPayload {
        title: "Valid project title".to_string(),
        description: None,
        priority: None,
        goal_id: None,
    }
}

#[test]
fn create_project_title_is_trimmed() {
    let payload = CreateProjectPayload {
        title: "  Van purchase  ".to_string(),
        ..base_create_project_payload()
    };
    let title = validate_create_project(&payload).expect("should validate");
    assert_eq!(title, "Van purchase");
}

#[test]
fn create_project_rejects_empty_title() {
    for bad in ["", "   ", "\t\n"] {
        let payload = CreateProjectPayload {
            title: bad.to_string(),
            ..base_create_project_payload()
        };
        let errors = validate_create_project(&payload).expect_err("empty title must be rejected");
        assert!(
            errors.contains(&ValidationError::EmptyTitle),
            "expected EmptyTitle for {bad:?}, got {errors:?}"
        );
    }
}

#[test]
fn create_project_rejects_title_too_long() {
    let payload = CreateProjectPayload {
        title: "x".repeat(TITLE_MAX_LEN + 1),
        ..base_create_project_payload()
    };
    let errors = validate_create_project(&payload).expect_err("overlong title must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::TitleTooLong { .. })),
        "expected TitleTooLong, got {errors:?}"
    );
}

#[test]
fn create_project_rejects_priority_out_of_bounds() {
    let payload = CreateProjectPayload {
        priority: Some(PRIORITY_MAX + 1),
        ..base_create_project_payload()
    };
    let errors =
        validate_create_project(&payload).expect_err("out-of-bounds priority must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::PriorityOutOfBounds { .. })),
        "expected PriorityOutOfBounds, got {errors:?}"
    );
}

#[test]
fn create_project_rejects_description_too_long() {
    let payload = CreateProjectPayload {
        description: Some("x".repeat(DESCRIPTION_MAX_LEN + 1)),
        ..base_create_project_payload()
    };
    let errors =
        validate_create_project(&payload).expect_err("overlong description must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::DescriptionTooLong { .. })),
        "expected DescriptionTooLong, got {errors:?}"
    );
}

#[test]
fn create_project_confirmation_floor_is_explicit_add_for_explicit_provenance() {
    // Architecture-freeze report §F confirmation table lists
    // CreateProject under EXPLICIT_ADD - same floor as CreateGoal.
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Explicit),
        ConfirmationClass::ExplicitAdd
    );
}

#[test]
fn create_project_inferred_provenance_becomes_inferred_change() {
    // Same mechanism as CreateGoal + Inferred - the type floor
    // (ExplicitAdd) is lower severity than Inferred's provenance floor
    // (InferredChange), so the action must block until confirmed. See
    // `services::tests::req_p_inferred_create_project_requires_confirmation_and_does_not_write`
    // for the end-to-end DB proof.
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Inferred),
        ConfirmationClass::InferredChange
    );
    assert!(blocks_until_confirmed(ConfirmationClass::InferredChange));
}

#[test]
fn update_progress_accepts_in_bounds_value() {
    let payload = UpdateProgressPayload {
        id: ProjectId::new(),
        progress: 50,
    };
    assert!(validate_update_progress(&payload).is_ok());
}

#[test]
fn update_progress_rejects_out_of_bounds_value() {
    // Progress is `u8`, so ">100" is representable but out of the
    // canonical 0..=100 bound - PROGRESS_MAX + 1 stays in range for the
    // type (100 < 255) so this exercises the bounds check, not a type
    // overflow.
    let payload = UpdateProgressPayload {
        id: ProjectId::new(),
        progress: PROGRESS_MAX + 1,
    };
    let errors =
        validate_update_progress(&payload).expect_err("out-of-bounds progress must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::ProgressOutOfBounds { .. })),
        "expected ProgressOutOfBounds, got {errors:?}"
    );
}

#[test]
fn update_progress_confirmation_floor_is_explicit_add() {
    // Deliberately different from UpdateGoal (IMPORTANT_CHANGE) - the
    // architecture-freeze report §F confirmation table lists
    // UpdateProgress under EXPLICIT_ADD explicitly. Not a bug.
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Explicit),
        ConfirmationClass::ExplicitAdd
    );
    assert!(!blocks_until_confirmed(ConfirmationClass::ExplicitAdd));
}

// =========================================================================
// GoalRelation / LinkGoals (M2 GoalRelation vertical slice, 2026-09-17) -
// direct unit tests. `GoalRelationType`'s 11 string mappings, 11 parse
// round trips, and unknown-text rejection live in `domain::tests`
// instead (it is a pure domain-type concern, same precedent as
// `LifecycleStatus`'s tests) - not duplicated here. The `from`/`to`
// existence and exact-duplicate checks need a database read and are
// proven in `services::tests` instead.
// =========================================================================

fn base_link_goals_payload() -> LinkGoalsPayload {
    LinkGoalsPayload {
        from_goal_id: GoalId::new(),
        to_goal_id: GoalId::new(),
        relation_type: GoalRelationType::Supports,
        note: None,
    }
}

#[test]
fn link_goals_self_link_is_rejected() {
    let id = GoalId::new();
    let payload = LinkGoalsPayload {
        from_goal_id: id,
        to_goal_id: id,
        ..base_link_goals_payload()
    };
    let errors =
        validate_link_goals(&payload).expect_err("linking a goal to itself must be rejected");
    assert!(errors.contains(&ValidationError::SelfGoalRelation));
}

#[test]
fn link_goals_valid_two_goal_payload_is_accepted() {
    let payload = base_link_goals_payload();
    assert_ne!(payload.from_goal_id, payload.to_goal_id);
    assert!(validate_link_goals(&payload).is_ok());
}

#[test]
fn link_goals_rejects_overlong_note() {
    let payload = LinkGoalsPayload {
        note: Some("x".repeat(DESCRIPTION_MAX_LEN + 1)),
        ..base_link_goals_payload()
    };
    let errors = validate_link_goals(&payload).expect_err("overlong note must be rejected");
    assert!(errors
        .iter()
        .any(|e| matches!(e, ValidationError::NoteTooLong { .. })));
}

#[test]
fn link_goals_explicit_provenance_is_explicit_add() {
    // Architecture-freeze report §F confirmation table lists LinkGoals
    // under EXPLICIT_ADD.
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Explicit),
        ConfirmationClass::ExplicitAdd
    );
    assert!(!blocks_until_confirmed(ConfirmationClass::ExplicitAdd));
}

#[test]
fn link_goals_inferred_provenance_becomes_inferred_change() {
    // Same mechanism as CreateGoal/CreateProject + Inferred - the type
    // floor (ExplicitAdd) is lower severity than Inferred's provenance
    // floor (InferredChange), so the action must block until confirmed.
    // See `services::tests` for the end-to-end DB proof of zero writes.
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Inferred),
        ConfirmationClass::InferredChange
    );
    assert!(blocks_until_confirmed(ConfirmationClass::InferredChange));
}
