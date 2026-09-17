//! LifeOS domain types (M2 Goal vertical slice, 2026-09-17; Project
//! vertical slice added 2026-09-17; Project/storage integrity hardening
//! 2026-09-17 - see that report for the `LifecycleStatus` rename and
//! the canonical `Project.progress` resolution below).
//!
//! Pure data, no I/O, no SQL - matches `LIFEOS_SPEC.md` §5.6's module
//! boundaries. Only `Goal` and `Project` are implemented so far
//! (`LIFEOS_SPEC.md` §5.1's frozen v0.1 domain model has more;
//! `GoalRelation`/`Milestone`/`Task`/`Fact`/etc. are explicitly out of
//! scope for this run).

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

/// `LIFEOS_SPEC.md` §5.1 / §C of the architecture-freeze report. Shared
/// by both `Goal.status` and `Project.status` - the freeze defines
/// Project's status as "the same 4 values as Goal," so this one type
/// backs both rather than a duplicate `ProjectStatus` enum existing
/// alongside an original `GoalStatus`.
///
/// **Renamed from `GoalStatus` (2026-09-17, M2 Project/storage
/// integrity hardening).** The Project vertical slice already reused
/// this type for `Project.status`, which left a Goal-specific name
/// backing two different entities - purely a Rust identifier rename,
/// behavior-preserving: the variants, their `as_str()`/`parse()`
/// mappings, and therefore every `TEXT` value ever written to a
/// `status` column (`'ACTIVE'`/`'PAUSED'`/`'COMPLETED'`/`'ABANDONED'`)
/// are all unchanged. No migration was needed or written for this -
/// nothing in the database's own schema or data references a Rust type
/// name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum LifecycleStatus {
    Active,
    Paused,
    Completed,
    Abandoned,
}

impl LifecycleStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            LifecycleStatus::Active => "ACTIVE",
            LifecycleStatus::Paused => "PAUSED",
            LifecycleStatus::Completed => "COMPLETED",
            LifecycleStatus::Abandoned => "ABANDONED",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "ACTIVE" => Some(LifecycleStatus::Active),
            "PAUSED" => Some(LifecycleStatus::Paused),
            "COMPLETED" => Some(LifecycleStatus::Completed),
            "ABANDONED" => Some(LifecycleStatus::Abandoned),
            _ => None,
        }
    }
}

// No `can_transition_to`/terminal-state rule here (removed 2026-09-17, M2
// Goal hardening, before this type was shared with Project). The Goal
// vertical slice invented a rule - COMPLETED and ABANDONED cannot
// transition out - that was never part of the frozen Goal status
// definition; a real user may reasonably reopen a completed or
// abandoned goal or project. All four statuses are peer states for
// both entities that use this type: every explicit user-requested
// transition between them is allowed and, for Goal, always
// audit-logged (`actions::validate_update_goal` no longer restricts
// this - Project currently has no analogous status-changing action to
// restrict, §6 of the M2 Project/storage integrity hardening report).
// Do not reintroduce a terminal-state policy here unless
// `LIFEOS_SPEC.md` explicitly freezes one first.

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
    pub status: LifecycleStatus,
    pub target_date: Option<DateTime<Utc>>,
    pub priority_user: Option<Priority>,
    pub priority_system: Option<Priority>,
    pub progress: Progress,
    pub domain_tag: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A stable, globally unique Project identifier - same UUID v7 design as
/// `GoalId` (M2 Project vertical slice, 2026-09-17). Kept as its own
/// newtype rather than reusing `GoalId` for a different entity: the two
/// ID spaces must stay distinct at the type level (a `ProjectId` must
/// never be usable where a `GoalId` is expected, and vice versa) - this
/// is type-safety, not the kind of duplication `Progress`/`Priority`
/// reuse below is meant to avoid.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct ProjectId(pub Uuid);

impl ProjectId {
    pub(crate) fn new() -> Self {
        ProjectId(Uuid::now_v7())
    }

    pub(crate) fn parse(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(ProjectId)
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// `Project` (M2 Project vertical slice, 2026-09-17). Field set is
/// exactly the frozen architecture-freeze definition (M2 report §C):
/// `id`, `title`, `description`, `status` (same 4 values as `Goal` -
/// reuses `LifecycleStatus` directly rather than defining a duplicate
/// `ProjectStatus` enum, since the freeze explicitly says "same 4
/// values as Goal"), `priority` (reuses the shared `Priority`
/// type/scale, singular - unlike Goal there is no user/system split),
/// `goal_id` (nullable - the first real cross-entity relationship in
/// production M2; NOT the future `GoalRelation` graph, which remains
/// unimplemented), `created_at`, `updated_at`.
///
/// `progress: Progress` is this struct's one field beyond the
/// architecture-freeze report's terse one-line table - **canonically
/// resolved (2026-09-17, M2 Project/storage integrity hardening,
/// `LIFEOS_SPEC.md` §5.1)**, no longer an open interpretation: this is
/// the CURRENT normalized progress snapshot for the Project (0..=100,
/// same shared `Progress` type/semantics as `Goal.progress`), mutated
/// in place by `UpdateProgress`. It is conceptually distinct from the
/// separate, still-unimplemented, polymorphic `ProgressUpdate` entity
/// (GOAL/PROJECT/TASK) in the freeze, which is a temporal *history* of
/// progress observations/changes over time - `ProgressUpdate` does not
/// replace this field and does not remove the need for a current
/// snapshot living directly on `Project`, the same way `Goal.progress`
/// already does. `ProgressUpdate` itself remains out of scope; only
/// this field's own status changed from "open question" to "frozen."
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Project {
    pub id: ProjectId,
    pub title: String,
    pub description: Option<String>,
    pub status: LifecycleStatus,
    pub priority: Option<Priority>,
    pub goal_id: Option<GoalId>,
    pub progress: Progress,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests;
