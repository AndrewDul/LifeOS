# REPORT — M2 Milestone Vertical Slice (2026-09-17)

Fourth real M2 domain entity, and the first required-parent child entity.
Implements `AddMilestone`/`UpdateMilestone` end to end through
`domain → actions → services → private services::storage → encrypted
SQLCipher`, reusing the pattern proven and hardened across Goal,
Project, and GoalRelation. **Stops after Milestone** — `Task` and every
other domain entity remain out of scope for this run.

---

## 1. Pre-run state

- M1 accepted, unchanged.
- M2: `Goal`, `Project`, `GoalRelation` all implemented and hardened
  (see `REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`,
  `REPORT_M2_GOAL_HARDENING_2026-09-17.md`,
  `REPORT_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`,
  `REPORT_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md`,
  `REPORT_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md`).
- `REPORT_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md` (documentation
  only, zero source changes) froze `UpdateProject`/`UpdateMilestone`/
  `UpdateTask`'s action-set decisions ahead of implementation, and left
  `Milestone.order_index` uniqueness as an explicit open question.
- 104/104 automated tests passing. Local checkpoint `296fc2c`. Clean
  working tree. No push authorized this run either.

---

## 2. Patch-semantics correction

The Project Child Action Freeze report's own conceptual shapes (its §6
`FieldPatch` table) were correct. `LIFEOS_SPEC.md`'s separate §5.1 prose
had drifted from that and used `FieldPatch<T>` too broadly — stating
`title`/`target_date`/`order_index` (Milestone) and `title`/`due_at`/
`project_id`/`goal_id` (Task) all as `FieldPatch<T>`, when only the
genuinely nullable fields need the tri-state.

**Canonical rule, now written into `LIFEOS_SPEC.md` §5.1:**
`FieldPatch<T>` is used ONLY for nullable persisted fields where
"leave unchanged" and "explicitly clear to NULL" must be distinguishable.
For non-nullable fields, plain `Option<T>` is correct and sufficient —
`None` means unchanged, `Some(x)` means set to `x`; there is no
meaningful "clear" state for a field the database schema says can never
be `NULL`.

Applied to `Milestone`: `title: Option<String>`, `status:
Option<MilestoneStatus>`, `order_index: Option<OrderIndex>` (all
non-nullable) vs. `target_date: FieldPatch<DateTime<Utc>>` (the one
genuinely nullable field). Applied to `Task` (documentation only — not
implemented this run): `title: Option<String>` stays `Option`;
`due_at`/`project_id`/`goal_id` stay `FieldPatch<T>` since all three are
genuinely nullable. This same rule now also governs the still-
unimplemented `UpdateProject`.

Fixed in `docs/LIFEOS_SPEC.md` (§5.1's action-set bullet, §13's open-
questions list) and `docs/reports/REPORT_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md`
(a "Correction note" addendum after its §6 — the original text is kept,
not silently rewritten).

---

## 3. `Milestone.order_index` uniqueness — RESOLVED

**Decision: `(project_id, order_index)` must be UNIQUE within a
Project.** Different Projects MAY freely reuse the same `order_index`
values — the constraint is scoped to the pair, not `order_index` alone.
No automatic shifting/rebalancing of other Milestones' positions exists
or is planned; if `AddMilestone`/`UpdateMilestone` requests an
`order_index` already occupied within the same Project, it is rejected
with a structured validation error and zero writes. A `Milestone` being
updated keeping its own current `order_index` is never treated as a
conflict with itself.

Enforced twice: a Core-level pre-check
(`storage::milestone_repo::order_index_taken`, parameterized to
optionally exclude the Milestone being updated) returning
`ValidationError::DuplicateMilestoneOrderIndex` before any write, and a
real `UNIQUE(project_id, order_index)` SQL constraint
(`migrations/V4__create_milestones.sql`) as defense in depth — proven
directly with a raw-SQL bypass test (§12).

`order_index` itself has one canonical numeric bound: it cannot be
negative (`domain::ORDER_INDEX_MIN = 0`). No upper bound is invented —
no existing document specifies one, and inventing one would repeat the
exact mistake the Goal terminal-status rule made.

Updated `docs/LIFEOS_SPEC.md` §5.1 (open question → resolved) and §13
(same), and added a "Resolved" addendum to
`REPORT_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md` §11.

---

## 4. Frozen Milestone domain model

`src-tauri/src/domain/mod.rs`:

```rust
pub(crate) struct MilestoneId(pub Uuid);          // UUID v7, own newtype

pub(crate) enum MilestoneStatus { Pending, Done }  // frozen, closed 2-value enum
// as_str(): "PENDING"/"DONE"; parse(): case-sensitive, exact match only

pub(crate) type OrderIndex = i64;
pub(crate) const ORDER_INDEX_MIN: OrderIndex = 0;

pub(crate) struct Milestone {
    pub id: MilestoneId,
    pub project_id: ProjectId,
    pub title: String,
    pub target_date: Option<DateTime<Utc>>,
    pub status: MilestoneStatus,
    pub order_index: OrderIndex,
}
```

Field set is EXACTLY the frozen architecture-freeze definition: `id`,
`project_id`, `title`, `target_date` (nullable), `status`,
`order_index`. Deliberately excludes `description`/`progress`/
`priority`/`weight`/`blockers`/an AI score/any other hidden metadata —
none are in the frozen model.

**Deliberately excludes `created_at`/`updated_at`.** Every other entity
implemented so far (`Goal`, `Project`) carries both timestamps, and
`GoalRelation` carries `created_at`. This is a genuine exception, not an
oversight: the frozen architecture-freeze field table for `Milestone`
lists no timestamps at all, in contrast to `Task`'s own frozen row,
which explicitly does list `created_at`/`updated_at`. Per this run's
explicit instruction to STOP and document rather than silently add or
silently omit, timestamps were NOT added — if a real need for one
emerges later, it must be frozen in `LIFEOS_SPEC.md` first, not added
here silently.

`MilestoneStatus` is deliberately not `LifecycleStatus` — a genuinely
different, closed 2-value set, not a reuse of the 4-value Goal/Project
status. Reusing `LifecycleStatus` here would repeat the exact mistake
the `GoalStatus` → `LifecycleStatus` rename fixed, in reverse (forcing
two genuinely different value sets to share one type).

`target_date` reuses exactly the same representation as `Goal.target_date`
(`Option<DateTime<Utc>>`, RFC3339 in storage) — no new date format was
invented, and (matching Goal precedent) no additional runtime content
validation exists beyond being a well-formed, typed `DateTime<Utc>` (see
Known problems, §19).

3 new pure round-trip unit tests in `domain::tests`
(`milestone_status_as_str_matches_frozen_db_text_values`,
`..._parse_round_trips_through_as_str`, `..._parse_rejects_unknown_text`)
— same discipline as `LifecycleStatus`/`GoalRelationType` before it.

---

## 5. `AddMilestone`

Payload: `project_id: ProjectId, title: String, target_date:
Option<DateTime<Utc>>, order_index: OrderIndex`. **`status` is not part
of the payload** — a new Milestone always starts `PENDING`; completion
happens only through `UpdateMilestone`. Confirmation floor
`EXPLICIT_ADD` (frozen, architecture-freeze report §F).

Validation order (pure-first, matching the Goal/Project/GoalRelation
precedent):

1. `validate_add_milestone` (pure, no I/O): title trim/non-empty/
   `TITLE_MAX_LEN`, `order_index >= ORDER_INDEX_MIN`.
2. `storage::project_repo::project_exists(project_id)` — a real
   database read; `ValidationError::ReferencedProjectNotFound` if not.
3. `storage::milestone_repo::order_index_taken(project_id, order_index,
   exclude_id: None)` — a real database read;
   `ValidationError::DuplicateMilestoneOrderIndex` if occupied.
4. Confirmation gate (`blocks_until_confirmed`).
5. Insert `Milestone` row + `milestone_audit_log` row, one transaction.

`project_exists` was added to `project_repo.rs` (not `milestone_repo.rs`)
— same convention `goal_repo::goal_exists` already established: a
repo function checking a *different* entity's existence lives in that
entity's own repo module.

---

## 6. `UpdateMilestone`

Payload: `id: MilestoneId, title: Option<String>, target_date:
FieldPatch<DateTime<Utc>>, status: Option<MilestoneStatus>,
order_index: Option<OrderIndex>` (patch semantics per §2). Confirmation
floor `EXPLICIT_ADD` — the frozen floor from the Project Child Action
Freeze report, deliberately NOT `IMPORTANT_CHANGE` like `UpdateGoal`; no
`IMPORTANT_CHANGE` floor was invented for it.

Validation order:

1. `storage::milestone_repo::get_milestone(id)` — `ActionOutcome::NotFound`
   if absent.
2. `validate_update_milestone` (pure): at least one field touched
   (`NoFieldsToUpdate` otherwise), `title` if `Some` validated the same
   way as create, `order_index` if `Some` bounds-checked. No status-
   transition restriction — `PENDING`/`DONE` are freely reversible peer
   states (§13).
3. If `order_index` is `Some` and differs from the Milestone's current
   value: `order_index_taken(project_id, order_index,
   exclude_id: Some(existing.id))` — `DuplicateMilestoneOrderIndex` if a
   *different* Milestone in the same Project already occupies it. The
   Milestone keeping its own current index is never flagged.
4. Confirmation gate.
5. Apply the patch onto the existing `Milestone`, update row + audit
   row, one transaction. `previous_snapshot` is the pre-update JSON
   snapshot (same shape as `UpdateGoal`/`UpdateProgress`).

---

## 7. Confirmation behavior

Both actions: `effective_class(ConfirmationClass::ExplicitAdd,
meta.provenance)`. `Provenance::Explicit` → stays `EXPLICIT_ADD`,
executes immediately. `Provenance::Inferred` → becomes
`INFERRED_CHANGE`, `blocks_until_confirmed` is true,
`ActionOutcome::ConfirmationRequired` is returned and **zero rows are
written** — proven for both actions against a real encrypted DB
(`req_m23_24`, `req_m25`), then a confirmed resubmission (`confirmed:
true`) persists (`req_m24`).

---

## 8. Project FK validation

`AddMilestone.project_id` is checked against a real database read
(`project_repo::project_exists`) before persistence, reported through
the same typed `ValidationFailed` channel as structural checks —
`ValidationError::ReferencedProjectNotFound`, never a raw SQL failure
surfaced to the caller. The real `milestones.project_id REFERENCES
projects(id) ON DELETE RESTRICT` constraint is independent defense in
depth (`PRAGMA foreign_keys = ON` is enforced on every connection via
`storage::open_encrypted`, inherited unchanged from the Project/storage
integrity hardening pass) — proven directly with a raw-SQL bypass
(`req_m26`, §12).

---

## 9. `(project_id, order_index)` UNIQUE behavior

Proven at both layers:

- **Core**: `req_m8` — a second `AddMilestone` at an occupied slot in
  the same Project is rejected, zero writes, still exactly 1 row.
  `req_m9` — the same `order_index` reused by a *different* Project
  succeeds (2 distinct rows). `req_m15_16` — an `UpdateMilestone` moving
  a Milestone to a free slot succeeds; moving a *different* Milestone
  onto an already-occupied slot is rejected with zero mutation.
- **Database**: `req_m27` — a raw SQL `INSERT` bypassing `AddMilestone`
  entirely, naming an already-occupied `(project_id, order_index)`, is
  rejected by the real `UNIQUE` constraint.

---

## 10. Atomic mutation + audit

Both `add_milestone`/`update_milestone` use the same `&mut Connection`
+ `conn.transaction()` + `CoreError`-on-any-failure shape as every prior
entity — the entity write and its `milestone_audit_log` write happen
inside one transaction; any failure means the transaction is never
committed, and `Transaction::drop` rolls it back automatically. No
`let _ = ...` anywhere discards an audit-write result.

Proven with real forced-failure SQLite triggers
(`BEFORE INSERT ON milestone_audit_log ... RAISE(FAIL, ...)`, installed
only inside the test, removed after): `req_m21` (AddMilestone) and
`req_m22` (UpdateMilestone) both show zero milestone rows / an
unmutated pre-update row survive the forced failure, the action returns
a typed `StorageError` (never `Success`), and the connection remains
fully usable afterward (a following normal call succeeds).

---

## 11. Database defense-in-depth

All via raw SQL bypassing the normal action API entirely, kept local to
the test file (never exposed as production API):

- `req_m26` — nonexistent `project_id` rejected by the FK constraint.
- `req_m27` — duplicate `(project_id, order_index)` rejected by
  `UNIQUE`.
- `req_m28` — invalid `status` text (`'IN_PROGRESS'`) rejected by
  `CHECK (status IN ('PENDING', 'DONE'))`.
- `req_m29` — a whitespace-only title rejected by
  `CHECK (length(trim(title)) > 0)`.
- `req_m30` — a hard `DELETE FROM projects` against a Project with a
  live Milestone rejected by `ON DELETE RESTRICT`; the Project and the
  Milestone both remain afterward.

---

## 12. PENDING/DONE reversible status — proof

No terminal-state rule exists or was added. `req_m13_14` proves both
directions succeed against a real DB: `PENDING → DONE` and, in the same
test, `DONE → PENDING` on the same Milestone — matching §5 of the
Project Child Action Freeze report and applying the Goal hardening
lesson (§4c of that report) proactively rather than retroactively.
Mirrored by pure unit tests
(`update_milestone_status_pending_to_done_is_valid`/
`..._done_to_pending_is_valid`).

---

## 13. Patch semantics — proof

`req_m10_11_12` (integration) + the `actions::tests` unit tests prove:
`title` `None` = unchanged, `Some(x)` = set (trimmed); `target_date`
`Unchanged` = untouched, `Set(x)` = written, `Clear` = written as real
SQL `NULL` (re-verified via a fresh `get_milestone` read, not just the
in-memory returned value); `status` `None` = unchanged, `Some(x)` =
updates; `order_index` `None` = unchanged, `Some(x)` = updates (subject
to the occupancy check, §9). None of `title`/`status`/`order_index`
has, or needs, a `Clear` state — they are non-nullable fields.

---

## 14. V3 → V4 migration — non-destructive upgrade proof

`req_m3_v3_populated_database_upgrades_cleanly_to_v4`: builds a database
frozen at V3 only (`bootstrap_db_at_version(..., 3)`), populates it with
a real `Goal`, a second `Goal`, a `Project` linked to the first Goal,
and a `GoalRelation` between the two Goals — the actual shape of an
existing user's database before this update — then reopens it normally
(`bootstrap_db_at`, applying only the newly-pending V4 migration) and
confirms: the Goal, Project, and GoalRelation data are all byte/
logically unchanged; the `milestones`/`milestone_audit_log` tables now
exist; and a new `AddMilestone` succeeds against the pre-existing
Project. Neither `V1__create_goals.sql`, `V2__create_projects.sql`, nor
`V3__create_goal_relations.sql` was edited.

`req_m2_fresh_v1_v2_v3_v4_database_works_end_to_end` separately proves a
fresh database applies all four migrations in one run and supports
Goal, Project, GoalRelation, and Milestone together.

---

## 15. Read-side behavior

`services::get_milestone(id)` (narrow internal read, same precedent as
`get_goal`/`get_project`/`get_goal_relation`) and
`services::get_milestones_for_project(project_id)` — deterministic
`ORDER BY order_index ASC`, unambiguous because
`(project_id, order_index)` is UNIQUE, so no two returned rows for one
Project ever share a position. Proven by `req_m31`, which inserts three
Milestones out of order (order_index 2, 0, 1) and confirms the read
returns them First/Second/Third regardless of insertion order. No
public capability transport was added.

---

## 16. Module-privacy verification

`services/storage/milestone_repo.rs` follows the exact `pub(in
crate::services)` discipline as `goal_repo`/`project_repo`/
`goal_relation_repo`, registered as `pub(super) mod milestone_repo;` in
`services/storage/mod.rs` (itself still a plain, non-`pub` `mod storage;`
inside `services/mod.rs`). Re-verified with the same illegal-access
probe used for every prior entity: a temporary
`services::storage::milestone_repo::get_milestone` reference added to
`lib.rs`, `cargo check` run, the real compiler rejection captured
(`error[E0603]: module 'storage' is private`), then the probe removed.
Not assumed to still hold just because the pattern is unchanged — proven
again against the new module.

---

## 17. Exact test/build results (MEASURED)

- `cargo fmt --check`: clean (after one `cargo fmt` pass fixed line-
  wrapping in new test/action code — no logic changed).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **148 passed; 0 failed; 0 ignored**
  (up from 104). Breakdown of the 44 new tests: 17 direct `actions::tests`
  unit tests (7 `AddMilestone`, 10 `UpdateMilestone`/patch-semantics/
  status), 3 direct `domain::tests` `MilestoneStatus` round-trip tests,
  24 `services::tests` integration test functions covering all 32
  numbered requirements from this run's brief (several requirements
  share one test function, matching the `req_gr19_20`-style precedent
  from the GoalRelation report). All prior 104 tests remain green,
  unmodified.
- `cargo build --release`: succeeded (`Finished \`release\` profile
  [optimized] target(s)`).
- `npm run build` (repo root): succeeded — `tsc && vite build`, 27
  modules transformed, built in 516ms.
- `npm run tauri build`: succeeded end to end, verified by reading the
  captured log file directly (never the wrapper's reported exit code
  alone, per established project practice) — `Finished \`release\`
  profile [optimized] target(s) in 2m 43s`, both installers built:
  `LifeOS_0.1.0_x64_en-US.msi` and `LifeOS_0.1.0_x64-setup.exe`, log
  ends `EXITCODE:0`.

---

## 18. Files changed

New:
- `src-tauri/migrations/V4__create_milestones.sql`
- `src-tauri/src/services/storage/milestone_repo.rs`
- `docs/reports/REPORT_M2_MILESTONE_VERTICAL_SLICE_2026-09-17.md` (this
  file)

Modified:
- `docs/LIFEOS_SPEC.md` — §2/§4 corrections, §5.1/§13 resolutions
  (§2/§3 above)
- `docs/PROJECT_STATE.md` — full Milestone implementation status (this
  update)
- `docs/reports/REPORT_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md` —
  two addenda (correction note, resolved note), original text preserved
- `src-tauri/src/domain/mod.rs` — `MilestoneId`, `MilestoneStatus`,
  `OrderIndex`/`ORDER_INDEX_MIN`, `Milestone`
- `src-tauri/src/domain/tests.rs` — 3 `MilestoneStatus` unit tests
- `src-tauri/src/actions/mod.rs` — `AddMilestonePayload`,
  `UpdateMilestonePayload`, `validate_add_milestone`,
  `validate_update_milestone`, `ValidationError::
  ReferencedProjectNotFound`/`OrderIndexOutOfBounds`/
  `DuplicateMilestoneOrderIndex`
- `src-tauri/src/actions/tests.rs` — 17 direct unit tests
- `src-tauri/src/services/mod.rs` — `add_milestone`, `update_milestone`,
  `get_milestone`, `get_milestones_for_project`
- `src-tauri/src/services/tests.rs` — 24 integration test functions
- `src-tauri/src/services/storage/mod.rs` — `pub(super) mod
  milestone_repo;`, doc comment updated
- `src-tauri/src/services/storage/project_repo.rs` — `project_exists`

`src-tauri/src/lib.rs` was temporarily modified for the illegal-access
probe (§16) and restored to its exact prior state before this run ends
— `git diff` shows no net change to that file.

---

## 19. Remaining risks / open questions

- Same pre-existing, unchanged gaps carried forward from prior reports:
  no Tauri command layer wires any of Goal/Project/GoalRelation/
  Milestone into the running app yet; `target_date` has no content
  validation beyond being a well-formed `DateTime<Utc>` (e.g. nothing
  rejects a date far in the past — same as `Goal.target_date`, not a
  new gap); `CoreError`'s `Debug` impl still includes the internal
  diagnostic string (fine for tests, a future capability layer must use
  `Display`/`kind()` for any external caller).
- **New**: `Milestone` has no read capability beyond
  `get_milestone`/`get_milestones_for_project` — no aggregate "all
  Milestones due before X" query, no cross-Project query. Deliberately
  out of scope; add only when a real caller needs it.
- **New**: `order_index`'s upper bound is unconstrained beyond `i64`'s
  own range (no canonical application-level maximum exists or was
  invented, per §3/§4) — a caller could in principle write an
  absurdly large value; not a correctness risk (SQLite stores it fine),
  only a UX consideration for whatever eventually assigns these values.
- `Task` is confirmed the next safest M2 slice (§20) — its own known
  risks (two optional parents, four statuses) remain exactly as
  documented in the Project Child Action Freeze report, unchanged by
  this run.

---

## 20. Recommended next M2 slice

**`Task`.** Same frozen order as `REPORT_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md`
already decided (§g of that report) — `Milestone` first (done, this
report), `Task` next, then reassess. `Task`'s action set
(`AddTask`/`UpdateTask`), patch semantics (`title: Option<String>`;
`due_at`/`project_id`/`goal_id: FieldPatch<T>`, corrected per §2 above),
confirmation floor (`EXPLICIT_ADD` for both), creation default (`TODO`),
and FK/delete policy (`project_id`/`goal_id` both nullable,
`ON DELETE RESTRICT`) are already frozen — implementation should not
need to invent any of those. `Task` adds two genuinely new complexities
Milestone did not have to resolve: two independent optional parent
foreign keys (a Task may be standalone, Project-only, Goal-only, or
both — already confirmed unconstrained, no new requirement to invent),
and a four-value status enum (`TODO`/`IN_PROGRESS`/`DONE`/`CANCELLED`,
all peer states — no terminal-state rule, matching the lesson applied
again in this slice). Do not implement Personal Knowledge, Calendar,
Challenge, `ProgressUpdate`, capability transport, M3 UI, or any LLM
integration in that pass — same scope discipline this run followed.
