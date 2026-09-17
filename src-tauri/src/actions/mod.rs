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

use crate::domain::{
    Goal, GoalId, GoalStatus, Priority, Progress, DESCRIPTION_MAX_LEN, DOMAIN_TAG_MAX_LEN,
    PRIORITY_MAX, PRIORITY_MIN, PROGRESS_MAX, PROGRESS_MIN, TITLE_MAX_LEN,
};
use chrono::{DateTime, Utc};

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

pub(crate) fn goal_blocks_until_confirmed(class: ConfirmationClass) -> bool {
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
    InvalidStatusTransition {
        from: GoalStatus,
        to: GoalStatus,
    },
    NoFieldsToUpdate,
}

#[derive(Debug, Clone)]
pub(crate) struct CreateGoalPayload {
    pub title: String,
    pub description: Option<String>,
    pub target_date: Option<DateTime<Utc>>,
    pub priority_user: Option<Priority>,
    pub domain_tag: Option<String>,
}

/// `Some(x)` sets the field to `x`; `None` leaves it unchanged. This
/// slice does not support explicitly clearing an optional field back to
/// NULL (a deliberate, documented simplification - see the report).
#[derive(Debug, Clone, Default)]
pub(crate) struct UpdateGoalPayload {
    pub id: GoalId,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<GoalStatus>,
    pub target_date: Option<DateTime<Utc>>,
    pub priority_user: Option<Priority>,
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

/// Validates an update against the *existing* goal (needed for status-
/// transition and no-op checks). Returns nothing on success - the caller
/// applies `payload`'s `Some` fields onto `existing`.
pub(crate) fn validate_update_goal(
    existing: &Goal,
    payload: &UpdateGoalPayload,
) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    let touches_any_field = payload.title.is_some()
        || payload.description.is_some()
        || payload.status.is_some()
        || payload.target_date.is_some()
        || payload.priority_user.is_some()
        || payload.progress.is_some();
    if !touches_any_field {
        errors.push(ValidationError::NoFieldsToUpdate);
    }

    if let Some(title) = &payload.title {
        if let Err(e) = validate_title(title) {
            errors.push(e);
        }
    }
    if let Some(d) = &payload.description {
        if let Err(e) = validate_description(d) {
            errors.push(e);
        }
    }
    if let Some(p) = payload.priority_user {
        if let Err(e) = validate_priority(p) {
            errors.push(e);
        }
    }
    if let Some(progress) = payload.progress {
        if let Err(e) = validate_progress(progress) {
            errors.push(e);
        }
    }
    if let Some(status) = payload.status {
        if !existing.status.can_transition_to(status) {
            errors.push(ValidationError::InvalidStatusTransition {
                from: existing.status,
                to: status,
            });
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
    /// An unexpected storage-layer failure (e.g. disk I/O) - distinct
    /// from `ValidationFailed`/`NotFound`, which are expected outcomes
    /// of otherwise-correct calls.
    StorageError(String),
}
