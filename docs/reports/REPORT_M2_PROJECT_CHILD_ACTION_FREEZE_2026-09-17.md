# REPORT — M2 Project Child Action Freeze

**Date:** 2026-09-17
**Scope:** Architecture / action-set clarification only. **Zero source-
code changes.** Resolves the remaining mutation/action gaps around
`Project` → `Milestone` → `Task` before either child entity is
implemented. Explicitly **not** in scope: implementing `Milestone`,
`Task`, `Personal Knowledge`, `Calendar`, `Challenge`, M3 UI, a
`capability/` transport, Qwen, GPT/Luna.

---

## 1. Why this pass exists

The M2 GoalRelation vertical-slice report's own "next recommended step"
named a fourth entity as the natural next slice, with `Milestone`/`Task`
as leading candidates. Before starting either, architectural review of
the frozen domain model surfaced the same category of problem the
Project vertical slice already hit once (and the Project/storage
hardening report already documented, rather than silently resolved):
the frozen *action set* does not yet cover every mutation the frozen
*domain model* implies is meaningful. Unlike Project's gap - discovered
mid-implementation and then documented after the fact - this run
resolves the equivalent gaps for `Milestone`/`Task` **before** any of
their code exists, so their vertical slices inherit an unambiguous
target instead of having to invent one under implementation pressure
(exactly the failure mode that produced the Goal vertical slice's
unapproved terminal-status rule).

---

## 2. Existing action-set gaps

Three gaps, all of the same shape - a domain field exists that no
frozen action can mutate after creation:

**A. `Project`** (already schema/code-implemented): `title`/
`description`/`status`/`priority`/`goal_id` have no update path. The
frozen set was `CreateProject`/`AddMilestone`/`AddTask`/`UpdateProgress`
- already documented as an open gap in the Project/storage integrity
hardening report §10, explicitly *not* resolved there per that run's
own instructions.

**B. `Milestone`** (schema/code not yet implemented, fields frozen at
M2): `title`/`target_date`/`status`/`order_index` would have no update
path once implemented. Concretely: a `Milestone` created `PENDING` via
`AddMilestone` would have **no typed action capable of marking it
`DONE`** - `AddMilestone` only creates, `AddMilestone` again would just
create a second milestone, not transition the first one.

**C. `Task`** (schema/code not yet implemented, fields frozen at M2):
`title`/`status`/`due_at`/`project_id`/`goal_id` would have no update
path once implemented. Concretely: a `Task` could be created `TODO` but
could never naturally move `TODO → IN_PROGRESS → DONE` or `CANCELLED`.

Leaving B and C unresolved until mid-implementation would repeat gap A's
mistake a second and third time, in a run where `Milestone`/`Task` code
is actually being written and under the same pressure that produced the
Goal terminal-status invention.

---

## 3. UpdateProject decision

**Frozen:** add `UpdateProject` to the Project action set. `Project`
action set is now: `CreateProject`, `UpdateProject`, `AddMilestone`,
`AddTask`, `UpdateProgress`.

**Rejected alternative:** several narrow actions (`RenameProject`,
`ChangeProjectStatus`, `ChangeProjectPriority`, `RelinkProjectGoal`).
Rejected because the existing `UpdateGoal` pattern already proves one
typed update action with explicit patch semantics is cleaner than an
action-type explosion - each narrow action would need its own payload
type, its own confirmation-floor decision, its own tests, for no
behavioral gain over one payload with several optional/patchable
fields. `UpdateGoal` already demonstrates the pattern scales to five
mutable fields (`title`/`description`/`status`/`target_date`/
`priority_user`) cleanly; `Project` has the same five-field shape
(`title`/`description`/`status`/`priority`/`goal_id`).

`UpdateProgress` is explicitly **not** folded into `UpdateProject` and
vice versa - `UpdateProgress` continues to own `progress` alone. Two
reasons: (1) they already have different, frozen confirmation floors
(`EXPLICIT_ADD` vs. `IMPORTANT_CHANGE`, §7) that a single action could
not cleanly represent, and (2) `progress` is the one field the frozen
architecture-freeze table never listed for `Project` at all (it was a
Project-vertical-slice-run interpretation, §6 of the Project/storage
hardening report) - keeping it on its own dedicated action keeps that
distinction visible rather than blurring it into a general-purpose
field bag.

---

## 4. UpdateMilestone decision

**Frozen:** add `UpdateMilestone` to the Project (child) action set,
now: `CreateProject`, `UpdateProject`, `AddMilestone`,
`UpdateMilestone`, `AddTask`, `UpdateTask`, `UpdateProgress`.

Covers all four frozen `Milestone` fields beyond `id`/`project_id`
(which are identity, not mutable): `title`, `target_date`, `status`,
`order_index`. `project_id` itself is **not** included in
`UpdateMilestone` - re-parenting a milestone to a different project was
not requested by this brief and was not frozen; if that need arises
later it is a separate, deliberate decision, not implied by this one.

Same "one typed update action, not several narrow ones" reasoning as
§3 - `CompleteMilestone`/`ReopenMilestone` were considered and rejected
in favor of `status: Some(Done)`/`status: Some(Pending)` inside the
general update payload.

---

## 5. UpdateTask decision

**Frozen:** add `UpdateTask` to the Project (child) action set (final
shape shown in §3/§4 above).

Covers `title`, `status`, `due_at`, `project_id`, `goal_id` -
deliberately **excludes** `recurring_rule_id` mutation (§12 - recurrence
behavior itself is not designed yet, so mutating a reference to it would
be premature) and `created_at`/`updated_at` (system-managed, same as
every other entity's timestamps).

Same "one typed update action" reasoning - `StartTask`/`CompleteTask`/
`CancelTask`/`MoveTask`/`RelinkTask` were all considered and rejected in
favor of `status: Some(...)`/patch fields inside `UpdateTask`.

---

## 6. FieldPatch semantics

Reuses `actions::FieldPatch<T>` exactly as already proven by
`UpdateGoal` - no new pattern invented. Conceptual shapes (Rust syntax
not frozen verbatim; the eventual `Milestone`/`Task` vertical slices may
adjust field ordering/derive lists as long as the *semantics* below
hold):

```
UpdateProjectPayload {
    id: ProjectId,
    title: Option<String>,              // non-nullable field -> plain Option
    description: FieldPatch<String>,    // nullable field -> FieldPatch
    status: Option<LifecycleStatus>,    // non-nullable -> plain Option
    priority: FieldPatch<Priority>,     // nullable -> FieldPatch
    goal_id: FieldPatch<GoalId>,        // nullable -> FieldPatch
}

UpdateMilestonePayload {
    id: MilestoneId,
    title: Option<String>,
    target_date: FieldPatch<DateTime<Utc>>,   // nullable
    status: Option<MilestoneStatus>,          // non-nullable (PENDING/DONE)
    order_index: Option<i64>,                 // non-nullable (a milestone always has *some* position)
}

UpdateTaskPayload {
    id: TaskId,
    title: Option<String>,
    status: Option<TaskStatus>,               // non-nullable
    due_at: FieldPatch<DateTime<Utc>>,        // nullable
    project_id: FieldPatch<ProjectId>,        // nullable
    goal_id: FieldPatch<GoalId>,              // nullable
}
```

The governing rule, exactly as `UpdateGoal` already established: **any
field that is `Option<T>` on the domain entity itself uses
`FieldPatch<T>` in the update payload** (so "leave unchanged" and
"explicitly clear to `NULL`" stay distinct, unambiguous states); any
field that is **not** nullable on the entity stays a plain `Option<T>`
in the payload (`None` = leave unchanged is unambiguous there, since
there is no "clear" state to confuse it with). This is exactly why
`Task.project_id`/`Task.goal_id` use `FieldPatch` - clearing either is a
legitimate, meaningful "unlink" operation, distinct from "don't touch
this field."

`Project.progress` is deliberately **absent** from `UpdateProjectPayload`
- `UpdateProgress` already owns that mutation exclusively (§3).

---

## 7. Confirmation floors

Frozen exactly as specified, no derivation needed - explicit per-action
floors, consistent with how `CreateProject`/`UpdateProgress`/
`LinkGoals` were already frozen directly rather than derived from a
generic default:

| Action | Floor |
|---|---|
| `UpdateProject` | `IMPORTANT_CHANGE` |
| `UpdateMilestone` | `EXPLICIT_ADD` |
| `UpdateTask` | `EXPLICIT_ADD` |

**Rationale for `UpdateProject` = `IMPORTANT_CHANGE`:** a Project's
title/status/priority/Goal association are structural identity changes
- consistent with `UpdateGoal`'s own floor (also `IMPORTANT_CHANGE`),
and a materially different kind of edit than flipping a milestone's
status or nudging a task's due date.

**Rationale for `UpdateMilestone`/`UpdateTask` = `EXPLICIT_ADD`:**
`UpdateProgress` already established the precedent that an *update*
action can sit at `EXPLICIT_ADD` - the class name describes risk level,
not literally "this inserts a row." Milestone/task status changes are
the routine, high-frequency case a user makes constantly while actually
using the app (checking off a milestone, moving a task along its
workflow) - treating every one of those as an `IMPORTANT_CHANGE`
requiring an acknowledgement step would make the app tedious to use for
its most common interaction, unlike Project's comparatively rare
structural edits.

**Behavior, using the same `effective_class = max(type_floor,
provenance_class)` mechanism already proven by every prior action:**

- Explicit user `UpdateMilestone`/`UpdateTask` → `effective_class =
  EXPLICIT_ADD` → executes immediately, inline confirmation + Undo.
- Inferred (AI-proposed) `UpdateMilestone`/`UpdateTask` →
  `provenance_class = INFERRED_CHANGE` (severity 2) exceeds the
  `EXPLICIT_ADD` floor (severity 1) → `effective_class =
  INFERRED_CHANGE` → blocks until explicit confirmation, zero writes
  before confirmed resubmission - exactly the same mechanism already
  proven for `CreateGoal`/`CreateProject`/`LinkGoals` with
  `Provenance::Inferred`.
- `UpdateProject` follows the identical mechanism at its own
  `IMPORTANT_CHANGE` floor - inferred provenance still never *lowers*
  the required confirmation, only ever raises it (same as every other
  action; the ladder is `SAFE_READ < EXPLICIT_ADD < INFERRED_CHANGE <
  IMPORTANT_CHANGE < DELETE`, so `IMPORTANT_CHANGE`'s floor already
  exceeds `INFERRED_CHANGE`'s provenance-forced minimum).
- `SENSITIVE` remains the orthogonal, content-derived overlay - untouched
  by this freeze, unrelated to any of these three actions specifically.

No implementation was written to prove this in a test this run
(documentation-only) - the mechanism itself (`effective_class`,
`blocks_until_confirmed`) is already implemented and tested via
`CreateGoal`/`CreateProject`/`LinkGoals`; `UpdateMilestone`/`UpdateTask`/
`UpdateProject` will exercise the identical, already-proven code path
once implemented, not a new one.

---

## 8. Status-transition policy

**Frozen: no terminal-state restrictions for either entity.**

`Milestone.status` (`PENDING`/`DONE`): both states remain reachable from
each other via explicit user action through `UpdateMilestone`. A
completed milestone can be reopened.

`Task.status` (`TODO`/`IN_PROGRESS`/`DONE`/`CANCELLED`): all four remain
peer persisted states. No frozen rule prevents `TODO → DONE` directly
(skipping `IN_PROGRESS`), reopening from `DONE`, or reopening from
`CANCELLED`.

This is a direct, deliberate application of the lesson already learned
and corrected once: the Goal vertical slice silently invented "`
COMPLETED`/`ABANDONED` cannot transition further," which had to be
removed in the Goal hardening pass because it was never part of the
frozen Goal status definition. Applying that same restriction to
`Milestone`/`Task` - even though it might feel intuitive ("why would you
un-complete a milestone?") - would be inventing exactly the same kind of
unapproved product rule a second and third time. If a future run
determines such a workflow rule is actually wanted, it must be frozen
explicitly in `LIFEOS_SPEC.md` first, the same way this run is freezing
the action set itself - not invented silently inside a `Milestone`/
`Task` vertical slice.

---

## 9. FK/delete policy

Frozen to match `GoalRelation`'s already-proven precedent exactly (§8
of the GoalRelation vertical-slice report):

| Foreign key | Nullable? | Delete behavior |
|---|---|---|
| `Milestone.project_id` | No (required) | `ON DELETE RESTRICT` |
| `Task.project_id` | Yes | `ON DELETE RESTRICT` (when present) |
| `Task.goal_id` | Yes | `ON DELETE RESTRICT` (when present) |

**Rationale, identical to `GoalRelation`'s:** neither `Project` nor
`Goal` has a normal hard-delete production action today (only soft-
state `LifecycleStatus` transitions exist for `Goal`; `Project` has no
status-delete concept either). `RESTRICT` prevents child work
(`Milestone`s, `Task`s) from silently disappearing or becoming orphaned
if a hard-delete action is ever added later without first explicitly
deciding what happens to dependent rows - the same reasoning that
produced `GoalRelation`'s `RESTRICT` choice over cascade.

**This run does not implement the schema** - `V4__create_milestones.sql`
(or however it is eventually named) does not exist yet; this section
only freezes the decision so the `Milestone` vertical slice does not
have to make it under implementation pressure.

---

## 10. Task ownership semantics

**Frozen: no new requirement.** `Task.project_id` and `Task.goal_id` are
both nullable in the already-frozen domain model, and remain so. A
`Task` may be:

- standalone (neither `project_id` nor `goal_id` set),
- linked to a `Project` only,
- linked to a `Goal` only, or
- linked to both.

No existing canonical document (`LIFEOS_SPEC.md`, the architecture-
freeze report) states or implies a requirement that a `Task` must belong
to a `Project` or `Goal` - inventing such a requirement now would be an
unrequested, unfrozen restriction on the already-frozen field
nullability. This section exists only to make that absence-of-a-rule
explicit and intentional, not to introduce a new one.

---

## 11. Milestone order_index unresolved/decision status

**Genuinely left as an OPEN QUESTION, not silently decided.**
`Milestone.order_index` exists in the frozen field list to provide
deterministic ordering within a `Project`. This run freezes only what
is unambiguous:

- `order_index` is persisted explicitly (not computed/derived at read
  time).
- Deterministic sort order for a Project's milestones uses
  `order_index`.
- No automatic renumbering/rebalancing algorithm is frozen or implied
  for M2 - if two milestones ever need to be reordered, that is a
  future decision, not this run's.

**What is explicitly NOT decided:** whether duplicate `order_index`
values within one `Project` are allowed or forbidden. No existing
canonical document resolves this either way - inventing a `UNIQUE
(project_id, order_index)` constraint (or explicitly deciding to allow
duplicates) here would be exactly the kind of silent implementation
invention this whole run exists to prevent. Recorded as an open question
in `LIFEOS_SPEC.md` §13 and `PROJECT_STATE.md` §10 - **must be resolved
as part of the `Milestone` vertical slice itself**, deliberately, not
before and not silently deferred past that slice either.

---

## 12. Deliberately deferred functionality

Explicitly **not** decided or implemented by this run (all correctly
out of scope, listed here for completeness so nothing is later assumed
to have been silently resolved):

- Automatic Project progress computed from Milestone/Task completion
  (`Project.progress` remains a manually-set value via `UpdateProgress`
  only).
- Weighted milestones.
- Task dependencies.
- Subtasks.
- Automatic scheduling.
- Recurrence implementation (`recurring_rule_id` remains an unmutated,
  future field - `UpdateTask` does not touch it, §5).
- Goal Intelligence reasoning of any kind (conflict detection, priority
  computation, auto-relation-creation from text).
- Auto-prioritization.
- AI-created tasks (the `Provenance::Inferred` confirmation mechanism,
  §7, is the guardrail for when that eventually exists - not a
  statement that it exists now).
- Personal Knowledge extraction.
- M3 UI behavior (exact confirmation-class UX per class remains an M3
  decision, unchanged).

---

## 13. Exact next implementation order

**Frozen: `Milestone` vertical slice first, then `Task`, then reassess
remaining M2 entities.**

**Reason:** `Milestone` is the smaller of the two - a single required
parent (`Project`), two frozen statuses, no recurrence field - and
proves the parent-child FK/action pattern (a required, `RESTRICT`-
enforced foreign key; a paired `Add*`/`Update*` action set) before
`Task`'s additional complexity (two *optional* parent references,
four statuses instead of two, a deferred recurrence field) is
introduced. This mirrors the same "prove the pattern on the simpler
case first" discipline already used for Goal → Project →
`GoalRelation`.

**Personal Knowledge Layer is explicitly not started this run** and is
not implied to be next after `Task` either - "then reassess remaining
M2 entities" is deliberate: the choice between `Task` → Personal
Knowledge vs. `Task` → `Challenge`/`Calendar`/etc. is not frozen by this
run and should be made consciously when `Milestone`/`Task` are actually
done, not assumed now.

---

## 14. Files changed

**New:**
- `docs/reports/REPORT_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md`
  (this report).

**Modified (documentation only):**
- `docs/LIFEOS_SPEC.md` - §5.1 (Project action-set gap marked resolved;
  new "Project child action set" block covering §3-§11 above; §13
  cross-references updated), §5.4 (frozen action list updated:
  `UpdateProject`/`UpdateMilestone`/`UpdateTask` added to the `Project`
  bullet).
- `docs/PROJECT_STATE.md` - new §4g (Project Child Action Freeze),
  renumbered §4h ("what's still frozen"); §1/§7/§8/§9/§10 updated for
  the resolved gap, the new open question, the commit list, and the
  frozen next-implementation-order.

**Explicitly NOT changed (verified via `git diff --stat` before
committing):** no file under `src-tauri/src/` or `src/`, no migration
file, `Cargo.toml`/`Cargo.lock`, `CLAUDE.md` (no operating-policy
change), any test file. Zero source-code behavior changed this run -
every decision above is a documentation freeze, to be implemented when
the `Milestone`/`Task` vertical slices actually happen.
