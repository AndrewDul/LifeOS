# REPORT — M2 UpdateProject Implementation (2026-09-17)

Implements `UpdateProject`, the last already-frozen, shovel-ready action
on an existing M2 core entity — closing the Project action-set gap
first identified in the M2 Project/storage integrity hardening report
and formally frozen (shape, floor, semantics) by the M2 Project Child
Action Freeze report. **Stops after UpdateProject** — `RecurringRule`,
Calendar, Reminder, Challenge, Personal Knowledge, the `ProgressUpdate`
entity/history, capability transport, M3 UI, and any LLM integration
all remain out of scope for this run.

---

## 1. Pre-run state

- M1 accepted, unchanged.
- M2: `Goal`, `Project`, `GoalRelation`, `Milestone`, `Task` all
  implemented (Task additionally parent-validation-hardened). 200/200
  tests passing. Local checkpoint `3cebe82`. Clean working tree. No
  push authorized this run either.
- `Project` had `CreateProject`/`UpdateProgress` only — no generic
  update path for `title`/`description`/`status`/`priority`/`goal_id`
  existed in code, though the shape, floor, and semantics for
  `UpdateProject` were already fully frozen (M2 Project Child Action
  Freeze report).

---

## 2. Frozen UpdateProject decision

`UpdateProject` (floor `IMPORTANT_CHANGE`) was frozen, not invented,
this run — the M2 Project Child Action Freeze report already specified
the exact payload shape and patch semantics. This run implements
exactly that frozen shape, with no design changes.

---

## 3. Exact payload

```rust
pub(crate) struct UpdateProjectPayload {
    pub id: ProjectId,
    pub title: Option<String>,
    pub description: FieldPatch<String>,
    pub status: Option<LifecycleStatus>,
    pub priority: FieldPatch<Priority>,
    pub goal_id: FieldPatch<GoalId>,
}
```

`title`/`status` (non-nullable `Project` fields) use plain `Option<T>`;
`description`/`priority`/`goal_id` (all genuinely nullable) use
`FieldPatch<T>`. No `Clear` state exists for `title`/`status` — matches
the corrected canonical rule already established by Milestone/Task.

### 3a. Unplanned V6 migration

The brief's assumption that "the Project schema already contains every
field UpdateProject mutates" was correct for the `projects` table
itself, but review of `migrations/V2__create_projects.sql` found
`project_audit_log.action_type CHECK (action_type IN ('CreateProject',
'UpdateProgress'))` — a constraint scoped to exactly the two actions
that existed when V2 was written, which does **not** include
`'UpdateProject'`. Every real `UpdateProject` call would fail its audit
insert with a `CHECK constraint failed` `StorageError`, confirmed
directly by 9 failing integration tests before this was fixed.

This is a genuine conflict between two of the brief's own requirements
("implement a working, atomically-audited UpdateProject" vs. "no
migration, V1-V5 untouched") that could not be resolved by guessing.
**Flagged to the user directly; the user chose to add a minimal V6
migration** rather than leave `UpdateProject` non-functional or
mislabel its audit rows under a different `action_type`.

`migrations/V6__widen_project_audit_log_action_types.sql` uses SQLite's
standard rebuild pattern (SQLite has no `ALTER TABLE ... MODIFY
CONSTRAINT`): create `project_audit_log_new` with the corrected
`CHECK (... IN ('CreateProject', 'UpdateProgress', 'UpdateProject'))`,
copy every existing row across unchanged, drop the old table, rename
the new one into place, recreate its one index. No column added,
removed, or renamed; `projects` itself is completely untouched; V1-V5
remain historical and unedited. Proven non-destructive with a dedicated
migration-compatibility test (§15/§14 below).

---

## 4. Why progress is excluded

`Project.progress` is **not** part of `UpdateProjectPayload` and is
never touched by `update_project`. `UpdateProgress` remains its
exclusive owner — its own payload, its own `EXPLICIT_ADD` floor, its
own repository function (`project_repo::update_project_progress`,
unchanged). The two actions are never merged, and progress is never
derived from Task/Milestone status or anything else. Proven directly
(`req_up23_24`): setting `progress = 63` via `UpdateProgress`, then
running `UpdateProject` changing `title`/`status`/`priority`, leaves
`progress` at exactly `63`; a subsequent `UpdateProgress` call still
works independently and does not revert `UpdateProject`'s own changes.

---

## 5. FieldPatch semantics

Proven against the real encrypted DB (`req_up2_3_5_6_7_8`):
`description`/`priority`/`goal_id` each independently proven `Set`
(persists) and `Clear` (`Clear` re-verified via a fresh `get_project`
read, not just the in-memory returned value, for `description`).
`title`/`status` proven `Some`/`None` only — no `Clear` state exists for
either, matching the frozen non-nullable-field rule.

---

## 6. Validation

Order: Project must exist (`NotFound` otherwise) → pure patch
validation (`validate_update_project` — at least one field touched,
`title` if `Some` trimmed/non-empty/`TITLE_MAX_LEN`, `description` if
`Set` reuses the existing `DESCRIPTION_MAX_LEN` bound, `priority` if
`Set` reuses the existing `Priority` range) → `goal_id` existence check
(a database read, only for `Set`) → confirmation gate → atomic
mutation+audit. No new text-validation rule was invented anywhere in
this chain — every check reuses the exact same `validate_title`/
`validate_description`/`validate_priority` functions `CreateProject`/
`UpdateGoal`/`AddMilestone` already use.

---

## 7. Goal reference behavior

`FieldPatch::Set(goal_id)` triggers `storage::goal_repo::goal_exists`
before persistence — missing Goal → `ValidationError::
ReferencedGoalNotFound`, zero mutation (`req_up9`). `Clear`/`Unchanged`
trigger no lookup. `Project.goal_id` is never inferred from
`GoalRelation`, `Task`, `Milestone`, or anything else — it remains its
own direct, frozen field, exactly as before this run.

---

## 8. Status transition policy

No terminal-state restriction. `req_up14_to_18` proves five
representative real-DB transitions in sequence: `ACTIVE → PAUSED →
COMPLETED → ACTIVE (reopen) → ABANDONED → ACTIVE (reopen again)`. This
is consistent with the earlier removal of the unapproved Goal
terminal-status rule and with Milestone/Task's identical policy — no
transition matrix was added.

---

## 9. IMPORTANT_CHANGE confirmation classification

`effective_class(ConfirmationClass::ImportantChange, meta.provenance)`
is reused unchanged — no custom Project confirmation logic, no change
to the generic severity ladder or `blocks_until_confirmed`. Direct unit
tests prove `effective_class(ImportantChange, Explicit) ==
ImportantChange` and `effective_class(ImportantChange, Inferred) ==
ImportantChange` (`IMPORTANT_CHANGE`'s type-floor severity, 3, is
already above `InferredChange`'s provenance-derived severity, 2, so
`Inferred` provenance changes nothing — `effective_class` takes the
max), and `blocks_until_confirmed(ImportantChange) == false` against
the existing, unmodified generic policy. No UI modal is built; the
typed `ConfirmationClass`/`ActionOutcome` metadata is returned exactly
as every other entity's does, for a future M3 UI to render against.

---

## 10. Atomic update + audit

`update_project` uses the identical `&mut Connection` +
`conn.transaction()` + `CoreError`-on-any-failure shape as every prior
entity, reusing `project_audit_log` (no new audit table). Proven with a
real forced-failure SQLite trigger (`BEFORE INSERT ON project_audit_log
... RAISE(FAIL, ...)`, installed only inside the test, removed after):
`req_up27_28_29_30` shows the Project is byte/logically identical
afterward (including `updated_at` and `progress`), no orphan audit row
exists, the action returns a typed `StorageError` (never `Success`),
and the connection remains fully usable afterward (a following normal
`UpdateProject` call succeeds).

---

## 11. Previous snapshot

`previous_snapshot = serde_json::to_string(&existing).ok()`, taken
before the patch is applied — identical pattern to `UpdateGoal`/
`UpdateMilestone`/`UpdateTask`. Proven (`req_up25_26`) that the audit
row's `previous_snapshot` contains the pre-update title (and, by the
same serialization, every other pre-update field: description, status,
priority, goal_id, progress, timestamps). No generic event-sourcing
system was built — one row per audited mutation, matching the narrow
Undo-support pattern already established.

---

## 12. Timestamps

`req_up1_19`/`req_up20_21_22` prove: `id` never changes across a
successful update; `created_at` never changes; `updated_at` advances on
a successful update; a rejected update (no-fields payload) leaves both
timestamps byte-identical. `req_up27_28_29_30` additionally proves
`updated_at` does not leak from a forced-audit-failure rollback.

---

## 13. Progress-preservation proof

See §4 above (`req_up23_24`) — this is the dedicated regression test
required by the brief's §12: create a Project, set `progress = 63` via
`UpdateProgress`, run `UpdateProject` changing `title`/`status`/
`priority`, verify `progress` is still exactly `63`, then run
`UpdateProgress` again and verify it still works independently and
does not get reverted by anything `UpdateProject` did.

---

## 14. GoalRelation non-side-effect proof

`req_up31_32`: creates two Goals, links them with an unrelated
`LinkGoals` call, then runs `UpdateProject`'s `goal_id` `Set` and
`Clear` against a separate Project. Proves both the `GoalRelation` row
count and the referenced Goal's own byte/logically-identical state are
completely unaffected by either operation — `Project.goal_id` and
`GoalRelation` remain fully separate concepts, exactly as frozen.

---

## 15. Tests

- **19 direct action-layer unit tests** (`actions::tests`,
  `update_project_*`): no-fields rejection, `FieldPatch::Unchanged`-only
  rejection, title trim/empty/overlong, description Set/Clear/overlong,
  priority Set/Clear/out-of-bounds, `goal_id` Set/Clear at the pure
  layer, all four `LifecycleStatus` values accepted, `COMPLETED
  → ACTIVE`/`ABANDONED → ACTIVE` not rejected, `IMPORTANT_CHANGE` under
  both `Explicit`/`Inferred` provenance, and `blocks_until_confirmed`
  against the existing generic policy.
- **14 integration test functions** (`services::tests`, `req_up*`)
  covering all 33 numbered requirements from the brief (several
  requirements share one function, matching established precedent),
  plus one unplanned migration-compatibility test for the V6 addition
  (§3a) that the original brief could not have anticipated.
- All 200 prior tests remain green, unmodified.

---

## 16. Exact build results (MEASURED)

- `cargo fmt --check`: clean (after one `cargo fmt` pass fixed
  line-wrapping in new test/action code — no logic changed).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **233 passed; 0 failed; 0 ignored**
  (up from 200). Note: an intermediate run failed 9 tests with `CHECK
  constraint failed` errors purely because of a stale `embed_migrations!`
  macro expansion after the new V6 file was added — `cargo clean -p
  lifeos` (a full rebuild of this crate only) resolved it; this was a
  build-cache artifact, not a code or test defect, and is recorded here
  for completeness rather than silently omitted.
- `cargo build --release`: succeeded (`Finished \`release\` profile
  [optimized] target(s) in 2m 51s`).
- `npm run build` (repo root): succeeded — `tsc && vite build`, 27
  modules transformed.
- `npm run tauri build`: run this pass (not merely optional) because a
  real schema change (V6) was introduced, contrary to the brief's
  original no-schema-change assumption — see the chat reply for the
  actual measured result, verified from the captured log file directly.

---

## 17. Files changed

New:
- `src-tauri/migrations/V6__widen_project_audit_log_action_types.sql`
  (unplanned, added after user authorization — see §3a)
- `docs/reports/REPORT_M2_UPDATE_PROJECT_2026-09-17.md` (this file)

Modified:
- `docs/PROJECT_STATE.md` — full UpdateProject implementation status
- `docs/LIFEOS_SPEC.md` — implementation-status wording only, where it
  changes from "frozen, unimplemented" to "implemented"
- `src-tauri/src/actions/mod.rs` — `UpdateProjectPayload`,
  `validate_update_project`; `Project` added to the domain import list;
  Project section doc comment corrected (no longer says "no frozen
  UpdateProject")
- `src-tauri/src/actions/tests.rs` — 19 direct unit tests, `base_project`
  helper
- `src-tauri/src/services/mod.rs` — `update_project`; Project section
  doc comment corrected
- `src-tauri/src/services/storage/project_repo.rs` — `update_project`
  (narrow SQL, `title`/`description`/`status`/`priority`/`goal_id`/
  `updated_at` only — never `progress`/`created_at`/`id`)
- `src-tauri/src/services/tests.rs` — 14 integration test functions

No module-privacy change was needed — `project_repo`'s visibility
(`pub(in crate::services)`) was already correct and untouched, so no
new illegal-access probe was required per the brief's own instruction.

---

## 18. Remaining M2 gaps

With `UpdateProject` implemented, the Project action-set gap identified
in the M2 Project/storage integrity hardening report is now closed
**both in design and in code** — every frozen mutable Project field
(`title`/`description`/`status`/`priority`/`goal_id`) now has a working
update path, and `progress` remains exclusively `UpdateProgress`'s.

Remaining, explicitly out of scope for this run:

- `RecurringRule` (deliberately deferred, needs its own design/freeze
  pass — `Task.recurring_rule_id` remains `NULL`-only).
- Calendar, Reminder, Challenge, Personal Knowledge Layer — all still
  schema-frozen only, not implemented.
- `ProgressUpdate` (the separate, still-unimplemented polymorphic
  history entity) — `Project.progress`/`Goal.progress` remain
  current-state snapshots only, no temporal history exists yet.
- No Tauri command layer / capability transport exists yet — none of
  Goal/Project/GoalRelation/Milestone/Task/UpdateProject is wired into
  the running app; `services`' functions are only reachable from tests.

---

## 19. Recommended next step

With every frozen, shovel-ready M2 CRUD action on an existing entity
now implemented, the next M2 step needs an explicit product/planning
decision rather than another "already-frozen, just build it" slice:
either a dedicated design/freeze pass for `RecurringRule` (small,
narrowly scoped, unblocks nothing else urgent), or a dedicated
planning pass for the Personal Knowledge Layer (larger, the next real
net-new entity family). Neither should be started without that
planning pass first, per the precedent set by the M2 Project Child
Action Freeze report before Milestone/Task were built.
