//! LifeOS domain types (M2 Goal vertical slice, 2026-09-17; Project
//! vertical slice added 2026-09-17; Project/storage integrity hardening
//! 2026-09-17 - see that report for the `LifecycleStatus` rename and
//! the canonical `Project.progress` resolution; GoalRelation vertical
//! slice added 2026-09-17 - the first true relationship-graph entity;
//! Milestone vertical slice added 2026-09-17 - the fourth entity, the
//! first required-parent child entity; Task vertical slice added
//! 2026-09-17 - the fifth entity, the first with two independent
//! nullable parent references).
//!
//! Pure data, no I/O, no SQL - matches `LIFEOS_SPEC.md` §5.6's module
//! boundaries. `Goal`, `Project`, `GoalRelation`, `Milestone`, and
//! `Task` are implemented so far (`LIFEOS_SPEC.md` §5.1's frozen v0.1
//! domain model has more; `Fact`/etc. are explicitly out of scope for
//! this run).

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

/// A stable, globally unique GoalRelation identifier - same UUID v7
/// design as `GoalId`/`ProjectId` (M2 GoalRelation vertical slice,
/// 2026-09-17). Its own newtype for the same type-safety reason
/// `ProjectId` is not `GoalId`: a `GoalRelationId` must never be usable
/// where a `GoalId` is expected, and vice versa.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct GoalRelationId(pub Uuid);

impl GoalRelationId {
    pub(crate) fn new() -> Self {
        GoalRelationId(Uuid::now_v7())
    }

    pub(crate) fn parse(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(GoalRelationId)
    }
}

impl std::fmt::Display for GoalRelationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The frozen, closed 11-value `GoalRelation` relation-type enum
/// (architecture-freeze report §D / `LIFEOS_SPEC.md` §5.2). **Distinct
/// from, and not to be confused with, the Personal Knowledge Layer's
/// separate `Relation.relation_type`** (`LIFEOS_SPEC.md` §5.3), which
/// is an intentionally free string deferred until M6 usage patterns
/// exist - the two are unrelated fields on unrelated entities that
/// happen to share a name. This one is closed and frozen: do not add or
/// remove variants without `LIFEOS_SPEC.md` re-freezing the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum GoalRelationType {
    Supports,
    ConflictsWith,
    DependsOn,
    Blocks,
    ContributesTo,
    Requires,
    /// Canonical hierarchy direction: write `ParentOf(parent -> child)`.
    /// The application derives "child of" by querying the reverse
    /// direction - never auto-insert the `ChildOf` inverse row (M2
    /// GoalRelation vertical-slice report §5).
    ParentOf,
    /// Kept as a valid, storable, parseable enum value because the
    /// freeze explicitly retains it - not a dead variant. Only ever
    /// written when a caller *deliberately* states `ChildOf` directly
    /// (e.g. recording "B is a child of A" from B's perspective without
    /// restating it as `ParentOf` from A's) - `LinkGoals` never
    /// auto-generates it as an inverse of `ParentOf`.
    ChildOf,
    Duplicates,
    Supersedes,
    CompetesForResource,
}

impl GoalRelationType {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            GoalRelationType::Supports => "SUPPORTS",
            GoalRelationType::ConflictsWith => "CONFLICTS_WITH",
            GoalRelationType::DependsOn => "DEPENDS_ON",
            GoalRelationType::Blocks => "BLOCKS",
            GoalRelationType::ContributesTo => "CONTRIBUTES_TO",
            GoalRelationType::Requires => "REQUIRES",
            GoalRelationType::ParentOf => "PARENT_OF",
            GoalRelationType::ChildOf => "CHILD_OF",
            GoalRelationType::Duplicates => "DUPLICATES",
            GoalRelationType::Supersedes => "SUPERSEDES",
            GoalRelationType::CompetesForResource => "COMPETES_FOR_RESOURCE",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "SUPPORTS" => Some(GoalRelationType::Supports),
            "CONFLICTS_WITH" => Some(GoalRelationType::ConflictsWith),
            "DEPENDS_ON" => Some(GoalRelationType::DependsOn),
            "BLOCKS" => Some(GoalRelationType::Blocks),
            "CONTRIBUTES_TO" => Some(GoalRelationType::ContributesTo),
            "REQUIRES" => Some(GoalRelationType::Requires),
            "PARENT_OF" => Some(GoalRelationType::ParentOf),
            "CHILD_OF" => Some(GoalRelationType::ChildOf),
            "DUPLICATES" => Some(GoalRelationType::Duplicates),
            "SUPERSEDES" => Some(GoalRelationType::Supersedes),
            "COMPETES_FOR_RESOURCE" => Some(GoalRelationType::CompetesForResource),
            _ => None,
        }
    }
}

/// `GoalRelation` (M2 GoalRelation vertical slice, 2026-09-17) - the
/// first true relationship-graph entity in production M2, distinct from
/// `Project.goal_id`'s single direct foreign key. Field set is exactly
/// the frozen model: `id`, `from_goal_id`, `to_goal_id`, `relation_type`,
/// `note` (nullable), `created_at`. Deliberately **excludes**
/// `status`/`updated_at`/`confidence`/`weight`/an AI score/an "inferred"
/// flag/`priority`/any other hidden metadata - none of those are in the
/// frozen model, and provenance belongs to `ActionMeta`/the audit log,
/// not this entity itself. A row is a directional, stored fact: one
/// `LinkGoals` call produces exactly one row, never an automatic
/// inverse (`GoalRelationType::ParentOf`'s doc comment, above).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct GoalRelation {
    pub id: GoalRelationId,
    pub from_goal_id: GoalId,
    pub to_goal_id: GoalId,
    pub relation_type: GoalRelationType,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A stable, globally unique Milestone identifier - same UUID v7 design
/// as `GoalId`/`ProjectId`/`GoalRelationId` (M2 Milestone vertical
/// slice, 2026-09-17). Its own newtype for the same type-safety reason
/// every other entity ID is its own type.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct MilestoneId(pub Uuid);

impl MilestoneId {
    pub(crate) fn new() -> Self {
        MilestoneId(Uuid::now_v7())
    }

    pub(crate) fn parse(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(MilestoneId)
    }
}

impl std::fmt::Display for MilestoneId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// `Milestone`'s frozen 2-value status enum (architecture-freeze report
/// §C / `LIFEOS_SPEC.md` §5.1). **Not** `LifecycleStatus` - a genuinely
/// different value set (`PENDING`/`DONE`, not the four Goal/Project
/// values), so reusing `LifecycleStatus` here would be the same mistake
/// the `GoalStatus` rename was created to fix, in reverse (forcing two
/// genuinely different value sets to share one type, rather than one
/// genuinely shared value set needlessly duplicated into two types).
/// No terminal-state rule: `PENDING`/`DONE` are freely reversible peer
/// states (M2 Project Child Action Freeze report §5 / Milestone
/// vertical-slice report §13) - do not reintroduce one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum MilestoneStatus {
    Pending,
    Done,
}

impl MilestoneStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            MilestoneStatus::Pending => "PENDING",
            MilestoneStatus::Done => "DONE",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "PENDING" => Some(MilestoneStatus::Pending),
            "DONE" => Some(MilestoneStatus::Done),
            _ => None,
        }
    }
}

/// A `Milestone`'s deterministic position among its `Project`'s other
/// milestones. Plain `i64` - no bounded/scaled type like `Progress`/
/// `Priority`, since no canonical numeric policy for it exists beyond
/// non-negativity (a position cannot be negative) - see
/// `ORDER_INDEX_MIN`/the Milestone vertical-slice report §3 for why no
/// upper bound is invented.
pub(crate) type OrderIndex = i64;
pub(crate) const ORDER_INDEX_MIN: OrderIndex = 0;

/// `Milestone` (M2 Milestone vertical slice, 2026-09-17) - the fourth
/// real M2 domain entity, a required child of `Project`
/// (`ON DELETE RESTRICT` - see `migrations/V4__create_milestones.sql`).
/// Field set is EXACTLY the frozen architecture-freeze definition (M2
/// report §C): `id`, `project_id`, `title`, `target_date` (nullable),
/// `status`, `order_index`. Deliberately **excludes** `description`/
/// `progress`/`priority`/`weight`/`blockers`/an AI score/hidden
/// metadata - none are in the frozen model. **Deliberately excludes
/// `created_at`/`updated_at` too** - unlike every other entity so far,
/// the frozen Milestone field table genuinely lists no timestamps at
/// all (contrast `Task`'s own frozen row, which explicitly lists
/// `created_at`/`updated_at`); adding them here would be exactly the
/// kind of silent, undocumented field addition this codebase has
/// repeatedly avoided (`Project.progress`'s original open-question
/// status, `GoalRelation`'s explicit exclusion list). If a real need
/// for a Milestone timestamp ever emerges, it must be frozen in
/// `LIFEOS_SPEC.md` first, not added here silently - see the Milestone
/// vertical-slice report §4 for the full reasoning.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Milestone {
    pub id: MilestoneId,
    pub project_id: ProjectId,
    pub title: String,
    pub target_date: Option<DateTime<Utc>>,
    pub status: MilestoneStatus,
    pub order_index: OrderIndex,
}

/// A stable, globally unique Task identifier - same UUID v7 design as
/// every other entity ID (M2 Task vertical slice, 2026-09-17). Its own
/// newtype for the same type-safety reason every other entity ID is its
/// own type.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct TaskId(pub Uuid);

impl TaskId {
    pub(crate) fn new() -> Self {
        TaskId(Uuid::now_v7())
    }

    pub(crate) fn parse(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(TaskId)
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A stable, globally unique identifier reserved for the future,
/// **not-yet-implemented** `RecurringRule` entity (M2 Task vertical
/// slice, 2026-09-17). Defined now, alongside `Task.recurring_rule_id`
/// below, purely as a typed placeholder so the frozen field can exist
/// without a stringly-typed or `Uuid`-raw stand-in - `RecurringRule`
/// itself (the table, the rrule/RFC5545 machinery, recurrence
/// expansion, Calendar/Reminder) is deliberately **not built this
/// run** - see the Task vertical-slice report §3 for the full
/// reasoning. Nothing in this codebase constructs a real
/// `RecurringRuleId` yet; `AddTask`/`UpdateTask` never accept one
/// (`Task.recurring_rule_id` is always `None`, enforced additionally by
/// a database `CHECK (recurring_rule_id IS NULL)` -
/// `migrations/V5__create_tasks.sql`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct RecurringRuleId(pub Uuid);

impl RecurringRuleId {
    pub(crate) fn new() -> Self {
        RecurringRuleId(Uuid::now_v7())
    }

    pub(crate) fn parse(s: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(s).map(RecurringRuleId)
    }
}

impl std::fmt::Display for RecurringRuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// `Task`'s frozen, closed 4-value status enum (architecture-freeze
/// report §C / `LIFEOS_SPEC.md` §5.1). No terminal-state rule: all four
/// are freely reversible peer states (M2 Project Child Action Freeze
/// report §5 / Task vertical-slice report §7) - do not reintroduce one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum TaskStatus {
    Todo,
    InProgress,
    Done,
    Cancelled,
}

impl TaskStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Todo => "TODO",
            TaskStatus::InProgress => "IN_PROGRESS",
            TaskStatus::Done => "DONE",
            TaskStatus::Cancelled => "CANCELLED",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s {
            "TODO" => Some(TaskStatus::Todo),
            "IN_PROGRESS" => Some(TaskStatus::InProgress),
            "DONE" => Some(TaskStatus::Done),
            "CANCELLED" => Some(TaskStatus::Cancelled),
            _ => None,
        }
    }
}

/// `Task` (M2 Task vertical slice, 2026-09-17) - the fifth real M2
/// domain entity, and the first entity with **two independent,
/// both-nullable** parent references: `project_id`/`goal_id`. A Task
/// may be standalone (both `None`), Project-only, Goal-only, or linked
/// to both at once - all four configurations are valid, neither parent
/// is required, and neither is ever inferred from the other (Task
/// vertical-slice report §3 - a Task's `Project` being linked to some
/// `Goal` does NOT imply anything about the Task's own `goal_id`).
/// Field set is EXACTLY the frozen architecture-freeze definition:
/// `id`, `title`, `status`, `due_at` (nullable), `project_id`
/// (nullable), `goal_id` (nullable), `recurring_rule_id` (nullable,
/// reserved for the future - see `RecurringRuleId`'s doc comment),
/// `created_at`/`updated_at`. Deliberately **excludes**
/// `description`/`priority`/`milestone_id`/`progress`/
/// `parent_task_id`/`tags`/a completion percentage/AI metadata/any
/// other hidden workflow field - none are in the frozen model, and
/// `milestone_id` specifically is NOT inferred just because `Milestone`
/// and `Task` are both `Project` children (Task vertical-slice report
/// §24 - if a future product requirement needs tasks attached directly
/// to milestones, that must be frozen separately, not added here
/// silently).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Task {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub due_at: Option<DateTime<Utc>>,
    pub project_id: Option<ProjectId>,
    pub goal_id: Option<GoalId>,
    pub recurring_rule_id: Option<RecurringRuleId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests;
