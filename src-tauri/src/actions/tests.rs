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

// =========================================================================
// Milestone / AddMilestone / UpdateMilestone (M2 Milestone vertical
// slice, 2026-09-17) - direct unit tests. `MilestoneStatus`'s string
// mappings/parse round trips live in `domain::tests` (pure domain-type
// concern, same precedent as `LifecycleStatus`/`GoalRelationType`) - not
// duplicated here. `project_id` existence and `(project_id, order_index)`
// occupancy both need a database read and are proven in
// `services::tests` instead.
// =========================================================================

fn base_milestone(project_id: ProjectId, status: MilestoneStatus) -> Milestone {
    Milestone {
        id: MilestoneId::new(),
        project_id,
        title: "Base milestone".to_string(),
        target_date: None,
        status,
        order_index: 0,
    }
}

fn base_add_milestone_payload() -> AddMilestonePayload {
    AddMilestonePayload {
        project_id: ProjectId::new(),
        title: "Valid milestone title".to_string(),
        target_date: None,
        order_index: 0,
    }
}

#[test]
fn add_milestone_title_is_trimmed() {
    let payload = AddMilestonePayload {
        title: "  Ship v1  ".to_string(),
        ..base_add_milestone_payload()
    };
    let title = validate_add_milestone(&payload).expect("should validate");
    assert_eq!(title, "Ship v1");
}

#[test]
fn add_milestone_rejects_empty_title() {
    for bad in ["", "   ", "\t\n"] {
        let payload = AddMilestonePayload {
            title: bad.to_string(),
            ..base_add_milestone_payload()
        };
        let errors = validate_add_milestone(&payload).expect_err("empty title must be rejected");
        assert!(
            errors.contains(&ValidationError::EmptyTitle),
            "expected EmptyTitle for {bad:?}, got {errors:?}"
        );
    }
}

#[test]
fn add_milestone_rejects_title_too_long() {
    let payload = AddMilestonePayload {
        title: "x".repeat(TITLE_MAX_LEN + 1),
        ..base_add_milestone_payload()
    };
    let errors = validate_add_milestone(&payload).expect_err("overlong title must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::TitleTooLong { .. })),
        "expected TitleTooLong, got {errors:?}"
    );
}

#[test]
fn add_milestone_rejects_negative_order_index() {
    let payload = AddMilestonePayload {
        order_index: ORDER_INDEX_MIN - 1,
        ..base_add_milestone_payload()
    };
    let errors =
        validate_add_milestone(&payload).expect_err("negative order_index must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::OrderIndexOutOfBounds { .. })),
        "expected OrderIndexOutOfBounds, got {errors:?}"
    );
}

#[test]
fn add_milestone_accepts_min_order_index() {
    let payload = AddMilestonePayload {
        order_index: ORDER_INDEX_MIN,
        ..base_add_milestone_payload()
    };
    assert!(validate_add_milestone(&payload).is_ok());
}

#[test]
fn add_milestone_explicit_provenance_is_explicit_add() {
    // Architecture-freeze report §F confirmation table lists
    // AddMilestone under EXPLICIT_ADD.
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Explicit),
        ConfirmationClass::ExplicitAdd
    );
    assert!(!blocks_until_confirmed(ConfirmationClass::ExplicitAdd));
}

#[test]
fn add_milestone_inferred_provenance_becomes_inferred_change() {
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Inferred),
        ConfirmationClass::InferredChange
    );
    assert!(blocks_until_confirmed(ConfirmationClass::InferredChange));
}

// --- UpdateMilestone validation -----------------------------------------

#[test]
fn update_milestone_rejects_when_no_fields_touched() {
    let existing = base_milestone(ProjectId::new(), MilestoneStatus::Pending);
    let payload = UpdateMilestonePayload {
        id: existing.id,
        ..Default::default()
    };
    let errors =
        validate_update_milestone(&existing, &payload).expect_err("no-op update must be rejected");
    assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
}

#[test]
fn update_milestone_field_patch_unchanged_alone_is_not_a_touch() {
    let existing = base_milestone(ProjectId::new(), MilestoneStatus::Pending);
    let payload = UpdateMilestonePayload {
        id: existing.id,
        target_date: FieldPatch::Unchanged,
        ..Default::default()
    };
    let errors = validate_update_milestone(&existing, &payload)
        .expect_err("all-Unchanged update must be rejected as touching nothing");
    assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
}

#[test]
fn update_milestone_title_some_invalid_is_rejected() {
    let existing = base_milestone(ProjectId::new(), MilestoneStatus::Pending);
    let payload = UpdateMilestonePayload {
        id: existing.id,
        title: Some("   ".to_string()),
        ..Default::default()
    };
    let errors = validate_update_milestone(&existing, &payload)
        .expect_err("empty Some(title) must be rejected");
    assert!(errors.contains(&ValidationError::EmptyTitle));
}

#[test]
fn update_milestone_target_date_set_alone_is_valid() {
    let existing = base_milestone(ProjectId::new(), MilestoneStatus::Pending);
    let payload = UpdateMilestonePayload {
        id: existing.id,
        target_date: FieldPatch::Set(Utc::now()),
        ..Default::default()
    };
    assert!(validate_update_milestone(&existing, &payload).is_ok());
}

#[test]
fn update_milestone_target_date_clear_alone_is_valid() {
    let existing = base_milestone(ProjectId::new(), MilestoneStatus::Pending);
    let payload = UpdateMilestonePayload {
        id: existing.id,
        target_date: FieldPatch::Clear,
        ..Default::default()
    };
    assert!(validate_update_milestone(&existing, &payload).is_ok());
}

#[test]
fn update_milestone_status_pending_to_done_is_valid() {
    let existing = base_milestone(ProjectId::new(), MilestoneStatus::Pending);
    let payload = UpdateMilestonePayload {
        id: existing.id,
        status: Some(MilestoneStatus::Done),
        ..Default::default()
    };
    assert!(
        validate_update_milestone(&existing, &payload).is_ok(),
        "PENDING -> DONE must be allowed"
    );
}

#[test]
fn update_milestone_status_done_to_pending_is_valid() {
    // No terminal-state rule: PENDING/DONE are freely reversible peer
    // states (M2 Project Child Action Freeze report §5).
    let existing = base_milestone(ProjectId::new(), MilestoneStatus::Done);
    let payload = UpdateMilestonePayload {
        id: existing.id,
        status: Some(MilestoneStatus::Pending),
        ..Default::default()
    };
    assert!(
        validate_update_milestone(&existing, &payload).is_ok(),
        "DONE -> PENDING must be allowed - no terminal-state rule is frozen"
    );
}

#[test]
fn update_milestone_rejects_negative_order_index() {
    let existing = base_milestone(ProjectId::new(), MilestoneStatus::Pending);
    let payload = UpdateMilestonePayload {
        id: existing.id,
        order_index: Some(ORDER_INDEX_MIN - 1),
        ..Default::default()
    };
    let errors = validate_update_milestone(&existing, &payload)
        .expect_err("negative order_index must be rejected");
    assert!(errors
        .iter()
        .any(|e| matches!(e, ValidationError::OrderIndexOutOfBounds { .. })));
}

#[test]
fn update_milestone_explicit_provenance_is_explicit_add() {
    // Frozen floor (M2 Project Child Action Freeze report) - do not
    // invent an IMPORTANT_CHANGE floor for UpdateMilestone.
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Explicit),
        ConfirmationClass::ExplicitAdd
    );
    assert!(!blocks_until_confirmed(ConfirmationClass::ExplicitAdd));
}

#[test]
fn update_milestone_inferred_provenance_becomes_inferred_change() {
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Inferred),
        ConfirmationClass::InferredChange
    );
    assert!(blocks_until_confirmed(ConfirmationClass::InferredChange));
}

// =========================================================================
// Task / AddTask / UpdateTask (M2 Task vertical slice, 2026-09-17) -
// direct unit tests. `TaskStatus`'s string mappings/parse round trips
// live in `domain::tests` (pure domain-type concern, same precedent as
// `LifecycleStatus`/`MilestoneStatus`) - not duplicated here.
// `project_id`/`goal_id` existence both need a database read and are
// proven in `services::tests` instead.
// =========================================================================

fn base_task(status: TaskStatus) -> Task {
    Task {
        id: TaskId::new(),
        title: "Base task".to_string(),
        status,
        due_at: None,
        project_id: None,
        goal_id: None,
        recurring_rule_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn base_add_task_payload() -> AddTaskPayload {
    AddTaskPayload {
        title: "Valid task title".to_string(),
        due_at: None,
        project_id: None,
        goal_id: None,
    }
}

#[test]
fn add_task_title_is_trimmed() {
    let payload = AddTaskPayload {
        title: "  Buy groceries  ".to_string(),
        ..base_add_task_payload()
    };
    let title = validate_add_task(&payload).expect("should validate");
    assert_eq!(title, "Buy groceries");
}

#[test]
fn add_task_rejects_empty_title() {
    for bad in ["", "   ", "\t\n"] {
        let payload = AddTaskPayload {
            title: bad.to_string(),
            ..base_add_task_payload()
        };
        let errors = validate_add_task(&payload).expect_err("empty title must be rejected");
        assert!(
            errors.contains(&ValidationError::EmptyTitle),
            "expected EmptyTitle for {bad:?}, got {errors:?}"
        );
    }
}

#[test]
fn add_task_rejects_title_too_long() {
    let payload = AddTaskPayload {
        title: "x".repeat(TITLE_MAX_LEN + 1),
        ..base_add_task_payload()
    };
    let errors = validate_add_task(&payload).expect_err("overlong title must be rejected");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, ValidationError::TitleTooLong { .. })),
        "expected TitleTooLong, got {errors:?}"
    );
}

#[test]
fn add_task_explicit_provenance_is_explicit_add() {
    // Architecture-freeze report §F confirmation table / M2 Project
    // Child Action Freeze report list AddTask under EXPLICIT_ADD.
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Explicit),
        ConfirmationClass::ExplicitAdd
    );
    assert!(!blocks_until_confirmed(ConfirmationClass::ExplicitAdd));
}

#[test]
fn add_task_inferred_provenance_becomes_inferred_change() {
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Inferred),
        ConfirmationClass::InferredChange
    );
    assert!(blocks_until_confirmed(ConfirmationClass::InferredChange));
}

// --- UpdateTask validation ------------------------------------------------

#[test]
fn update_task_rejects_when_no_fields_touched() {
    let existing = base_task(TaskStatus::Todo);
    let payload = UpdateTaskPayload {
        id: existing.id,
        ..Default::default()
    };
    let errors =
        validate_update_task(&existing, &payload).expect_err("no-op update must be rejected");
    assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
}

#[test]
fn update_task_field_patch_unchanged_alone_is_not_a_touch() {
    let existing = base_task(TaskStatus::Todo);
    let payload = UpdateTaskPayload {
        id: existing.id,
        due_at: FieldPatch::Unchanged,
        project_id: FieldPatch::Unchanged,
        goal_id: FieldPatch::Unchanged,
        ..Default::default()
    };
    let errors = validate_update_task(&existing, &payload)
        .expect_err("all-Unchanged update must be rejected as touching nothing");
    assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
}

#[test]
fn update_task_title_some_invalid_is_rejected() {
    let existing = base_task(TaskStatus::Todo);
    let payload = UpdateTaskPayload {
        id: existing.id,
        title: Some("   ".to_string()),
        ..Default::default()
    };
    let errors =
        validate_update_task(&existing, &payload).expect_err("empty Some(title) must be rejected");
    assert!(errors.contains(&ValidationError::EmptyTitle));
}

#[test]
fn update_task_due_at_field_patch_semantics_all_valid_alone() {
    let existing = base_task(TaskStatus::Todo);
    for patch in [FieldPatch::Set(Utc::now()), FieldPatch::Clear] {
        let payload = UpdateTaskPayload {
            id: existing.id,
            due_at: patch,
            ..Default::default()
        };
        assert!(validate_update_task(&existing, &payload).is_ok());
    }
}

#[test]
fn update_task_project_id_field_patch_semantics_all_valid_alone() {
    let existing = base_task(TaskStatus::Todo);
    for patch in [FieldPatch::Set(ProjectId::new()), FieldPatch::Clear] {
        let payload = UpdateTaskPayload {
            id: existing.id,
            project_id: patch,
            ..Default::default()
        };
        assert!(validate_update_task(&existing, &payload).is_ok());
    }
}

#[test]
fn update_task_goal_id_field_patch_semantics_all_valid_alone() {
    let existing = base_task(TaskStatus::Todo);
    for patch in [FieldPatch::Set(GoalId::new()), FieldPatch::Clear] {
        let payload = UpdateTaskPayload {
            id: existing.id,
            goal_id: patch,
            ..Default::default()
        };
        assert!(validate_update_task(&existing, &payload).is_ok());
    }
}

#[test]
fn update_task_all_four_statuses_are_accepted_by_pure_validation() {
    // No transition matrix exists - any typed TaskStatus is accepted by
    // pure validation regardless of the existing status.
    let existing = base_task(TaskStatus::Todo);
    for status in [
        TaskStatus::Todo,
        TaskStatus::InProgress,
        TaskStatus::Done,
        TaskStatus::Cancelled,
    ] {
        let payload = UpdateTaskPayload {
            id: existing.id,
            status: Some(status),
            ..Default::default()
        };
        assert!(
            validate_update_task(&existing, &payload).is_ok(),
            "status {status:?} should be accepted - no terminal-state rule exists"
        );
    }
}

#[test]
fn update_task_no_terminal_state_validation_exists() {
    // Reopening a DONE or CANCELLED task must not be rejected by pure
    // validation - matches the Milestone/Goal hardening precedent.
    for existing_status in [TaskStatus::Done, TaskStatus::Cancelled] {
        let existing = base_task(existing_status);
        let payload = UpdateTaskPayload {
            id: existing.id,
            status: Some(TaskStatus::Todo),
            ..Default::default()
        };
        assert!(
            validate_update_task(&existing, &payload).is_ok(),
            "{existing_status:?} -> Todo must be allowed - no terminal-state rule is frozen"
        );
    }
}

#[test]
fn update_task_explicit_provenance_is_explicit_add() {
    // Frozen floor (M2 Project Child Action Freeze report) - do not
    // invent an IMPORTANT_CHANGE floor for UpdateTask.
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Explicit),
        ConfirmationClass::ExplicitAdd
    );
    assert!(!blocks_until_confirmed(ConfirmationClass::ExplicitAdd));
}

#[test]
fn update_task_inferred_provenance_becomes_inferred_change() {
    assert_eq!(
        effective_class(ConfirmationClass::ExplicitAdd, Provenance::Inferred),
        ConfirmationClass::InferredChange
    );
    assert!(blocks_until_confirmed(ConfirmationClass::InferredChange));
}

// =========================================================================
// UpdateProject (M2 UpdateProject implementation, 2026-09-17) - direct
// unit tests. `goal_id` existence needs a database read and is proven in
// `services::tests` instead. `progress` is deliberately absent from
// `UpdateProjectPayload` entirely - there is no field to test here,
// which is itself the point (see `services::tests` for the dedicated
// progress-preservation regression proof).
// =========================================================================

fn base_project(status: LifecycleStatus) -> Project {
    Project {
        id: ProjectId::new(),
        title: "Base project".to_string(),
        description: None,
        status,
        priority: None,
        goal_id: None,
        progress: 0,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[test]
fn update_project_rejects_when_no_fields_touched() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        ..Default::default()
    };
    let errors =
        validate_update_project(&existing, &payload).expect_err("no-op update must be rejected");
    assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
}

#[test]
fn update_project_field_patch_unchanged_alone_is_not_a_touch() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        description: FieldPatch::Unchanged,
        priority: FieldPatch::Unchanged,
        goal_id: FieldPatch::Unchanged,
        ..Default::default()
    };
    let errors = validate_update_project(&existing, &payload)
        .expect_err("all-Unchanged update must be rejected as touching nothing");
    assert!(errors.contains(&ValidationError::NoFieldsToUpdate));
}

#[test]
fn update_project_title_some_is_trimmed_before_validation() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        title: Some("  Renamed Project  ".to_string()),
        ..Default::default()
    };
    assert!(validate_update_project(&existing, &payload).is_ok());
}

#[test]
fn update_project_empty_title_is_rejected() {
    let existing = base_project(LifecycleStatus::Active);
    for bad in ["", "   ", "\t\n"] {
        let payload = UpdateProjectPayload {
            id: existing.id,
            title: Some(bad.to_string()),
            ..Default::default()
        };
        let errors = validate_update_project(&existing, &payload)
            .expect_err("empty Some(title) must be rejected");
        assert!(
            errors.contains(&ValidationError::EmptyTitle),
            "expected EmptyTitle for {bad:?}, got {errors:?}"
        );
    }
}

#[test]
fn update_project_overlong_title_is_rejected() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        title: Some("x".repeat(TITLE_MAX_LEN + 1)),
        ..Default::default()
    };
    let errors =
        validate_update_project(&existing, &payload).expect_err("overlong title must be rejected");
    assert!(errors
        .iter()
        .any(|e| matches!(e, ValidationError::TitleTooLong { .. })));
}

#[test]
fn update_project_description_set_valid_is_accepted() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        description: FieldPatch::Set("A real description".to_string()),
        ..Default::default()
    };
    assert!(validate_update_project(&existing, &payload).is_ok());
}

#[test]
fn update_project_description_clear_is_accepted() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        description: FieldPatch::Clear,
        ..Default::default()
    };
    assert!(
        validate_update_project(&existing, &payload).is_ok(),
        "Clear should never fail length validation"
    );
}

#[test]
fn update_project_overlong_description_set_is_rejected() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        description: FieldPatch::Set("x".repeat(DESCRIPTION_MAX_LEN + 1)),
        ..Default::default()
    };
    let errors = validate_update_project(&existing, &payload)
        .expect_err("overlong Set(description) must be rejected");
    assert!(errors
        .iter()
        .any(|e| matches!(e, ValidationError::DescriptionTooLong { .. })));
}

#[test]
fn update_project_priority_set_is_accepted() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        priority: FieldPatch::Set(3),
        ..Default::default()
    };
    assert!(validate_update_project(&existing, &payload).is_ok());
}

#[test]
fn update_project_priority_clear_is_accepted() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        priority: FieldPatch::Clear,
        ..Default::default()
    };
    assert!(validate_update_project(&existing, &payload).is_ok());
}

#[test]
fn update_project_priority_set_out_of_bounds_is_rejected() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        priority: FieldPatch::Set(PRIORITY_MAX + 1),
        ..Default::default()
    };
    let errors = validate_update_project(&existing, &payload)
        .expect_err("out-of-bounds Set(priority) must be rejected");
    assert!(errors
        .iter()
        .any(|e| matches!(e, ValidationError::PriorityOutOfBounds { .. })));
}

#[test]
fn update_project_goal_id_set_is_accepted_at_pure_layer() {
    // Existence of the referenced Goal needs a database read and is
    // proven in services::tests - this pure layer only proves the
    // structural shape is accepted.
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        goal_id: FieldPatch::Set(GoalId::new()),
        ..Default::default()
    };
    assert!(validate_update_project(&existing, &payload).is_ok());
}

#[test]
fn update_project_goal_id_clear_is_accepted() {
    let existing = base_project(LifecycleStatus::Active);
    let payload = UpdateProjectPayload {
        id: existing.id,
        goal_id: FieldPatch::Clear,
        ..Default::default()
    };
    assert!(validate_update_project(&existing, &payload).is_ok());
}

#[test]
fn update_project_all_four_statuses_are_accepted_by_pure_validation() {
    let existing = base_project(LifecycleStatus::Active);
    for status in [
        LifecycleStatus::Active,
        LifecycleStatus::Paused,
        LifecycleStatus::Completed,
        LifecycleStatus::Abandoned,
    ] {
        let payload = UpdateProjectPayload {
            id: existing.id,
            status: Some(status),
            ..Default::default()
        };
        assert!(
            validate_update_project(&existing, &payload).is_ok(),
            "status {status:?} should be accepted - no terminal-state rule exists"
        );
    }
}

#[test]
fn update_project_completed_to_active_is_not_rejected() {
    let existing = base_project(LifecycleStatus::Completed);
    let payload = UpdateProjectPayload {
        id: existing.id,
        status: Some(LifecycleStatus::Active),
        ..Default::default()
    };
    assert!(
        validate_update_project(&existing, &payload).is_ok(),
        "reopening a COMPLETED project must be allowed - no terminal-state rule is frozen"
    );
}

#[test]
fn update_project_abandoned_to_active_is_not_rejected() {
    let existing = base_project(LifecycleStatus::Abandoned);
    let payload = UpdateProjectPayload {
        id: existing.id,
        status: Some(LifecycleStatus::Active),
        ..Default::default()
    };
    assert!(
        validate_update_project(&existing, &payload).is_ok(),
        "reopening an ABANDONED project must be allowed - no terminal-state rule is frozen"
    );
}

#[test]
fn update_project_explicit_important_change_stays_important_change() {
    // Frozen floor (M2 Project Child Action Freeze report) -
    // IMPORTANT_CHANGE's type floor is already above ExplicitAdd's
    // provenance-derived class, so Explicit provenance changes nothing.
    assert_eq!(
        effective_class(ConfirmationClass::ImportantChange, Provenance::Explicit),
        ConfirmationClass::ImportantChange
    );
}

#[test]
fn update_project_inferred_important_change_stays_important_change() {
    // IMPORTANT_CHANGE's type floor (severity 3) is already above
    // InferredChange's provenance-derived class (severity 2), so
    // Inferred provenance does not raise it further - effective_class
    // takes the max of the two, and the type floor already wins.
    assert_eq!(
        effective_class(ConfirmationClass::ImportantChange, Provenance::Inferred),
        ConfirmationClass::ImportantChange
    );
}

#[test]
fn update_project_important_change_blocks_until_confirmed_matches_generic_policy() {
    // Do not change blocks_until_confirmed merely to make UpdateProject
    // behave differently - this asserts against the EXISTING generic
    // policy (ImportantChange executes immediately, always audited and
    // Undo-able; only InferredChange/Delete block).
    assert!(!blocks_until_confirmed(ConfirmationClass::ImportantChange));
}

// --- AddMemory (M2 Personal Knowledge Slice B, 2026-09-18) --------------
//
// Pure `validate_add_memory` unit tests - no database, matching this
// file's existing precedent for Goal/Project. Database-dependent
// behavior (Existing-reference existence, domain-ref authoritative
// existence, persisted-duplicate-domain-ref, confirmation-gate wiring
// through `services::add_memory`, atomic rollback, audit rows) is
// covered by `services::tests` instead (R021).

fn base_add_memory_payload() -> AddMemoryPayload {
    AddMemoryPayload {
        subject: EntityInputRef::SelfEntity,
        predicate: "likes".to_string(),
        value: FactValueInput::ScalarJson("\"coffee\"".to_string()),
        memory_type: MemoryType::Preference,
        epistemic_state: EpistemicState::UserConfirmed,
        confidence: None,
        sensitivity: Sensitivity::Internal,
        source: SourceRef::New(NewSourceData {
            source_type: "conversation".to_string(),
            label: None,
            reference: None,
            sensitivity: Sensitivity::Internal,
            captured_at: Utc::now(),
        }),
        evidence: NewEvidenceData {
            content: Some("user said they like coffee".to_string()),
            locator: None,
            sensitivity: Sensitivity::Internal,
        },
    }
}

#[test]
fn add_memory_inferred_provenance_effective_class_blocks_with_zero_writes_property() {
    // AddMemory's confirmation floor is ExplicitAdd (severity 1). The
    // *generic* effective_class formula alone must already force a
    // pre-write block for Provenance::Inferred (InferredChange, severity
    // 2, dominates) - no Personal-Knowledge-specific override exists for
    // AddMemory (unlike CorrectMemory's ImportantChange floor). This is
    // the frozen safety property R018 §3/R021 §2 require a dedicated
    // test for, even though it "falls out for free" from shared
    // infrastructure.
    let class = effective_class(ConfirmationClass::ExplicitAdd, Provenance::Inferred);
    assert_eq!(class, ConfirmationClass::InferredChange);
    assert!(blocks_until_confirmed(class));

    // Explicit provenance never blocks at this floor.
    let explicit_class = effective_class(ConfirmationClass::ExplicitAdd, Provenance::Explicit);
    assert_eq!(explicit_class, ConfirmationClass::ExplicitAdd);
    assert!(!blocks_until_confirmed(explicit_class));
}

#[test]
fn add_memory_valid_baseline_payload_is_accepted() {
    assert!(validate_add_memory(&base_add_memory_payload()).is_ok());
}

#[test]
fn add_memory_confidence_accepted_values() {
    for c in [None, Some(0.0f32), Some(1.0f32), Some(0.75f32)] {
        let payload = AddMemoryPayload {
            confidence: c,
            ..base_add_memory_payload()
        };
        assert!(
            validate_add_memory(&payload).is_ok(),
            "confidence {c:?} should be accepted"
        );
    }
}

#[test]
fn add_memory_confidence_rejected_values() {
    for c in [
        -0.0001f32,
        1.0001f32,
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ] {
        let payload = AddMemoryPayload {
            confidence: Some(c),
            ..base_add_memory_payload()
        };
        match validate_add_memory(&payload) {
            Err(errors) => assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ValidationError::ConfidenceOutOfBounds { .. })),
                "confidence {c:?} should produce ConfidenceOutOfBounds, got {errors:?}"
            ),
            Ok(_) => panic!("confidence {c:?} should have been rejected"),
        }
    }
}

#[test]
fn add_memory_scalar_json_accepted_types() {
    for json in ["\"a string\"", "42", "true", "null", "{\"k\":1}", "[1,2,3]"] {
        let payload = AddMemoryPayload {
            value: FactValueInput::ScalarJson(json.to_string()),
            ..base_add_memory_payload()
        };
        assert!(
            validate_add_memory(&payload).is_ok(),
            "JSON value {json:?} should be accepted - value_json is not scalar-only"
        );
    }
}

#[test]
fn add_memory_scalar_json_rejected_malformed_and_overlong() {
    let malformed = AddMemoryPayload {
        value: FactValueInput::ScalarJson("{not valid json".to_string()),
        ..base_add_memory_payload()
    };
    match validate_add_memory(&malformed) {
        Err(errors) => assert!(errors.contains(&ValidationError::InvalidValueJson)),
        Ok(_) => panic!("malformed JSON should have been rejected"),
    }

    let overlong = AddMemoryPayload {
        value: FactValueInput::ScalarJson(format!("\"{}\"", "x".repeat(VALUE_JSON_MAX_LEN + 1))),
        ..base_add_memory_payload()
    };
    match validate_add_memory(&overlong) {
        Err(errors) => assert!(errors.iter().any(|e| matches!(
            e,
            ValidationError::PersonalKnowledgeFieldTooLong {
                field: "value_json",
                ..
            }
        ))),
        Ok(_) => panic!("overlong value_json should have been rejected"),
    }
}

#[test]
fn add_memory_evidence_requires_content_or_locator() {
    let payload = AddMemoryPayload {
        evidence: NewEvidenceData {
            content: None,
            locator: None,
            sensitivity: Sensitivity::Internal,
        },
        ..base_add_memory_payload()
    };
    match validate_add_memory(&payload) {
        Err(errors) => assert!(errors.contains(&ValidationError::EvidenceRequiresContentOrLocator)),
        Ok(_) => panic!("evidence with neither content nor locator should have been rejected"),
    }
}

#[test]
fn add_memory_required_field_empty_after_trim_rejected() {
    let payload = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "   ".to_string(),
            canonical_name: "Real Name".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: None,
        }),
        ..base_add_memory_payload()
    };
    match validate_add_memory(&payload) {
        Err(errors) => assert!(
            errors.contains(&ValidationError::PersonalKnowledgeFieldEmpty {
                field: "entity_type"
            })
        ),
        Ok(_) => panic!("whitespace-only entity_type should have been rejected"),
    }
}

#[test]
fn add_memory_required_and_optional_field_bounds_enforced() {
    let over = |max: usize| "x".repeat(max + 1);

    let required_cases: &[(&str, usize)] = &[
        ("entity_type", ENTITY_TYPE_MAX_LEN),
        ("canonical_name", CANONICAL_NAME_MAX_LEN),
        ("predicate", PREDICATE_MAX_LEN),
        ("source_type", SOURCE_TYPE_MAX_LEN),
    ];
    for (field, max) in required_cases {
        let mut payload = base_add_memory_payload();
        match *field {
            "entity_type" => {
                payload.subject = EntityInputRef::New(NewEntityData {
                    entity_type: over(*max),
                    canonical_name: "Name".to_string(),
                    sensitivity: Sensitivity::Internal,
                    domain_ref: None,
                })
            }
            "canonical_name" => {
                payload.subject = EntityInputRef::New(NewEntityData {
                    entity_type: "person".to_string(),
                    canonical_name: over(*max),
                    sensitivity: Sensitivity::Internal,
                    domain_ref: None,
                })
            }
            "predicate" => payload.predicate = over(*max),
            "source_type" => {
                payload.source = SourceRef::New(NewSourceData {
                    source_type: over(*max),
                    label: None,
                    reference: None,
                    sensitivity: Sensitivity::Internal,
                    captured_at: Utc::now(),
                })
            }
            _ => unreachable!(),
        }
        match validate_add_memory(&payload) {
            Err(errors) => assert!(
                errors.iter().any(|e| matches!(
                    e,
                    ValidationError::PersonalKnowledgeFieldTooLong { field: f, .. } if *f == *field
                )),
                "field {field} over max {max} should report PersonalKnowledgeFieldTooLong, got {errors:?}"
            ),
            Ok(_) => panic!("field {field} over max {max} should have been rejected"),
        }
    }

    // Optional fields: bounded, but empty/absent is fine (no trim/empty
    // requirement, matching validate_description's precedent).
    let mut payload = base_add_memory_payload();
    payload.evidence = NewEvidenceData {
        content: Some(over(EVIDENCE_CONTENT_MAX_LEN)),
        locator: None,
        sensitivity: Sensitivity::Internal,
    };
    match validate_add_memory(&payload) {
        Err(errors) => assert!(errors.iter().any(|e| matches!(
            e,
            ValidationError::PersonalKnowledgeFieldTooLong {
                field: "content",
                ..
            }
        ))),
        Ok(_) => panic!("overlong evidence content should have been rejected"),
    }
}

#[test]
fn add_memory_same_payload_duplicate_domain_ref_rejected_subject_and_object() {
    let domain_ref = DomainRef {
        domain_ref_type: "Goal".to_string(),
        domain_ref_id: GoalId::new().to_string(),
    };
    let payload = AddMemoryPayload {
        subject: EntityInputRef::New(NewEntityData {
            entity_type: "goal".to_string(),
            canonical_name: "Some Goal".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(domain_ref.clone()),
        }),
        value: FactValueInput::Entity(EntityInputRef::New(NewEntityData {
            entity_type: "goal".to_string(),
            canonical_name: "Same Goal Again".to_string(),
            sensitivity: Sensitivity::Internal,
            domain_ref: Some(domain_ref),
        })),
        ..base_add_memory_payload()
    };
    // Both a real, independent domain-ref existence check AND the
    // same-payload duplicate check would need a database read for the
    // former - this pure test cannot prove existence, only that the
    // structural duplicate is caught. Both New candidates sharing one
    // domain_ref must be rejected regardless of whether that domain_ref
    // would otherwise resolve.
    match validate_add_memory(&payload) {
        Err(errors) => assert!(
            errors.contains(&ValidationError::DuplicateDomainReference),
            "expected DuplicateDomainReference, got {errors:?}"
        ),
        Ok(_) => panic!("same-payload duplicate domain_ref should have been rejected"),
    }
}

#[test]
fn add_memory_self_edge_subject_equals_object_is_structurally_allowed() {
    // No invented restriction against subject == object (R021 §10) -
    // both Existing referencing the same EntityId must pass pure
    // structural validation (existence itself needs a database read;
    // services::tests proves the full self-edge end-to-end).
    let id = EntityId::new();
    let payload = AddMemoryPayload {
        subject: EntityInputRef::Existing(id),
        value: FactValueInput::Entity(EntityInputRef::Existing(id)),
        ..base_add_memory_payload()
    };
    assert!(validate_add_memory(&payload).is_ok());
}

#[test]
fn add_memory_normalized_predicate_is_trimmed() {
    let payload = AddMemoryPayload {
        predicate: "  likes  ".to_string(),
        ..base_add_memory_payload()
    };
    let normalized = validate_add_memory(&payload).expect("baseline payload should validate");
    assert_eq!(normalized.predicate, "likes");
}
