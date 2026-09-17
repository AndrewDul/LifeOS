//! LifeOS domain types (M2 Goal vertical slice, 2026-09-17).
//!
//! Pure data, no I/O, no SQL - matches `LIFEOS_SPEC.md` §5.6's module
//! boundaries. Only `Goal` is implemented this slice
//! (`LIFEOS_SPEC.md` §5.1's frozen v0.1 domain model has more; Project/
//! Task/Fact/etc. are explicitly out of scope for this run).

// Not wired into `run()` yet this run (no capability/Tauri-command
// layer exists) - only exercised by services::tests, hence dead_code
// would otherwise warn on every non-test build.
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// A stable, globally unique Goal identifier. UUID v7 (time-ordered) per
/// the build brief - suitable for future multi-device/NeXa sync where
/// IDs must be assignable offline without a central authority, while
/// still sorting roughly by creation time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct GoalId(pub Uuid);

impl GoalId {
    pub(crate) fn new() -> Self {
        GoalId(Uuid::now_v7())
    }

    pub(crate) fn parse(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(GoalId)
    }
}

impl std::fmt::Display for GoalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// `LIFEOS_SPEC.md` §5.1 / §C of the architecture-freeze report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum GoalStatus {
    Active,
    Paused,
    Completed,
    Abandoned,
}

impl GoalStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            GoalStatus::Active => "ACTIVE",
            GoalStatus::Paused => "PAUSED",
            GoalStatus::Completed => "COMPLETED",
            GoalStatus::Abandoned => "ABANDONED",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "ACTIVE" => Some(GoalStatus::Active),
            "PAUSED" => Some(GoalStatus::Paused),
            "COMPLETED" => Some(GoalStatus::Completed),
            "ABANDONED" => Some(GoalStatus::Abandoned),
            _ => None,
        }
    }

    /// COMPLETED and ABANDONED are terminal - once a goal reaches either,
    /// it cannot transition further. Every other transition (including a
    /// same-status no-op) is allowed. Documented here since this is the
    /// single source of truth `actions::validate_update_goal` enforces.
    pub(crate) fn can_transition_to(self, to: GoalStatus) -> bool {
        if self == to {
            return true;
        }
        !matches!(self, GoalStatus::Completed | GoalStatus::Abandoned)
    }
}

/// Progress is a deterministic bounded integer percentage, 0..=100.
/// Chosen over a float or an open-ended scale specifically to keep the
/// representation unambiguous (`LIFEOS_SPEC.md` §5.1 note).
pub(crate) type Progress = u8;
pub(crate) const PROGRESS_MIN: Progress = 0;
pub(crate) const PROGRESS_MAX: Progress = 100;

/// User- and system-declared priority share one deliberately small,
/// documented scale: 1 (lowest) to 5 (highest). `priority_system` is a
/// design placeholder for M6 Goal Intelligence
/// (`LIFEOS_SPEC.md` §5.2) - nothing computes it yet in M2.
pub(crate) type Priority = i32;
pub(crate) const PRIORITY_MIN: Priority = 1;
pub(crate) const PRIORITY_MAX: Priority = 5;

pub(crate) const TITLE_MAX_LEN: usize = 200;
pub(crate) const DESCRIPTION_MAX_LEN: usize = 4000;
pub(crate) const DOMAIN_TAG_MAX_LEN: usize = 60;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Goal {
    pub id: GoalId,
    pub title: String,
    pub description: Option<String>,
    pub status: GoalStatus,
    pub target_date: Option<DateTime<Utc>>,
    pub priority_user: Option<Priority>,
    pub priority_system: Option<Priority>,
    pub progress: Progress,
    pub domain_tag: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
