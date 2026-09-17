# REPORT — M2 Project Vertical Slice

**Date:** 2026-09-17
**Scope:** Implement the second real M2 domain entity, `Project`, reusing
the hardened `Goal` pattern exactly (`docs/reports/REPORT_M2_GOAL_HARDENING_2026-09-17.md`).
Explicitly **not** in scope this run: `GoalRelation`, `Milestone`, `Task`,
`Calendar`, `Challenge`, Personal Knowledge/Fact/Memory, a `capability/`
transport, M3 UI, Qwen, GPT/Luna.

---

## 1. Pre-run state

M0 done; M1 accepted and closed; M2 architecture frozen; SQLCipher
storage proven and hardened; `Goal` vertical slice implemented and then
hardened (atomic mutation+audit, typed `CoreError`, `FieldPatch`
tri-state patch semantics, no invented terminal-status rule, direct
action-layer unit tests, `Provenance::Inferred` proven end-to-end); 42
tests passing. Full detail: `docs/reports/REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`,
`docs/reports/REPORT_M2_GOAL_HARDENING_2026-09-17.md`.

Two pre-existing documentation inconsistencies were corrected **before**
any implementation, per this run's explicit instruction:

**A. `LIFEOS_SPEC.md`'s M2 status text** still said "schema/action
implementation not yet started" (in the §12 milestone list) despite Goal
already being implemented and hardened - stale from before the Goal
work. Corrected to state storage proven/hardened, Goal implemented and
hardened, and (once this run's work landed) Project implemented too.

**B. `PROJECT_STATE.md` §9** suggested the Project slice might implement
"`CreateProject`/`UpdateProject`" - but the frozen M2 action set
(architecture-freeze report §C) is `CreateProject`, `AddMilestone`,
`AddTask`, `UpdateProgress` - there is no frozen `UpdateProject` action
at all. Corrected before implementation began, and this run implements
`CreateProject` + `UpdateProgress` only, exactly as the corrected text
specifies.

---

## 2. Frozen Project model used

Architecture-freeze report §C, the one-line entity table (verbatim):

> **Project** | `id`, `title`, `description`, `status` (same 4 values as
> Goal), `priority`, `goal_id` (nullable), `created_at`, `updated_at`

Every field in `domain::Project` traces to this row:

- `id` → `ProjectId` (own newtype, UUID v7 - not `GoalId` reused; see §3
  for why a distinct type is not the kind of duplication this run was
  told to avoid).
- `title`, `description` → same shape as `Goal`'s (required non-empty
  trimmed string; optional string), reusing `TITLE_MAX_LEN`/
  `DESCRIPTION_MAX_LEN`.
- `status` → reuses `domain::GoalStatus` directly. The freeze's own
  words - "same 4 values as Goal" - are read as license to reuse the
  type, not just the value set, per this run's explicit "do not
  duplicate equivalent primitive/domain types unnecessarily" and "do
  NOT create `ProjectStatus` unless the frozen model actually defines
  one" instructions. No terminal-state rule applies (per the Goal
  hardening correction - `GoalStatus` has none).
- `priority` → reuses the shared `Priority`/`PRIORITY_MIN`/
  `PRIORITY_MAX` type/constants, singular (unlike Goal's
  `priority_user`/`priority_system` split - the freeze lists only one
  `priority` field for Project).
- `goal_id` → `Option<GoalId>`, the first real cross-entity relationship
  in production M2.
- `created_at`/`updated_at` → `DateTime<Utc>`, same as Goal.

---

## 3. Ambiguity discovered before implementation

**The frozen one-line Project field table does not list `progress`.**
Goal's own row explicitly lists `progress`; Project's does not. But the
frozen M2 action set includes `UpdateProgress` for Project, which needs
something concrete to mutate, and the architecture-freeze report also
defines a separate, more general entity:

> **ProgressUpdate** | `id`, `subject_type` (`GOAL`/`PROJECT`/`TASK`),
> `subject_id`, `value`, `note` (nullable), `recorded_at`

This is a plausible alternative reading: maybe `UpdateProgress` is meant
to *create a `ProgressUpdate` log row* referencing the Project, rather
than mutate a field on `Project` itself - the same way Goal's own
`progress` field is a direct field, not a log.

**Decision made, documented here rather than silently picked:** added
`progress: Progress` as a direct field on `domain::Project`, for three
reasons:

1. This run's own instructions describe `UpdateProgress` operationally
   in terms that only make sense for a direct field - "validate progress
   against the existing canonical 0..100 semantics," "update
   `updated_at`" (singular, on the entity being updated) - not "create a
   new `ProgressUpdate` row."
2. `Goal`'s own precedent (proven, hardened, shipped) stores `progress`
   directly on the entity and mutates it in place via its own action.
   Consistency with the just-hardened template was the explicit goal of
   this run.
3. `ProgressUpdate` is a separate, unimplemented, polymorphic, log-
   shaped entity - implementing it (even partially) would have pulled in
   more than "one entity, fully proven," and this run's STOP list
   explicitly excludes anything resembling a generic event-sourcing
   mechanism (echoing the Goal hardening report's own restraint around
   `goal_audit_log`).

This is flagged as an **open question**, not a closed decision:
`PROJECT_STATE.md` §10 records it explicitly, so a future run correcting
`LIFEOS_SPEC.md` to resolve it (either freezing `progress` on `Project`
explicitly, or redirecting `UpdateProgress` to write `ProgressUpdate`
instead) is a deliberate, visible choice, not something this run's own
interpretation quietly locks in unnoticed. (Corroborating, non-
authoritative context: `LIFEOS_SPEC.md` §9.7's M3 UI mockup for Projects
shows a per-project progress bar, consistent with a direct field - not
used as the source of the schema decision, per this run's explicit "do
not infer missing fields from UI mockups" instruction, but noted as it
does not contradict the decision made above.)

**Confirmation floors were NOT ambiguous** - the architecture-freeze
report §F confirmation table explicitly lists both actions:

> `EXPLICIT_ADD` | ..., `CreateProject`, ..., `UpdateProgress`, ...

Both floors are `EXPLICIT_ADD`. `UpdateProgress` being `EXPLICIT_ADD`
rather than `IMPORTANT_CHANGE` (unlike `UpdateGoal`) is a real, deliberate,
frozen difference - not derived, not invented, read directly off the
table and implemented as-is.

---

## 4. V2 migration

`src-tauri/migrations/V2__create_projects.sql` - `V1__create_goals.sql`
was **not** edited.

```sql
CREATE TABLE projects (
    id, title, description, status, priority,
    goal_id TEXT REFERENCES goals (id),
    progress, created_at, updated_at
);
CREATE INDEX idx_projects_goal_id ON projects (goal_id);
CREATE INDEX idx_projects_status ON projects (status);
CREATE INDEX idx_projects_priority ON projects (priority);
CREATE INDEX idx_projects_updated_at ON projects (updated_at);

CREATE TABLE project_audit_log (
    id, project_id TEXT NOT NULL REFERENCES projects (id),
    action_type CHECK IN ('CreateProject','UpdateProgress'),
    actor, provenance, confirmation_class, previous_snapshot, occurred_at
);
CREATE INDEX idx_project_audit_log_project_id ON project_audit_log (project_id);
```

(Full file has the real `CHECK`/`NOT NULL` constraints - abbreviated
here for readability.) No `milestones`/`tasks`/`goal_relations`/calendar
tables. Indexes match exactly the frozen fields the brief named as
"obvious" candidates (`goal_id`, `status`, `priority`, `updated_at`) - no
speculative index was added.

**FK behavior - no cascade rule invented.** `goal_id REFERENCES
goals(id)` with no `ON DELETE`/`ON UPDATE` clause - SQLite's plain
default behavior, since no cascade rule is frozen or documented anywhere
in the architecture. `PRAGMA foreign_keys` is not set anywhere in this
codebase (unchanged from Goal-only behavior), so this constraint is not
even enforced at runtime today - it documents intent and would activate
as defense-in-depth automatically if that pragma is ever turned on. The
actual, primary enforcement is application-level (§5/§6 below).

---

## 5. Project → Goal FK design

`storage::project_repo::goal_exists(conn, goal_id)` - a plain `SELECT
count(*) FROM goals WHERE id = ?1`, called from `services::create_project`
**before** the transaction begins, whenever `payload.goal_id.is_some()`.
If it returns `false`, `create_project` returns
`ActionOutcome::ValidationFailed(vec![ValidationError::ReferencedGoalNotFound])`
immediately - no SQL `INSERT` is attempted, so there is no foreign-key
failure to catch in the first place. This directly satisfies the
instruction "do not rely only on the SQL FK failure" - the check happens
in Rust, before any write, and reports through the same deterministic,
typed channel every other structural validation error uses.

---

## 6. CreateProject action

`actions::validate_create_project` (pure, no I/O): title trim/non-empty/
`TITLE_MAX_LEN`, description ≤ `DESCRIPTION_MAX_LEN`, priority within
`PRIORITY_MIN..=PRIORITY_MAX` if supplied - mirrors
`validate_create_goal` exactly. The `goal_id` reference check is
deliberately **not** here (needs a DB read); it lives in
`services::create_project`.

`services::create_project(conn: &mut Connection, meta, payload)`:
validate → check `goal_id` reference if present → classify confirmation
→ block if `Inferred` and unconfirmed → build `Project` (status
`Active`, `progress: 0`) → open transaction → insert project → insert
audit row → commit. Any failure at any write step returns a typed
`ActionOutcome::StorageError(CoreError)` and the transaction rolls back
(§9).

---

## 7. UpdateProgress action

`actions::validate_update_progress` (pure): only checks
`progress` is within `PROGRESS_MIN..=PROGRESS_MAX` - the frozen action
has nothing else to validate structurally (it is deliberately narrow:
one field, one purpose, no generic `UpdateProject`).

`services::update_progress(conn: &mut Connection, meta, payload)`:
fetch existing Project (→ `NotFound` if missing) → validate → classify
confirmation → block if needed → snapshot previous state as JSON →
mutate `progress`/`updated_at` → open transaction → update → insert
audit row → commit. Same atomic shape as `update_goal`.

---

## 8. Confirmation classification

Both actions' floor is `ConfirmationClass::ExplicitAdd`
(`effective_class(ExplicitAdd, meta.provenance)`), taken directly from
the architecture-freeze report §F table (§3 above) - not derived from
"the already-frozen generic policy" as a fallback, since the table
already states it explicitly for both actions. `UpdateProgress` being
`EXPLICIT_ADD` rather than `IMPORTANT_CHANGE` is called out with an
in-code comment at both the `services::mod` function and its test, so it
is not mistaken for a bug and "fixed" to match `UpdateGoal` later
without `LIFEOS_SPEC.md` changing first.

`blocks_until_confirmed` (renamed from `goal_blocks_until_confirmed` -
§11) makes `Provenance::Inferred` block both actions exactly like Goal's
- proven end-to-end for `CreateProject` (`req_p16`).

---

## 9. Atomic Project mutation + audit design

Identical shape to the hardened Goal actions (report §2): `conn: &mut
Connection`, `let tx = conn.transaction()?`, both the domain write
(`insert_project`/`update_project_progress`) and the audit write
(`insert_audit_entry`) go through `&tx`, and `tx.commit()` only runs if
both succeeded. Any `Err` at any step returns immediately without
committing - `tx`'s `Drop` impl issues the `ROLLBACK`. No
`let _ = insert_audit_entry(...)` anywhere.

---

## 10. Forced audit failure rollback evidence

Same mechanism as the Goal hardening report §3, retargeted at
`project_audit_log`:

```sql
CREATE TRIGGER force_project_audit_failure
BEFORE INSERT ON project_audit_log
BEGIN
    SELECT RAISE(FAIL, 'forced project audit failure');
END;
```

**`req_p13_create_project_audit_failure_rolls_back_project_insert`**:
installs the trigger, calls `create_project`, asserts a typed
`StorageError` with `kind() == CoreErrorKind::Storage`, asserts
`SELECT count(*) FROM projects` is `0` and `project_audit_log` is `0`,
removes the trigger, then calls `create_project` again to prove the
connection remains usable after the rollback.

**`req_p14_update_progress_audit_failure_rolls_back_project_update`**:
creates a project (trigger not yet installed), snapshots it via
`get_project`, installs the trigger, attempts
`update_progress(progress: 77)`, asserts a typed `StorageError`, removes
the trigger, re-fetches and asserts the project is byte/logically
identical (`PartialEq`) to the pre-update snapshot, and asserts exactly
1 audit row exists (the original `CreateProject`'s - the failed
`UpdateProgress`'s attempted row did not survive).

Both pass - see §16.

---

## 11. Read side

Only `services::get_project(conn: &Connection, id: ProjectId)` was
added - a narrow internal read, same reasoning as `get_goal`. The frozen
capability API's minimum function set (architecture-freeze report §I -
`get_current_situation`, `get_active_goals`, `get_today_plan`,
`get_relevant_memory`, `create_goal`, `update_goal`, `create_challenge`,
`record_solution_attempt`, `record_outcome`, `create_calendar_event`,
`change_priority`, `log_wellbeing`, `get_progress`) does **not** include
any Project-specific read - no `get_projects()`, no
`get_active_projects()`, only a generic, unspecified `get_progress()`.
Per this run's explicit "do not invent a large repository API"
instruction, no list/read capability was added beyond what tests and
`update_progress` actually need.

---

## 12. Direct action tests

`actions/tests.rs`, 10 new pure tests (no DB, no I/O), mirroring the
Goal action tests exactly:

- `create_project_title_is_trimmed`
- `create_project_rejects_empty_title`
- `create_project_rejects_title_too_long`
- `create_project_rejects_priority_out_of_bounds`
- `create_project_rejects_description_too_long`
- `create_project_confirmation_floor_is_explicit_add_for_explicit_provenance`
- `create_project_inferred_provenance_becomes_inferred_change`
- `update_progress_accepts_in_bounds_value`
- `update_progress_rejects_out_of_bounds_value`
- `update_progress_confirmation_floor_is_explicit_add`

The `goal_id` reference check is deliberately **not** tested here (it
needs I/O) - see `req_p6` in §13.

---

## 13. DB integration tests

`services/tests.rs`, 14 new tests against a real encrypted, migrated
database - exactly the 17 required scenarios, several combined where
they exercise the same setup (noted below), plus requirement 17 (Goal
regression) verified by the full suite, not a dedicated test function:

| # | Scenario | Test |
|---|---|---|
| 1 | V2 migration creates Project schema | `req_p1_v2_migration_creates_project_schema` |
| 2, 14 | Existing Goal data survives V2 migration / V1→V2 upgrade path | `req_p2_v1_goal_database_upgrades_cleanly_to_v2_project` (§14) |
| 3 | Project without `goal_id` can be created | `req_p3_project_without_goal_id_can_be_created` |
| 4, 5, 7 | Project with valid GoalId created, link survives reopen, persists after reopen | `req_p4_5_7_project_with_valid_goal_id_persists_and_survives_reopen` |
| 6 | Nonexistent GoalId rejected before write | `req_p6_nonexistent_goal_id_rejected_before_write` |
| 8 | Invalid Project input makes zero writes | `req_p8_invalid_project_input_makes_zero_writes` |
| 9 | UpdateProgress succeeds | `req_p9_update_progress_succeeds` |
| 10 | Progress <0 or >100 rejected | `req_p10_progress_out_of_bounds_is_rejected` (see note below) |
| 11 | UpdateProgress missing Project → NotFound | `req_p11_update_progress_missing_project_returns_not_found` |
| 12 | Invalid UpdateProgress causes zero mutation | `req_p12_invalid_update_progress_causes_zero_mutation` |
| 13 | CreateProject + audit atomic under forced failure | `req_p13_create_project_audit_failure_rolls_back_project_insert` (§10) |
| 14 | UpdateProgress + audit atomic under forced failure | `req_p14_update_progress_audit_failure_rolls_back_project_update` (§10) |
| 15 | Audit rows contain actor/provenance/confirmation metadata | `req_p15_project_audit_rows_contain_required_metadata` |
| 16 | Inferred CreateProject blocks before write | `req_p16_inferred_create_project_requires_confirmation_and_does_not_write` |
| 17 | Existing 42 Goal/storage tests remain green | verified by the full suite (§16) - not a dedicated test |

**Note on requirement 10:** `Progress` is `u8` (0..=255 representable),
so a negative value cannot even be *constructed* through this API - the
"<0 is rejected" half of the requirement is a **stronger, type-level**
guarantee than a runtime check, not merely untested. Only the ">100"
half (`progress: 101`) is a runtime validation check, and is what the
test exercises. Stated explicitly here per this repo's "never overclaim"
policy.

---

## 14. Migration V1 → V2 compatibility test

`req_p2_v1_goal_database_upgrades_cleanly_to_v2_project` simulates a
real existing user's database, not just a fresh one that gets every
migration at once:

1. A new test-only helper, `storage::run_production_migrations_to_version`
   (uses `refinery::Target::Version(n)` via `Runner::set_target`,
   confirmed present in `refinery-core` 0.9.2's public API before use)
   and `services::bootstrap_db_at_version` (both `#[cfg(test)]`), builds
   a database frozen at **V1 only** - `goals` exists, `projects` does
   not (asserted).
2. A real `Goal` is created against that V1-only database via the normal
   `create_goal` action.
3. The connection is dropped and the database is **reopened normally**
   (`bootstrap_db_at`, no version cap) - this applies only the one
   newly-pending migration, V2, exactly as a real app update would.
4. Asserts: the Goal's title is unchanged, the `projects` table now
   exists, and a new `Project` can be created referencing that
   pre-existing Goal's id, succeeding and returning the correct
   `goal_id`.

`V1__create_goals.sql` was not touched to make this easier - the test
proves the real migration path works against the real V1 file as
committed.

---

## 15. Module privacy proof

After adding `services::storage::project_repo`, the deliberate
compile-fail probe was re-run (not assumed to still hold just because
the shape matches `goal_repo`'s):

```rust
#[allow(dead_code)]
fn illegal_boundary_probe() {
    let _ = services::storage::project_repo::insert_project;
}
```

added temporarily to `lib.rs`, `cargo check` run:

```
error[E0603]: module `storage` is private
```

confirmed, probe removed, `cargo check --tests` re-run clean (§16).

**DRY note, not a boundary change:** `goal_repo.rs`'s
`actor_str`/`provenance_str`/`confirmation_class_str`/`parse_rfc3339`
helpers were promoted to `storage/mod.rs` as `pub(super)` (visible
throughout `crate::services`, which includes both repo modules as
descendants) rather than duplicated into `project_repo.rs` - this is a
refactor of already-hardened Goal code, done because the brief's own
"do not duplicate" instruction applies as much to these helpers as to
domain types. The Goal regression suite (§16) re-proves this refactor
did not change behavior. Separately, `actions::goal_blocks_until_confirmed`
was renamed to `blocks_until_confirmed` - its logic was already generic
across `ConfirmationClass` values, just under a Goal-specific name; two
Goal call sites in `services::create_goal`/`update_goal` were updated to
match.

---

## 16. Exact build/test results

All commands run from `D:\Dev\LiFeOS\src-tauri` unless noted, PowerShell,
`cargo 1.98.1`.

| Command | Result |
|---|---|
| `cargo fmt --check` | Initially FAILED (pure formatting diffs - import ordering, line wrapping - across `actions/mod.rs`, `actions/tests.rs`, `services/tests.rs`, no logic changes); `cargo fmt` applied, re-ran `cargo fmt --check` → **PASS** |
| `cargo check --tests` | **PASS** - `Finished \`dev\` profile [unoptimized + debuginfo] target(s)`, zero warnings (confirmed via a warning/error-only filtered rerun) |
| `cargo test -- --test-threads=1` | **PASS** - `running 66 tests` / `test result: ok. 66 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`. Before this run: 42 (9 spike + 17 Goal integration + 16 Goal action-layer). After: 66 = 42 + 24 new (10 Project action-layer + 14 Project integration). All 42 original tests still pass unmodified - the Goal regression requirement (§13, requirement 17) is satisfied. |
| `cargo build --release` | **PASS** - `Finished \`release\` profile [optimized] target(s) in 2m 48s` |
| `npm.cmd run build` (repo root) | **PASS** - `tsc && vite build` succeeded, `✓ 27 modules transformed`, `✓ built in 148ms`. No frontend files were touched this run. |
| `npm.cmd run tauri build` | **PASS** - run in the background, verified genuinely by reading the captured log file directly (not trusting the background wrapper's reported exit code, per this repo's standing lesson): the log's own `$LASTEXITCODE` line reads `EXIT_CODE=0`. `Finished \`release\` profile [optimized] target(s) in 2m 34s`, `Finished 2 bundles at: ...msi\LifeOS_0.1.0_x64_en-US.msi` and `...nsis\LifeOS_0.1.0_x64-setup.exe`. Both files independently confirmed present on disk: `LifeOS_0.1.0_x64_en-US.msi` (2,973,696 bytes) and `LifeOS_0.1.0_x64-setup.exe` (1,371,154 bytes), timestamps matching the build run. Justified this run (not skipped like the hardening run) because a real migration/domain/backend slice (V2 migration, `project_repo`, two new `services` functions) was added. |
| Module-boundary probe | Re-run for the new `project_repo` (§15) → `error[E0603]: module 'storage' is private`, confirmed, probe removed, re-verified clean. |

---

## 17. Files changed

**New:**
- `src-tauri/migrations/V2__create_projects.sql`
- `src-tauri/src/services/storage/project_repo.rs`
- `docs/reports/REPORT_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`

**Modified:**
- `src-tauri/src/domain/mod.rs` - added `ProjectId`, `Project`.
- `src-tauri/src/actions/mod.rs` - added `CreateProjectPayload`,
  `UpdateProgressPayload`, `validate_create_project`,
  `validate_update_progress`, `ValidationError::ReferencedGoalNotFound`;
  renamed `goal_blocks_until_confirmed` → `blocks_until_confirmed`.
- `src-tauri/src/actions/tests.rs` - 10 new Project unit tests; updated
  2 renamed-function call sites.
- `src-tauri/src/services/mod.rs` - added `create_project`,
  `update_progress`, `get_project`; added `#[cfg(test)]`
  `bootstrap_db_at_version`; updated 2 renamed-function call sites.
- `src-tauri/src/services/tests.rs` - 14 new Project integration tests
  and their helpers.
- `src-tauri/src/services/storage/mod.rs` - added
  `pub(super) mod project_repo;`; promoted `parse_rfc3339`/
  `actor_str`/`provenance_str`/`confirmation_class_str` from
  `goal_repo.rs`; added `#[cfg(test)] run_production_migrations_to_version`.
- `src-tauri/src/services/storage/goal_repo.rs` - removed its own copies
  of the four promoted helpers, now calls `super::*` versions (behavior
  unchanged, re-proven by the Goal regression suite).
- `src-tauri/src/lib.rs` - illegal-boundary probe added and removed
  (§15), net diff is none.
- `docs/LIFEOS_SPEC.md` - status banner, §5, §5.1, §12 milestone entry
  (implementation-status text updated for Project); §1A's stale
  "schema/action implementation not yet started" phrase corrected.
- `docs/PROJECT_STATE.md` - §9's `UpdateProject` wording corrected
  (§1B); new §4d (Project vertical slice), renumbered §4e; file tree,
  test counts, commit list, known risks updated throughout.

No changes to: `Cargo.toml`/`Cargo.lock` (no dependency changes),
`src-tauri/migrations/V1__create_goals.sql` (untouched, historical),
`services/storage/spike/*` (untouched, still passing as regression), any
file under `src/` (React frontend, untouched), `scripts/setup-windows-storage-env.ps1`
(untouched), `CLAUDE.md` (no operating-policy change this run).

---

## 18. Remaining risks / open questions

- **`Project.progress` is an interpretation, not a literal frozen
  field** - see §3. The single most important open question from this
  run; revisit if `LIFEOS_SPEC.md` is ever updated to resolve it
  explicitly (either freezing it on `Project`, or redirecting to a
  future `ProgressUpdate` implementation).
- **Neither Goal nor Project is wired into the running app** - unchanged,
  no Tauri command layer exists yet.
- **`projects.goal_id`'s FK is declared but not enforced at runtime** -
  `PRAGMA foreign_keys` is not set; the real enforcement is
  `services::create_project`'s application-level check (§5). Deliberate,
  documented, not an oversight - but worth remembering if a future
  direct-SQL debugging session assumes the constraint is live.
  (Consistent with Goal-only behavior - no new gap introduced.)
- **Project's frozen `AddMilestone`/`AddTask` actions remain
  unimplemented** - they need `Milestone`/`Task`, both out of scope this
  run.
- **`domain::GoalStatus` is now shared by two entities under a
  Goal-specific name** - flagged (not fixed) in the Goal hardening
  report already; still true, now with one more caller. A future rename
  to something entity-neutral (e.g. `LifecycleStatus`) remains an
  explicit, undone candidate, not touched this run to avoid an
  unrequested, unrelated refactor of already-hardened Goal call sites.
- **Everything already listed as a risk in the Goal hardening report**
  (`target_date` content validation, `CoreError`'s `Debug` exposing
  `diagnostic`, clean-machine runtime inferred from `dumpbin`, exact
  NeXa transport/confirmation UX) is unchanged by this run.

---

## 19. Exact recommended next M2 slice

A third domain entity, or `GoalRelation` (the first cross-entity
relationship *graph*, distinct from the direct `Project.goal_id`/future
`Task.project_id`-style foreign keys already proven this run and last).
Either way, reuse `domain → actions → services →
services::storage::<entity>_repo` with the atomicity/typed-error
discipline applied **from the start** (as this run did for Project, not
retrofitted afterward as Goal needed) - this run required zero
transaction/error-model rework specifically because it started from the
already-hardened template. Do not implement `Milestone`/`Task` in the
same pass as whichever entity is chosen next, and do not silently invent
product rules inside the implementation - this run's own §3 shows the
right way to handle a genuine ambiguity: make a reasoned, minimal-scope
call, and flag it plainly rather than either guessing silently or
blocking on it.
