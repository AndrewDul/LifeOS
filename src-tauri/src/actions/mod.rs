//! LifeOS action layer (M2 Goal vertical slice, 2026-09-17).
//!
//! Typed action payloads, deterministic validation, and confirmation
//! classification - pure logic, no I/O, no SQL. Matches
//! `LIFEOS_SPEC.md` §5.4/§5.5 and the architecture-freeze report §E/§F
//! (as corrected by the storage-spike report's errata: `ForgetMemory`
//! is not implemented here, but the correction is honored by not
//! treating `DELETE` as uniformly soft-delete anywhere in this module).
//!
//! Validation never happens in SQL and never involves an LLM - matches
//! `LIFEOS_SPEC.md` §4.1.

// Not wired into `run()` yet this run - only exercised by
// services::tests, hence dead_code would otherwise warn (unused Actor/
// Provenance variants this slice never triggers - CloudLLM, NeXa,
// Inferred - plus confirmation classes this slice's tests don't reach).
#![allow(dead_code)]

use crate::core_error::CoreError;
use crate::domain::{
    Goal, GoalId, GoalRelationType, LifecycleStatus, Priority, Progress, ProjectId,
    DESCRIPTION_MAX_LEN, DOMAIN_TAG_MAX_LEN, PRIORITY_MAX, PRIORITY_MIN, PROGRESS_MAX,
    PROGRESS_MIN, TITLE_MAX_LEN,
};
use chrono::{DateTime, Utc};

/// A tri-state patch for an optional `Goal` field, replacing an
/// ambiguous nested `Option<Option<T>>` (M2 Goal hardening, 2026-09-17).
/// `Some(x)`/`None` on a plain `Option<T>` update field cannot express
/// "explicitly clear this field back to NULL" separately from "leave it
/// alone" - this can, and does so as an explicit named state rather than
/// two nested `Option`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldPatch<T> {
    /// Do not touch this field - no database modification.
    Unchanged,
    /// Write this value.
    Set(T),
    /// Explicitly clear this field to SQL NULL.
    Clear,
}

impl<T> Default for FieldPatch<T> {
    fn default() -> Self {
        FieldPatch::Unchanged
    }
}

impl<T> FieldPatch<T> {
    fn is_unchanged(&self) -> bool {
        matches!(self, FieldPatch::Unchanged)
    }
}

/// Who requested an action. `LIFEOS_SPEC.md` §5.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Actor {
    Ui,
    CloudLlm,
    NeXa,
}

/// Whether the actor directly and unambiguously requested this specific
/// change, or is proposing it without a literal instruction (e.g. an AI
/// inference). Drives the confirmation-class provenance rule -
/// orthogonal to *which* actor it is: a `CloudLlm` actor relaying an
/// explicit user instruction is `Explicit`; a `CloudLlm` actor proposing
/// something on its own is `Inferred`. `LIFEOS_SPEC.md` §5.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Provenance {
    Explicit,
    Inferred,
}

/// Common envelope every action carries. `LIFEOS_SPEC.md` §5.4.
/// `confirmed` is how a caller who already resolved a prior
/// `ActionOutcome::ConfirmationRequired` resubmits - there is no session/
/// UI state kept here, the caller owns that.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ActionMeta {
    pub actor: Actor,
    pub provenance: Provenance,
    pub requested_at: DateTime<Utc>,
    pub confirmed: bool,
}

impl ActionMeta {
    pub(crate) fn now(actor: Actor, provenance: Provenance) -> Self {
        ActionMeta {
            actor,
            provenance,
            requested_at: Utc::now(),
            confirmed: false,
        }
    }
}

/// `LIFEOS_SPEC.md` §5.5 - six frozen classes. `Sensitive` is an
/// orthogonal overlay (not part of the severity ladder below) - not
/// exercised by Goal fields in this slice (no Goal field is Privacy-
/// Firewall-tagged sensitive), so it is represented but unused here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmationClass {
    SafeRead,
    ExplicitAdd,
    InferredChange,
    ImportantChange,
    Delete,
    Sensitive,
}

impl ConfirmationClass {
    fn severity(self) -> u8 {
        match self {
            ConfirmationClass::SafeRead => 0,
            ConfirmationClass::ExplicitAdd => 1,
            ConfirmationClass::InferredChange => 2,
            ConfirmationClass::ImportantChange => 3,
            ConfirmationClass::Delete => 4,
            ConfirmationClass::Sensitive => {
                unreachable!("Sensitive is an overlay flag, not a severity rung")
            }
        }
    }

    /// Only `InferredChange` (and, not implemented this slice, `Delete`)
    /// blocks persistence pending confirmation. `ImportantChange`
    /// executes immediately but is always audit-logged and Undo-able;
    /// `ExplicitAdd` executes immediately. `LIFEOS_SPEC.md` §5.5.
    fn blocks_until_confirmed(self) -> bool {
        matches!(
            self,
            ConfirmationClass::InferredChange | ConfirmationClass::Delete
        )
    }
}

/// `effective_class = max(type_floor(action), provenance_class)` -
/// `LIFEOS_SPEC.md` §5.5.
pub(crate) fn effective_class(
    type_floor: ConfirmationClass,
    provenance: Provenance,
) -> ConfirmationClass {
    let provenance_class = match provenance {
        Provenance::Explicit => ConfirmationClass::ExplicitAdd,
        Provenance::Inferred => ConfirmationClass::InferredChange,
    };
    if type_floor.severity() >= provenance_class.severity() {
        type_floor
    } else {
        provenance_class
    }
}

/// Generic across entities - not Goal-specific despite the historical
/// name of its call sites (renamed from `goal_blocks_until_confirmed`,
/// M2 Project vertical slice, 2026-09-17, so `services::create_project`/
/// `update_progress` can reuse it instead of duplicating an identical
/// function under a `project_` prefix).
pub(crate) fn blocks_until_confirmed(class: ConfirmationClass) -> bool {
    class.blocks_until_confirmed()
}

/// Never a bare string. `LIFEOS_SPEC.md` §5.4.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ValidationError {
    EmptyTitle,
    TitleTooLong {
        max: usize,
        actual: usize,
    },
    DescriptionTooLong {
        max: usize,
        actual: usize,
    },
    DomainTagTooLong {
        max: usize,
        actual: usize,
    },
    ProgressOutOfBounds {
        min: Progress,
        max: Progress,
        actual: i64,
    },
    PriorityOutOfBounds {
        min: Priority,
        max: Priority,
        actual: i64,
    },
    NoFieldsToUpdate,
    /// A referenced entity does not exist - e.g. `CreateProject`'s
    /// `goal_id` pointing at a nonexistent `Goal`. This check needs a
    /// database read, which this module never performs itself (pure
    /// logic, no I/O) - `services::create_project` performs the read
    /// and reports the result through this same deterministic
    /// `ValidationFailed` channel as every other structural check,
    /// rather than relying solely on the SQL foreign-key constraint
    /// failing (M2 Project vertical slice, 2026-09-17).
    ReferencedGoalNotFound,
    /// Free-text length bound reusing the same `DESCRIPTION_MAX_LEN`
    /// policy Goal/Project descriptions already use - not a new,
    /// invented taxonomy, just the existing generic bound applied to
    /// `LinkGoals`'s `note` field (M2 GoalRelation vertical slice,
    /// 2026-09-17). Named `NoteTooLong` rather than reusing
    /// `DescriptionTooLong` so a caller matching on this error isn't
    /// told about a "description" that does not exist on `GoalRelation`.
    NoteTooLong {
        max: usize,
        actual: usize,
    },
    /// `LinkGoals`'s `from_goal_id == to_goal_id` - a Goal cannot be
    /// linked to itself in `GoalRelation` v0.1 (deterministic structural
    /// rule, checked here with no database read needed).
    SelfGoalRelation,
    /// `LinkGoals`'s `from_goal_id` does not reference an existing
    /// `Goal` - needs a database read, so `services::link_goals`
    /// performs the check and reports it here.
    RelationFromGoalNotFound,
    /// `LinkGoals`'s `to_goal_id` does not reference an existing `Goal` -
    /// same reasoning as `RelationFromGoalNotFound`.
    RelationToGoalNotFound,
    /// The exact `(from_goal_id, to_goal_id, relation_type)` triple
    /// `LinkGoals` was asked to create already exists - needs a database
    /// read, checked before persistence rather than left to the SQL
    /// `UNIQUE` constraint to catch (M2 GoalRelation vertical slice,
    /// 2026-09-17). A different `relation_type` for the same pair, or
    /// the same type in the reverse direction, is NOT a duplicate.
    DuplicateGoalRelation,
}

#[derive(Debug, Clone)]
pub(crate) struct CreateGoalPayload {
    pub title: String,
    pub description: Option<String>,
    pub target_date: Option<DateTime<Utc>>,
    pub priority_user: Option<Priority>,
    pub domain_tag: Option<String>,
}

/// `title`/`status`/`progress` are non-nullable `Goal` fields, so a
/// plain `Option` still works for them: `Some(x)` sets the field, `None`
/// leaves it unchanged. `description`/`target_date`/`priority_user`/
/// `domain_tag` are nullable `Goal` fields and use `FieldPatch` (M2 Goal
/// hardening, 2026-09-17) so "leave unchanged" and "explicitly clear to
/// NULL" are distinct, unambiguous states.
#[derive(Debug, Clone, Default)]
pub(crate) struct UpdateGoalPayload {
    pub id: GoalId,
    pub title: Option<String>,
    pub description: FieldPatch<String>,
    pub status: Option<LifecycleStatus>,
    pub target_date: FieldPatch<DateTime<Utc>>,
    pub priority_user: FieldPatch<Priority>,
    pub domain_tag: FieldPatch<String>,
    pub progress: Option<Progress>,
}

fn validate_title(title: &str) -> Result<String, ValidationError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(ValidationError::EmptyTitle);
    }
    if trimmed.chars().count() > TITLE_MAX_LEN {
        return Err(ValidationError::TitleTooLong {
            max: TITLE_MAX_LEN,
            actual: trimmed.chars().count(),
        });
    }
    Ok(trimmed.to_string())
}

fn validate_description(description: &str) -> Result<(), ValidationError> {
    if description.chars().count() > DESCRIPTION_MAX_LEN {
        return Err(ValidationError::DescriptionTooLong {
            max: DESCRIPTION_MAX_LEN,
            actual: description.chars().count(),
        });
    }
    Ok(())
}

fn validate_domain_tag(tag: &str) -> Result<(), ValidationError> {
    if tag.chars().count() > DOMAIN_TAG_MAX_LEN {
        return Err(ValidationError::DomainTagTooLong {
            max: DOMAIN_TAG_MAX_LEN,
            actual: tag.chars().count(),
        });
    }
    Ok(())
}

fn validate_priority(priority: Priority) -> Result<(), ValidationError> {
    if priority < PRIORITY_MIN || priority > PRIORITY_MAX {
        return Err(ValidationError::PriorityOutOfBounds {
            min: PRIORITY_MIN,
            max: PRIORITY_MAX,
            actual: priority as i64,
        });
    }
    Ok(())
}

fn validate_progress(progress: Progress) -> Result<(), ValidationError> {
    if progress < PROGRESS_MIN || progress > PROGRESS_MAX {
        return Err(ValidationError::ProgressOutOfBounds {
            min: PROGRESS_MIN,
            max: PROGRESS_MAX,
            actual: progress as i64,
        });
    }
    Ok(())
}

/// Returns the normalized (trimmed) title on success, alongside all
/// validation errors found (not just the first) so a caller can report
/// everything wrong at once.
pub(crate) fn validate_create_goal(
    payload: &CreateGoalPayload,
) -> Result<String, Vec<ValidationError>> {
    let mut errors = Vec::new();
    let mut normalized_title = None;

    match validate_title(&payload.title) {
        Ok(t) => normalized_title = Some(t),
        Err(e) => errors.push(e),
    }
    if let Some(d) = &payload.description {
        if let Err(e) = validate_description(d) {
            errors.push(e);
        }
    }
    if let Some(tag) = &payload.domain_tag {
        if let Err(e) = validate_domain_tag(tag) {
            errors.push(e);
        }
    }
    if let Some(p) = payload.priority_user {
        if let Err(e) = validate_priority(p) {
            errors.push(e);
        }
    }

    if errors.is_empty() {
        Ok(normalized_title.expect("title validated Ok implies normalized_title is Some"))
    } else {
        Err(errors)
    }
}

/// Validates an update against the *existing* goal - `existing` is
/// taken so a future field could validate against prior state if ever
/// needed; no status-transition restriction is applied (M2 Goal
/// hardening, 2026-09-17 - see `domain::LifecycleStatus`'s doc comment: that
/// rule was an unapproved invention of the original vertical slice and
/// has been removed). Returns nothing on success - the caller applies
/// `payload`'s `Some`/`FieldPatch::Set`/`FieldPatch::Clear` onto
/// `existing`.
pub(crate) fn validate_update_goal(
    existing: &Goal,
    payload: &UpdateGoalPayload,
) -> Result<(), Vec<ValidationError>> {
    let _ = existing; // reserved for future validation that needs prior state
    let mut errors = Vec::new();

    let touches_any_field = payload.title.is_some()
        || !payload.description.is_unchanged()
        || payload.status.is_some()
        || !payload.target_date.is_unchanged()
        || !payload.priority_user.is_unchanged()
        || !payload.domain_tag.is_unchanged()
        || payload.progress.is_some();
    if !touches_any_field {
        errors.push(ValidationError::NoFieldsToUpdate);
    }

    if let Some(title) = &payload.title {
        if let Err(e) = validate_title(title) {
            errors.push(e);
        }
    }
    if let FieldPatch::Set(d) = &payload.description {
        if let Err(e) = validate_description(d) {
            errors.push(e);
        }
    }
    if let FieldPatch::Set(tag) = &payload.domain_tag {
        if let Err(e) = validate_domain_tag(tag) {
            errors.push(e);
        }
    }
    if let FieldPatch::Set(p) = &payload.priority_user {
        if let Err(e) = validate_priority(*p) {
            errors.push(e);
        }
    }
    if let Some(progress) = payload.progress {
        if let Err(e) = validate_progress(progress) {
            errors.push(e);
        }
    }
    // No status-transition check: every LifecycleStatus -> LifecycleStatus change
    // an explicit action requests is allowed and audited, per the
    // hardening correction above.

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// --- Project (M2 Project vertical slice, 2026-09-17) --------------------
//
// The frozen M2 action set for Project (architecture-freeze report §C)
// is CreateProject, AddMilestone, AddTask, UpdateProgress - there is no
// frozen UpdateProject. Only CreateProject and UpdateProgress are
// implemented this slice; AddMilestone/AddTask need Milestone/Task,
// which are explicitly out of scope.

/// `CreateProject` payload. `goal_id`, if present, must reference an
/// existing `Goal` - that check needs a database read, so it cannot
/// happen in this pure module; `services::create_project` performs it
/// and reports failure via `ValidationError::ReferencedGoalNotFound`.
#[derive(Debug, Clone)]
pub(crate) struct CreateProjectPayload {
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<Priority>,
    pub goal_id: Option<GoalId>,
}

/// `UpdateProgress` payload - deliberately narrow (one field, one
/// purpose), matching the frozen action's scope exactly. There is no
/// tri-state `FieldPatch` need here: `progress` is Project's one
/// non-nullable mutable field this action touches.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UpdateProgressPayload {
    pub id: ProjectId,
    pub progress: Progress,
}

/// Returns the normalized (trimmed) title on success, mirroring
/// `validate_create_goal`. Deliberately does **not** check `goal_id`
/// existence - pure logic, no I/O; see `CreateProjectPayload`'s doc.
pub(crate) fn validate_create_project(
    payload: &CreateProjectPayload,
) -> Result<String, Vec<ValidationError>> {
    let mut errors = Vec::new();
    let mut normalized_title = None;

    match validate_title(&payload.title) {
        Ok(t) => normalized_title = Some(t),
        Err(e) => errors.push(e),
    }
    if let Some(d) = &payload.description {
        if let Err(e) = validate_description(d) {
            errors.push(e);
        }
    }
    if let Some(p) = payload.priority {
        if let Err(e) = validate_priority(p) {
            errors.push(e);
        }
    }

    if errors.is_empty() {
        Ok(normalized_title.expect("title validated Ok implies normalized_title is Some"))
    } else {
        Err(errors)
    }
}

/// Progress-bounds check only - the frozen `UpdateProgress` action has
/// nothing else to validate structurally. Project existence is checked
/// at the `services` layer, the same way `update_goal` checks `Goal`
/// existence (needs a database read).
pub(crate) fn validate_update_progress(
    payload: &UpdateProgressPayload,
) -> Result<(), Vec<ValidationError>> {
    match validate_progress(payload.progress) {
        Ok(()) => Ok(()),
        Err(e) => Err(vec![e]),
    }
}

// --- GoalRelation (M2 GoalRelation vertical slice, 2026-09-17) ----------
//
// The frozen M2 action for this graph is LinkGoals only - do not invent
// a second CreateGoalRelation action alongside it.

fn validate_note(note: &str) -> Result<(), ValidationError> {
    if note.chars().count() > DESCRIPTION_MAX_LEN {
        return Err(ValidationError::NoteTooLong {
            max: DESCRIPTION_MAX_LEN,
            actual: note.chars().count(),
        });
    }
    Ok(())
}

/// `LinkGoals` payload. `from_goal_id`/`to_goal_id` existence and the
/// exact-duplicate check all need a database read, so they cannot
/// happen in this pure module - `services::link_goals` performs them.
#[derive(Debug, Clone)]
pub(crate) struct LinkGoalsPayload {
    pub from_goal_id: GoalId,
    pub to_goal_id: GoalId,
    pub relation_type: GoalRelationType,
    pub note: Option<String>,
}

/// Pure structural validation only: `from_goal_id != to_goal_id`.
/// `relation_type` is already a typed `GoalRelationType` - an invalid
/// enum value cannot be constructed through normal Rust action
/// construction, so there is nothing to validate there. `note`, if
/// present, reuses the same `DESCRIPTION_MAX_LEN` bound Goal/Project
/// descriptions already use (`validate_note`, above) - not a new,
/// invented taxonomy.
pub(crate) fn validate_link_goals(payload: &LinkGoalsPayload) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    if payload.from_goal_id == payload.to_goal_id {
        errors.push(ValidationError::SelfGoalRelation);
    }
    if let Some(note) = &payload.note {
        if let Err(e) = validate_note(note) {
            errors.push(e);
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// `LIFEOS_SPEC.md` §5.4 - never a persisted result, a bare string
/// error, and a "confirmation required" response all mixed together;
/// each is its own variant. `services` always writes an audit-log entry
/// for a persisted Create/UpdateGoal in this slice, regardless of class
/// - the frozen policy only *requires* it at `ImportantChange` and
/// above; logging more than required is a strict superset, not a
/// deviation.
#[derive(Debug)]
pub(crate) enum ActionOutcome<T> {
    Success {
        value: T,
        confirmation_class: ConfirmationClass,
    },
    ConfirmationRequired {
        confirmation_class: ConfirmationClass,
    },
    ValidationFailed(Vec<ValidationError>),
    NotFound,
    /// An unexpected storage-layer failure (e.g. disk I/O, a failed
    /// transaction commit) - distinct from `ValidationFailed`/
    /// `NotFound`, which are expected outcomes of otherwise-correct
    /// calls. Typed (M2 Goal hardening, 2026-09-17) - never a bare
    /// database-driver string; see `crate::core_error`.
    StorageError(CoreError),
}

#[cfg(test)]
mod tests;
