# REPORT — M2 GoalRelation Vertical Slice

**Date:** 2026-09-17
**Scope:** The third real M2 domain entity, `GoalRelation`, and its one
frozen action, `LinkGoals` - the first true relationship-*graph*
structure in production M2, built directly with the twice-hardened
Goal/Project pattern. Explicitly **not** in scope: `Milestone`, `Task`,
`Calendar`, `Challenge`, Personal Knowledge Relation/Fact/Memory, Goal
Intelligence reasoning (automatic conflict detection, inference, priority
computation, cycle detection, auto-relation-creation from text), a
`capability/` transport, M3 UI, Qwen, GPT/Luna.

---

## 1. Pre-run state

M1 accepted and closed; SQLCipher storage proven and hardened; SQLite FK
enforcement genuinely enabled and proven (`PRAGMA foreign_keys = ON` on
every connection via `storage::open_encrypted`); Goal implemented and
hardened; Project implemented and storage-integrity hardened
(`LifecycleStatus` rename, canonical `Project.progress`, documented
action-set gap); 73/73 tests passing at checkpoint commit `9bac125`.
Full detail: `docs/reports/REPORT_M2_GOAL_HARDENING_2026-09-17.md`,
`docs/reports/REPORT_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`,
`docs/reports/REPORT_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md`.

---

## 2. Correction of GoalRelation-vs-PersonalKnowledge Relation taxonomy confusion

Fixed **before** any implementation, per this run's explicit instruction.

The Project/storage hardening report's §14 ("Whether GoalRelation is now
safe to start") had claimed:

> ...plus a `relation_type` enum the architecture freeze already flags as
> an `UNRESOLVED QUESTION` (no rigid taxonomy yet - `LIFEOS_SPEC.md`
> §5.1's Goal entity row).

This was a factual mistake, conflating three distinct things:

1. **`GoalRelation.relation_type`** - a **frozen, closed 11-value enum**
   (`SUPPORTS`/`CONFLICTS_WITH`/`DEPENDS_ON`/`BLOCKS`/`CONTRIBUTES_TO`/
   `REQUIRES`/`PARENT_OF`/`CHILD_OF`/`DUPLICATES`/`SUPERSEDES`/
   `COMPETES_FOR_RESOURCE`) - not open at all.
2. **The Personal Knowledge Layer's separate `Relation.relation_type`**
   (`LIFEOS_SPEC.md` §5.3) - genuinely an intentionally free string,
   deferred until M6 usage patterns exist. Unrelated entity, unrelated
   field, same name only by coincidence.
3. **`Goal.domain_tag`** - the field the architecture-freeze report's own
   entity table actually flags: "(free string - `UNRESOLVED QUESTION`:
   no rigid taxonomy yet)." That note was never about any
   `relation_type` at all.

**Corrected in:**
- `docs/reports/REPORT_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md` §14 -
  the erroneous paragraph rewritten in place with an explicit
  "Correction" note (reports are otherwise treated as historical/
  append-only, but a factual error is fixed rather than left standing,
  per this run's explicit instruction).
- `docs/LIFEOS_SPEC.md` §5.2 - now states plainly that
  `GoalRelation.relation_type` is frozen and closed, with an explicit
  "do not confuse with §5.3" cross-reference.
- `docs/LIFEOS_SPEC.md` §5.3 - now states plainly that the Personal
  Knowledge Layer's `Relation.relation_type` is free and deferred, with
  the reverse cross-reference.
- `docs/PROJECT_STATE.md` was checked and found to already state the
  Personal Knowledge Layer fact correctly (§10's existing "The Personal
  Knowledge Layer's `Relation.relation_type` remains an intentionally
  free string" line was never wrong) - no change was needed there beyond
  this run's own new content.

No other history was rewritten - this is a targeted correction of one
specific claim, not a rewrite of either report's other findings.

---

## 3. Frozen GoalRelation model

Architecture-freeze report §D, implemented exactly:

```rust
pub(crate) struct GoalRelation {
    pub id: GoalRelationId,
    pub from_goal_id: GoalId,
    pub to_goal_id: GoalId,
    pub relation_type: GoalRelationType,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}
```

Deliberately **excludes** `status`, `updated_at`, `confidence`, `weight`,
an AI score, an "inferred" flag, `priority`, or any other hidden
metadata - none of those are in the frozen model. Provenance belongs to
`ActionMeta`/the audit log (`actor`/`provenance`/`confirmation_class` on
`goal_relation_audit_log`), not the entity itself - the same separation
already established for Goal/Project.

`GoalRelationId` is its own UUID v7 newtype, distinct from `GoalId`/
`ProjectId` at the type level - same type-safety reasoning as
`ProjectId` not reusing `GoalId`.

---

## 4. Frozen 11-value GoalRelationType enum

Implemented exactly, no additions, no removals:

| Rust variant | Database `TEXT` value |
|---|---|
| `Supports` | `SUPPORTS` |
| `ConflictsWith` | `CONFLICTS_WITH` |
| `DependsOn` | `DEPENDS_ON` |
| `Blocks` | `BLOCKS` |
| `ContributesTo` | `CONTRIBUTES_TO` |
| `Requires` | `REQUIRES` |
| `ParentOf` | `PARENT_OF` |
| `ChildOf` | `CHILD_OF` |
| `Duplicates` | `DUPLICATES` |
| `Supersedes` | `SUPERSEDES` |
| `CompetesForResource` | `COMPETES_FOR_RESOURCE` |

`as_str()`/`parse()` proven for all 11 with direct round-trip unit tests
(`domain::tests::goal_relation_type_*`), plus rejection of unknown/
lowercase/malformed text. The database `CHECK` constraint in
`V3__create_goal_relations.sql` lists exactly these 11 strings.

---

## 5. Direction convention

Relations are stored as directional rows; **no automatic inverse row is
ever written**. `LinkGoals` writes exactly the one relation requested.

For hierarchy: `PARENT_OF`/`CHILD_OF` are logical inverses, but the
canonical application convention is to write `PARENT_OF(parent ->
child)` and derive "child of" by querying the reverse direction
(`get_goal_relations_for_goal` already returns both directions, so this
requires no new query shape) - `LinkGoals` never auto-writes a
`CHILD_OF` row alongside a `PARENT_OF` one. `CHILD_OF` remains a valid,
storable, parseable enum value - the freeze explicitly keeps it - for a
caller who deliberately requests it directly (e.g. stating "B is a
child of A" from B's own perspective without restating it as `A
PARENT_OF B`).

For every other relation type: one stated relation = one directional
row, no automatic inverse. `A SUPPORTS B` and `B SUPPORTS A` are two
distinct rows if both are explicitly requested - `LinkGoals` neither
auto-creates nor auto-collapses them.

**Proven:**
- `req_gr23_parent_of_convention_stores_exactly_one_row` - links
  `Parent PARENT_OF Child`, asserts exactly one row exists, and that
  querying relations for `Child` returns that same row (`from = Parent,
  to = Child, type = PARENT_OF`), not a fabricated inverse.
- `req_gr24_explicit_child_of_remains_valid_when_deliberately_requested`
  - links `Child CHILD_OF Parent` directly and confirms it stores and
  round-trips correctly.
- `req_gr11_reversed_direction_is_a_distinct_stored_relation` - `A
  SUPPORTS B` then `B SUPPORTS A` both succeed, 2 distinct rows.

---

## 6. Self-link decision

**Frozen rule (v0.1, documented before coding, not a silent
implementation invention):** `from_goal_id == to_goal_id` is rejected. A
Goal cannot be linked to itself in `GoalRelation` v0.1.

**Reason:** every currently-frozen relation type describes a
relationship between *distinct* Goal records; self-edges would add
noise and create pathological graph cases (e.g. a Goal `DEPENDS_ON`
itself) with no product meaning today. This is a deterministic
structural rule, not AI reasoning - checked in pure `actions::
validate_link_goals` with zero I/O, before any database read.

Enforced at two independent layers:
- **Application**: `ValidationError::SelfGoalRelation`, pure, no I/O
  (`link_goals_self_link_is_rejected` unit test, and
  `req_gr8_self_relation_rejected_before_persistence` integration test).
- **Database**: `CHECK (from_goal_id <> to_goal_id)` in
  `V3__create_goal_relations.sql`, proven by a raw SQL insert bypassing
  `LinkGoals` entirely (`req_gr14_raw_sql_self_edge_rejected_by_check`).

---

## 7. Duplicate-edge decision

**Frozen rule:** two rows with the exact same `(from_goal_id,
to_goal_id, relation_type)` are not allowed. A different `relation_type`
between the same Goal pair, or the same type in the reverse direction,
**is** allowed - direction and type both matter to identity.

Enforced at two independent layers:
- **Application**: `storage::goal_relation_repo::relation_exists` (a
  real database read) checked by `services::link_goals` before
  persistence, reporting `ValidationError::DuplicateGoalRelation`
  through the normal channel
  (`req_gr9_exact_duplicate_rejected_before_persistence`).
- **Database**: `UNIQUE(from_goal_id, to_goal_id, relation_type)`,
  proven by a raw SQL insert bypassing `LinkGoals`
  (`req_gr15_raw_sql_duplicate_rejected_by_unique`).

Proven distinct-and-allowed cases:
`req_gr10_different_relation_types_for_same_pair_both_succeed` (`A
SUPPORTS B` + `A DEPENDS_ON B` coexist) and
`req_gr11_reversed_direction_is_a_distinct_stored_relation` (`A SUPPORTS
B` + `B SUPPORTS A` coexist).

---

## 8. Hard-delete RESTRICT decision

The architecture freeze required cascade-vs-block behavior to be
resolved for `GoalRelation`'s endpoints before M2 completes. **Resolved:
`ON DELETE RESTRICT` on both `from_goal_id` and `to_goal_id`, not
cascade.**

**Rationale:**
- `Goal` currently has no hard `DeleteGoal` action in the frozen action
  set - only soft-state transitions exist (`LifecycleStatus` changes).
- A relation must never silently disappear because a Goal row was
  removed - history/graph integrity should be preserved.
- Any future real Goal-deletion action must explicitly deal with its
  relations first (e.g. requiring them to be removed or reassigned),
  not have them vanish as a side effect.
- `LifecycleStatus` transitions (`PAUSED`/`COMPLETED`/`ABANDONED`) never
  delete the Goal row, so they never interact with this constraint at
  all - relations survive every status transition trivially.

**Proven, not just documented:**
`req_gr26_raw_hard_delete_of_referenced_goal_is_restricted` - creates a
relation `A -> B`, then attempts a raw SQL `DELETE FROM goals WHERE id =
A` directly (bypassing the production action layer entirely, since no
`DeleteGoal` action exists to call) - SQLite itself rejects the delete
with a constraint-violation error, `Goal A` remains present, and the
relation row remains present. No dangling relation is ever created.

---

## 9. V3 migration

`src-tauri/migrations/V3__create_goal_relations.sql` - neither
`V1__create_goals.sql` nor `V2__create_projects.sql` was edited.

```sql
CREATE TABLE goal_relations (
    id, from_goal_id REFERENCES goals(id) ON DELETE RESTRICT,
    to_goal_id REFERENCES goals(id) ON DELETE RESTRICT,
    relation_type CHECK (... IN (11 frozen values ...)),
    note, created_at,
    CHECK (from_goal_id <> to_goal_id),
    UNIQUE (from_goal_id, to_goal_id, relation_type)
);
CREATE INDEX idx_goal_relations_from_goal_id ON goal_relations (from_goal_id);
CREATE INDEX idx_goal_relations_to_goal_id ON goal_relations (to_goal_id);

CREATE TABLE goal_relation_audit_log (
    id, goal_relation_id REFERENCES goal_relations (id),
    action_type CHECK (action_type IN ('LinkGoals')),
    actor, provenance, confirmation_class, previous_snapshot, occurred_at
);
CREATE INDEX idx_goal_relation_audit_log_goal_relation_id ON goal_relation_audit_log (goal_relation_id);
```

(Abbreviated for readability - the real file has the exact `NOT NULL`/
type declarations.) No speculative indexes beyond `from_goal_id`/
`to_goal_id` were added. No `milestones`/`tasks`/Personal-Knowledge
tables were created.

---

## 10. LinkGoals implementation

`actions::LinkGoalsPayload { from_goal_id, to_goal_id, relation_type,
note }` + `actions::validate_link_goals` (pure: self-link check, and
`note` length reusing the existing `DESCRIPTION_MAX_LEN` policy via a
new, honestly-named `ValidationError::NoteTooLong` rather than reusing
`DescriptionTooLong` on an unrelated field).

`services::link_goals(conn: &mut Connection, meta, payload)`: pure
validation → `from_goal_id` existence → `to_goal_id` existence →
exact-duplicate check → confirmation classification → block if
`Inferred` and unconfirmed → build `GoalRelation` → open transaction →
insert relation → insert audit row → commit. There is **no**
`CreateGoalRelation` action - `LinkGoals` is the entire frozen action
for this graph, exactly as specified.

---

## 11. Confirmation classification

`LinkGoals`'s confirmation floor is `EXPLICIT_ADD`, per the
architecture-freeze report §F's confirmation table, which lists
`LinkGoals` explicitly. Same mechanism as `CreateGoal`/`CreateProject`:
`effective_class(ExplicitAdd, meta.provenance)` - an `Explicit` caller
gets `EXPLICIT_ADD` and executes immediately;
`Provenance::Inferred` raises the effective class to `INFERRED_CHANGE`
(higher severity than the `EXPLICIT_ADD` floor), which blocks until
confirmed.

**Proven end-to-end, not just asserted:**
`req_gr19_20_inferred_link_goals_requires_confirmation_then_confirmed_resubmission_succeeds`
- an `Actor::CloudLlm`/`Provenance::Inferred`, unconfirmed `LinkGoals`
call returns `ActionOutcome::ConfirmationRequired { confirmation_class:
InferredChange }` and writes **zero** rows; a resubmission with
`confirmed: true` then succeeds and persists exactly one row. Also
covered at the pure-logic level by
`actions::tests::link_goals_inferred_provenance_becomes_inferred_change`.

---

## 12. Endpoint validation

Deterministic, DB-aware validation runs in `services::link_goals`
**before** the transaction begins:

1. `storage::goal_repo::goal_exists(conn, from_goal_id)` - if false,
   `ValidationError::RelationFromGoalNotFound`.
2. `storage::goal_repo::goal_exists(conn, to_goal_id)` - if false,
   `ValidationError::RelationToGoalNotFound`.
3. `storage::goal_relation_repo::relation_exists(...)` - if true,
   `ValidationError::DuplicateGoalRelation`.

Each check is deterministic and ordered (from before to before
duplicate) - if multiple endpoints are missing, the caller gets a
structured `RelationFromGoalNotFound` first, never a random first SQL
exception. `goal_exists` itself was moved from `project_repo` into
`goal_repo` (its natural home - a Goal-repo concern) during this run,
since it now has three callers (`create_project`, and `link_goals`
twice) rather than being duplicated a second and third time.

---

## 13. DB FK/CHECK/UNIQUE defense-in-depth

All five proven with raw SQL that intentionally bypasses `LinkGoals`
entirely - kept local to the test module, never exposed as a production
function:

| Constraint | Test |
|---|---|
| Nonexistent `from_goal_id` → FK failure | `req_gr12_raw_sql_missing_from_goal_rejected_by_fk` |
| Nonexistent `to_goal_id` → FK failure | `req_gr13_raw_sql_missing_to_goal_rejected_by_fk` |
| Self-link → CHECK failure | `req_gr14_raw_sql_self_edge_rejected_by_check` |
| Duplicate `(from, to, type)` → UNIQUE failure | `req_gr15_raw_sql_duplicate_rejected_by_unique` |
| Invalid `relation_type` text → CHECK failure | `req_gr16_raw_sql_invalid_relation_type_rejected_by_check` |

Each asserts `rusqlite::Error::SqliteFailure` with `err.code ==
rusqlite::ErrorCode::ConstraintViolation`, and that zero rows were
written. Application-level checks (§12) remain the friendlier first line
of defense for normal `LinkGoals` callers - the same two-layer pattern
established for `Project.goal_id` in the Project/storage integrity
hardening report.

---

## 14. Atomic relation+audit proof

`link_goals` uses the identical `&mut Connection` + `rusqlite::
Transaction` + `CoreError`-on-any-failure shape as `create_goal`/
`create_project`/`update_progress`: relation insert and audit insert
both go through `&tx`; `tx.commit()` only runs if both succeeded.

**Proven with the same forced-failure trigger technique**, retargeted at
`goal_relation_audit_log`:

```sql
CREATE TRIGGER force_goal_relation_audit_failure
BEFORE INSERT ON goal_relation_audit_log
BEGIN SELECT RAISE(FAIL, 'forced goal relation audit failure'); END;
```

`req_gr17_link_goals_audit_failure_rolls_back_relation_insert`: installs
the trigger, calls `link_goals`, asserts a typed `StorageError` with
`kind() == CoreErrorKind::Storage`, asserts zero relation rows and zero
audit rows survived, removes the trigger, then calls `link_goals` again
to prove the connection remains usable after the rollback. No `let _ =
insert_audit_entry(...)` anywhere - the audit write is never
best-effort.

`req_gr18_audit_row_contains_required_metadata` separately confirms the
audit row carries `action_type = 'LinkGoals'`, `actor = 'UI'`,
`provenance = 'Explicit'`, `confirmation_class = 'EXPLICIT_ADD'`.

---

## 15. Hierarchy PARENT_OF convention proof

Covered fully in §5, above - `req_gr23`/`req_gr24`/`req_gr11`.

---

## 16. Read-side behavior

`services::get_goal_relation(conn, id)` and
`services::get_goal_relations_for_goal(conn, goal_id)` - the latter
queries `WHERE from_goal_id = ?1 OR to_goal_id = ?1`, returning the
actual stored rows in whichever direction they were written; no inverse
row is ever fabricated at read time either.
`req_gr21_22_get_goal_relations_for_goal_finds_incoming_and_outgoing`
proves both directions are found for one Goal in a single call. No
public capability transport was added.

---

## 17. V2→V3 compatibility

`req_gr3_v2_populated_database_upgrades_cleanly_to_v3`: builds a
database via the existing test-only `bootstrap_db_at_version(..., 2)`
helper (real `Goal` rows, a real `Project` linked to a Goal - the actual
shape of an existing user's database before this update), confirms
`goal_relations` does not yet exist, reopens normally (`bootstrap_db_at`,
applying only the newly-pending V3 migration), and confirms: the Goal's
title is unchanged, the Project's `goal_id` link is unchanged,
`goal_relations` now exists, and a new `LinkGoals` between the
pre-existing Goals succeeds.

`req_gr2_fresh_v1_v2_v3_database_works_end_to_end` separately proves a
brand-new database applies V1→V2→V3 cleanly and Goal/Project/
GoalRelation all work together in one session.

---

## 18. Exact test/build results

All commands run from `D:\Dev\LiFeOS\src-tauri` unless noted, PowerShell,
`cargo 1.98.1`.

| Command | Result |
|---|---|
| `cargo fmt --check` | Initially FAILED (pure formatting diffs - line wrapping in `services/tests.rs` - no logic changes); `cargo fmt` applied, re-ran `cargo fmt --check` → **PASS** |
| `cargo check --tests` | **PASS** - `Finished \`dev\` profile [unoptimized + debuginfo] target(s)`, zero warnings (confirmed via a warning/error-only filtered rerun) |
| `cargo test -- --test-threads=1` | **PASS** - `running 104 tests` / `test result: ok. 104 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`. Before this run: 73. After: 104 = 73 + 31 new: 23 GoalRelation integration test *functions* (covering all 26 numbered integration requirements - `req_gr4_5`/`req_gr19_20`/`req_gr21_22` each combine 2 numbered requirements that share one natural setup) + 5 direct `LinkGoals` action-layer unit + 3 direct `GoalRelationType` unit. All 73 previous tests still pass unmodified - the regression requirement (§17 of the brief) is satisfied. |
| `cargo build --release` | **PASS** - `Finished \`release\` profile [optimized] target(s) in 2m 35s` |
| `npm.cmd run build` (repo root) | **PASS** - `tsc && vite build` succeeded, `✓ 27 modules transformed`, `✓ built in 118ms`. No frontend files touched this run. |
| `npm.cmd run tauri build` | **PASS** - run because V3 changes the production schema. Run in the background, verified genuinely by reading the captured log file directly (not the background wrapper's reported exit code): the log's own `$LASTEXITCODE` line reads `EXIT_CODE=0`. `Finished \`release\` profile [optimized] target(s) in 2m 36s`, `Finished 2 bundles at: ...msi\LifeOS_0.1.0_x64_en-US.msi` and `...nsis\LifeOS_0.1.0_x64-setup.exe`. Both files independently confirmed present on disk: `LifeOS_0.1.0_x64_en-US.msi` (2,973,696 bytes) and `LifeOS_0.1.0_x64-setup.exe` (1,370,874 bytes), timestamps matching the build run. |
| Module-boundary probe | `fn illegal_boundary_probe() { let _ = services::storage::goal_relation_repo::insert_goal_relation; }` added temporarily to `lib.rs`, `cargo check` → `error[E0603]: module 'storage' is private`, confirmed, probe removed, `cargo check --tests` re-run clean. |

**Exact new total: 104** (73 unchanged + 31 new).

---

## 19. Files changed

**New:**
- `src-tauri/migrations/V3__create_goal_relations.sql`
- `src-tauri/src/services/storage/goal_relation_repo.rs`
- `docs/reports/REPORT_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md`

**Modified:**
- `src-tauri/src/domain/mod.rs` - added `GoalRelationId`, `GoalRelationType`
  (11 variants), `GoalRelation`.
- `src-tauri/src/domain/tests.rs` - 3 new `GoalRelationType` round-trip
  unit tests.
- `src-tauri/src/actions/mod.rs` - added `LinkGoalsPayload`,
  `validate_link_goals`, `validate_note`; added
  `ValidationError::{NoteTooLong, SelfGoalRelation,
  RelationFromGoalNotFound, RelationToGoalNotFound,
  DuplicateGoalRelation}`.
- `src-tauri/src/actions/tests.rs` - 5 new `LinkGoals` unit tests.
- `src-tauri/src/services/mod.rs` - added `link_goals`,
  `get_goal_relation`, `get_goal_relations_for_goal`; updated
  `create_project`'s `goal_exists` call site to `storage::goal_repo::
  goal_exists`.
- `src-tauri/src/services/tests.rs` - 23 new GoalRelation integration
  test functions (covering 26 numbered requirements) and their helpers.
- `src-tauri/src/services/storage/mod.rs` - added `pub(super) mod
  goal_relation_repo;`; updated module doc comment for V3.
- `src-tauri/src/services/storage/goal_repo.rs` - added `goal_exists`
  (moved from `project_repo.rs`, now shared by `project_repo` and
  `goal_relation_repo`).
- `src-tauri/src/services/storage/project_repo.rs` - removed its own
  `goal_exists` (now calls `super::goal_repo::goal_exists` via
  `services::create_project`, not directly - `project_repo` itself no
  longer defines this function).
- `src-tauri/src/lib.rs` - illegal-boundary probe added and removed,
  net diff is none.
- `docs/LIFEOS_SPEC.md` - §5.2 rewritten (frozen/implemented status,
  taxonomy correction), §5.3 cross-reference added, top-level status
  banner, §5 status line, §12 milestone entry.
- `docs/PROJECT_STATE.md` - new §4f (GoalRelation vertical slice),
  renumbered §4g; §1/§2/§7/§8/§9/§10 updated for the new test count,
  commit list, file tree, and known-problems changes.
- `docs/reports/REPORT_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md` - §14
  corrected in place (§2, above).

No changes to: `Cargo.toml`/`Cargo.lock` (no dependency changes),
`src-tauri/migrations/V1__create_goals.sql`/`V2__create_projects.sql`
(untouched, historical), `services/storage/spike/*` (untouched, still
passing as regression), any file under `src/` (React frontend,
untouched), `scripts/setup-windows-storage-env.ps1` (untouched),
`CLAUDE.md` (no operating-policy change this run).

---

## 20. Remaining open issues

- **Neither Goal, Project, nor GoalRelation is wired into the running
  app** - unchanged, no Tauri command layer exists yet.
- **`GoalRelation` has no read capability beyond
  `get_goal_relation`/`get_goal_relations_for_goal`** - no graph
  traversal, no cycle detection, no conflict surfacing. Deliberately out
  of scope (§19 of the brief) - that is M6 Goal Intelligence *reasoning*
  over the graph structure this run only makes available.
- **The Project action-set gap remains open** (documented in the
  Project/storage hardening report, unchanged by this run) -
  `GoalRelation` has no analogous gap: `LinkGoals` is its entire frozen
  action set, and there is no expectation of an "UpdateGoalRelation" or
  "DeleteGoalRelation."
- **Everything already listed as a risk in the Goal hardening,
  Project vertical-slice, and Project/storage hardening reports**
  (`target_date` content validation, `CoreError`'s `Debug` exposing
  `diagnostic`, clean-machine runtime inferred from `dumpbin`, exact
  NeXa transport/confirmation UX, FK enforcement depending on
  `open_encrypted` being the sole connection-opening path) is unchanged
  by this run.

---

## 21. Exact recommended next M2 slice

A fourth domain entity. Two natural candidates given what already
exists: **`Milestone`/`Task`** (both need Project's frozen
`AddMilestone`/`AddTask` actions, not yet implemented - would close that
part of Project's action set) or the **Personal Knowledge Layer**
(`Entity`/`Fact`/`Relation`/`Episode`/`Evidence`/`Source` - note this
`Relation` is the genuinely-free-string one, §5.3, not another
`GoalRelation`). Whichever is chosen, reuse `domain → actions → services
→ services::storage::<entity>_repo` with the atomicity/typed-error
discipline applied from the start (as Project and GoalRelation both
did - neither needed a separate hardening pass, unlike Goal). Do not
invent product rules inside the implementation - this run's own §6/§7/§8
show the right way to handle a genuine architecture decision: freeze the
rule explicitly *before* writing code, document the reasoning, and prove
it at both the application and database level.
