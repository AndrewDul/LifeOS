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
    validate_personal_knowledge_text_len, DomainRef, EntityId, EpistemicState, FactId, Goal,
    GoalId, GoalRelationType, LifecycleStatus, MemoryType, Milestone, MilestoneId, MilestoneStatus,
    OrderIndex, Priority, Progress, Project, ProjectId, Sensitivity, SourceId, Task, TaskId,
    TaskStatus, CANONICAL_NAME_MAX_LEN, DESCRIPTION_MAX_LEN, DOMAIN_REF_ID_MAX_LEN,
    DOMAIN_REF_TYPE_MAX_LEN, DOMAIN_TAG_MAX_LEN, ENTITY_TYPE_MAX_LEN, EVIDENCE_CONTENT_MAX_LEN,
    EVIDENCE_LOCATOR_MAX_LEN, ORDER_INDEX_MIN, PREDICATE_MAX_LEN, PRIORITY_MAX, PRIORITY_MIN,
    PROGRESS_MAX, PROGRESS_MIN, SOURCE_LABEL_MAX_LEN, SOURCE_REFERENCE_MAX_LEN,
    SOURCE_TYPE_MAX_LEN, TITLE_MAX_LEN, VALUE_JSON_MAX_LEN,
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
    /// `AddMilestone`'s `project_id` does not reference an existing
    /// `Project` - needs a database read, so `services::add_milestone`
    /// performs the check and reports it here, the same relationship
    /// `create_project`'s `ReferencedGoalNotFound` has with the FK
    /// constraint on `projects.goal_id` (M2 Milestone vertical slice,
    /// 2026-09-17).
    ReferencedProjectNotFound,
    /// A `Milestone`'s `order_index` is negative - the one canonical
    /// numeric bound `OrderIndex` has (`domain::ORDER_INDEX_MIN`); no
    /// upper bound is invented (M2 Milestone vertical-slice report §3).
    OrderIndexOutOfBounds {
        min: OrderIndex,
        actual: OrderIndex,
    },
    /// `AddMilestone`/`UpdateMilestone` requested an `order_index`
    /// already occupied by another `Milestone` within the same
    /// `Project` - needs a database read, checked before persistence
    /// rather than left to the SQL `UNIQUE(project_id, order_index)`
    /// constraint to catch first (M2 Milestone vertical-slice report
    /// §9, resolving the M2 Project Child Action Freeze report's open
    /// question). A different Project reusing the same `order_index` is
    /// NOT a duplicate; a `Milestone` being updated keeping its own
    /// current `order_index` is NOT a duplicate either.
    DuplicateMilestoneOrderIndex,
    /// `AddMemory`'s `EntityInputRef::Existing(id)` does not reference an
    /// existing `Entity` - needs a database read, so
    /// `services::add_memory` performs it and reports it here (M2
    /// Personal Knowledge Slice B, 2026-09-18). An Existing reference is
    /// never mutated on reuse regardless of the rest of the payload.
    ReferencedEntityNotFound,
    /// `AddMemory`'s `SourceRef::Existing(id)` does not reference an
    /// existing `Source` - same reasoning as `ReferencedEntityNotFound`.
    ReferencedSourceNotFound,
    /// Two `New` Entity candidates in one `AddMemory` payload (subject
    /// and/or object) share the same `domain_ref`, or a `New` Entity
    /// candidate's `domain_ref` already belongs to a persisted `Entity` -
    /// checked explicitly (same-payload comparison, then a
    /// `find_entity_by_domain_ref` read) rather than left to the real
    /// `idx_entities_domain_ref` UNIQUE index to catch first, which
    /// remains defense-in-depth only (R018 §9/R019 §9).
    DuplicateDomainReference,
    /// A `NewEntityData.domain_ref.domain_ref_type` is not one of the
    /// domain types Core can currently authoritatively validate
    /// (`Goal`/`Project`/`Milestone`/`Task` - see R021 §7). This does
    /// **not** close the open `domain_ref_type` vocabulary at the schema
    /// level, it only limits what `AddMemory` can validate today.
    UnsupportedDomainReferenceType,
    /// A `NewEntityData.domain_ref` names a supported `domain_ref_type`
    /// but the `domain_ref_id` is either not a well-formed id for that
    /// type or does not reference an existing row - needs a database
    /// read (or a parse of a supported id format), so
    /// `services::add_memory` performs it and reports it here. Both the
    /// syntactically-invalid and the well-formed-but-nonexistent cases
    /// use this same variant (both are "cannot be validated as an
    /// existing X"; see R021 for the deliberate non-split).
    ReferencedDomainObjectNotFound,
    /// A required Personal Knowledge free-text field (`entity_type`/
    /// `canonical_name`/`domain_ref_type`/`domain_ref_id`/`predicate`/
    /// `source_type`) was empty after trimming. One generic
    /// field-parameterized variant rather than six near-identical
    /// `Empty*` variants, matching the same field-parameterized design
    /// as `PersonalKnowledgeFieldTooLong` below (R021 - deliberately
    /// smaller than the per-field taxonomy `EmptyTitle`/`NoteTooLong`
    /// established, given the number of Personal Knowledge free-text
    /// fields involved).
    PersonalKnowledgeFieldEmpty {
        field: &'static str,
    },
    /// A Personal Knowledge free-text field exceeded its frozen max
    /// length (`domain::validate_personal_knowledge_text_len`, R018
    /// §15). One generic field-parameterized variant covering all
    /// eleven bounded fields (`entity_type`/`canonical_name`/
    /// `domain_ref_type`/`domain_ref_id`/`predicate`/`value_json`/
    /// `source_type`/`label`/`reference`/`content`/`locator`) rather
    /// than eleven separate `*TooLong` variants - `field` names exactly
    /// which one failed, so nothing is lost versus the per-field style
    /// (R021).
    PersonalKnowledgeFieldTooLong {
        field: &'static str,
        max: usize,
        actual: usize,
    },
    /// `AddMemory`'s `FactValueInput::ScalarJson` is not syntactically
    /// valid JSON (`serde_json::from_str` failed). The JSON is never
    /// executed or evaluated, only checked for well-formedness -
    /// `value_json` accepts any JSON type (string/number/bool/null/
    /// object/array), not "scalar-only" despite the field's name (R021
    /// §10).
    InvalidValueJson,
    /// `AddMemory`'s `confidence` was `Some(c)` where `c` is not finite
    /// (`NaN`/`+Infinity`/`-Infinity`) or falls outside `[0.0, 1.0]`.
    /// `None` (no confidence supplied) is always valid and never reaches
    /// this check (R021 §11).
    ConfidenceOutOfBounds {
        min: f32,
        max: f32,
        actual: f32,
    },
    /// `AddMemory`'s `NewEvidenceData` supplied neither `content` nor
    /// `locator` - the DB `CHECK ((content IS NOT NULL) OR (locator IS
    /// NOT NULL))` on `evidence` (`V7__create_personal_knowledge.sql`)
    /// mirrored here so the failure is a typed `ValidationError` with
    /// zero writes, not a raw SQLite constraint failure (R021 §12).
    EvidenceRequiresContentOrLocator,
    /// `CorrectMemory`'s `predecessor_fact_id` resolves to a real `Fact`
    /// row, but `domain::is_active_fact` returns `false` for it (already
    /// closed - `valid_until` set - or already `Stale`) - needs a
    /// database read, so `services::correct_memory` performs it and
    /// reports it here (M2 Personal Knowledge Slice C, 2026-09-18).
    /// `CorrectMemory` never revives a historical Fact. Reused unchanged
    /// by `ConfirmMemory` (Slice D) for the exact same target-inactive
    /// case.
    FactNotActive,
    /// `ConfirmMemory`'s target `Fact` is active but already
    /// `epistemic_state = USER_CONFIRMED` - confirming it again is a
    /// no-op, rejected explicitly rather than silently returning
    /// `Success` with zero writes (M2 Personal Knowledge Slice D,
    /// 2026-09-18). Needs a database read, so `services::confirm_memory`
    /// performs it and reports it here.
    FactAlreadyUserConfirmed,
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

// --- Project (M2 Project vertical slice, 2026-09-17; UpdateProject
// added 2026-09-17, M2 UpdateProject implementation) --------------------
//
// The frozen M2 action set for Project (architecture-freeze report §C,
// as extended by the M2 Project Child Action Freeze report) is
// CreateProject, UpdateProject, AddMilestone, AddTask, UpdateProgress.
// All are now implemented except AddMilestone/AddTask, which live under
// their own entities (`domain::Milestone`/`domain::Task`), not here.

/// `UpdateProject` payload (M2 UpdateProject implementation, 2026-09-17)
/// - the frozen shape from the M2 Project Child Action Freeze report.
/// `title`/`status` are non-nullable `Project` fields, so a plain
/// `Option` is correct for them (`Some(x)` sets, `None` leaves
/// unchanged). `description`/`priority`/`goal_id` are all genuinely
/// nullable `Project` fields and use `FieldPatch` so "leave unchanged"
/// and "explicitly clear to NULL" stay distinct. **`progress` is
/// deliberately absent** - `UpdateProgress` remains its exclusive
/// owner (§2 of the UpdateProject brief); do not add it here, do not
/// merge the two actions, and do not derive progress from anything.
/// `goal_id`'s existence, when `Set`, needs a database read, so it
/// cannot happen in this pure module - `services::update_project`
/// performs it, the same pattern `create_project` already uses.
#[derive(Debug, Clone, Default)]
pub(crate) struct UpdateProjectPayload {
    pub id: ProjectId,
    pub title: Option<String>,
    pub description: FieldPatch<String>,
    pub status: Option<LifecycleStatus>,
    pub priority: FieldPatch<Priority>,
    pub goal_id: FieldPatch<GoalId>,
}

/// Validates an update against the *existing* Project - `existing` is
/// taken for the same shape-consistency reason
/// `validate_update_goal`/`validate_update_milestone`/
/// `validate_update_task` take it, though no field here actually needs
/// prior state to validate. No status-transition restriction: all four
/// `LifecycleStatus` values remain peer states for Project, exactly as
/// already established for Goal - a completed or abandoned Project may
/// be reopened; do not reintroduce a terminal-state rule here. Does
/// **not** check whether a `Set` `goal_id` references a real Goal -
/// needs a database read; `services::update_project` performs it.
pub(crate) fn validate_update_project(
    existing: &Project,
    payload: &UpdateProjectPayload,
) -> Result<(), Vec<ValidationError>> {
    let _ = existing; // reserved for shape-consistency with validate_update_goal/validate_update_milestone/validate_update_task
    let mut errors = Vec::new();

    let touches_any_field = payload.title.is_some()
        || !payload.description.is_unchanged()
        || payload.status.is_some()
        || !payload.priority.is_unchanged()
        || !payload.goal_id.is_unchanged();
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
    if let FieldPatch::Set(p) = &payload.priority {
        if let Err(e) = validate_priority(*p) {
            errors.push(e);
        }
    }
    // No goal_id existence check here - needs a database read;
    // services::update_project performs it. No status-transition
    // check: every LifecycleStatus -> LifecycleStatus change an
    // explicit action requests is allowed, matching Goal's precedent.

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

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

// --- Milestone (M2 Milestone vertical slice, 2026-09-17) ----------------
//
// The frozen M2 action set for Milestone is AddMilestone/UpdateMilestone
// only - Task is explicitly out of scope for this slice. Both actions
// have an `EXPLICIT_ADD` confirmation floor (architecture-freeze report
// §F) - do not invent an `IMPORTANT_CHANGE` floor for `UpdateMilestone`.
//
// Patch-semantics correction (2026-09-17, this report's own §2, applied
// here directly rather than repeating the freeze report's original
// mistake): `title`/`status`/`order_index` are non-nullable `Milestone`
// fields, so a plain `Option` is correct for them (`Some(x)` sets,
// `None` leaves unchanged) - `FieldPatch` would offer a meaningless
// `Clear` state no non-nullable field can use. `target_date` is
// `Milestone`'s one genuinely nullable field and uses `FieldPatch` so
// "leave unchanged" and "explicitly clear to NULL" stay distinct.

fn validate_order_index(order_index: OrderIndex) -> Result<(), ValidationError> {
    if order_index < ORDER_INDEX_MIN {
        return Err(ValidationError::OrderIndexOutOfBounds {
            min: ORDER_INDEX_MIN,
            actual: order_index,
        });
    }
    Ok(())
}

/// `AddMilestone` payload. `status` is deliberately absent - a new
/// Milestone always starts `PENDING` (M2 Milestone vertical-slice
/// report §5); completion happens only through `UpdateMilestone`.
/// `project_id` existence and `(project_id, order_index)` occupancy both
/// need a database read, so neither can happen in this pure module -
/// `services::add_milestone` performs them.
#[derive(Debug, Clone)]
pub(crate) struct AddMilestonePayload {
    pub project_id: ProjectId,
    pub title: String,
    pub target_date: Option<DateTime<Utc>>,
    pub order_index: OrderIndex,
}

/// `UpdateMilestone` payload - see the patch-semantics note above for
/// why `title`/`status`/`order_index` are plain `Option` while
/// `target_date` is `FieldPatch`. Whether a changed `order_index`
/// collides with another Milestone in the same Project needs a database
/// read (`services::update_milestone`), as does `Milestone` existence
/// itself.
#[derive(Debug, Clone, Default)]
pub(crate) struct UpdateMilestonePayload {
    pub id: MilestoneId,
    pub title: Option<String>,
    pub target_date: FieldPatch<DateTime<Utc>>,
    pub status: Option<MilestoneStatus>,
    pub order_index: Option<OrderIndex>,
}

/// Returns the normalized (trimmed) title on success, mirroring
/// `validate_create_goal`/`validate_create_project`. Deliberately does
/// **not** check `project_id` existence or `order_index` occupancy -
/// pure logic, no I/O; see `AddMilestonePayload`'s doc.
pub(crate) fn validate_add_milestone(
    payload: &AddMilestonePayload,
) -> Result<String, Vec<ValidationError>> {
    let mut errors = Vec::new();
    let mut normalized_title = None;

    match validate_title(&payload.title) {
        Ok(t) => normalized_title = Some(t),
        Err(e) => errors.push(e),
    }
    if let Err(e) = validate_order_index(payload.order_index) {
        errors.push(e);
    }

    if errors.is_empty() {
        Ok(normalized_title.expect("title validated Ok implies normalized_title is Some"))
    } else {
        Err(errors)
    }
}

/// Validates an update against the *existing* Milestone - `existing` is
/// taken for the same shape-consistency reason `validate_update_goal`
/// takes it, though no field here actually needs prior state to
/// validate. No status-transition restriction: `PENDING`/`DONE` are
/// freely reversible peer states (M2 Project Child Action Freeze report
/// §5 / Milestone vertical-slice report §13) - do not add one. Does
/// **not** check whether a changed `order_index` collides with another
/// Milestone - needs a database read; `services::update_milestone`
/// performs it.
pub(crate) fn validate_update_milestone(
    existing: &Milestone,
    payload: &UpdateMilestonePayload,
) -> Result<(), Vec<ValidationError>> {
    let _ = existing; // reserved for shape-consistency with validate_update_goal
    let mut errors = Vec::new();

    let touches_any_field = payload.title.is_some()
        || !payload.target_date.is_unchanged()
        || payload.status.is_some()
        || payload.order_index.is_some();
    if !touches_any_field {
        errors.push(ValidationError::NoFieldsToUpdate);
    }

    if let Some(title) = &payload.title {
        if let Err(e) = validate_title(title) {
            errors.push(e);
        }
    }
    if let Some(order_index) = payload.order_index {
        if let Err(e) = validate_order_index(order_index) {
            errors.push(e);
        }
    }
    // No target_date-specific validation: same as Goal.target_date, a
    // typed DateTime<Utc> has nothing further this pure layer checks.
    // No status-transition check: PENDING <-> DONE is always allowed.

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// --- Task (M2 Task vertical slice, 2026-09-17) --------------------------
//
// The frozen M2 action set for Task is AddTask/UpdateTask only -
// RecurringRule/Calendar/Reminder/Challenge/Personal Knowledge are all
// explicitly out of scope for this slice. Both actions have an
// `EXPLICIT_ADD` confirmation floor (architecture-freeze report §F /
// M2 Project Child Action Freeze report) - do not invent an
// `IMPORTANT_CHANGE` floor for `UpdateTask`.
//
// Patch semantics (Task vertical-slice report §5, applying the same
// corrected rule Milestone already established): `title`/`status` are
// non-nullable `Task` fields -> plain `Option<T>`. `due_at`/`project_id`/
// `goal_id` are all genuinely nullable `Task` fields -> `FieldPatch<T>`,
// so "leave unchanged" and "explicitly clear to NULL" (e.g. unlinking a
// Task from its Project) stay distinct. `recurring_rule_id` is not part
// of `UpdateTaskPayload` at all - `UpdateTask` never mutates it (Task
// vertical-slice report §2C).

/// `AddTask` payload. `status` is deliberately absent - a new Task
/// always starts `TODO` (Task vertical-slice report §6); completion (or
/// any other transition) happens only through `UpdateTask`.
/// `recurring_rule_id` is not part of this payload at all - `AddTask`
/// always creates one with `recurring_rule_id = None` (report §2C).
/// `project_id`/`goal_id` existence, when supplied, both need a database
/// read, so neither can happen in this pure module -
/// `services::add_task` performs them, independently of each other (a
/// Task may reference a Project and a Goal that have no relationship to
/// one another at all - report §3).
#[derive(Debug, Clone)]
pub(crate) struct AddTaskPayload {
    pub title: String,
    pub due_at: Option<DateTime<Utc>>,
    pub project_id: Option<ProjectId>,
    pub goal_id: Option<GoalId>,
}

/// `UpdateTask` payload - see the patch-semantics note above for why
/// `title`/`status` are plain `Option` while `due_at`/`project_id`/
/// `goal_id` are all `FieldPatch`. Whether a `Set` parent reference
/// exists needs a database read (`services::update_task`), as does
/// `Task` existence itself. No `recurring_rule_id` field - `UpdateTask`
/// never mutates it.
#[derive(Debug, Clone, Default)]
pub(crate) struct UpdateTaskPayload {
    pub id: TaskId,
    pub title: Option<String>,
    pub status: Option<TaskStatus>,
    pub due_at: FieldPatch<DateTime<Utc>>,
    pub project_id: FieldPatch<ProjectId>,
    pub goal_id: FieldPatch<GoalId>,
}

/// Returns the normalized (trimmed) title on success, mirroring
/// `validate_create_goal`/`validate_create_project`/
/// `validate_add_milestone`. Deliberately does **not** check
/// `project_id`/`goal_id` existence - pure logic, no I/O; see
/// `AddTaskPayload`'s doc. No `due_at`-specific validation exists, same
/// as `Goal.target_date`/`Milestone.target_date` - a typed
/// `DateTime<Utc>` has nothing further this pure layer checks (no extra
/// due-date product rule is invented, per the brief).
pub(crate) fn validate_add_task(payload: &AddTaskPayload) -> Result<String, Vec<ValidationError>> {
    let mut errors = Vec::new();
    let mut normalized_title = None;

    match validate_title(&payload.title) {
        Ok(t) => normalized_title = Some(t),
        Err(e) => errors.push(e),
    }

    if errors.is_empty() {
        Ok(normalized_title.expect("title validated Ok implies normalized_title is Some"))
    } else {
        Err(errors)
    }
}

/// Validates an update against the *existing* Task - `existing` is
/// taken for the same shape-consistency reason
/// `validate_update_goal`/`validate_update_milestone` take it, though no
/// field here actually needs prior state to validate. No status-
/// transition restriction: all four `TaskStatus` values are freely
/// reversible peer states (M2 Project Child Action Freeze report §7 /
/// Task vertical-slice report §7) - do not add one. Does **not** check
/// whether a `Set` parent reference exists - needs a database read;
/// `services::update_task` performs it.
pub(crate) fn validate_update_task(
    existing: &Task,
    payload: &UpdateTaskPayload,
) -> Result<(), Vec<ValidationError>> {
    let _ = existing; // reserved for shape-consistency with validate_update_goal/validate_update_milestone
    let mut errors = Vec::new();

    let touches_any_field = payload.title.is_some()
        || payload.status.is_some()
        || !payload.due_at.is_unchanged()
        || !payload.project_id.is_unchanged()
        || !payload.goal_id.is_unchanged();
    if !touches_any_field {
        errors.push(ValidationError::NoFieldsToUpdate);
    }

    if let Some(title) = &payload.title {
        if let Err(e) = validate_title(title) {
            errors.push(e);
        }
    }
    // No due_at-specific validation, same reasoning as validate_add_task
    // above. No status-transition check: any TaskStatus -> TaskStatus
    // change an explicit action requests is allowed.

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// --- Personal Knowledge: AddMemory (M2 Personal Knowledge Slice B,
// 2026-09-18) -----------------------------------------------------------
//
// The first production Memory action/mutation. Frozen input shapes from
// R018 §4/R019 §4-§7, restated verbatim by the Slice B brief. Deliberately
// NO `valid_from`/`valid_until`/`superseded_by_fact_id`/`created_at`/
// `updated_at`/`EvidenceId`/`FactId` anywhere in this payload - those are
// always Core-generated (`services::add_memory`), never caller-supplied.
//
// Confirmation floor is `ExplicitAdd` (R018 §3) - the *generic*
// `effective_class` formula alone already forces a pre-write block for
// `Provenance::Inferred` (severity 2, `InferredChange`, dominates
// `ExplicitAdd`'s severity 1), so unlike `CorrectMemory`'s `ImportantChange`
// floor, `AddMemory` needs no Personal-Knowledge-specific confirmation
// override here - only a dedicated test proving this frozen safety
// property holds (`services::tests`), since it is safety-critical even
// where it falls out "for free" from shared infrastructure.

/// An `AddMemory` (or future Memory-action) reference to an `Entity` -
/// the user themselves, an already-persisted `Entity`, or a brand-new one
/// to create in the same transaction. `SelfEntity` is resolved only
/// through the existing trusted bootstrap path (`services::
/// get_self_entity`) - this action never creates, searches for, or
/// mutates SELF.
#[derive(Debug, Clone)]
pub(crate) enum EntityInputRef {
    SelfEntity,
    Existing(EntityId),
    New(NewEntityData),
}

/// Fields for a brand-new `Entity` `AddMemory` creates inline. `domain_ref`
/// is optional - most new Entities (a person, a place) have none; when
/// present, both `domain_ref_type`/`domain_ref_id` must resolve
/// authoritatively (R021 §7) and must not collide with any other `New`
/// candidate in the same payload or any already-persisted `Entity`
/// (`ValidationError::DuplicateDomainReference`).
#[derive(Debug, Clone)]
pub(crate) struct NewEntityData {
    pub entity_type: String,
    pub canonical_name: String,
    pub sensitivity: Sensitivity,
    pub domain_ref: Option<DomainRef>,
}

/// A Fact's value - either an opaque JSON scalar/structure, or a
/// reference to another `Entity` (which may itself be Self/Existing/New,
/// exactly like the subject). The DB enforces "exactly one of
/// `value_json`/`object_entity_id`" as a `CHECK`; this Rust sum type makes
/// the same invariant unrepresentable-otherwise at the type level (R018
/// §2).
#[derive(Debug, Clone)]
pub(crate) enum FactValueInput {
    ScalarJson(String),
    Entity(EntityInputRef),
}

/// `AddMemory`'s reference to a `Source` - reuse an existing one (never
/// mutated) or create a new one in the same transaction.
#[derive(Debug, Clone)]
pub(crate) enum SourceRef {
    Existing(SourceId),
    New(NewSourceData),
}

/// Fields for a brand-new `Source` `AddMemory` creates inline.
/// `captured_at` is preserved exactly as supplied (it may legitimately
/// predate `created_at` - a Source can describe when something was
/// originally captured, not when LifeOS learned of it).
#[derive(Debug, Clone)]
pub(crate) struct NewSourceData {
    pub source_type: String,
    pub label: Option<String>,
    pub reference: Option<String>,
    pub sensitivity: Sensitivity,
    pub captured_at: DateTime<Utc>,
}

/// `AddMemory` always creates a brand-new `Evidence` row - Evidence is
/// never reused across Facts (R018 §11). Sensitivity is independent of
/// the Fact/Source/Entity it is attached to, never inherited. At least
/// one of `content`/`locator` is required, mirroring the DB `CHECK` on
/// `evidence`.
#[derive(Debug, Clone)]
pub(crate) struct NewEvidenceData {
    pub content: Option<String>,
    pub locator: Option<String>,
    pub sensitivity: Sensitivity,
}

/// The frozen `AddMemory` payload (R018 §4/R019 §4-§7). No implicit
/// deduplication/contradiction-detection/correction/auto-`STALE`/
/// similarity-merge/alias-merge ever happens as a side effect of this
/// action - two explicit `AddMemory` calls may create two Facts that look
/// alike (R021 §16).
#[derive(Debug, Clone)]
pub(crate) struct AddMemoryPayload {
    pub subject: EntityInputRef,
    pub predicate: String,
    pub value: FactValueInput,
    pub memory_type: MemoryType,
    pub epistemic_state: EpistemicState,
    pub confidence: Option<f32>,
    pub sensitivity: Sensitivity,
    pub source: SourceRef,
    pub evidence: NewEvidenceData,
}

/// `validate_add_memory`'s success output - the same payload shape with
/// every free-text field that gets trimmed (identifier-like fields only:
/// `entity_type`/`canonical_name`/`domain_ref_type`/`domain_ref_id`/
/// `predicate`/`source_type`, matching `validate_title`'s precedent)
/// replaced by its normalized value, so `services::add_memory` never
/// re-derives what was actually validated. `value_json`/`label`/
/// `reference`/`content`/`locator` are free text, never trimmed, matching
/// `validate_description`'s precedent (semantic content preserved
/// exactly as supplied).
#[derive(Debug, Clone)]
pub(crate) struct NormalizedAddMemory {
    pub subject: EntityInputRef,
    pub predicate: String,
    pub value: FactValueInput,
    pub memory_type: MemoryType,
    pub epistemic_state: EpistemicState,
    pub confidence: Option<f32>,
    pub sensitivity: Sensitivity,
    pub source: SourceRef,
    pub evidence: NewEvidenceData,
}

/// A required Personal Knowledge free-text field: trims, rejects empty,
/// rejects over-length, and returns the normalized value on success.
/// Pushes directly into the shared `errors` accumulator (matching
/// `validate_update_milestone`'s aggregate-everything style) rather than
/// returning a `Result`, so every caller composes uniformly regardless of
/// how many fields it validates.
fn validate_pk_required_field(
    field: &'static str,
    value: &str,
    max_len: usize,
    errors: &mut Vec<ValidationError>,
) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        errors.push(ValidationError::PersonalKnowledgeFieldEmpty { field });
        return None;
    }
    match validate_personal_knowledge_text_len(trimmed, max_len) {
        Ok(()) => Some(trimmed.to_string()),
        Err(actual) => {
            errors.push(ValidationError::PersonalKnowledgeFieldTooLong {
                field,
                max: max_len,
                actual,
            });
            None
        }
    }
}

/// An optional Personal Knowledge free-text field: no trim, no
/// emptiness requirement (matching `validate_description`'s precedent -
/// free text is preserved exactly as supplied), only a length bound.
fn validate_pk_optional_field(
    field: &'static str,
    value: &str,
    max_len: usize,
    errors: &mut Vec<ValidationError>,
) {
    if let Err(actual) = validate_personal_knowledge_text_len(value, max_len) {
        errors.push(ValidationError::PersonalKnowledgeFieldTooLong {
            field,
            max: max_len,
            actual,
        });
    }
}

/// Structurally validates an `EntityInputRef` - `SelfEntity`/`Existing`
/// pass through unchanged (their existence needs a database read;
/// `services::add_memory` performs it, not this pure module). `New`
/// validates and normalizes `entity_type`/`canonical_name`/`domain_ref`.
/// Returns `None` (having pushed the specific errors) if `New` data is
/// invalid, without short-circuiting the caller's other checks.
fn validate_entity_input_ref(
    input: &EntityInputRef,
    errors: &mut Vec<ValidationError>,
) -> Option<EntityInputRef> {
    match input {
        EntityInputRef::SelfEntity => Some(EntityInputRef::SelfEntity),
        EntityInputRef::Existing(id) => Some(EntityInputRef::Existing(*id)),
        EntityInputRef::New(data) => {
            let start_len = errors.len();
            let entity_type = validate_pk_required_field(
                "entity_type",
                &data.entity_type,
                ENTITY_TYPE_MAX_LEN,
                errors,
            );
            let canonical_name = validate_pk_required_field(
                "canonical_name",
                &data.canonical_name,
                CANONICAL_NAME_MAX_LEN,
                errors,
            );
            let domain_ref = match &data.domain_ref {
                None => Some(None),
                Some(r) => {
                    let t = validate_pk_required_field(
                        "domain_ref_type",
                        &r.domain_ref_type,
                        DOMAIN_REF_TYPE_MAX_LEN,
                        errors,
                    );
                    let i = validate_pk_required_field(
                        "domain_ref_id",
                        &r.domain_ref_id,
                        DOMAIN_REF_ID_MAX_LEN,
                        errors,
                    );
                    match (t, i) {
                        (Some(t), Some(i)) => Some(Some(DomainRef {
                            domain_ref_type: t,
                            domain_ref_id: i,
                        })),
                        _ => None,
                    }
                }
            };
            if errors.len() > start_len {
                return None;
            }
            Some(EntityInputRef::New(NewEntityData {
                entity_type: entity_type.expect("validated above"),
                canonical_name: canonical_name.expect("validated above"),
                sensitivity: data.sensitivity,
                domain_ref: domain_ref.expect("validated above"),
            }))
        }
    }
}

/// Structurally validates a `FactValueInput`. `ScalarJson` checks length
/// then syntactic JSON validity (`serde_json::from_str`) - any JSON type
/// is accepted, the value is never executed/evaluated and never silently
/// altered (returned exactly as supplied, not re-serialized). `Entity`
/// delegates to `validate_entity_input_ref` - subject==object self-edges
/// are structurally identical to any other Entity reference and are not
/// restricted here (R021 §10: explicitly allowed).
fn validate_fact_value_input(
    input: &FactValueInput,
    errors: &mut Vec<ValidationError>,
) -> Option<FactValueInput> {
    match input {
        FactValueInput::ScalarJson(s) => {
            if let Err(actual) = validate_personal_knowledge_text_len(s, VALUE_JSON_MAX_LEN) {
                errors.push(ValidationError::PersonalKnowledgeFieldTooLong {
                    field: "value_json",
                    max: VALUE_JSON_MAX_LEN,
                    actual,
                });
                return None;
            }
            if serde_json::from_str::<serde_json::Value>(s).is_err() {
                errors.push(ValidationError::InvalidValueJson);
                return None;
            }
            Some(FactValueInput::ScalarJson(s.clone()))
        }
        FactValueInput::Entity(entity_ref) => {
            validate_entity_input_ref(entity_ref, errors).map(FactValueInput::Entity)
        }
    }
}

/// Structurally validates a `SourceRef`. `Existing`'s existence needs a
/// database read; `services::add_memory` performs it. `New` validates
/// `source_type` (required) and `label`/`reference` (optional, unbounded
/// emptiness).
fn validate_source_ref(input: &SourceRef, errors: &mut Vec<ValidationError>) -> Option<SourceRef> {
    match input {
        SourceRef::Existing(id) => Some(SourceRef::Existing(*id)),
        SourceRef::New(data) => {
            let start_len = errors.len();
            let source_type = validate_pk_required_field(
                "source_type",
                &data.source_type,
                SOURCE_TYPE_MAX_LEN,
                errors,
            );
            if let Some(label) = &data.label {
                validate_pk_optional_field("label", label, SOURCE_LABEL_MAX_LEN, errors);
            }
            if let Some(reference) = &data.reference {
                validate_pk_optional_field(
                    "reference",
                    reference,
                    SOURCE_REFERENCE_MAX_LEN,
                    errors,
                );
            }
            if errors.len() > start_len {
                return None;
            }
            Some(SourceRef::New(NewSourceData {
                source_type: source_type.expect("validated above"),
                label: data.label.clone(),
                reference: data.reference.clone(),
                sensitivity: data.sensitivity,
                captured_at: data.captured_at,
            }))
        }
    }
}

/// Validates `NewEvidenceData`: `content`/`locator` are each bounded when
/// present (no trim/emptiness requirement, free text), and at least one
/// of the two must be present at all (mirrors the DB `CHECK` on
/// `evidence`).
fn validate_new_evidence_data(
    data: &NewEvidenceData,
    errors: &mut Vec<ValidationError>,
) -> Option<NewEvidenceData> {
    let start_len = errors.len();
    if let Some(content) = &data.content {
        validate_pk_optional_field("content", content, EVIDENCE_CONTENT_MAX_LEN, errors);
    }
    if let Some(locator) = &data.locator {
        validate_pk_optional_field("locator", locator, EVIDENCE_LOCATOR_MAX_LEN, errors);
    }
    if data.content.is_none() && data.locator.is_none() {
        errors.push(ValidationError::EvidenceRequiresContentOrLocator);
    }
    if errors.len() > start_len {
        return None;
    }
    Some(NewEvidenceData {
        content: data.content.clone(),
        locator: data.locator.clone(),
        sensitivity: data.sensitivity,
    })
}

/// `confidence` must be `None`, or `Some(c)` with `c` finite and in
/// `[0.0, 1.0]` - `NaN`/`+Infinity`/`-Infinity` are explicitly rejected
/// even though a naive `c < 0.0 || c > 1.0` range check alone would let
/// `NaN` slip through (`NaN` compares false to everything) (R021 §11).
/// Shared bound check reused by both `validate_confidence` (`AddMemory`'s
/// plain `Option<f32>`) and `validate_confidence_patch` (`CorrectMemory`'s
/// `FieldPatch<f32>`) - one canonical rule, not two near-identical ones.
fn confidence_out_of_bounds(c: f32) -> Option<ValidationError> {
    if !c.is_finite() || !(0.0..=1.0).contains(&c) {
        Some(ValidationError::ConfidenceOutOfBounds {
            min: 0.0,
            max: 1.0,
            actual: c,
        })
    } else {
        None
    }
}

fn validate_confidence(confidence: Option<f32>, errors: &mut Vec<ValidationError>) {
    if let Some(c) = confidence {
        if let Some(e) = confidence_out_of_bounds(c) {
            errors.push(e);
        }
    }
}

/// `CorrectMemory`'s `confidence: FieldPatch<f32>` - `Unchanged`/`Clear`
/// are always valid (no numeric value to bound-check); `Set(c)` is
/// bound-checked exactly like `AddMemory`'s `Option<f32>` (M2 Personal
/// Knowledge Slice C, 2026-09-18).
fn validate_confidence_patch(patch: &FieldPatch<f32>, errors: &mut Vec<ValidationError>) {
    if let FieldPatch::Set(c) = patch {
        if let Some(e) = confidence_out_of_bounds(*c) {
            errors.push(e);
        }
    }
}

/// Pure structural validation for `AddMemory` - no database read (Entity/
/// Source existence, domain-ref authoritative existence, and persisted-
/// duplicate-domain-ref checks all need one; `services::add_memory`
/// performs them). Collects every structural error before returning
/// (matching every other `validate_*` in this module), including the
/// same-payload duplicate-domain-ref check: two `New` Entity candidates
/// (subject and/or object) sharing one `domain_ref` are rejected here,
/// before any SQL runs at all - the persisted-duplicate check against
/// already-committed Entities is a second, separate layer in
/// `services::add_memory`.
pub(crate) fn validate_add_memory(
    payload: &AddMemoryPayload,
) -> Result<NormalizedAddMemory, Vec<ValidationError>> {
    let mut errors = Vec::new();

    let subject = validate_entity_input_ref(&payload.subject, &mut errors);
    let predicate = validate_pk_required_field(
        "predicate",
        &payload.predicate,
        PREDICATE_MAX_LEN,
        &mut errors,
    );
    let value = validate_fact_value_input(&payload.value, &mut errors);
    validate_confidence(payload.confidence, &mut errors);
    let source = validate_source_ref(&payload.source, &mut errors);
    let evidence = validate_new_evidence_data(&payload.evidence, &mut errors);

    if let (Some(EntityInputRef::New(s)), Some(FactValueInput::Entity(EntityInputRef::New(o)))) =
        (&subject, &value)
    {
        if let (Some(sd), Some(od)) = (&s.domain_ref, &o.domain_ref) {
            if sd == od {
                errors.push(ValidationError::DuplicateDomainReference);
            }
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(NormalizedAddMemory {
        subject: subject.expect("validated above"),
        predicate: predicate.expect("validated above"),
        value: value.expect("validated above"),
        memory_type: payload.memory_type,
        epistemic_state: payload.epistemic_state,
        confidence: payload.confidence,
        sensitivity: payload.sensitivity,
        source: source.expect("validated above"),
        evidence: evidence.expect("validated above"),
    })
}

// --- Personal Knowledge: CorrectMemory (M2 Personal Knowledge Slice C,
// 2026-09-18) -------------------------------------------------------------
//
// "The retained semantic knowledge is wrong/outdated; replace it while
// preserving history" - never an in-place semantic edit. One active
// predecessor Fact becomes historical (`Stale`); one new successor Fact
// becomes active. `subject_entity_id`/`predicate` are inherited from the
// predecessor EXACTLY - this payload has no way to change either;
// changing subject or predicate is `AddMemory` for a different
// assertion, never `CorrectMemory`.
//
// Type floor is `ImportantChange` (R018 §6) - unlike `AddMemory`'s
// `ExplicitAdd` floor, `effective_class(ImportantChange, Inferred)`
// stays `ImportantChange` (severity 3 already dominates
// `InferredChange`'s severity 2), so the generic ladder alone does NOT
// block an inferred, unconfirmed correction. `services::
// memory_action_requires_confirmation` (services/mod.rs) restores the
// required block without inventing a new `ConfirmationClass` or
// changing `effective_class`/`blocks_until_confirmed` themselves.

/// The frozen `CorrectMemory` payload (R018, restated by the Slice C
/// brief). Deliberately NO `subject`/`predicate`/`epistemic_state`/
/// `valid_from`/`valid_until`/`superseded_by_fact_id`/successor `FactId`/
/// `EvidenceId`/`created_at`/`updated_at` - all Core-generated or
/// inherited from the predecessor, never caller-supplied. Every
/// successful correction persists `epistemic_state = USER_CONFIRMED` on
/// the successor unconditionally - there is no v0.1 path where
/// `CorrectMemory` produces `INFERRED`/`OBSERVED`/`DISPUTED`/`STALE` (a
/// model wanting to retain a tentative alternative without replacing the
/// current Fact uses `AddMemory` with `HYPOTHESIS`/`INFERRED` instead).
#[derive(Debug, Clone)]
pub(crate) struct CorrectMemoryPayload {
    pub predecessor_fact_id: FactId,
    pub value: FactValueInput,
    pub memory_type: Option<MemoryType>,
    pub confidence: FieldPatch<f32>,
    pub sensitivity: Option<Sensitivity>,
    pub source: SourceRef,
    pub evidence: NewEvidenceData,
}

/// `validate_correct_memory`'s success output - same normalization
/// convention as `NormalizedAddMemory` (identifier-like fields trimmed,
/// free text preserved exactly). No `subject`/`predicate` fields exist
/// here at all - both are inherited from the predecessor, which this
/// pure layer never reads (needs a database read; `services::
/// correct_memory` performs it).
#[derive(Debug, Clone)]
pub(crate) struct NormalizedCorrectMemory {
    pub predecessor_fact_id: FactId,
    pub value: FactValueInput,
    pub memory_type: Option<MemoryType>,
    pub confidence: FieldPatch<f32>,
    pub sensitivity: Option<Sensitivity>,
    pub source: SourceRef,
    pub evidence: NewEvidenceData,
}

/// Pure structural validation for `CorrectMemory` - no database read
/// (predecessor existence/activeness, object Entity/Source existence,
/// domain-ref authoritative existence, and persisted-duplicate-domain-
/// ref all need one; `services::correct_memory` performs them). Reuses
/// `AddMemory`'s exact structural validators
/// (`validate_fact_value_input`/`validate_source_ref`/
/// `validate_new_evidence_data`) rather than duplicating eleven
/// slightly different ones - only `confidence`'s `FieldPatch<f32>` shape
/// differs from `AddMemory`'s plain `Option<f32>`, handled by
/// `validate_confidence_patch` sharing the same bound check. Unlike
/// `AddMemory`, there is only ever at most ONE `New` Entity candidate
/// here (the object - there is no subject candidate, since subject is
/// always inherited), so no same-payload duplicate-domain-ref preflight
/// is needed or possible at this layer.
pub(crate) fn validate_correct_memory(
    payload: &CorrectMemoryPayload,
) -> Result<NormalizedCorrectMemory, Vec<ValidationError>> {
    let mut errors = Vec::new();

    let value = validate_fact_value_input(&payload.value, &mut errors);
    validate_confidence_patch(&payload.confidence, &mut errors);
    let source = validate_source_ref(&payload.source, &mut errors);
    let evidence = validate_new_evidence_data(&payload.evidence, &mut errors);

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(NormalizedCorrectMemory {
        predecessor_fact_id: payload.predecessor_fact_id,
        value: value.expect("validated above"),
        memory_type: payload.memory_type,
        confidence: payload.confidence.clone(),
        sensitivity: payload.sensitivity,
        source: source.expect("validated above"),
        evidence: evidence.expect("validated above"),
    })
}

// --- Personal Knowledge: ConfirmMemory (M2 Personal Knowledge Slice D,
// 2026-09-18) -------------------------------------------------------------
//
// "The semantic assertion already stored in this Fact is correct
// according to the user" - a pure EPISTEMIC confirmation, never a
// semantic-value correction, never a new/successor Fact, never a
// contradiction-resolution workflow, never an Evidence-creation
// operation. Mutates the SAME Fact row in place: exactly
// `epistemic_state` (always to `UserConfirmed`) and `updated_at`.
//
// Type floor is `ExplicitAdd` - the same floor `AddMemory` has, and for
// the same reason its generic `effective_class` formula alone already
// blocks `Provenance::Inferred` (`InferredChange`, severity 2, dominates
// `ExplicitAdd`, severity 1) - no Personal-Knowledge-specific override
// is needed here (unlike `CorrectMemory`'s `ImportantChange` floor).

/// The frozen `ConfirmMemory` payload - a single typed `FactId`, nothing
/// else. No `value`/`subject`/`predicate`/`confidence`/`sensitivity`/
/// `memory_type`/`Source`/`Evidence`/`epistemic_state`/timestamp field
/// exists - Core decides the one permitted state transition
/// unconditionally (`UserConfirmed`); there is nothing else for a caller
/// to supply.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConfirmMemoryPayload {
    pub fact_id: FactId,
}

/// `validate_confirm_memory`'s success output - identical shape to
/// `ConfirmMemoryPayload` (there is nothing to normalize: a `FactId` is
/// already a validated typed value by construction). Kept as its own
/// type only for consistency with every other Memory action's
/// validate/normalize split.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NormalizedConfirmMemory {
    pub fact_id: FactId,
}

/// Pure structural validation for `ConfirmMemory` - deliberately
/// minimal, since the payload carries only a typed `FactId` with no
/// further structure to check here. A syntactically valid `FactId` may
/// still not reference an existing row, or may reference an inactive or
/// already-confirmed one - both are database-dependent checks
/// `services::confirm_memory` performs after the confirmation gate, not
/// invented fake rules here.
pub(crate) fn validate_confirm_memory(
    payload: &ConfirmMemoryPayload,
) -> Result<NormalizedConfirmMemory, Vec<ValidationError>> {
    Ok(NormalizedConfirmMemory {
        fact_id: payload.fact_id,
    })
}

// --- Personal Knowledge: ForgetMemory (M2 Personal Knowledge Slice E,
// 2026-09-18) -------------------------------------------------------------
//
// "The user wants this one exact Fact removed from LifeOS's canonical
// Personal Knowledge" - the first, and only, hard-DELETE Memory action.
// Never soft delete, never a `Stale` transition, never a correction.
// The target Fact is physically removed from the SQLite model and is
// not retrievable as a historical Fact afterward (R019's final 14-step
// privacy-safe algorithm - `services::forget_memory`).
//
// Type floor is `ConfirmationClass::Delete` - the highest severity rung
// (4), always blocking before execution regardless of provenance: even
// `effective_class(Delete, Explicit)` stays `Delete` (severity 4 is
// never dominated by either provenance-derived class), so a direct
// explicit delete request still requires `meta.confirmed == true` -
// unlike every other Memory action, there is no "the call itself is the
// confirmation" path here. Uses the exact same generic
// `effective_class`/`blocks_until_confirmed` gate every other action
// already uses - no new helper, no change to `ConfirmationClass` or the
// generic ladder.

/// The frozen `ForgetMemory` payload - a single typed `FactId`, nothing
/// else. No `reason`/`value`/`Source`/`Evidence`/snapshot input/delete-
/// mode/cascade option - targets exactly one Fact, never a bulk
/// "forget everything about X."
#[derive(Debug, Clone, Copy)]
pub(crate) struct ForgetMemoryPayload {
    pub fact_id: FactId,
}

/// `validate_forget_memory`'s success output - identical shape to
/// `ForgetMemoryPayload` (nothing to normalize, same reasoning as
/// `NormalizedConfirmMemory`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct NormalizedForgetMemory {
    pub fact_id: FactId,
}

/// Pure structural validation for `ForgetMemory` - deliberately
/// minimal, same reasoning as `validate_confirm_memory`. Unlike
/// `ConfirmMemory`, there is no "already in the target state" check
/// here at all, pure or otherwise: `ForgetMemory` does not care whether
/// the target Fact is active or historical (R019 - the right to forget
/// applies to historical knowledge too), so no `domain::is_active_fact`
/// check exists anywhere in this action's validation path.
pub(crate) fn validate_forget_memory(
    payload: &ForgetMemoryPayload,
) -> Result<NormalizedForgetMemory, Vec<ValidationError>> {
    Ok(NormalizedForgetMemory {
        fact_id: payload.fact_id,
    })
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
