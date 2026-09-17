# REPORT — M2 Task Vertical Slice (2026-09-17)

Fifth real M2 domain entity, and the first entity with two independent,
both-nullable parent references. Implements `AddTask`/`UpdateTask` end
to end through `domain → actions → services → private
services::storage → encrypted SQLCipher`, reusing the pattern proven
across Goal, Project, GoalRelation, and Milestone. **Stops after
Task** — `RecurringRule`, `Calendar`, `Reminder`, `Challenge`, Personal
Knowledge, `ProgressUpdate`, `UpdateProject`, capability transport, M3
UI, and any LLM integration all remain out of scope for this run.

---

## 1. Pre-run state

- M1 accepted, unchanged.
- M2: `Goal`, `Project`, `GoalRelation`, `Milestone` all implemented and
  proven (see the Goal/Project/GoalRelation/Milestone vertical-slice and
  hardening reports).
- `REPORT_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md` had already
  frozen `UpdateTask` (floor `EXPLICIT_ADD`) and Task's ownership/FK/
  delete/creation-default policy ahead of implementation.
- 148/148 automated tests passing. Local checkpoint `05f02db`. Clean
  working tree. No push authorized this run either.

---

## 2. Frozen Task model

`src-tauri/src/domain/mod.rs`:

```rust
pub(crate) struct TaskId(pub Uuid);              // UUID v7, own newtype

pub(crate) enum TaskStatus { Todo, InProgress, Done, Cancelled }
// as_str(): "TODO"/"IN_PROGRESS"/"DONE"/"CANCELLED"; parse(): exact match only

pub(crate) struct RecurringRuleId(pub Uuid);     // reserved, see §3

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
```

Field set is EXACTLY the frozen model given in this run's brief:
`id`, `title`, `status`, `due_at`, `project_id`, `goal_id`,
`recurring_rule_id`, `created_at`, `updated_at`. Deliberately
**excludes** `description`/`priority`/`milestone_id`/`progress`/
`parent_task_id`/`tags`/a completion percentage/AI metadata/any other
hidden workflow field - none are in the frozen model. `milestone_id` in
particular is NOT inferred just because `Milestone` and `Task` are both
`Project` children (§24 of this run's brief) - if a real product
requirement needs tasks attached directly to milestones, that must be
frozen separately, not added here silently.

`created_at`/`updated_at` use the exact same UTC/RFC3339 convention
already established by Goal/Project/Milestone - no new timestamp policy
was invented. `created_at` never changes after creation; `updated_at`
changes only on a successful `UpdateTask` (proven, §12).

---

## 3. `recurring_rule_id` — explicit deferred design

The frozen Task model reserves `recurring_rule_id` (nullable, FUTURE),
but `RecurringRule` itself is **not implemented this run** - no `rrule`
crate, no RFC5545 parsing, no recurrence expansion, no scheduling, no
cloned future occurrences, no background jobs, no Calendar, no
Reminder, no `AddRecurringRule` action.

**Domain**: `RecurringRuleId` is defined now (same UUID v7 newtype
pattern as every other ID) purely as a typed placeholder for the future
entity - nothing in this codebase constructs a real one yet.

**Production schema** (`migrations/V5__create_tasks.sql`):
`recurring_rule_id TEXT CHECK (recurring_rule_id IS NULL)`. Because
there is currently no `recurring_rules` table and therefore no real FK
target, an unconstrained nullable `TEXT` column would let normal M2
production code silently write an arbitrary dangling value - this
`CHECK` makes the deferred state explicit and database-safe instead.
Proven directly: a raw SQL `INSERT` naming a non-`NULL`
`recurring_rule_id` is rejected by this `CHECK` (`req_t47`).

**Actions**: `AddTask` always creates `recurring_rule_id = None`
(never taken from the payload - `AddTaskPayload` has no such field).
`UpdateTask` does not expose `recurring_rule_id` at all -
`UpdateTaskPayload` has no such field either, so there is no code path
through which a caller could even attempt to set one.

**Future**: a later `RecurringRule`/Calendar migration may create the
real `recurring_rules` table and replace this temporary `CHECK` with a
real `REFERENCES recurring_rules(id)` foreign key, migrating/rebuilding
the `tasks` schema as needed. Recurrence does not exist today - nothing
in this codebase should be read as implying otherwise.

---

## 4. `TaskStatus`

Frozen, closed 4-value enum: `TODO`/`IN_PROGRESS`/`DONE`/`CANCELLED`.
No terminal-state rule - all four are freely reversible peer states
(matching the Project Child Action Freeze report's frozen policy and
the lesson from the original Goal hardening pass, applied proactively).
3 new pure round-trip unit tests in `domain::tests`
(`task_status_as_str_matches_frozen_db_text_values`,
`..._parse_round_trips_through_as_str_for_all_4`,
`..._parse_rejects_unknown_text`).

---

## 5. V5 migration

`src-tauri/migrations/V5__create_tasks.sql` creates `tasks`
(`id`, `title` NOT NULL with a non-empty-after-trim `CHECK`, `status`
NOT NULL with a 4-value `CHECK`, `due_at` nullable, `project_id`
nullable `REFERENCES projects(id) ON DELETE RESTRICT`, `goal_id`
nullable `REFERENCES goals(id) ON DELETE RESTRICT`,
`recurring_rule_id` nullable with the temporary NULL-only `CHECK` (§3),
`created_at`/`updated_at` NOT NULL) plus `task_audit_log` (same minimal
shape as every prior entity's audit table). Four single-column indexes
(`project_id`, `goal_id`, `due_at`, `status`) for the obvious reads this
slice actually implements - no speculative composite index. V1-V4 were
not edited.

---

## 6. Ownership semantics

`Task.project_id`/`Task.goal_id` are both independently nullable - a
Task may be standalone (both `None`), Project-only, Goal-only, or
linked to both. All four configurations are valid; neither parent is
required; neither is ever inferred from the other - a Task's Project
being itself linked to some Goal implies nothing about the Task's own
`goal_id`. Proven directly: `req_t4_5_6_7_8_9` creates a Task in all
four modes against the same Project/Goal pair (so a Task's
`project_id`-linked Project and `goal_id`-linked Goal have no
relationship to each other), and `req_t13_14_to_21`/`req_t22_23` prove
the full cross-parent patch matrix from the brief's §12: standalone →
Project-only → Project+Goal → Goal-only → standalone, both parents
cleared together in one update, and independent relinking of each
parent to a different valid target.

---

## 7. `AddTask`

Payload: `title: String, due_at: Option<DateTime<Utc>>, project_id:
Option<ProjectId>, goal_id: Option<GoalId>`. `status` and
`recurring_rule_id` are NOT part of the payload - the system always
sets `status = TODO`, `recurring_rule_id = None`, and stamps
`created_at`/`updated_at` to `meta.requested_at`. Confirmation floor
`EXPLICIT_ADD` (frozen).

Validation order (pure-first, matching every prior entity):

1. `validate_add_task` (pure): title trim/non-empty/`TITLE_MAX_LEN`. No
   extra `due_at` product rule - a typed `DateTime<Utc>` has nothing
   further this pure layer checks, same as `Goal.target_date`/
   `Milestone.target_date`.
2. If `project_id` is `Some`: `storage::project_repo::project_exists` -
   a real database read; `ValidationError::ReferencedProjectNotFound`
   if absent.
3. If `goal_id` is `Some`: `storage::goal_repo::goal_exists` -
   independently, a real database read; `ValidationError::
   ReferencedGoalNotFound` if absent. Neither check is ever skipped
   because the other ran - a Task's Project and Goal need not have any
   relationship to each other (§6).
4. Confirmation gate.
5. Insert `Task` row + `task_audit_log` row, one transaction.

`ReferencedProjectNotFound`/`ReferencedGoalNotFound` are reused directly
from `ValidationError` (already defined for `CreateProject`/
`AddMilestone`/`LinkGoals`) rather than adding Task-specific duplicate
variants - the semantic is identical.

---

## 8. `UpdateTask`

Payload: `id: TaskId, title: Option<String>, status: Option<TaskStatus>,
due_at: FieldPatch<DateTime<Utc>>, project_id: FieldPatch<ProjectId>,
goal_id: FieldPatch<GoalId>`. No `recurring_rule_id` field exists on
this payload at all (§3). Confirmation floor `EXPLICIT_ADD` - the
frozen floor, NOT `IMPORTANT_CHANGE` like `UpdateGoal`.

Validation order:

1. `storage::task_repo::get_task(id)` - `NotFound` if absent.
2. `validate_update_task` (pure): at least one field touched
   (`NoFieldsToUpdate` otherwise), `title` if `Some` validated the same
   way as create. No status-transition restriction.
3. If `project_id` is `FieldPatch::Set(x)`: `x` must reference a real
   Project - checked independently of `goal_id`. `Clear`/`Unchanged`
   need no existence check.
4. If `goal_id` is `FieldPatch::Set(x)`: `x` must reference a real Goal
   - same reasoning.
5. Confirmation gate.
6. Apply the patch onto the existing `Task`, update row + audit row,
   one transaction. `updated_at` is stamped to `meta.requested_at`;
   `created_at` is never touched by the repo's `UPDATE` statement at
   all (§12).

---

## 9. `FieldPatch` semantics

Applying the corrected canonical rule Milestone already established:
`title`/`status` are non-nullable `Task` fields → plain `Option<T>`
(`None` = unchanged, `Some(x)` = set). `due_at`/`project_id`/`goal_id`
are all genuinely nullable `Task` fields → `FieldPatch<T>`, so "leave
unchanged" and "explicitly clear to NULL" (e.g. unlinking a Task from
its Project) stay distinct states. Proven for all three `FieldPatch`
fields, both `Set` and `Clear`, against a real DB
(`req_t13_14_to_21`) - `Clear` re-verified via a fresh `get_task` read,
not just the in-memory returned value.

---

## 10. Parent reference validation

Both `AddTask`'s and `UpdateTask`'s `project_id`/`goal_id` checks are
independent - each is a real database read, reported through the same
typed `ValidationFailed` channel as structural checks, and neither is
ever skipped just because the other one ran or failed. Proven directly:
`req_t11`/`req_t12` (AddTask, missing Project/Goal, zero writes),
`req_t24_25` (UpdateTask relink to a nonexistent Project/Goal, zero
mutation). The real `tasks.project_id`/`tasks.goal_id REFERENCES ...
ON DELETE RESTRICT` constraints are independent defense in depth
(`PRAGMA foreign_keys = ON` inherited unchanged from the Project/
storage integrity hardening pass) - proven directly with raw-SQL
bypasses (`req_t43`/`req_t44`, §14).

> **Correction note (2026-09-17, M2 Task parent-validation hardening
> pass).** The claim above that "neither is ever skipped just because
> the other one ran or failed" was **inaccurate as originally written**.
> The original `services::add_task`/`services::update_task`
> implementation ran both checks independently *only when the first one
> passed* - each check returned immediately (`ActionOutcome::
> ValidationFailed(vec![single_error])`) the moment it found a missing
> reference, so when BOTH `project_id` and `goal_id` were invalid at
> once, the `goal_id` check never actually executed and its error was
> never reported; only `ReferencedProjectNotFound` came back. This never
> allowed an invalid write (`ReferencedProjectNotFound` alone was still
> a correct rejection with zero writes) - the defect was that the
> report's own claim was stronger than the implementation, not that
> invalid data could reach the database. Fixed in the dedicated
> hardening pass immediately following this report - see
> `REPORT_M2_TASK_PARENT_VALIDATION_HARDENING_2026-09-17.md` for the
> full before/after and new test evidence (`req_tpv_a` through
> `req_tpv_f`). Both checks now always run when both parents are
> supplied, and both errors are returned together in stable
> `ReferencedProjectNotFound`-then-`ReferencedGoalNotFound` order when
> both are invalid.

---

## 11. Confirmation behavior

Both actions: `effective_class(ConfirmationClass::ExplicitAdd,
meta.provenance)`. `Provenance::Explicit` → stays `EXPLICIT_ADD`,
executes immediately. `Provenance::Inferred` → becomes
`INFERRED_CHANGE`, blocks until confirmed, `ActionOutcome::
ConfirmationRequired` returned with **zero writes/mutation** - proven
for both actions (`req_t40_41`, `req_t42`), then a confirmed
resubmission (`confirmed: true`) persists (`req_t41`).

---

## 12. Timestamps

Task is the first entity after Milestone whose frozen model explicitly
contains both `created_at`/`updated_at`. Proven directly
(`req_t33_34_35`): a freshly created Task has `created_at == updated_at`;
a **rejected** `UpdateTask` (no fields touched) leaves both timestamps
byte-identical; a **successful** `UpdateTask` leaves `created_at`
unchanged while `updated_at` strictly advances. `req_t39` additionally
proves `updated_at` does not leak from a forced-audit-failure rollback -
the pre-update `Task` (timestamps included) is byte/logically identical
after the rollback. No separate Task timestamp policy was invented -
this is the same UTC/RFC3339 convention Goal/Project/Milestone already
use (Milestone has neither timestamp field at all, by contrast - see
that entity's own report §4).

---

## 13. Atomic mutation + audit

Both `add_task`/`update_task` use the same `&mut Connection` +
`conn.transaction()` + `CoreError`-on-any-failure shape as every prior
entity. Proven with real forced-failure SQLite triggers (`BEFORE INSERT
ON task_audit_log ... RAISE(FAIL, ...)`, installed only inside the
test, removed after): `req_t38` (AddTask) shows zero task rows survive
a forced audit failure; `req_t39` (UpdateTask) shows the exact
pre-update `Task` (including `updated_at`) survives unmutated, no
orphan audit row, and the connection remains usable afterward (a
following normal call succeeds in both tests).

---

## 14. Database defense-in-depth

All via raw SQL bypassing the normal action API entirely, kept local to
the test file (never exposed as production API):

- `req_t43` — nonexistent `project_id` rejected by the FK constraint.
- `req_t44` — nonexistent `goal_id` rejected by the FK constraint.
- `req_t45` — invalid `status` text (`'BLOCKED'`) rejected by
  `CHECK (status IN (...))`.
- `req_t46` — a whitespace-only title rejected by
  `CHECK (length(trim(title)) > 0)`.
- `req_t47` — a non-`NULL` `recurring_rule_id` rejected by the temporary
  `CHECK (recurring_rule_id IS NULL)` (§3) - proves the deferred
  RecurringRule design is genuinely enforced, not just documented.
- `req_t48` — a hard `DELETE FROM projects` against a Project with a
  live Task rejected by `ON DELETE RESTRICT`; the Project and the Task
  both remain afterward.
- `req_t49` — same as `req_t48`, for a referenced Goal.

---

## 15. V4 → V5 migration — non-destructive upgrade proof

`req_t3_v4_populated_database_upgrades_cleanly_to_v5`: builds a database
frozen at V4 only (`bootstrap_db_at_version(..., 4)`), populates it with
real `Goal`/`Project`/`GoalRelation`/`Milestone` rows - the actual shape
of an existing user's database before this update - then reopens it
normally (applying only the newly-pending V5 migration) and confirms:
all four prior entities' data is byte/logically unchanged; the
`tasks` table now exists; and new `AddTask` calls succeed in all four
ownership modes (standalone, Project-only, Goal-only, both) against the
pre-existing Project/Goal. Neither V1, V2, V3, nor V4 was edited.

`req_t2_fresh_v1_v2_v3_v4_v5_database_works_end_to_end` separately
proves a fresh database applies all five migrations in one run and
supports Goal, Project, GoalRelation, Milestone, and Task together.

---

## 16. Read side

`services::get_task(id)` (narrow internal read, same precedent as every
prior entity) and `services::get_tasks_for_project(project_id)`/
`get_tasks_for_goal(goal_id)`. Ordering is `due_at ASC, created_at ASC,
id ASC` - a **deterministic technical ordering, explicitly NOT product
prioritization** (per the brief's instruction): SQLite's default `ASC`
ordering sorts `NULL` due dates before any real timestamp (undated
tasks sort first), `created_at`/`id` are stable tie-breakers for rows
that still compare equal. No user-facing priority/urgency algorithm is
implemented. Proven by `req_t50_51`, which confirms each read returns
only Tasks actually referencing the queried parent (not, e.g., Tasks
referencing an unrelated Project/Goal).

---

## 17. Module privacy verification

`services/storage/task_repo.rs` follows the exact `pub(in
crate::services)` discipline as every prior repo module, registered as
`pub(super) mod task_repo;` in `services/storage/mod.rs`. Re-verified
with the same illegal-access probe used for every prior entity: a
temporary `services::storage::task_repo::get_task` reference added to
`lib.rs`, `cargo check` run, the real compiler rejection captured
(`error[E0603]: module 'storage' is private`), then the probe removed -
`git diff` shows zero net change to `lib.rs`.

---

## 18. Exact test/build results (MEASURED)

- `cargo fmt --check`: clean (after one `cargo fmt` pass fixed
  line-wrapping in new test/action code - no logic changed).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **194 passed; 0 failed; 0 ignored**
  (up from 148). Breakdown of the 46 new tests: 15 direct
  `actions::tests` unit tests (5 `AddTask`, 10 `UpdateTask`/patch-
  semantics/status), 3 direct `domain::tests` `TaskStatus` round-trip
  tests, 28 `services::tests` integration test functions covering all
  52 numbered requirements from this run's brief (several requirements
  share one test function, matching the `req_m10_11_12`-style precedent
  from the Milestone report). All prior 148 tests remain green,
  unmodified.
- `cargo build --release`: succeeded (`Finished \`release\` profile
  [optimized] target(s)`).
- `npm run build` (repo root): succeeded — `tsc && vite build`, 27
  modules transformed.
- `npm run tauri build`: succeeded end to end, verified by reading the
  captured log file directly (never the wrapper's reported exit code
  alone) — both installers built:
  `LifeOS_0.1.0_x64_en-US.msi` and `LifeOS_0.1.0_x64-setup.exe`, log
  ends `EXITCODE:0`.

---

## 19. Files changed

New:
- `src-tauri/migrations/V5__create_tasks.sql`
- `src-tauri/src/services/storage/task_repo.rs`
- `docs/reports/REPORT_M2_TASK_VERTICAL_SLICE_2026-09-17.md` (this file)

Modified:
- `docs/PROJECT_STATE.md` — full Task implementation status
- `docs/LIFEOS_SPEC.md` — implementation-status/deferred-recurrence
  wording only, where it actually changed
- `src-tauri/src/domain/mod.rs` — `TaskId`, `TaskStatus`,
  `RecurringRuleId`, `Task`
- `src-tauri/src/domain/tests.rs` — 3 `TaskStatus` unit tests
- `src-tauri/src/actions/mod.rs` — `AddTaskPayload`,
  `UpdateTaskPayload`, `validate_add_task`, `validate_update_task`
  (no new `ValidationError` variants - `ReferencedProjectNotFound`/
  `ReferencedGoalNotFound` reused directly)
- `src-tauri/src/actions/tests.rs` — 15 direct unit tests
- `src-tauri/src/services/mod.rs` — `add_task`, `update_task`,
  `get_task`, `get_tasks_for_project`, `get_tasks_for_goal`
- `src-tauri/src/services/tests.rs` — 28 integration test functions
- `src-tauri/src/services/storage/mod.rs` — `pub(super) mod task_repo;`,
  doc comment updated

`src-tauri/src/lib.rs` was temporarily modified for the illegal-access
probe (§17) and restored to its exact prior state before this run ends
— `git diff` shows no net change to that file.

---

## 20. Remaining risks / open questions

- Same pre-existing, unchanged gaps carried forward from prior reports:
  no Tauri command layer wires any of Goal/Project/GoalRelation/
  Milestone/Task into the running app yet; `due_at` has no content
  validation beyond being a well-formed `DateTime<Utc>` (same as every
  other entity's date field, not a new gap); `CoreError`'s `Debug` impl
  still includes the internal diagnostic string.
- **New**: `Task` has no read capability beyond
  `get_task`/`get_tasks_for_project`/`get_tasks_for_goal` - no
  aggregate "all tasks due before X across every Project" query, no
  standalone-task listing (a Task with both `project_id`/`goal_id` =
  `None` is only reachable via `get_task(id)` today). Deliberately out
  of scope - add only when a real caller needs it.
- **New**: `get_tasks_for_project`/`get_tasks_for_goal`'s `due_at ASC`
  ordering is a deliberate, documented *technical* default, not a
  product prioritization decision - if/when a real "what should I work
  on next" feature is designed, it should not assume this ordering is
  the intended UX; that is a separate, future product decision.
- `recurring_rule_id`'s temporary `CHECK (recurring_rule_id IS NULL)`
  constraint will need a real migration to lift once `RecurringRule` is
  actually designed and built - flagged explicitly, not left implicit
  (§3).
- The next safest M2 slice needs an explicit decision from the user/
  next run's brief - see §21.

---

## 21. Recommended next M2 step

With `Goal`, `Project`, `GoalRelation`, `Milestone`, and `Task` all now
implemented, every action in the frozen v0.1 action set that has a
concrete, ready-to-build entity is done except `UpdateProject` (frozen,
`IMPORTANT_CHANGE`, ready-to-implement but explicitly out of scope this
run) and `AddTask`'s sibling `RecurringRule` work (explicitly deferred,
§3). Two reasonable next steps, in order of how "shovel-ready" they
are:

1. **`UpdateProject`** - the action set/patch semantics/confirmation
   floor are already fully frozen (`LIFEOS_SPEC.md` §5.1/§5.4); this is
   the last remaining frozen-but-unimplemented action on an entity that
   already exists in full, and closes out the M2 core CRUD surface for
   Goal/Project.
2. **Personal Knowledge Layer** (or another net-new entity) - a larger,
   less "shovel-ready" step; needs its own dedicated planning pass
   before implementation the way Milestone/Task both got a Project
   Child Action Freeze pass first.

`RecurringRule`/`Calendar`/`Reminder`/`Challenge`/`ProgressUpdate`
remain explicitly out of scope until a dedicated freeze/design pass
happens for each, per this run's own instructions and the precedent
already set by the Project Child Action Freeze report.
