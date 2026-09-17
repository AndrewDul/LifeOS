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
