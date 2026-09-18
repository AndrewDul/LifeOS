# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it
whenever project state actually changes (`CLAUDE.md`, Documentation
policy) — not only as an end-of-run formality.

Last updated: 2026-09-18 (M2 Key Lifecycle Determinism and Failure
Atomicity Hardening, R016 - a focused correctness/verification pass on
top of R015, **not** a new entity/action. Fixes two review findings
against R015's own implementation, found by re-inspecting R015's actual
code rather than trusting its report text: (A) R015's Case-A
orphan-credential cleanup deleted a freshly-generated credential on ANY
subsequent database-initialization failure, even one occurring AFTER a
database file had already been created under that key - which could
self-inflict "database exists, credential absent" (Case C) as a side
effect of its own cleanup path; corrected so the credential is deleted
only if the database path still does not exist afterward. (B) R015's
key-lifecycle tests ran against the real OS keyring and could return
early as "BLOCKED" (still counted as PASS by Cargo) - not sufficient
deterministic proof of the canonical A/B/C/D state machine; a new
`security::CredentialStore` seam + deterministic in-memory test double
now proves all four cases, orphan cleanup (both pre- and
post-file-creation), and three backend-error paths independent of OS
keyring availability (12 new tests). Also hardened
`serialize_required_snapshot`'s diagnostic to structurally exclude the
underlying serializer's raw error text (previously safe only by
assumption about `serde_json`'s current wording). See §4n. On top of
2026-09-18's M2 Storage and Audit Safety Hardening (R015, §4m),
2026-09-17's Goal vertical slice/storage hardening, Goal hardening
pass, Project vertical slice, Project/storage integrity hardening,
GoalRelation vertical slice, Project Child Action Freeze pass,
Milestone vertical slice, Task vertical slice, Task parent-validation
hardening, and UpdateProject implementation).

---

## 1. Milestone status

| Milestone | Status |
|---|---|
| M0 — Environment ready | **DONE** (§5) |
| M1 — Visual shell | **ACCEPTED BY USER on 2026-09-17** (§3) |
| M2 — LifeOS Core + encrypted storage | Architecture frozen; storage PROVEN + hardened; **Goal, Project, GoalRelation, Milestone, and Task all implemented; the Project action-set gap is now fully closed (`UpdateProject` implemented)** 2026-09-17 (§4); **master-key lifecycle + mandatory audit-snapshot safety hardened** 2026-09-18 (§4m), **then verified deterministically + corrected for a post-file-creation cleanup edge case** 2026-09-18 (§4n); rest of domain model not started |
| M3-M10 | Not started |

**M1 is closed** — unchanged, not touched this run.

**M2 now has five real, working, tested domain entities: Goal,
Project, GoalRelation, Milestone, and Task**, end to end through
`domain → actions → services → private services/storage → encrypted
SQLCipher`. Ten runs on 2026-09-17 built this: the Goal vertical slice
(storage foundation hardening + Goal implemented), a dedicated Goal
hardening pass (fixed 7 issues before the pattern was reused), the
**Project vertical slice** (second entity, built directly with the
already-hardened pattern), a **Project/storage integrity hardening
pass** (SQLite FK enforcement genuinely enabled, `Project.progress`
canonically resolved, `GoalStatus` renamed to `LifecycleStatus`, an
action-set gap documented), the **GoalRelation vertical slice** (third
entity, the first true relationship-graph structure - two foreign keys
into `goals`, a frozen 11-value relation-type enum, the `LinkGoals`
action - built directly with the twice-hardened pattern, no separate
hardening pass needed), a **documentation-only Project Child Action
Freeze pass** (`UpdateProject` frozen, closing the Project action-set
gap; `UpdateMilestone`/`UpdateTask` frozen pre-emptively before either
entity exists in code), the **Milestone vertical slice** (fourth
entity, the first required-parent child entity - `AddMilestone`/
`UpdateMilestone`, `(project_id, order_index)` UNIQUE resolved, built
directly with the pattern - no separate hardening pass needed), and the
**Task vertical slice** (fifth entity, the first with two independent
nullable parent references - `AddTask`/`UpdateTask`, `recurring_rule_id`
deliberately deferred behind a database-enforced NULL-only `CHECK`,
built directly with the pattern), a ninth, **Task parent-validation
hardening pass** (`add_task`/`update_task` now independently validate
both `project_id`/`goal_id` and aggregate both errors in stable order
when both are invalid - a review finding, not a data-integrity bug; see
§4k), and a tenth, the **UpdateProject implementation** (the last
already-frozen, shovel-ready action on an existing entity -
`title`/`description`/`status`/`priority`/`goal_id` now all have a
working update path, floor `IMPORTANT_CHANGE`, `progress` stays
exclusively `UpdateProgress`'s; required an unplanned but authorized V6
migration to widen `project_audit_log`'s `action_type` CHECK - see
§4l). Next step needs an explicit decision - see §9.

---

## 2. What exists right now

A Tauri 2 + React + TypeScript desktop app that launches in a real
window (accepted M1 shell, untouched), plus a working, hardened
encrypted store for five domain entities:

- **Goal**: create and update goals with deterministic validation,
  atomic mutation+audit transactions, tri-state patch semantics
  (Set/Unchanged/Clear), typed storage errors, confirmation
  classification, and a read API.
- **Project**: create projects (optionally linked to an existing Goal by
  `goal_id`, validated to actually exist before persistence), update
  their `title`/`description`/`status`/`priority`/`goal_id` via
  `UpdateProject` (floor `IMPORTANT_CHANGE`; `description`/`priority`/
  `goal_id` use `FieldPatch<T>`, `title`/`status` plain `Option<T>`; no
  terminal-state rule - `ACTIVE`/`PAUSED`/`COMPLETED`/`ABANDONED` are
  freely reversible peer states), and update their progress separately
  via `UpdateProgress` (floor `EXPLICIT_ADD`, exclusively owns
  `progress` - the two actions' payloads/audit semantics are never
  merged), same atomic-transaction/typed-error discipline from the
  start, its own `project_audit_log`, and a read API. The first *direct
  foreign key* cross-entity relationship in production M2
  (`Project.goal_id -> Goal.id`).
- **GoalRelation**: link two existing Goals via `LinkGoals` with one of
  11 frozen relation types (`SUPPORTS`/`CONFLICTS_WITH`/`DEPENDS_ON`/
  `BLOCKS`/`CONTRIBUTES_TO`/`REQUIRES`/`PARENT_OF`/`CHILD_OF`/
  `DUPLICATES`/`SUPERSEDES`/`COMPETES_FOR_RESOURCE`) and an optional
  note - the first true relationship-*graph* structure (two foreign keys
  into `goals`, both `ON DELETE RESTRICT`), directional (no automatic
  inverse rows), self-links and exact duplicates rejected before
  persistence, same atomic-transaction/typed-error discipline, its own
  `goal_relation_audit_log`, and a narrow read API
  (`get_goal_relation(id)`/`get_goal_relations_for_goal(goal_id)`).
- **Milestone**: add a Milestone to an existing Project via
  `AddMilestone` (always defaults to `status = PENDING`; `project_id`
  validated to actually exist before persistence; `order_index`
  validated non-negative and not already occupied within the same
  Project) and edit it via `UpdateMilestone` (`title`/`status`/
  `order_index` plain `Option<T>`, `target_date` the one genuinely
  nullable field using `FieldPatch<T>` - the first entity to apply the
  corrected patch-semantics rule, §4i) - the first *required-parent*
  child entity (`milestones.project_id REFERENCES projects(id) ON
  DELETE RESTRICT`, non-nullable). `PENDING`/`DONE` are freely
  reversible peer states, no terminal-state rule. `(project_id,
  order_index)` is UNIQUE (different Projects may freely reuse the same
  values), enforced both at the Core validation layer and by a real SQL
  `UNIQUE` constraint. Same atomic-transaction/typed-error discipline,
  its own `milestone_audit_log`, and a narrow read API
  (`get_milestone(id)`/`get_milestones_for_project(project_id)`,
  deterministic `ORDER BY order_index ASC`).
- **Task**: add a Task via `AddTask` (always defaults to
  `status = TODO`, `recurring_rule_id = None`; optional `project_id`/
  `goal_id`, each independently validated to actually exist when
  supplied) and edit it via `UpdateTask` (`title`/`status` plain
  `Option<T>`, `due_at`/`project_id`/`goal_id` all `FieldPatch<T>` -
  the first entity with more than one genuinely nullable field) - the
  first entity with **two independent, both-nullable** parent
  references (`tasks.project_id`/`tasks.goal_id`, both `ON DELETE
  RESTRICT`); a Task may be standalone, Project-only, Goal-only, or
  linked to both, and neither parent is ever inferred from the other.
  All four `TaskStatus` values (`TODO`/`IN_PROGRESS`/`DONE`/
  `CANCELLED`) are freely reversible peer states, no terminal-state
  rule. `recurring_rule_id` is a frozen but deliberately unimplemented
  field - a typed `RecurringRuleId` placeholder exists, but the real
  `RecurringRule` entity does not; the database enforces
  `recurring_rule_id IS NULL` via a temporary `CHECK` until that future
  entity is designed. Same atomic-transaction/typed-error discipline,
  its own `task_audit_log`, and a narrow read API (`get_task(id)`/
  `get_tasks_for_project(project_id)`/`get_tasks_for_goal(goal_id)`,
  deterministic `due_at ASC, created_at ASC, id ASC` - a technical
  ordering, not product prioritization).

All five are backed by a database that genuinely enforces referential
integrity at the SQLite level (`PRAGMA foreign_keys = ON`, set on every
connection `open_encrypted` produces - not just declared in schema and
left unenforced, §4e), on top of the already-proven SQLCipher encryption
and keyring master-key lifecycle.

262 automated tests prove all of this (9 storage-spike regression, 17
Goal-slice/hardening integration, 16 direct Goal action-layer unit, 14
Project integration, 10 direct Project action-layer unit, 4 Project/
storage integrity integration, 3 direct `LifecycleStatus` unit, 23
GoalRelation integration, 5 direct `LinkGoals` action-layer unit, 3
direct `GoalRelationType` unit, 24 Milestone integration covering 32
numbered requirements, 17 direct Milestone/`AddMilestone`/
`UpdateMilestone` action-layer unit, 3 direct `MilestoneStatus` unit, 28
Task integration covering 52 numbered requirements, 15 direct
Task/`AddTask`/`UpdateTask` action-layer unit, 3 direct `TaskStatus`
unit, 6 Task parent-validation-hardening integration tests, 14
UpdateProject integration functions covering 33 numbered requirements
plus an unplanned V6 migration-compatibility proof, 19 direct
`UpdateProject` action-layer unit tests, and (R015, 2026-09-18) 17 new
M2 Storage and Audit Safety Hardening tests - 10 key-lifecycle
(`req_kl1`-`req_kl10`, all four DB/key-existence cases, orphan-
credential cleanup, and a static scan confirming no test touches the
real production credential identity), 1 production-bootstrap
regression test, and 6 snapshot-deserialization regression tests
(`req_snap1`-`req_snap6`, one per mandatory-snapshot update action),
plus (R016, 2026-09-18) 12 new deterministic key-lifecycle tests
(`req_dkl1`-`req_dkl12`, driven through an in-memory
`security::CredentialStore` test double rather than the real OS
keyring - proving all four DB/key cases, orphan-credential cleanup
both before AND after a database file is created, and three
credential-backend error paths, independent of OS keyring
availability). **Not wired into the running app yet** - no Tauri
command layer exists, so the UI cannot create/see real goals,
projects, relations, milestones, or tasks yet.

Repository contents as of this run:

```
LiFeOS/
├── .git/, .gitignore, .vscode/extensions.json, CLAUDE.md, README.md
├── scripts/
│   └── setup-windows-storage-env.ps1   (new - see §4b)
├── docs/
│   ├── LIFEOS_SPEC.md, RESEARCH_DECISIONS.md, PROJECT_STATE.md
│   └── reports/    R001_M1_UI_REFINEMENT_HOME_DRAWERS_AND_CHAT_SIDEBAR_2026-09-17.md
│                    R002_M1_NAVIGATION_CORRECTION_FULLSCREEN_EDGE_TRANSITIONS_2026-09-17.md
│                    R003_M2_ARCHITECTURE_FREEZE_2026-09-17.md
│                    R004_M2_STORAGE_SPIKE_2026-09-17.md
│                    R005_M2_GOAL_VERTICAL_SLICE_2026-09-17.md
│                    R006_M2_GOAL_HARDENING_2026-09-17.md
│                    R007_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md
│                    R008_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md
│                    R009_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md
│                    R010_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md
│                    R011_M2_MILESTONE_VERTICAL_SLICE_2026-09-17.md
│                    R012_M2_TASK_VERTICAL_SLICE_2026-09-17.md
│                    R013_M2_TASK_PARENT_VALIDATION_HARDENING_2026-09-17.md
│                    R014_M2_UPDATE_PROJECT_IMPLEMENTATION_2026-09-17.md
│                    R015_M2_STORAGE_AND_AUDIT_SAFETY_HARDENING_2026-09-18.md
│                    R016_M2_KEY_LIFECYCLE_DETERMINISM_AND_FAILURE_ATOMICITY_HARDENING_2026-09-18.md
├── index.html, vite.config.ts, tsconfig*.json, package.json
├── public/, src/    (React frontend - M1, accepted, untouched)
└── src-tauri/
    ├── migrations/V1__create_goals.sql          (PRODUCTION schema - Goal only, untouched)
    │              V2__create_projects.sql        (PRODUCTION schema - Project only, untouched)
    │              V3__create_goal_relations.sql  (PRODUCTION schema - GoalRelation only, untouched)
    │              V4__create_milestones.sql      (PRODUCTION schema - Milestone only, untouched)
    │              V5__create_tasks.sql           (PRODUCTION schema - Task only, untouched)
    │              V6__widen_project_audit_log_action_types.sql (new - widens
    │                             project_audit_log.action_type's CHECK to allow
    │                             'UpdateProject'; projects itself untouched, see
    │                             R014_M2_UPDATE_PROJECT_IMPLEMENTATION_2026-09-17.md §3a)
    ├── src/
    │   ├── main.rs, lib.rs       (run() unchanged; declares the 5 modules, all private)
    │   ├── core_error.rs          CoreError/CoreErrorKind - typed Core-boundary error model
    │   │                          (Storage/Migration/Security/Internal)
    │   ├── domain/
    │   │   ├── mod.rs              Goal, GoalId, Project, ProjectId, GoalRelation,
    │   │   │                       GoalRelationId, Milestone, MilestoneId, Task, TaskId,
    │   │   │                       RecurringRuleId (new, all UUID v7), LifecycleStatus
    │   │   │                       (shared by Goal.status/Project.status),
    │   │   │                       GoalRelationType (frozen closed 11-value enum, NOT
    │   │   │                       the same thing as the Personal Knowledge Layer's
    │   │   │                       separate free-string Relation.relation_type),
    │   │   │                       MilestoneStatus (frozen closed 2-value enum, NOT
    │   │   │                       LifecycleStatus), TaskStatus (new - frozen closed
    │   │   │                       4-value enum), OrderIndex/ORDER_INDEX_MIN, Progress,
    │   │   │                       Priority (shared)
    │   │   └── tests.rs             12 round-trip unit tests (3 LifecycleStatus, 3
    │   │                            GoalRelationType - all 11 values/parse round trips,
    │   │                            3 MilestoneStatus, 3 new TaskStatus)
    │   ├── actions/
    │   │   ├── mod.rs              Actor, Provenance, ActionMeta, ConfirmationClass,
    │   │   │                       ActionOutcome<T>, ValidationError (+
    │   │   │                       ReferencedGoalNotFound, SelfGoalRelation,
    │   │   │                       RelationFromGoalNotFound, RelationToGoalNotFound,
    │   │   │                       DuplicateGoalRelation, NoteTooLong,
    │   │   │                       ReferencedProjectNotFound, OrderIndexOutOfBounds,
    │   │   │                       DuplicateMilestoneOrderIndex - reused directly for
    │   │   │                       Task AND UpdateProject, no new variants needed),
    │   │   │                       FieldPatch<T>, CreateProjectPayload/
    │   │   │                       UpdateProjectPayload (new)/UpdateProgressPayload/
    │   │   │                       LinkGoalsPayload/AddMilestonePayload/
    │   │   │                       UpdateMilestonePayload/AddTaskPayload/
    │   │   │                       UpdateTaskPayload, blocks_until_confirmed
    │   │   └── tests.rs             82 direct action-layer unit tests (16 Goal, 10
    │   │                            Project, 5 LinkGoals, 17 AddMilestone/
    │   │                            UpdateMilestone, 15 AddTask/UpdateTask, 19 new
    │   │                            UpdateProject)
    │   ├── security/mod.rs        OS-keyring master-key lifecycle. narrow pub(crate)
    │   │                          primitives (get_existing_key/generate_and_store_key/
    │   │                          delete_key, R015) compose into services::
    │   │                          resolve_master_key's four-state DB/key matrix; the old
    │   │                          DB-existence-blind get_or_create_production_master_key
    │   │                          is removed. (R016) new CredentialStore trait +
    │   │                          KeyringCredentialStore production adapter - the free
    │   │                          primitives now delegate to it (unchanged behavior);
    │   │                          generate_key_hex widened to pub(crate)
    │   └── services/
    │       ├── mod.rs              bootstrap_production_db -> bootstrap_db_with_key_lifecycle
    │       │                       (R015) -> (R016) bootstrap_db_with_key_lifecycle_with_store/
    │       │                       resolve_master_key_with_store, the real generic
    │       │                       state-machine cores (generic over CredentialStore +,
    │       │                       for the former, an injectable db initializer); the
    │       │                       old names are thin wrappers over these; corrected
    │       │                       cleanup rule (R016) - a freshly-generated Case A
    │       │                       credential is deleted only if the db path still does
    │       │                       NOT exist after an init failure, otherwise kept.
    │       │                       create_goal/update_goal, create_project/
    │       │                       update_project/update_progress/get_project,
    │       │                       link_goals/get_goal_relation/
    │       │                       get_goal_relations_for_goal, add_milestone/
    │       │                       update_milestone/get_milestone/
    │       │                       get_milestones_for_project, add_task/update_task/
    │       │                       get_task/get_tasks_for_project/get_tasks_for_goal
    │       │                       - all atomic (&mut Connection + rusqlite
    │       │                       transaction); all 5 update_* actions' previous_snapshot
    │       │                       via serialize_required_snapshot (R015 - typed
    │       │                       CoreError + zero mutation on failure, never a silent
    │       │                       .ok(); R016 - diagnostic redacted to a fixed string,
    │       │                       no longer interpolates the raw serde_json error text)
    │       │                       - capability API contract's names, no dedicated
    │       │                       capability/module yet (LIFEOS_SPEC.md §5.6/§10.1)
    │       ├── tests.rs            159 integration test functions against a real
    │       │                       encrypted DB (17 Goal, 14 Project, 4 Project/
    │       │                       storage integrity, 23 GoalRelation functions covering
    │       │                       26 numbered requirements, 24 Milestone functions
    │       │                       covering 32 numbered requirements, 28 Task functions
    │       │                       covering 52 numbered requirements - incl. 10
    │       │                       forced-audit-failure/raw-SQL-FK-bypass proofs across
    │       │                       all 5 entities, a V4-populated-database upgrades-to-V5
    │       │                       migration-compatibility test, 7 raw-SQL DB-defense-
    │       │                       in-depth proofs for Task's FK/CHECK constraints (incl.
    │       │                       the temporary recurring_rule_id IS NULL CHECK), 2
    │       │                       raw hard-DELETE RESTRICT proofs for Project/Goal, 6
    │       │                       Task parent-validation-hardening functions
    │       │                       (req_tpv_a-f), 14 UpdateProject functions (req_up*)
    │       │                       covering 33 numbered requirements plus an unplanned
    │       │                       V5-to-V6 migration-compatibility proof, (R015,
    │       │                       2026-09-18) 17 functions: 10 key-lifecycle
    │       │                       (req_kl1-req_kl10, all four DB/key cases + orphan-
    │       │                       cleanup + static production-identity scan, now kept as
    │       │                       a supplementary real-OS-keyring smoke suite), 1
    │       │                       production-bootstrap regression test, and 6
    │       │                       snapshot-deserialization regression tests
    │       │                       (req_snap1-req_snap6), and (R016, 2026-09-18) 12 new
    │       │                       deterministic key-lifecycle functions
    │       │                       (req_dkl1-req_dkl12, via InMemoryCredentialStore - all
    │       │                       four DB/key cases, orphan cleanup both pre- AND
    │       │                       post-file-creation, 3 credential-backend error paths -
    │       │                       independent of OS keyring availability) - incl. the
    │       │                       progress-preservation regression test and the
    │       │                       GoalRelation non-side-effect proof)
    │       └── storage/            PRIVATE (mod storage; - no pub at all)
    │           ├── mod.rs           generic encrypted-SQLite primitives + prod migrations;
    │           │                    open_encrypted calls enable_foreign_keys (sets +
    │           │                    verifies PRAGMA foreign_keys = ON on every
    │           │                    connection - production, test, and spike alike,
    │           │                    since all go through this one primitive); also
    │           │                    hosts actor_str/provenance_str/
    │           │                    confirmation_class_str/parse_rfc3339 shared by all
    │           │                    5 repo modules (not duplicated per-module)
    │           ├── goal_repo.rs     Goal SQL access, pub(in crate::services) - also
    │           │                    hosts goal_exists (shared by project_repo,
    │           │                    goal_relation_repo, AND task_repo's caller)
    │           ├── project_repo.rs  Project SQL access, pub(in crate::services) - also
    │           │                    hosts project_exists (shared by milestone_repo's
    │           │                    caller AND task_repo's caller) and update_project
    │           │                    (new - the general-purpose UpdateProject mutation;
    │           │                    never touches progress/created_at/id, kept fully
    │           │                    separate from update_project_progress)
    │           ├── goal_relation_repo.rs  GoalRelation SQL access, pub(in
    │           │                    crate::services)
    │           ├── milestone_repo.rs  Milestone SQL access, pub(in crate::services) -
    │           │                    incl. order_index_taken, parameterized to
    │           │                    optionally exclude a Milestone from its own
    │           │                    occupancy check
    │           ├── task_repo.rs     Task SQL access, pub(in crate::services) (new) -
    │           │                    incl. get_tasks_for_project/get_tasks_for_goal
    │           │                    (deterministic due_at ASC, created_at ASC, id ASC
    │           │                    ordering - documented as technical, not product
    │           │                    prioritization)
    │           └── spike/           #[cfg(test)] only - old storage-spike suite, moved
    │               ├── mod.rs, tests.rs, migrations/V1__init.sql, V2__add_note_column.sql
    ├── Cargo.toml, Cargo.lock, tauri.conf.json, build.rs
    ├── capabilities/default.json
    └── icons/
```

`node_modules/`, `dist/`, `src-tauri/target/` (including this run's
MSI/NSIS installers) are gitignored and untracked. **No generated user
database, no real credential-store secret, and no build artifact was
committed** - verified before committing (§7).

**`src-tauri/.cargo/config.toml` was deleted this run** (was a
committed, machine-specific hardcoded OpenSSL path) - see §4b.

---

## 3. M1 — ACCEPTED (unchanged this run)

Not touched. See prior entries in this file's git history and the two
refinement reports + navigation-correction report for full detail.

---

## 4. M2 — LifeOS Core + encrypted storage

### 4a. Storage hardened for production use (this run)

- **Spike/production separation:** `src-tauri/migrations/` now contains
  **only** the real Goal schema (`V1__create_goals.sql`). The old
  `spike_records`-creating migrations were moved (`git mv`, history
  preserved) to `src-tauri/src/services/storage/spike/migrations/`,
  reachable only from `#[cfg(test)]`-only code, never a release build.
  Proven by a real test (`services::tests::req3_production_db_has_no_spike_records`),
  not just asserted.
- **Module-boundary re-verified after restructuring:** the tree grew a
  level deeper (`services::storage::goal_repo`); the same "attempt the
  illegal access, capture the real compiler error" proof was repeated
  against the new shape - `error[E0603]: module 'storage' is private`,
  confirmed again, not assumed to still hold.
- **OpenSSL runtime dependency measured, not assumed:** `dumpbin
  /dependents` on the release binary shows **no `libcrypto`/`libssl`
  DLL** in its import table - OpenSSL is statically linked, so the
  *shipped* app has no runtime dependency on it (build-time only). MSI
  and NSIS installers rebuilt successfully with the larger dependency
  graph (+ `uuid`, `chrono`, `dirs`) and confirmed on disk.
- **Machine-specific config removed:** `src-tauri/.cargo/config.toml`
  (hardcoded absolute OpenSSL path) deleted. Replaced by
  `scripts/setup-windows-storage-env.ps1` - checks the standard install
  location, fails clearly if missing (with the exact `winget` command to
  fix it), and sets `OPENSSL_DIR`/`OPENSSL_LIB_DIR`/`OPENSSL_INCLUDE_DIR`
  as **persistent user environment variables** (no admin needed) if
  found. Run once per dev machine; a new terminal picks it up
  automatically.

Full evidence: `docs/reports/R005_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`
Parts A1-A3.

### 4b. Goal vertical slice implemented (2026-09-17, later hardened - §4c)

The first real domain entity, end to end:

- **Domain** (`domain::Goal`): `id` (UUID v7), `title`, `description`,
  `status` (`ACTIVE`/`PAUSED`/`COMPLETED`/`ABANDONED` - all four are
  peer states with no terminal-state restriction, corrected in the
  hardening pass, §4c), `target_date`, `priority_user`/`priority_system`
  (bounded 1-5), `progress` (bounded integer 0-100), `domain_tag`,
  `created_at`/`updated_at` (UTC).
- **Actions**: `CreateGoal` (floor `EXPLICIT_ADD`), `UpdateGoal` (floor
  `IMPORTANT_CHANGE`) - deterministic validation (title trim/non-empty/
  length, description/tag length, priority/progress bounds) runs
  *before* any write; zero partial writes on failure (tested
  explicitly, and since the hardening pass this extends to the goal
  write + its audit row being one atomic transaction, §4c).
- **Confirmation classification**: `effective_class = max(type_floor,
  provenance_class)`, implemented exactly per the frozen policy - no UI
  modals invented, just typed metadata (`ConfirmationClass`) a future M3
  UI can render against. Every persisted Create/Update writes a
  `goal_audit_log` row (previous-state JSON snapshot for updates) - the
  minimum state to support a future Undo, not a full event-sourcing
  system.
- **Read capability**: `services::get_active_goals()` (matching the
  frozen v0.1 capability API contract's `get_active_goals()` name) and
  an internal `get_goal(id)` - both go through `services` directly today
  only because `services::tests` is their only caller; a dedicated
  `capability` module is deferred until a real Tauri command or NeXa
  transport exists to front (see `LIFEOS_SPEC.md` §5.6/§10.1's
  hardening-run wording correction - the capability *contract* is
  frozen, the module implementing it is not built).
- **Production DB lifecycle**: `services::production_db_path()`
  (`%APPDATA%\com.lifeos.app\lifeos.db` via `dirs::data_dir()`, the same
  mechanism Tauri's own path API uses) + `security::
  get_or_create_production_master_key()` (service `LifeOS`, account
  `database-master-key-v1`, random 256-bit, OS keyring, idempotent,
  never logged) + `services::bootstrap_production_db()` (create dir,
  open, verify key, migrate). **Not called from anywhere yet** - no
  Tauri command/`run()` wiring this run.

Full evidence, exact SQL, and every test result:
`docs/reports/R005_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`.

### 4c. Goal hardening (2026-09-17, dedicated follow-up run)

Architectural review of the vertical slice above found several issues
that had to be fixed before the Goal pattern could safely be copied to
`Project` or any other entity. All were fixed in this repo, not
deferred:

1. **Atomic mutation + audit.** `CreateGoal`/`UpdateGoal` previously
   wrote the goal row, then called
   `let _ = storage::goal_repo::insert_audit_entry(...)` - silently
   discarding a failed audit write while still returning
   `ActionOutcome::Success`. Both now run inside one `rusqlite`
   transaction (`conn.transaction()`); either write failing means the
   transaction is never committed and `Transaction::drop` rolls it back
   automatically. `create_goal`/`update_goal` now take `&mut Connection`
   (transactions need unique access), not `&Connection`.
2. **Proven with a real forced-failure test, not just code inspection.**
   A `BEFORE INSERT ... RAISE(FAIL, ...)` SQLite trigger on
   `goal_audit_log`, installed only inside the test, forces the audit
   write to fail after the goal write already succeeded within the same
   transaction. Two tests
   (`req_h1_create_goal_audit_failure_rolls_back_goal_insert`,
   `req_h2_update_goal_audit_failure_rolls_back_goal_update`) prove zero
   goal rows/no goal mutation survive, the connection remains usable
   afterward, and the action returns a typed `StorageError`, never
   `Success`.
3. **Typed Core error model.** `ActionOutcome::StorageError(String)` is
   now `StorageError(CoreError)` (new `core_error` module) -
   `CoreError` carries a closed `CoreErrorKind` (`Storage`/`Migration`/
   `Security`/`Internal`) as its public, matchable identity; the
   underlying driver error text is kept only as an internal
   `diagnostic()` string, never part of `Display`, never the
   authoritative API.
4. **`FieldPatch<T>` tri-state patch semantics.** `UpdateGoalPayload`'s
   `description`/`target_date`/`priority_user`/`domain_tag` (the
   nullable `Goal` fields; `domain_tag` is new to the payload this
   pass) now use `FieldPatch<T> { Unchanged, Set(T), Clear }` instead of
   a plain `Option<T>` that could not distinguish "leave alone" from
   "clear to NULL". `title`/`status`/`progress` stay `Option<T>` since
   those `Goal` fields are non-nullable. Proven against a real DB
   (`req_h4_update_goal_field_patch_set_clear_unchanged`) and with
   direct validation unit tests.
5. **Removed the unapproved terminal-status rule.** The vertical slice
   had invented "`COMPLETED`/`ABANDONED` cannot transition further" -
   never part of the frozen Goal status definition. Removed
   (`GoalStatus::can_transition_to` and
   `ValidationError::InvalidStatusTransition` deleted); every
   `GoalStatus -> GoalStatus` transition an explicit action requests is
   now allowed and still audit-logged
   (`req_h3_completed_goal_can_be_reopened_and_transition_is_audited`).
6. **Direct action-layer unit tests added.** 16 new tests in
   `actions/tests.rs` exercise `validate_create_goal`/
   `validate_update_goal`/`effective_class` directly (no DB, no I/O) -
   the vertical-slice report's flagged gap ("actions only tested
   indirectly") is closed.
7. **`Provenance::Inferred` path proven end-to-end, not deferred to
   M5.** `req_h5_inferred_create_goal_requires_confirmation_and_does_not_write`
   proves `CreateGoal` with `Provenance::Inferred` returns
   `ConfirmationRequired` and writes zero rows, then that a confirmed
   resubmission persists.
8. **Documentation corrected** (`LIFEOS_SPEC.md`): top-level status
   banner, the `GoalRelation` "from the start" overclaim (§5.2 - the V1
   migration implements `Goal` only), and the capability-module wording
   (§5.6/§10.1 - the capability API *contract* is frozen, a `capability/`
   module implementing it is not built yet).

Preserved, not regressed: SQLCipher encryption, keyring master-key
lifecycle, production/spike migration separation, the private
`services::storage` compile-time boundary (re-verified again via the
illegal-access probe), UUID v7 Goal IDs, UTC timestamps, progress 0-100,
priority 1-5, the release/package build, `setup-windows-storage-env.ps1`,
and all 9 storage-spike regression tests.

Full evidence: `docs/reports/R006_M2_GOAL_HARDENING_2026-09-17.md`.

**Goal is now considered safe to use as the template for `Project`** -
see that report's §13 for the exact reasoning.

### 4d. Project vertical slice implemented (2026-09-17)

The second real domain entity, built directly with the hardened Goal
pattern - no separate hardening pass was needed:

- **Domain** (`domain::Project`): `id` (UUID v7, own `ProjectId`
  newtype - distinct from `GoalId` at the type level even though both
  wrap a `Uuid`), `title`, `description`, `status` (reuses
  `domain::LifecycleStatus`, renamed from `GoalStatus` in the
  Project/storage integrity hardening pass, §4e - the architecture
  freeze says "same 4 values as Goal", so a separate `ProjectStatus`
  enum would have duplicated an already-frozen type), `priority`
  (reuses the shared `Priority` type, singular unlike Goal's user/system
  split), `goal_id` (nullable `Option<GoalId>` - the first real
  cross-entity relationship in production M2), `progress` (reuses
  `Progress`, 0-100 - originally added as a flagged open interpretation
  since the architecture-freeze report's one-line Project field table
  did not list it explicitly; **canonically resolved** in the
  Project/storage integrity hardening pass, §4e - no longer an open
  question), `created_at`/`updated_at` (UTC).
- **Actions**: `CreateProject` and `UpdateProgress` only - the frozen
  Project action set also has `AddMilestone`/`AddTask`
  (architecture-freeze report §C), out of scope (they need
  `Milestone`/`Task`, which do not exist). **Both actions' confirmation
  floor is `EXPLICIT_ADD`** (architecture-freeze report §F's
  confirmation table lists both explicitly) - `UpdateProgress` is
  deliberately *not* `IMPORTANT_CHANGE` like `UpdateGoal`; this is a
  frozen difference, not an oversight.
- **Cross-entity reference validation**: `CreateProject`'s optional
  `goal_id` is checked against a real database read
  (`storage::project_repo::goal_exists`) *before* persistence, returning
  a friendly, structured `ValidationError::ReferencedGoalNotFound`
  rather than surfacing a raw SQL failure. At the time of this slice the
  SQL foreign-key constraint itself was declared but not enforced at
  runtime (`PRAGMA foreign_keys` was never set) - **fixed in the
  Project/storage integrity hardening pass, §4e**: the constraint is now
  genuinely enforced too, and the application-level check is kept
  deliberately as the first, friendlier line of defense, not the only
  one. Proven for all four required cases: no `goal_id` succeeds, a
  valid `goal_id` succeeds and the link survives close/reopen, a
  nonexistent `goal_id` is rejected with zero rows written, and reading
  the project back returns the same linked `GoalId`.
- **Atomicity from day one**: `create_project`/`update_progress` use the
  exact same `&mut Connection` + `rusqlite::Transaction` +
  `CoreError`-on-any-failure shape as the hardened Goal actions - proven
  again with two forced-audit-failure tests
  (`req_p13`/`req_p14`), not assumed to transfer automatically just
  because the code looks similar.
- **Migration compatibility proven, not assumed**: a dedicated test
  builds a database frozen at V1 only (real Goal data, no `projects`
  table - the actual shape of an existing user's database before this
  update), then reopens it normally (applying only the newly-pending V2
  migration) and confirms the original Goal data is unchanged and a new
  Project can reference it. `V1__create_goals.sql` itself was not
  edited.
- **DRY cleanup carried out alongside, not deferred**: `goal_repo.rs`'s
  `actor_str`/`provenance_str`/`confirmation_class_str`/`parse_rfc3339`
  helpers were promoted to `storage/mod.rs` (`pub(super)`, reachable by
  both repo modules) rather than duplicated into `project_repo.rs`;
  `actions::goal_blocks_until_confirmed` was renamed to
  `blocks_until_confirmed` (it was already entity-generic logic under a
  Goal-specific name) rather than adding a second, identical
  `project_blocks_until_confirmed`.
- **Read side stays narrow**: only an internal `get_project(id)` was
  added. The frozen capability API's minimum function set
  (architecture-freeze report §I) does not list any Project-specific
  read (no `get_projects()`/`get_active_projects()`) - only a generic,
  unspecified `get_progress()` - so no list/read capability was
  invented.

Full evidence, exact SQL, and every test result:
`docs/reports/R007_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`.

### 4e. Project/storage integrity hardening (2026-09-17, dedicated follow-up run)

A focused hardening pass - **not** a new domain entity - closing gaps
found in the Project slice above before a third entity or `GoalRelation`
begins:

1. **SQLite foreign-key enforcement genuinely enabled.**
   `storage::open_encrypted` (the single connection-opening primitive
   every production connection, version-targeted migration test
   connection, and the storage spike's test connections all go through)
   now calls a new `enable_foreign_keys` helper: sets `PRAGMA
   foreign_keys = ON` and reads it back to confirm it actually took
   effect, returning a typed error rather than silently ignoring a
   failure. Previously `projects.goal_id REFERENCES goals(id)` was
   schema-documentation only.
2. **Proven at the database level, not just asserted.** Two new
   integration tests intentionally bypass `CreateProject`'s validation
   entirely - a direct, raw SQL `INSERT INTO projects` naming a
   nonexistent `goal_id` - and confirm SQLite itself now rejects it with
   a constraint-violation error
   (`req_i3_invalid_direct_project_goal_id_rejected_by_sqlite_fk`), while
   the same raw insert with a real `goal_id` succeeds
   (`req_i4_valid_direct_project_goal_id_accepted`). Two more confirm
   `PRAGMA foreign_keys` reads back `1` on a fresh connection and again
   after a full close/reopen (`req_i1`/`req_i2`). This raw SQL exists
   only inside the tests - no new production function exposes it.
3. **`CreateProject`'s application-level `ReferencedGoalNotFound`
   pre-check is kept, deliberately** - not replaced by the now-real FK
   constraint. It remains the *first* line of defense: a friendly, typed
   `ActionOutcome::ValidationFailed` through the normal channel, rather
   than a caller ever seeing a raw SQL constraint failure. The FK
   constraint is now real defense-in-depth behind it, not documentation
   standing in for enforcement.
4. **`Project.progress` canonically resolved** in `LIFEOS_SPEC.md` §5.1
   - no longer an open interpretation (§4d). It is the CURRENT
   normalized progress snapshot (0..=100, same `Progress` type as
   `Goal.progress`); the separate, still-unimplemented `ProgressUpdate`
   entity is a temporal *history* of progress observations and does not
   replace or remove the need for this current-state field.
   `ProgressUpdate` itself remains unimplemented - only what
   `Project.progress` *means* was resolved, not when it gets built.
5. **`GoalStatus` renamed to `LifecycleStatus`** (behavior-preserving
   Rust identifier rename only) - the Project vertical slice already
   reused this type for `Project.status`, leaving a Goal-specific name
   backing two entities. Variants, `as_str()`/`parse()` mappings, and
   therefore every `'ACTIVE'`/`'PAUSED'`/`'COMPLETED'`/`'ABANDONED'`
   value ever written to a `status` column are byte-for-byte unchanged -
   proven by 3 new pure round-trip tests
   (`domain::tests::lifecycle_status_*`) plus every pre-existing
   Goal/Project test continuing to pass unmodified after the rename.
   Neither `V1__create_goals.sql` nor `V2__create_projects.sql` was
   touched - no migration was needed for a Rust-only rename, and no
   schema/database-value change occurred.
6. **OPEN M2 ACTION-SET GAP documented, deliberately not resolved in
   code.** `Project` has no frozen action to edit its `title`/
   `description`/`status`/`priority`/`goal_id` after creation (the
   frozen set is only `CreateProject`/`AddMilestone`/`AddTask`/
   `UpdateProgress` - no `UpdateProject`). Recorded as an explicit open
   question in `LIFEOS_SPEC.md` §5.1/§13, requiring a conscious
   architecture decision (one `UpdateProject` vs. several narrower
   actions) before M3/UI wiring needs it - not invented inside this or
   any future implementation slice.

Preserved, not regressed: all 66 pre-existing tests (Goal + Project +
storage-spike), SQLCipher encryption, keyring lifecycle, migration
separation, the private `services::storage` boundary (re-verified again
via the illegal-access probe against the new state), atomic
mutation+audit for both entities, `ReferencedGoalNotFound`'s existing
behavior, and the V1→V2 migration-compatibility guarantee.

Full evidence: `docs/reports/R008_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md`.

### 4f. GoalRelation vertical slice implemented (2026-09-17)

The third real domain entity - and the first true relationship-*graph*
structure in production M2, built directly with the twice-hardened
Goal/Project pattern (no separate hardening pass needed):

- **Domain** (`domain::GoalRelation`): `id` (UUID v7, own
  `GoalRelationId` newtype), `from_goal_id`/`to_goal_id` (both
  `GoalId`), `relation_type` (`GoalRelationType` - a frozen, closed
  11-value enum: `SUPPORTS`/`CONFLICTS_WITH`/`DEPENDS_ON`/`BLOCKS`/
  `CONTRIBUTES_TO`/`REQUIRES`/`PARENT_OF`/`CHILD_OF`/`DUPLICATES`/
  `SUPERSEDES`/`COMPETES_FOR_RESOURCE`), `note` (nullable), `created_at`.
  Deliberately excludes `status`/`updated_at`/`confidence`/`weight`/an
  AI score/an "inferred" flag/`priority` - none are in the frozen model;
  provenance belongs to `ActionMeta`/the audit log, not the entity.
- **Action**: `LinkGoals` only - the entire frozen action for this
  graph, confirmation floor `EXPLICIT_ADD` per the architecture-freeze
  report §F's table (same mechanism as `CreateGoal`/`CreateProject`:
  `Provenance::Inferred` raises it to `INFERRED_CHANGE` and blocks until
  confirmed, proven end-to-end with zero writes before confirmation).
- **Direction is frozen, never fabricated.** One `LinkGoals` call
  produces exactly one directional row; no automatic inverse is ever
  written. `A SUPPORTS B` and `B SUPPORTS A` are two distinct rows if
  both are explicitly requested - neither auto-creates nor auto-collapses
  into the other. The canonical hierarchy convention -
  `PARENT_OF(parent -> child)`, deriving "child of" by querying the
  reverse direction - is proven to store exactly one row; `CHILD_OF`
  remains a valid, storable, parseable enum value when a caller
  deliberately requests it directly (the freeze keeps it; `LinkGoals`
  never auto-generates it as `PARENT_OF`'s inverse).
- **Three deterministic structural rules, frozen before coding, not
  invented mid-implementation:** (1) self-links (`from_goal_id ==
  to_goal_id`) rejected - every frozen relation type describes a
  relationship between distinct Goals; (2) exact duplicates (the same
  `from`/`to`/`relation_type` triple) rejected before persistence -
  different relation types or reversed direction for the same pair are
  NOT duplicates; (3) relations may reference Goals in **any**
  `LifecycleStatus` (ACTIVE/PAUSED/COMPLETED/ABANDONED) - no
  "active-only graph" rule was invented, proven by a dedicated test for
  all three non-active statuses.
- **Hard-delete resolution for GoalRelation's two foreign keys: `ON
  DELETE RESTRICT`, not cascade.** Goal has no hard `DeleteGoal` action
  today; a relation must never silently disappear because an endpoint
  Goal was removed, and any future real Goal deletion must explicitly
  deal with its relations first. Proven with a raw SQL `DELETE FROM
  goals` against a referenced Goal - genuinely rejected by SQLite, not
  merely documented.
- **Database defense-in-depth, all proven with raw SQL bypassing
  `LinkGoals` entirely** (kept local to tests, never exposed as
  production API): nonexistent `from_goal_id`/`to_goal_id` both rejected
  by the FK constraint; a self-edge rejected by `CHECK (from_goal_id <>
  to_goal_id)`; a duplicate triple rejected by `UNIQUE(from_goal_id,
  to_goal_id, relation_type)`; an invalid `relation_type` string
  rejected by its `CHECK (... IN (...))` list of exactly the 11 frozen
  values.
- **Application-level validation kept alongside the real DB constraints**
  (same pattern established for `Project.goal_id` in §4e): `LinkGoals`
  checks `from`/`to` existence and exact-duplicate-ness itself first,
  returning friendly typed errors (`RelationFromGoalNotFound`/
  `RelationToGoalNotFound`/`DuplicateGoalRelation`/`SelfGoalRelation`)
  through the normal `ValidationFailed` channel, never leaving a caller
  to interpret a raw SQL constraint failure.
- **Atomicity from day one, proven again**: `link_goals` uses the exact
  same `&mut Connection` + `rusqlite::Transaction` +
  `CoreError`-on-any-failure shape as Goal/Project - proven with a
  forced-audit-failure test targeting the new `goal_relation_audit_log`
  table, not assumed to transfer automatically.
- **Migration compatibility proven, not assumed**: a dedicated test
  builds a database populated only through V2 (real Goal rows, a real
  Project linked to a Goal), then reopens it normally (applying only the
  newly-pending V3 migration) and confirms all prior data is unchanged
  and a new `LinkGoals` works between the pre-existing Goals. Neither
  `V1__create_goals.sql` nor `V2__create_projects.sql` was edited.
- **DRY cleanup carried out alongside, not deferred**: the Goal-
  existence check (`goal_exists`) that `create_project` already needed
  moved from `project_repo` into `goal_repo` (its natural home) rather
  than being duplicated a third time in `goal_relation_repo` - both
  `project_repo`/`goal_relation_repo` now call `goal_repo::goal_exists`.
- **Read side stays narrow**: `get_goal_relation(id)` and
  `get_goal_relations_for_goal(goal_id)` (querying both `from_goal_id`
  and `to_goal_id` - the actual stored direction, never a fabricated
  inverse) only - no public capability transport added this run.

Full evidence, exact SQL, and every test result:
`docs/reports/R009_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md`.

**Documentation correction made before implementation began**: the
Project/storage hardening report (§14) had incorrectly claimed
`GoalRelation.relation_type` was an architecture-freeze
`UNRESOLVED QUESTION` - it is not; that note was actually about
`Goal.domain_tag`, and was separately conflated with the *different*,
genuinely-free-string Personal Knowledge Layer `Relation.relation_type`
(§5.3). `GoalRelation.relation_type` has always been a frozen, closed
11-value enum. Corrected in that report and in `LIFEOS_SPEC.md` - see
the GoalRelation vertical-slice report §2 for the full correction.

### 4g. Project child action freeze (2026-09-17, documentation-only)

**No code changed this run** - a focused architecture/action-set
clarification pass, resolving the mutation gaps around
Project→Milestone→Task *before* either child entity is implemented, so
their vertical slices don't have to invent product rules mid-
implementation the way the Goal vertical slice's terminal-status rule
did.

- **`UpdateProject` is now frozen** (floor `IMPORTANT_CHANGE`,
  `FieldPatch<T>` for the nullable fields - same shape as `UpdateGoal`)
  - resolves the previously-open Project action-set gap (§4e point 6).
  `UpdateProgress` stays completely separate, unchanged, floor
  `EXPLICIT_ADD`, owns only `progress`.
- **`UpdateMilestone` and `UpdateTask` are now frozen** (both floor
  `EXPLICIT_ADD`, `FieldPatch<T>` for their nullable fields, plain
  `Option<T>` for `status`) - pre-emptively, before either entity is
  implemented. Without `UpdateMilestone`, a `Milestone` created
  `PENDING` via `AddMilestone` would have no path to `DONE` at all.
- **No narrow action explosion** (`RenameProject`, `CompleteMilestone`,
  `StartTask`, `CompleteTask`, `CancelTask`, etc. were considered and
  rejected) - one typed update action per entity with patch semantics,
  matching the pattern `UpdateGoal` already proved works.
- **No terminal-state rules invented** for `Milestone.status`
  (`PENDING`/`DONE`, freely reversible) or `Task.status`
  (`TODO`/`IN_PROGRESS`/`DONE`/`CANCELLED`, all peer states, direct
  `TODO -> DONE` and reopening both allowed) - the Goal hardening
  pass's lesson (§4c) applied proactively this time, not retroactively.
- **FK/delete policy frozen to match `GoalRelation`'s precedent
  exactly**: `Milestone.project_id` (required) and
  `Task.project_id`/`Task.goal_id` (both nullable) all `ON DELETE
  RESTRICT`, never cascade - neither `Project` nor `Goal` has a hard
  delete action, so child work must never silently vanish.
- **Task ownership confirmed unconstrained**: a `Task` may be
  standalone, `Project`-only, `Goal`-only, or both - no new requirement
  invented, matching the already-frozen nullable fields exactly.
- **Creation defaults frozen**: `AddMilestone` → `PENDING`, `AddTask` →
  `TODO`.
- **`Milestone.order_index` uniqueness within a Project flagged as a
  genuine OPEN QUESTION** (`docs/LIFEOS_SPEC.md` §13) at the time this
  freeze pass was written - not silently decided either way. **Resolved
  in the Milestone vertical slice itself (§4i, 2026-09-17): YES,
  `(project_id, order_index)` must be unique; different Projects may
  freely reuse the same values.**
- **Next M2 implementation order frozen**: `Milestone` vertical slice,
  then `Task`, then reassess - `Milestone` is the smaller, single-parent
  entity and proves the parent-child FK/action pattern before `Task`'s
  two optional parents and four statuses add complexity. Personal
  Knowledge Layer explicitly not started this run.

Full reasoning: `docs/reports/R010_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md`.

### 4h. What's still architecturally frozen but not yet implemented

Personal Knowledge Layer, `Challenge`/`Plan`/`SolutionAttempt`/
`Outcome`/`CalendarEvent`/`Reminder`/`Question`/`ProgressUpdate`/
`WellbeingCheckin`/`RecurringRule` (the last one only reserved as a
typed `RecurringRuleId` placeholder + a deferred, database-enforced
NULL-only field on `Task` - see §4j), `ForgetMemory`'s corrected
(hard-delete) semantics, the `knowledge`/`backup` modules, a real
`capability` module, NeXa transport. `UpdateProject` is no longer in
this list - implemented, §4l. Full detail: `docs/LIFEOS_SPEC.md`
§5.1-§5.6.

### 4i. Milestone vertical slice implemented (2026-09-17)

The fourth real domain entity - and the first *required-parent* child
entity in production M2, built directly with the pattern (no separate
hardening pass needed):

- **Domain** (`domain::Milestone`): `id` (UUID v7, own `MilestoneId`
  newtype), `project_id` (`ProjectId`, required), `title`,
  `target_date` (nullable, same representation as `Goal.target_date`),
  `status` (`MilestoneStatus` - frozen, closed 2-value `PENDING`/`DONE`
  enum, deliberately NOT `LifecycleStatus`), `order_index` (`i64`,
  non-negative). Deliberately excludes `created_at`/`updated_at` too -
  a genuine exception, not an oversight: the frozen field table for
  `Milestone` lists no timestamps at all, unlike `Task`'s own frozen
  row (see the vertical-slice report §4 for the full reasoning).
- **Patch-semantics documentation correction applied first** (report
  §2): `LIFEOS_SPEC.md`'s prose had used `FieldPatch<T>` too broadly for
  Milestone/Task's non-nullable fields. Canonical rule now written down:
  `FieldPatch<T>` only for genuinely nullable fields (`target_date`);
  plain `Option<T>` for non-nullable ones (`title`/`status`/
  `order_index`) - `None` unchanged, `Some(x)` sets. Same rule now also
  governs the still-unimplemented `UpdateProject`.
- **`Milestone.order_index` uniqueness RESOLVED** (report §3, closing
  the Project Child Action Freeze report's open question): `(project_id,
  order_index)` must be UNIQUE within a Project; different Projects may
  freely reuse the same values; no automatic shifting/rebalancing.
  Enforced twice - a Core-level pre-check
  (`order_index_taken`, excludable for the Milestone being updated) and
  a real SQL `UNIQUE(project_id, order_index)` constraint, proven
  independently with a raw-SQL bypass test.
- **Actions**: `AddMilestone` (floor `EXPLICIT_ADD`; `status` not in
  the payload - always defaults to `PENDING`; `project_id` validated to
  exist, `order_index` validated non-negative and unoccupied, both
  before persistence) and `UpdateMilestone` (floor `EXPLICIT_ADD`, the
  frozen floor - NOT `IMPORTANT_CHANGE` like `UpdateGoal`; no
  terminal-state rule - `PENDING`/`DONE` are freely reversible peer
  states, proven both directions against a real DB).
- **Cross-entity reference validation**: `AddMilestone.project_id`
  checked against a real database read
  (`storage::project_repo::project_exists`, new) before persistence,
  `ValidationError::ReferencedProjectNotFound` if absent - same pattern
  as `CreateProject`'s `goal_id` check. The real
  `milestones.project_id REFERENCES projects(id) ON DELETE RESTRICT`
  constraint is independent defense in depth, proven directly with a
  raw-SQL bypass.
- **Atomicity from day one, proven again**: `add_milestone`/
  `update_milestone` use the exact same `&mut Connection` +
  `rusqlite::Transaction` + `CoreError`-on-any-failure shape as every
  prior entity - proven with two forced-audit-failure tests targeting
  the new `milestone_audit_log` table.
- **Migration compatibility proven, not assumed**: a dedicated test
  builds a database populated only through V3 (real Goal/Project/
  GoalRelation rows - the actual shape of an existing user's database
  before this update), then reopens it normally (applying only the
  newly-pending V4 migration) and confirms all prior data is unchanged
  and a new `AddMilestone` works against the pre-existing Project.
  Neither `V1__create_goals.sql`, `V2__create_projects.sql`, nor
  `V3__create_goal_relations.sql` was edited.
- **Database defense-in-depth, all proven with raw SQL bypassing the
  action API entirely** (kept local to tests): nonexistent `project_id`
  rejected by the FK constraint; a duplicate `(project_id,
  order_index)` rejected by `UNIQUE`; an invalid `status` string
  rejected by its `CHECK (... IN ('PENDING', 'DONE'))`; a whitespace-only
  title rejected by `CHECK (length(trim(title)) > 0)`; a hard `DELETE`
  of a Project with a live Milestone rejected by `ON DELETE RESTRICT`.
- **Read side stays narrow**: `get_milestone(id)` and
  `get_milestones_for_project(project_id)` (deterministic
  `ORDER BY order_index ASC`, unambiguous since the pair is UNIQUE) only
  - no public capability transport added this run.

Full evidence, exact SQL, and every test result:
`docs/reports/R011_M2_MILESTONE_VERTICAL_SLICE_2026-09-17.md`.

### 4j. Task vertical slice implemented (2026-09-17)

The fifth real domain entity - and the first with **two independent,
both-nullable** parent references in production M2, built directly with
the pattern (no separate hardening pass needed):

- **Domain** (`domain::Task`): `id` (UUID v7, own `TaskId` newtype),
  `title`, `status` (`TaskStatus` - frozen, closed 4-value `TODO`/
  `IN_PROGRESS`/`DONE`/`CANCELLED` enum), `due_at` (nullable),
  `project_id`/`goal_id` (both independently nullable - a Task may be
  standalone, Project-only, Goal-only, or both, and neither is ever
  inferred from the other), `recurring_rule_id` (nullable, reserved for
  the future, unimplemented `RecurringRule` entity - see below),
  `created_at`/`updated_at` (UTC, same convention as Goal/Project - the
  first entity after Milestone whose frozen model has both timestamps
  again). Deliberately excludes `description`/`priority`/
  `milestone_id`/`progress`/`parent_task_id`/`tags`/a completion
  percentage/AI metadata - `milestone_id` specifically is NOT inferred
  just because Milestone and Task are both Project children.
- **`recurring_rule_id` deliberately deferred, not silently dropped**
  (report §3): a typed `RecurringRuleId` placeholder exists, but
  `RecurringRule` itself - the table, rrule/RFC5545 parsing, recurrence
  expansion, Calendar/Reminder - is **not built this run**. The
  production schema enforces `CHECK (recurring_rule_id IS NULL)` since
  there is currently no real FK target; `AddTask`/`UpdateTask` never
  accept one. Proven directly with a raw-SQL bypass showing a non-NULL
  value is genuinely rejected, not just documented as forbidden.
- **Actions**: `AddTask` (floor `EXPLICIT_ADD`; `status` not in the
  payload - always defaults to `TODO`; `recurring_rule_id` always
  `None`; `project_id`/`goal_id`, when supplied, each independently
  validated to exist before persistence) and `UpdateTask` (floor
  `EXPLICIT_ADD`, the frozen floor - NOT `IMPORTANT_CHANGE`; no
  terminal-state rule - all four `TaskStatus` values are freely
  reversible peer states; no `recurring_rule_id` field on the payload
  at all).
- **Patch semantics applying Milestone's corrected rule directly**:
  `title`/`status` (non-nullable) → plain `Option<T>`; `due_at`/
  `project_id`/`goal_id` (all three genuinely nullable) → `FieldPatch<T>`
  - the first entity with more than one `FieldPatch` field, proving
  `Set`/`Clear`/`Unchanged` independently for each, plus both parents
  cleared together in one update.
- **Cross-entity reference validation, reusing existing error types**:
  `AddTask`/`UpdateTask`'s `project_id`/`goal_id` checks reuse
  `ValidationError::ReferencedProjectNotFound`/`ReferencedGoalNotFound`
  directly (already defined for `CreateProject`/`AddMilestone`/
  `LinkGoals`) - no new, duplicate Task-specific variants were added.
  Both parents are checked independently; neither is ever skipped
  because the other was checked or failed.
- **Atomicity from day one, proven again**: `add_task`/`update_task` use
  the exact same `&mut Connection` + `rusqlite::Transaction` +
  `CoreError`-on-any-failure shape as every prior entity - proven with
  two forced-audit-failure tests targeting the new `task_audit_log`
  table, including a check that `updated_at` does not leak from a
  rolled-back `UpdateTask`.
- **Timestamp behavior proven directly**: a freshly created Task has
  `created_at == updated_at`; a successful `UpdateTask` preserves
  `created_at` and strictly advances `updated_at`; a rejected
  `UpdateTask` changes neither.
- **Migration compatibility proven, not assumed**: a dedicated test
  builds a database populated only through V4 (real Goal/Project/
  GoalRelation/Milestone rows), then reopens it normally (applying only
  the newly-pending V5 migration) and confirms all prior data is
  unchanged and new `AddTask` calls succeed in all four ownership modes
  against the pre-existing Project/Goal. Neither V1, V2, V3, nor V4 was
  edited.
- **Database defense-in-depth, all proven with raw SQL bypassing the
  action API entirely** (kept local to tests): nonexistent `project_id`/
  `goal_id` both rejected by their FK constraints; an invalid `status`
  string rejected by its `CHECK`; a whitespace-only title rejected by
  `CHECK (length(trim(title)) > 0)`; a non-NULL `recurring_rule_id`
  rejected by the temporary `CHECK`; a hard `DELETE` of a Project or
  Goal with a live Task rejected by `ON DELETE RESTRICT`.
- **Read side stays narrow**: `get_task(id)`,
  `get_tasks_for_project(project_id)`, `get_tasks_for_goal(goal_id)`
  (deterministic `due_at ASC, created_at ASC, id ASC` - explicitly
  documented as a technical ordering, not product prioritization) - no
  public capability transport added this run.

Full evidence, exact SQL, and every test result:
`docs/reports/R012_M2_TASK_VERTICAL_SLICE_2026-09-17.md`.

### 4k. Task parent-validation hardening (2026-09-17, dedicated follow-up run)

A small, focused correctness fix found during review of §4j - **not** a
new entity or action:

- **Issue**: the Task vertical-slice report claimed `add_task`/
  `update_task` check `project_id`/`goal_id` independently, "neither
  ever skipped just because the other one ran or failed" - but the
  actual implementation returned immediately on the first missing
  reference, so when BOTH parents were invalid at once, only
  `ReferencedProjectNotFound` was ever returned and the `goal_id` check
  never actually ran. This never allowed an invalid write (a single
  error was still a correct rejection with zero persistence) - the
  defect was the report's claim being stronger than the code, not a
  data-integrity gap.
- **Fix**: both checks now accumulate into a shared
  `reference_errors: Vec<ValidationError>` and the function returns
  only after every applicable check has run. If both are invalid, both
  errors are returned together in stable, deterministic
  `ReferencedProjectNotFound`-then-`ReferencedGoalNotFound` order
  (guaranteed by push order). A real storage error from either lookup
  still aborts immediately as a typed `StorageError`, never silently
  downgraded into a validation failure.
- **Proven with 6 new tests** (`req_tpv_a` through `req_tpv_f`): both
  parents invalid (AddTask and UpdateTask, both return both errors in
  order, zero writes/mutation), Project-valid-Goal-invalid, and
  Project-invalid-Goal-valid (proving the Goal lookup genuinely still
  runs even when Project already failed) for both actions.
- **Documentation correction, not a silent rewrite**: a "Correction
  note" addendum was added to
  `R012_M2_TASK_VERTICAL_SLICE_2026-09-17.md` §10, explaining the
  inaccuracy; the original text is preserved above it.

Full evidence: `docs/reports/R013_M2_TASK_PARENT_VALIDATION_HARDENING_2026-09-17.md`.

### 4l. UpdateProject implemented (2026-09-17)

The last already-frozen, shovel-ready action on an existing M2 core
entity - closes the Project action-set gap first identified in the M2
Project/storage integrity hardening report, both in design (already
frozen by the M2 Project Child Action Freeze report) and now in code:

- **Payload** (`actions::UpdateProjectPayload`): `id`,
  `title: Option<String>`, `description: FieldPatch<String>`,
  `status: Option<LifecycleStatus>`, `priority: FieldPatch<Priority>`,
  `goal_id: FieldPatch<GoalId>` - exactly the frozen shape, no design
  changes. **`progress` is deliberately absent** - `UpdateProgress`
  remains its sole owner, own payload, own `EXPLICIT_ADD` floor, own
  repository function; proven by a dedicated regression test that
  `UpdateProject` never touches `progress` and `UpdateProgress` still
  works independently afterward.
- **Confirmation floor `IMPORTANT_CHANGE`**, reusing the existing
  generic `effective_class`/`blocks_until_confirmed` unchanged - no
  custom Project confirmation logic, no severity-ladder change.
- **No terminal-state rule**: all four `LifecycleStatus` values remain
  peer states for Project, exactly as for Goal - a completed or
  abandoned Project may be reopened, proven with 5 representative
  real-DB transitions in sequence.
- **`goal_id` stays its own direct field, never inferred**: `Set`
  triggers a real Goal-existence check (`ReferencedGoalNotFound` if
  missing, zero mutation); `Clear`/`Unchanged` need no lookup. Proven
  that `Project.goal_id` changes never create, delete, or modify a
  `Goal` or any `GoalRelation` row - the two concepts stay fully
  separate.
- **Atomicity from day one, proven again**: `update_project` uses the
  identical `&mut Connection` + `rusqlite::Transaction` +
  `CoreError`-on-any-failure shape as every prior entity, reusing
  `project_audit_log` (no new audit table) - proven with a forced-
  audit-failure test showing the Project (including `updated_at` and
  `progress`) survives byte/logically unchanged and the connection
  remains usable afterward.
- **Unplanned V6 migration, flagged and user-authorized, not silently
  done**: `project_audit_log.action_type`'s `CHECK` constraint (written
  when only `CreateProject`/`UpdateProgress` existed) did not allow
  `'UpdateProject'` - every real call would fail its audit insert. This
  directly conflicted with the run's own "no migration" instruction;
  surfaced to the user rather than guessed at, who authorized a minimal
  `V6__widen_project_audit_log_action_types.sql` (SQLite's standard
  table-rebuild pattern for altering a `CHECK`) rather than leaving the
  action non-functional or mislabeling its audit rows. `projects` itself
  is untouched; V1-V5 remain historical and unedited. Proven
  non-destructive with a dedicated V5-to-V6 migration-compatibility
  test (pre-existing `project_audit_log` rows, and their exact
  `action_type` values, survive the upgrade unchanged).

Full evidence, exact SQL, and every test result:
`docs/reports/R014_M2_UPDATE_PROJECT_IMPLEMENTATION_2026-09-17.md`.

### 4m. M2 Storage and Audit Safety Hardening (2026-09-18, R015, dedicated follow-up run)

A focused safety-infrastructure pass - **not** a new domain entity or
action, and no product/action semantics changed (confirmation floors,
`FieldPatch` behavior, status transition policy, entity schemas,
`Project.progress` ownership, `Task` ownership, `GoalRelation`
semantics, `Milestone` ordering, and action names are all unchanged).
Closes two known risks found by direct inspection of the actual
current code (not assumed from any prior report):

1. **Production master-key lifecycle hardened.** The previous
   `security::get_or_create_production_master_key()` always generated
   and stored a fresh key whenever the OS credential store had no
   matching entry - with no awareness of whether the production
   database file already existed. If a user's credential were ever
   lost (OS reinstall, credential-store reset, migration to a new
   machine) while their encrypted database remained on disk, this
   would have silently generated an unusable replacement key and
   opened the existing file under it - turning a recoverable situation
   into a confusing one. **Removed entirely**, replaced with a
   canonical, frozen four-state DB/key matrix:
   - DB absent + credential absent (Case A, normal first run) → generate
     + store a fresh 256-bit key, initialize the DB with it.
   - DB exists + credential exists (Case B) → reuse the existing key,
     never replace it.
   - DB exists + credential absent (Case C, **critical**) → typed
     `BootstrapError::MasterKeyMissingForExistingDatabase` recovery-
     required error; no replacement key is ever generated or stored,
     and the existing database file is never touched in any way.
   - DB absent + credential exists (Case D) → reuse the existing
     credential for the new DB; never replaced merely because the file
     happens to be absent (intentional deletion, restore prep,
     interrupted setup are all legitimate).
   New narrow primitives in `security/mod.rs`
   (`get_existing_key`/`generate_and_store_key`/`delete_key`, all
   `pub(crate)`) replace the single opaque get-or-create call; the
   state machine itself (`resolve_master_key`/
   `bootstrap_db_with_key_lifecycle`, in `services/mod.rs`) tracks via
   a `ResolvedMasterKey::{Reused, FreshlyGenerated}` enum whether *this
   invocation* created the credential, so that if database
   initialization subsequently fails, only a credential this same call
   just created is cleaned up (best-effort, never masking the original
   error) - a pre-existing credential is never touched by cleanup.
   `services::production_db_path()` remains the single, uncompeting
   database-existence check every part of this shares.
2. **Mandatory audit `previous_snapshot` can no longer silently
   disappear.** `serde_json::to_string(&existing).ok()` - previously
   used at all five mandatory-audit update call sites
   (`update_goal`/`update_project`/`update_progress`/
   `update_milestone`/`update_task`, found by searching the whole
   repository, not just the one call site R014 had flagged for
   `UpdateProject`) - is replaced by a new
   `serialize_required_snapshot::<T: Serialize>(&T) -> Result<String,
   CoreError>` helper. A serialization failure now returns a typed
   `CoreErrorKind::Internal` error with **zero mutation and zero audit
   write** (it runs before the mutation transaction starts, same
   position the old code already used) - never silently continuing
   with `previous_snapshot = None`. `Create*` actions are unaffected -
   they still legitimately pass `None` (no previous entity exists).
3. **CoreError/logging secret-leak review performed, nothing needed
   fixing.** Verified by direct inspection (not redesigned): no key
   material or serialized personal snapshot content flows into any
   `Display`/`Debug`/error-metadata path anywhere touched by this pass
   - `KeyError`/`BootstrapError`/`CoreError` all derive their
   diagnostic text only from the underlying driver's own safe error
   text; `serialize_required_snapshot`'s error text is
   `serde_json::Error`'s own structural-failure description, never the
   serialized values.
4. **17 new tests** (250 total, up from 233; all 233 prior tests
   remain green, unmodified): 10 key-lifecycle tests (`req_kl1`-
   `req_kl10`, one per required proof - all four DB/key cases, the
   generated key is exactly the key that encrypts the DB, a
   present-but-wrong key still fails normally with no replacement,
   orphan-credential cleanup only for a freshly-created credential,
   repeated reopen stability, and a static source scan confirming no
   test touches the real production credential identity), 1 production
   bootstrap regression test (first bootstrap creates the encrypted DB,
   second bootstrap reopens the same DB with the same key across a real
   close, no plaintext SQLite header), and 6 snapshot-deserialization
   regression tests (one per mandatory-snapshot update action,
   deserializing the stored `previous_snapshot` back into its real
   domain struct and asserting full structural equality against the
   captured pre-update state - stronger than the pre-existing
   title-substring checks) plus a dedicated snapshot-serialization-
   failure test using a test-only `AlwaysFailsToSerialize` type (no
   production domain struct distorted). All tests use isolated
   `tempfile` database paths and a distinct `"LifeOS-test"` keyring
   service identity - never the real production database or
   credential.
5. **No schema migration** - `src-tauri/migrations/` untouched
   (verified: zero diff), matching the pass's "no V7 unless genuinely
   unavoidable" instruction. **No `services::storage` visibility
   change** - the private module boundary is untouched, so no
   illegal-access probe was needed this run.

Full evidence, exact code, and every test result:
`docs/reports/R015_M2_STORAGE_AND_AUDIT_SAFETY_HARDENING_2026-09-18.md`.

### 4n. M2 Key Lifecycle Determinism and Failure Atomicity Hardening (2026-09-18, R016, dedicated follow-up run)

A focused correctness/verification pass on top of R015 (§4m) - **not** a
new domain entity or action, and no product/action semantics changed.
Fixes two review findings against R015's own implementation (found by
re-inspecting R015's actual current code, not by trusting its report
text) plus one narrow diagnostic-safety hardening:

1. **Case-A post-file-creation cleanup risk, fixed.** R015's
   `bootstrap_db_with_key_lifecycle` deleted a freshly-generated Case A
   credential on ANY subsequent database-initialization failure,
   unconditionally. This is only safe if the failure occurred *before*
   `bootstrap_db_at` created any database file. If instead
   `storage::open_encrypted` had already created/encrypted the file
   using the fresh key before a *later* step (`verify_key`/migration)
   failed, deleting the credential at that point would self-inflict
   "database exists, credential absent" (Case C) - the exact state R015
   exists to prevent - as a side effect of its own cleanup path. R015's
   `req_kl7` test could not have caught this: its blocker-file failure
   technique structurally guarantees no database file ever exists
   afterward. **Corrected cleanup rule**, implemented in
   `bootstrap_db_with_key_lifecycle_with_store`: delete the freshly-
   generated credential only if the database path still does NOT exist
   afterward; if a file now exists, KEEP the credential (the database
   file itself is never deleted by this cleanup path either way, so
   this preserves the DB+key pairing rather than destroying it).
   Proven with a new deterministic test (`req_dkl7`) using a custom,
   injectable database-initializer closure that writes real bytes to
   the target path before deliberately failing - the one sequence
   `bootstrap_db_at` cannot be forced into portably - which also
   confirms the resulting state re-resolves as Case B (safe to reopen),
   never Case C.
2. **Key-lifecycle tests made deterministic, independent of OS keyring
   availability.** R015's `req_kl1`-`req_kl10` ran against the real OS
   credential store and could return early as "BLOCKED" (still counted
   as PASS by Cargo) if it was unavailable - not sufficient
   deterministic proof of the canonical A/B/C/D state machine for
   safety-critical behavior. A new, narrow `security::CredentialStore`
   trait (`get`/`set`/`delete`, three methods, no DI framework) now
   sits behind `resolve_master_key`/`bootstrap_db_with_key_lifecycle` -
   production routes through a `KeyringCredentialStore` adapter
   (identical real-keyring behavior, unchanged), while 12 new
   deterministic tests (`req_dkl1`-`req_dkl12`) drive the exact same
   state-machine logic (`resolve_master_key_with_store`/
   `bootstrap_db_with_key_lifecycle_with_store` - never a parallel
   reimplementation) through an in-memory test double instead,
   deterministically proving: all four DB/key cases, orphan-credential
   cleanup (both pre- and post-file-creation, §1 above), a
   present-but-wrong key, repeated-reopen key stability, and three
   credential-backend error paths (GET error correctly distinct from
   "absent" with zero key generation; SET error on first run correctly
   prevents database initialization from ever starting, proven with a
   call-counting wrapper; DELETE error during cleanup correctly leaves
   the ORIGINAL initialization error as what the caller sees, with the
   resulting best-effort residual state explicitly asserted, not
   hidden). R015's original `req_kl1`-`req_kl10` are kept as a
   supplementary, environment-dependent smoke/integration suite against
   the real OS keyring - still valuable, no longer the primary proof.
   In this run's environment the real keyring was available, so all 11
   of those tests (including the bootstrap-regression test) genuinely
   PASSed, not BLOCKED.
3. **Snapshot diagnostic hardened from "safe by assumption" to "safe by
   construction".** R015 had verified `serialize_required_snapshot`'s
   `CoreError` diagnostic was safe because `serde_json::Error`'s
   *current* built-in messages never echo serialized field values - true
   at the time, but an assumption about a dependency's wording, not a
   structural guarantee; a future custom `Serialize` implementation on
   some domain type could return arbitrary error text via
   `serde::ser::Error::custom(...)`. The diagnostic is now a fixed
   string - the underlying `serde_json::Error`'s own message is
   discarded entirely rather than interpolated. Proven with a
   strengthened `req_snap1`: the existing deliberately-failing
   `AlwaysFailsToSerialize` test type now embeds an obvious marker
   string in its error text, and the test asserts that marker never
   appears in the resulting `CoreError`'s `Display`, `Debug`, or
   `diagnostic()` - structural proof, not an inspection of today's
   `serde_json` wording.
4. **12 new tests** (262 total, up from 250; all 250 prior tests remain
   green, unmodified): `req_dkl1`-`req_dkl12` (deterministic key
   lifecycle, in-memory credential store), plus the strengthened
   `req_snap1`. An incidental fix found along the way: `ResolvedMasterKey`
   needed a `Debug` impl for a new test's `{:?}` formatting - implemented
   by hand (not derived) so only the variant name prints, never the
   actual master key, which a derived `Debug` would have exposed in any
   caller's `{:?}`/panic/assertion-failure message.
5. **R015's report corrected via a clearly-labelled addendum, not a
   silent rewrite.** The original report body and its measured
   250-test/build results are left completely unedited; an addendum
   section at the end explains all three findings above and points to
   R016 for the fix.
6. **No schema migration** - `src-tauri/migrations/` untouched
   (verified: zero diff). **No `services::storage` visibility change.**

Full evidence, exact code, and every test result:
`docs/reports/R016_M2_KEY_LIFECYCLE_DETERMINISM_AND_FAILURE_ATOMICITY_HARDENING_2026-09-18.md`.

### Next implementation step (M2)

With `UpdateProject` implemented, every frozen, shovel-ready CRUD action
on an existing M2 entity is now built. What remains needs its own
planning/freeze pass first, not a direct implementation slice:
**`RecurringRule`** (deliberately deferred, needs its own design/freeze
pass before any code - `Task.recurring_rule_id` stays `NULL`-only until
then, §4j) or the **Personal Knowledge Layer** (the next real net-new
entity family, larger in scope). Neither should be started without that
planning pass first, the way Milestone/Task both got a Project Child
Action Freeze pass before implementation - do not invent product rules
inside an implementation pass; flag them as open questions instead
(§4c's lesson from the removed terminal-status rule; §4e's/§4g's
action-set-gap documentation-before-code discipline; §4f's
relation-integrity rules and §4i's order_index-uniqueness rule both
being frozen/resolved *before or as part of* coding rather than
invented ad hoc mid-implementation).

---

## 5. M0 status — Environment ready (RESOLVED, unchanged)

| Prerequisite | Status |
|---|---|
| Rust (`rustup`, `rustc`, `cargo`) | Installed — `rustc 1.98.1`, `cargo 1.98.1` |
| Node.js / npm | `v24.18.0` / `11.1.0` |
| Git | `2.47.1.windows.2` |
| WebView2 Runtime | `153.0.4234.32` |
| Visual Studio "Desktop development with C++" workload | Installed |
| OpenSSL (Windows dev package, for SQLCipher build) | Installed (`ShiningLight.OpenSSL.Dev`); env vars now set via `scripts/setup-windows-storage-env.ps1`, not committed config |

**Standing note, still true:** this automated session cannot perform
*admin-elevated* installs. `winget` package installs used so far have
not required elevation.

---

## 6. Measured hardware/software audit (carried forward, unchanged this run)

- OS: Microsoft Windows 11 Home, 10.0.26200, 64-bit
- CPU: 12th Gen Intel Core i5-12500H, 12 physical / 16 logical cores, 3.1GHz base
- RAM: 63.71 GB (2× 32GB Kingston, 3200 MT/s)
- GPU: NVIDIA GeForce RTX 3050 Ti Laptop — 4096 MiB (4GB) VRAM, driver 610.62
- Disks: `C:` 475.6GB (166.8GB free), `D:` 3815.4GB (2769.1GB free), both NVMe SSD
- Git 2.47.1.windows.2, Node v24.18.0, npm 11.1.0, Python 3.12.7, Rust 1.98.1
- Ollama: still not installed (needed starting M4)
- Visual Studio: Community 2022, 17.13.35931.197, C++ workload installed
- WebView2 Runtime: 153.0.4234.32 present
- Windows OpenSSL dev package (ShiningLight.OpenSSL.Dev, 4.0.2) at `C:\Program Files\OpenSSL-Win64` - build-time only, confirmed via `dumpbin` that `lifeos.exe` has no runtime dependency on it (§4a)
- Still missing: a full Perl (Git for Windows' bundled Perl cannot build OpenSSL from source)
- MSVC toolchain confirmed usable for binary inspection too: `dumpbin.exe` (VS 2022's `VC\Tools\MSVC\14.43.34808\bin\Hostx64\x64\`) used this run

No live NeXa git repository accessible — unchanged, `RESEARCH_DECISIONS.md` §3.

---

## 7. Git state

- Canonical repository: `https://github.com/AndrewDul/LifeOS.git`.
- Branch: `main`. Git identity: `AndrewDul <dulandrew.dev@gmail.com>`.
- Commits made in this project so far (oldest first):
  1. `chore: protect local user data in gitignore`
  2. `chore: harden gitignore for monorepo and Tauri build layout`
  3. `chore: ignore local Claude Code session state`
  4. `docs: establish LifeOS architecture baseline`
  5. `docs: lock v0.1 product direction (bilingual, GPT-5.6 Luna, Qwen3 4B, visual language, M0-M10 plan)`
  6. `feat: scaffold Tauri 2 + React + TS app and build the M1 visual shell`
  7. `docs: record M0/M1 build-start results`
  8. `feat: refine M1 visual shell and NeXa navigation`
  9. `feat: restore home edge drawers, refine face/background, add chat sidebar`
  10. `fix: full-screen edge transitions, remove global sidebar, NeXa Island as sole global nav`
  11. `docs: close M1, freeze M2 architecture, fix reporting policy`
  12. `feat: prove M2 encrypted storage (SQLCipher) and correct architecture errata`
  13. `feat: implement first M2 Goal vertical slice`
  14. `fix: harden M2 Goal action atomicity and patch semantics`
  15. `feat: implement M2 Project vertical slice`
  16. `fix: harden M2 project integrity and shared lifecycle types`
  17. `feat: implement M2 GoalRelation graph slice`
  18. `docs: freeze Project child mutation actions` — a
      documentation-only commit, zero source-code files changed.
  19. `feat: implement M2 Milestone vertical slice`
  20. `feat: implement M2 Task vertical slice`
  21. `fix: validate both Task parent references`
  22. `feat: implement M2 UpdateProject action`
  23. `fix: harden M2 key lifecycle and audit snapshots`
  24. (this run) `fix: make M2 key lifecycle failure-safe and
      deterministic` — see this run's report and chat reply for the
      exact hash.
- Pushed to `origin/main` through commit 7. **Commits 8-24 were not
  pushed** — not authorized in any of those runs' instructions.

### Note on an incident in an earlier run (kept for reference)

`npm create tauri-app@latest -- . --force` briefly deleted `CLAUDE.md`
and all `docs/*.md` files and overwrote `.gitignore` during the original
M1 scaffold. Restored via `git checkout`, no data lost.

---

## 8. What can the user actually do right now?

Launch the real app (`npm run tauri dev`) and use the accepted M1 shell -
unchanged, no real goals, projects, relations, milestones, or tasks
visible there yet (no Tauri command wiring). Separately,
`cd src-tauri && cargo test -- --test-threads=1` runs 262 tests proving:
the storage foundation (9, regression from the spike), a full
CreateGoal/UpdateGoal/get_active_goals lifecycle including atomic-
rollback-under-forced-failure and tri-state patch semantics (17
integration + 16 direct action-layer unit), a full CreateProject/
UpdateProgress/get_project lifecycle including its own atomic-rollback
proof and a V1-to-V2 migration-compatibility check (14 integration + 10
direct action-layer unit), that SQLite itself genuinely enforces
`projects.goal_id`'s foreign key (4 integration), that the `GoalStatus`
→ `LifecycleStatus` rename changed nothing observable (3 direct unit),
a full LinkGoals lifecycle - directional storage, self-link/duplicate
rejection, all 5 database-level FK/CHECK/UNIQUE/RESTRICT proofs via raw
SQL, the PARENT_OF convention, and a V2-to-V3 migration-compatibility
check (23 integration test functions covering 26 numbered requirements
+ 5 direct action-layer unit + 3 direct `GoalRelationType` unit), a
full AddMilestone/UpdateMilestone lifecycle - default-PENDING creation,
reversible PENDING/DONE status, `(project_id, order_index)` UNIQUE
enforced both in Core and the database, all 5 database-level FK/UNIQUE/
CHECK/RESTRICT proofs via raw SQL, and a V3-to-V4 migration-
compatibility check (24 integration test functions covering 32 numbered
requirements + 17 direct action-layer unit + 3 direct `MilestoneStatus`
unit), and a full AddTask/UpdateTask lifecycle - all four ownership
modes (standalone/Project-only/Goal-only/both), default-TODO creation,
NULL-only `recurring_rule_id` genuinely enforced, all four `TaskStatus`
values freely reversible, both nullable parents independently
Set/Clear-able (and clearable together), `created_at`/`updated_at`
timestamp behavior proven, all 7 database-level FK/CHECK/RESTRICT
proofs via raw SQL, and a V4-to-V5 migration-compatibility check (28
integration test functions covering 52 numbered requirements + 15
direct action-layer unit + 3 direct `TaskStatus` unit), 6 tests proving
`add_task`/`update_task` independently validate BOTH `project_id`/
`goal_id` when both are supplied, returning both `ValidationError`s
together in stable order when both are invalid (`req_tpv_a` through
`req_tpv_f`, §4k), and a full UpdateProject lifecycle - every mutable
Project field now has a working update path, `progress` proven
completely untouched (and `UpdateProgress` proven to still work
independently afterward), no terminal-state rule (5 representative
status transitions in sequence), `goal_id` Set/Clear proven to never
touch `Goal`/`GoalRelation`, atomic rollback/audit proven, and a
V5-to-V6 migration-compatibility check for the unplanned audit-CHECK
widening (14 integration test functions covering 33 numbered
requirements + 19 direct action-layer unit tests, §4l), and (as of
2026-09-18, R015, §4m) a hardened production master-key lifecycle -
all four DB/key existence cases proven deterministically, including
that an existing database with a missing credential now fails safely
with a typed recovery-required error instead of silently generating a
replacement key, and that orphan-credential cleanup only ever touches
a credential the same failed bootstrap invocation just created - plus
every mandatory-audit update action's `previous_snapshot` now
provably cannot silently disappear on a serialization failure (17 new
tests: 10 key-lifecycle + 1 bootstrap regression + 6 snapshot-
deserialization regression) - all passing against temp databases and
an isolated test credential identity, never the user's real data or
real credential, and (as of 2026-09-18, R016, §4n) that the master-key
orphan-credential cleanup is correct in BOTH failure timings (before
AND after a database file is created - the post-creation case is the
one R015 had not actually proven), that this holds true independent of
whether the real OS credential store is available (12 new deterministic
tests against an in-memory credential-store test double, exercising the
exact same production state-machine code), and that a credential-
backend GET/SET/DELETE error is handled distinctly and safely in each
case (12 new tests: `req_dkl1`-`req_dkl12`) - plus that the mandatory
audit-snapshot serialization-failure diagnostic structurally excludes
the underlying serializer's own error text, proven with a deliberate
marker string that must never appear in the resulting error's
Display/Debug (`req_snap1`, strengthened). **Test count rose from 200
to 233 to 250 to 262 across these two days' runs** (250→262 this run,
+12) - all 250 prior tests remain green, unmodified.

---

## 9. Next recommended step

See §4l's "Next implementation step" for the exact reasoning (this
duplicates it so it is visible without scrolling past the M2 detail
section). With Goal/Project/GoalRelation/Milestone/Task all
implemented, `UpdateProject` closing out the last frozen, shovel-ready
action, and (2026-09-18, R015 §4m then R016 §4n) the master-key
lifecycle and mandatory audit-snapshot safety gaps closed and then
verified deterministically/corrected for a post-file-creation failure
edge case, what remains needs its own planning/freeze pass first:
**`RecurringRule`** (deliberately deferred, §4j) or the **Personal
Knowledge Layer** (the next real net-new entity family). A direct
implementation slice is not recommended for either without that
planning pass first, the way Milestone/Task both got one. M2's
storage/security foundation is safe to proceed into that next
planning/freeze decision - see R016 §15.

---

## 10. Known problems / risks / unresolved questions

- **Neither Goal, Project, GoalRelation, Milestone, nor Task (nor the
  storage bootstrap) is wired into the running app** — no Tauri command
  layer exists yet; `services`' functions are only reachable from tests.
- **RESOLVED (2026-09-17, M2 Project Child Action Freeze, then M2
  UpdateProject implementation, §4l above): the Project action-set gap
  is closed both in design AND in code.** `UpdateProject` (floor
  `IMPORTANT_CHANGE`) is fully implemented - every mutable Project field
  (`title`/`description`/`status`/`priority`/`goal_id`) now has a
  working update path; `progress` remains exclusively `UpdateProgress`'s.
  `UpdateMilestone`/`UpdateTask`/`UpdateProject` are all implemented now
  - none remain in the "frozen but unimplemented" category.
- **RESOLVED (2026-09-17, M2 UpdateProject implementation, §4l/§3a of
  that report): `project_audit_log.action_type`'s CHECK constraint
  didn't allow `'UpdateProject'`.** Found during implementation (9
  failing tests), flagged to the user, who authorized a minimal V6
  migration (`V6__widen_project_audit_log_action_types.sql`, the
  standard SQLite table-rebuild pattern for altering a CHECK) rather
  than leaving the action non-functional. `projects` itself untouched;
  V1-V5 remain historical. Proven non-destructive with a dedicated
  migration-compatibility test.
- **RESOLVED (2026-09-17, M2 Milestone vertical slice, §4i above):
  `Milestone.order_index` uniqueness within a Project.** YES,
  `(project_id, order_index)` must be unique; different Projects may
  freely reuse the same values; enforced both in Core and by a real SQL
  `UNIQUE` constraint.
- **`Task.recurring_rule_id` remains deliberately deferred** (M2 Task
  vertical slice, §4j) - a typed `RecurringRuleId` placeholder exists,
  but `RecurringRule` itself (the table, recurrence expansion,
  Calendar/Reminder) is not built; the database enforces
  `recurring_rule_id IS NULL` via a temporary `CHECK` that a future
  `RecurringRule`/Calendar migration will need to replace with a real FK.
- **RESOLVED (2026-09-17, M2 Task parent-validation hardening, §4k
  above): `add_task`/`update_task` now genuinely validate BOTH
  `project_id`/`goal_id` independently whenever both are supplied.**
  The Task vertical-slice report had claimed this was already true;
  review found the implementation actually short-circuited on the first
  missing reference. Fixed - both errors are now returned together, in
  stable order, when both parents are invalid. Never allowed an invalid
  write either before or after this fix.
- **`bundled-sqlcipher`'s build-time OpenSSL dependency is real but no
  longer hardcoded** — any machine building this needs
  `scripts/setup-windows-storage-env.ps1` run once (§4a).
- **Clean-machine runtime is inferred from `dumpbin`, not an actual
  execution test** on a machine that never had any dev tooling installed
  — not available in this environment.
- **The Personal Knowledge Layer's `Relation.relation_type`** remains an
  intentionally free string — revisit once M6 usage patterns exist.
- **`target_date` still has no content validation beyond being a
  well-formed `DateTime<Utc>`** (e.g. nothing rejects a date far in the
  past) — pre-existing gap, unchanged again this run.
- **`CoreError` derives `Debug`, which does include the internal
  `diagnostic` string** (only `Display` is deliberately generic/safe) -
  fine for test assertions and future internal logging, but a future
  capability layer must format `CoreError` for any external caller via
  `Display`/`kind()`, never `{:?}`, or the "no arbitrary database string
  as the authoritative error" guarantee leaks back in through the side
  door. **Reviewed again 2026-09-18 (R015 §14): confirmed no key
  material or serialized personal snapshot content can ever reach that
  `diagnostic` string either** - this caveat about `{:?}` vs. `Display`
  remains the only reason to stay disciplined about it, not a sign
  something is currently leaking. **Hardened further 2026-09-18 (R016
  §10): `serialize_required_snapshot`'s diagnostic is now a fixed
  string** - the underlying `serde_json::Error`'s own message is
  discarded entirely rather than interpolated, so "no personal snapshot
  content in this diagnostic" is now true by construction, not merely
  by an assumption about `serde_json`'s current wording (a future custom
  `Serialize` impl on some domain type could otherwise have returned
  arbitrary error text). Proven with a deliberate marker string that
  must never appear in `Display`/`Debug`/`diagnostic()` (`req_snap1`).
- **RESOLVED (2026-09-18, M2 Storage and Audit Safety Hardening, R015
  §4m above): the production master-key lifecycle could silently
  generate a replacement key for an existing, now-undecryptable
  database.** `security::get_or_create_production_master_key()` (the
  unsafe, DB-existence-blind helper) is removed; replaced by a frozen
  four-state DB/key matrix (`services::resolve_master_key`) - an
  existing database with a missing credential now fails safely with a
  typed `BootstrapError::MasterKeyMissingForExistingDatabase` recovery-
  required error, never a silent replacement key.
- **RESOLVED (2026-09-18, M2 Storage and Audit Safety Hardening, R015
  §4m above): every mandatory audit `previous_snapshot` used
  `serde_json::to_string(&existing).ok()`, which could have silently
  turned a serialization failure into a missing audit record.**
  Replaced with `serialize_required_snapshot`, which returns a typed
  `CoreError` and guarantees zero mutation on failure, applied to all
  five update actions (`UpdateGoal`/`UpdateProject`/`UpdateProgress`/
  `UpdateMilestone`/`UpdateTask`) - not just the one call site R014 had
  originally flagged for `UpdateProject`.
- **RESOLVED (2026-09-18, M2 Key Lifecycle Determinism and Failure
  Atomicity Hardening, R016 §4n above): R015's own orphan-credential
  cleanup rule could self-inflict "database exists, credential absent"
  (Case C) if a database-initialization failure occurred AFTER a
  database file had already been created under a freshly-generated Case
  A key.** A review of R015's actual code (not its report text) found
  the cleanup branch deleted the fresh credential on ANY subsequent
  initialization failure, unconditionally - R015's own `req_kl7` test
  could not have caught this, since its failure-forcing technique
  structurally guarantees no database file ever exists afterward.
  Corrected: the fresh credential is now deleted only if the database
  path still does NOT exist afterward; if a file now exists, the
  credential is deliberately KEPT (the database file itself was never
  deleted by this cleanup path either way). Proven with a new
  deterministic test (`req_dkl7`) that forces failure AFTER a file is
  written, confirming both that the credential survives and that the
  resulting state correctly re-resolves as Case B (safe to reopen),
  never Case C.
- **RESOLVED (2026-09-18, M2 Key Lifecycle Determinism and Failure
  Atomicity Hardening, R016 §4n above): R015's key-lifecycle tests
  could return early as "BLOCKED" (still counted as PASS by Cargo) if
  the real OS credential store was unavailable, which was not
  sufficient deterministic proof of the canonical A/B/C/D master-key
  state machine.** A new, narrow `security::CredentialStore` trait +
  in-memory test double now proves all four cases, orphan-credential
  cleanup (both failure timings), and three credential-backend error
  paths (GET/SET/DELETE) deterministically, independent of OS keyring
  availability (12 new tests, `req_dkl1`-`req_dkl12`) - driving the
  exact same production state-machine logic
  (`resolve_master_key_with_store`/
  `bootstrap_db_with_key_lifecycle_with_store`), never a parallel
  reimplementation. R015's original `req_kl1`-`req_kl10` are kept as a
  supplementary, environment-dependent smoke/integration suite against
  the real OS keyring.
- **`PRAGMA foreign_keys = ON` enforcement depends on every connection
  going through `storage::open_encrypted`** — true today (production
  bootstrap, version-targeted migration tests, and the storage spike all
  share this one primitive, and `GoalRelation`'s two FKs benefit from
  this automatically), but a future code path that opens a raw
  `rusqlite::Connection` some other way would silently not get FK
  enforcement. Worth a lint/review note if the storage layer ever grows
  a second connection-opening path.
- **`GoalRelation` has no read capability beyond the two narrow
  functions implemented** (`get_goal_relation`/
  `get_goal_relations_for_goal`) - no graph traversal (e.g. "all
  ancestors via PARENT_OF"), no cycle detection, no conflict surfacing.
  Deliberately out of scope - that is M6 Goal Intelligence *reasoning*
  over the graph structure this run only makes available, not this
  run's job (M2 GoalRelation vertical-slice report §19 note).
- **`Milestone` has no read capability beyond
  `get_milestone`/`get_milestones_for_project`** - no aggregate "due
  before X" query, no cross-Project query. Deliberately out of scope,
  same reasoning as GoalRelation above - add only when a real caller
  needs it (M2 Milestone vertical-slice report §19).
- **`Milestone.order_index` has no application-level upper bound**
  beyond `i64`'s own range - deliberate (no canonical policy for one
  exists, and none was invented), not a correctness risk, only a future
  UX consideration for whatever eventually assigns these values.
- **`Task` has no read capability beyond
  `get_task`/`get_tasks_for_project`/`get_tasks_for_goal`** - no
  aggregate "due before X across every Project" query, no standalone-
  task listing (a Task with both parents `None` is only reachable via
  `get_task(id)` today). Deliberately out of scope, same reasoning as
  GoalRelation/Milestone above (M2 Task vertical-slice report §20).
- **`get_tasks_for_project`/`get_tasks_for_goal`'s `due_at ASC`
  ordering is a deliberate, documented *technical* default, not a
  product prioritization decision** - a future "what should I work on
  next" feature must not assume this ordering is the intended UX; that
  is a separate product decision (M2 Task vertical-slice report §20).
- **Exact NeXa transport, exact confirmation-class UX (modal/inline/
  toast)** — both deferred, unchanged.
- Everything already listed as pending from M1/M2-freeze (local/cloud
  model integration for M4/M5, Goal Intelligence and Personal Knowledge
  UI for M6, LifeOS's own software license, bilingual detection
  mechanism) is unchanged by this run.

---

## 11. LifeOS report index

Every substantial report, in chronological creation order (see
`CLAUDE.md`'s Documentation policy for the permanent `R###` numbering
rule, established 2026-09-18). Report numbers are global and permanent
- never renumbered, never reused. This index is pointers only, not
summaries - open the file for full reasoning.

| # | Title | Filename |
|---|---|---|
| R001 | M1 UI Refinement: Home Edge Drawers, Face Geometry, Constellation Background, Chat Sidebar | `docs/reports/R001_M1_UI_REFINEMENT_HOME_DRAWERS_AND_CHAT_SIDEBAR_2026-09-17.md` |
| R002 | M1 Navigation Correction: Full-Screen Edge Transitions, Global Sidebar Removed, NeXa Island as Sole Global Nav | `docs/reports/R002_M1_NAVIGATION_CORRECTION_FULLSCREEN_EDGE_TRANSITIONS_2026-09-17.md` |
| R003 | M2 Architecture Freeze: LifeOS Core + Encrypted Local Storage | `docs/reports/R003_M2_ARCHITECTURE_FREEZE_2026-09-17.md` |
| R004 | M2 Storage Spike: Encrypted SQLite Candidate Verification | `docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md` |
| R005 | M2 Goal Vertical Slice: Storage Hardening + First Real Domain | `docs/reports/R005_M2_GOAL_VERTICAL_SLICE_2026-09-17.md` |
| R006 | M2 Goal Hardening | `docs/reports/R006_M2_GOAL_HARDENING_2026-09-17.md` |
| R007 | M2 Project Vertical Slice | `docs/reports/R007_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md` |
| R008 | M2 Project / Storage Integrity Hardening | `docs/reports/R008_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md` |
| R009 | M2 GoalRelation Vertical Slice | `docs/reports/R009_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md` |
| R010 | M2 Project Child Action Freeze | `docs/reports/R010_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md` |
| R011 | M2 Milestone Vertical Slice | `docs/reports/R011_M2_MILESTONE_VERTICAL_SLICE_2026-09-17.md` |
| R012 | M2 Task Vertical Slice | `docs/reports/R012_M2_TASK_VERTICAL_SLICE_2026-09-17.md` |
| R013 | M2 Task Parent-Validation Hardening | `docs/reports/R013_M2_TASK_PARENT_VALIDATION_HARDENING_2026-09-17.md` |
| R014 | M2 UpdateProject Implementation | `docs/reports/R014_M2_UPDATE_PROJECT_IMPLEMENTATION_2026-09-17.md` |
| R015 | M2 Storage and Audit Safety Hardening | `docs/reports/R015_M2_STORAGE_AND_AUDIT_SAFETY_HARDENING_2026-09-18.md` |
| R016 | M2 Key Lifecycle Determinism and Failure Atomicity Hardening | `docs/reports/R016_M2_KEY_LIFECYCLE_DETERMINISM_AND_FAILURE_ATOMICITY_HARDENING_2026-09-18.md` |

Next report allocated will be **R017**.
