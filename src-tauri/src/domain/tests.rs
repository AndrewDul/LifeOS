//! Pure `LifecycleStatus` round-trip tests (M2 Project/storage
//! integrity hardening, 2026-09-17).
//!
//! Proves the `GoalStatus` -> `LifecycleStatus` rename (that report's
//! §5) is genuinely behavior-preserving: the same four `TEXT` values
//! this type has always mapped to/from (`'ACTIVE'`/`'PAUSED'`/
//! `'COMPLETED'`/`'ABANDONED'`) are unchanged - a Rust identifier
//! rename only, not a data-model change. Both `Goal` and `Project`
//! using this type identically is additionally proven by the full
//! existing Goal/Project test suites in `actions::tests`/
//! `services::tests` continuing to pass unmodified after the rename.

use super::*;

#[test]
fn lifecycle_status_as_str_matches_frozen_db_text_values() {
    assert_eq!(LifecycleStatus::Active.as_str(), "ACTIVE");
    assert_eq!(LifecycleStatus::Paused.as_str(), "PAUSED");
    assert_eq!(LifecycleStatus::Completed.as_str(), "COMPLETED");
    assert_eq!(LifecycleStatus::Abandoned.as_str(), "ABANDONED");
}

#[test]
fn lifecycle_status_parse_round_trips_through_as_str() {
    for status in [
        LifecycleStatus::Active,
        LifecycleStatus::Paused,
        LifecycleStatus::Completed,
        LifecycleStatus::Abandoned,
    ] {
        let text = status.as_str();
        assert_eq!(
            LifecycleStatus::parse(text),
            Some(status),
            "round trip through {text:?} did not return {status:?}"
        );
    }
}

#[test]
fn lifecycle_status_parse_rejects_unknown_text() {
    assert_eq!(LifecycleStatus::parse("SOMETHING_ELSE"), None);
    assert_eq!(LifecycleStatus::parse(""), None);
    assert_eq!(LifecycleStatus::parse("active"), None); // case-sensitive
}

// =========================================================================
// GoalRelationType (M2 GoalRelation vertical slice, 2026-09-17) - the
// frozen, closed 11-value enum. All 11 string mappings and all 11
// parse round trips proven directly, per the brief's explicit
// requirement, plus rejection of unknown text.
// =========================================================================

const ALL_GOAL_RELATION_TYPES: [GoalRelationType; 11] = [
    GoalRelationType::Supports,
    GoalRelationType::ConflictsWith,
    GoalRelationType::DependsOn,
    GoalRelationType::Blocks,
    GoalRelationType::ContributesTo,
    GoalRelationType::Requires,
    GoalRelationType::ParentOf,
    GoalRelationType::ChildOf,
    GoalRelationType::Duplicates,
    GoalRelationType::Supersedes,
    GoalRelationType::CompetesForResource,
];

#[test]
fn goal_relation_type_as_str_matches_all_11_frozen_values() {
    assert_eq!(GoalRelationType::Supports.as_str(), "SUPPORTS");
    assert_eq!(GoalRelationType::ConflictsWith.as_str(), "CONFLICTS_WITH");
    assert_eq!(GoalRelationType::DependsOn.as_str(), "DEPENDS_ON");
    assert_eq!(GoalRelationType::Blocks.as_str(), "BLOCKS");
    assert_eq!(GoalRelationType::ContributesTo.as_str(), "CONTRIBUTES_TO");
    assert_eq!(GoalRelationType::Requires.as_str(), "REQUIRES");
    assert_eq!(GoalRelationType::ParentOf.as_str(), "PARENT_OF");
    assert_eq!(GoalRelationType::ChildOf.as_str(), "CHILD_OF");
    assert_eq!(GoalRelationType::Duplicates.as_str(), "DUPLICATES");
    assert_eq!(GoalRelationType::Supersedes.as_str(), "SUPERSEDES");
    assert_eq!(
        GoalRelationType::CompetesForResource.as_str(),
        "COMPETES_FOR_RESOURCE"
    );
}

#[test]
fn goal_relation_type_parse_round_trips_through_as_str_for_all_11() {
    for relation_type in ALL_GOAL_RELATION_TYPES {
        let text = relation_type.as_str();
        assert_eq!(
            GoalRelationType::parse(text),
            Some(relation_type),
            "round trip through {text:?} did not return {relation_type:?}"
        );
    }
}

#[test]
fn goal_relation_type_parse_rejects_unknown_text() {
    assert_eq!(GoalRelationType::parse("SOMETHING_ELSE"), None);
    assert_eq!(GoalRelationType::parse(""), None);
    assert_eq!(GoalRelationType::parse("supports"), None); // case-sensitive
    assert_eq!(GoalRelationType::parse("PARENTOF"), None); // no missing underscore
}

// =========================================================================
// MilestoneStatus (M2 Milestone vertical slice, 2026-09-17) - the
// frozen, closed 2-value enum. Not LifecycleStatus - a genuinely
// different value set, see the type's own doc comment.
// =========================================================================

#[test]
fn milestone_status_as_str_matches_frozen_db_text_values() {
    assert_eq!(MilestoneStatus::Pending.as_str(), "PENDING");
    assert_eq!(MilestoneStatus::Done.as_str(), "DONE");
}

#[test]
fn milestone_status_parse_round_trips_through_as_str() {
    for status in [MilestoneStatus::Pending, MilestoneStatus::Done] {
        let text = status.as_str();
        assert_eq!(
            MilestoneStatus::parse(text),
            Some(status),
            "round trip through {text:?} did not return {status:?}"
        );
    }
}

#[test]
fn milestone_status_parse_rejects_unknown_text() {
    assert_eq!(MilestoneStatus::parse("SOMETHING_ELSE"), None);
    assert_eq!(MilestoneStatus::parse(""), None);
    assert_eq!(MilestoneStatus::parse("pending"), None); // case-sensitive
    assert_eq!(MilestoneStatus::parse("ACTIVE"), None); // not a LifecycleStatus value
}
