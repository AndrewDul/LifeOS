# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it
whenever project state actually changes (`CLAUDE.md`, Documentation
policy) — not only as an end-of-run formality.

Last updated: 2026-09-17 (M2 Milestone vertical slice — the fourth real
domain entity, and the first required-parent child entity:
`AddMilestone`/`UpdateMilestone` implemented end to end, `Milestone`
order_index uniqueness resolved, a patch-semantics documentation
correction applied. On top of the same day's Goal vertical slice/storage
hardening, Goal hardening pass, Project vertical slice, Project/storage
integrity hardening, GoalRelation vertical slice, and Project Child
Action Freeze pass).

---

## 1. Milestone status

| Milestone | Status |
|---|---|
| M0 — Environment ready | **DONE** (§5) |
| M1 — Visual shell | **ACCEPTED BY USER on 2026-09-17** (§3) |
| M2 — LifeOS Core + encrypted storage | Architecture frozen; storage PROVEN + hardened; **Goal, Project, GoalRelation, and Milestone (the first required-parent child entity) all implemented** 2026-09-17 (§4); rest of domain model not started |
| M3-M10 | Not started |

**M1 is closed** — unchanged, not touched this run.

**M2 now has four real, working, tested domain entities: Goal,
Project, GoalRelation, and Milestone**, end to end through
`domain → actions → services → private services/storage → encrypted
SQLCipher`. Seven runs on 2026-09-17 built this: the Goal vertical slice
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
entity exists in code), and the **Milestone vertical slice** (fourth
entity, the first required-parent child entity - `AddMilestone`/
`UpdateMilestone`, `(project_id, order_index)` UNIQUE resolved, built
directly with the pattern - no separate hardening pass needed). See
§4b-§4i. Next step is the `Task` vertical slice (frozen order, §4h/§4i)
- not the rest of the domain model at once, and still not M3.

---

## 2. What exists right now

A Tauri 2 + React + TypeScript desktop app that launches in a real
window (accepted M1 shell, untouched), plus a working, hardened
encrypted store for four domain entities:

- **Goal**: create and update goals with deterministic validation,
  atomic mutation+audit transactions, tri-state patch semantics
  (Set/Unchanged/Clear), typed storage errors, confirmation
  classification, and a read API.
- **Project**: create projects (optionally linked to an existing Goal by
  `goal_id`, validated to actually exist before persistence) and update
  their progress, same atomic-transaction/typed-error discipline from
  the start, its own `project_audit_log`, and a read API. The first
  *direct foreign key* cross-entity relationship in production M2
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

All four are backed by a database that genuinely enforces referential
integrity at the SQLite level (`PRAGMA foreign_keys = ON`, set on every
connection `open_encrypted` produces - not just declared in schema and
left unenforced, §4e), on top of the already-proven SQLCipher encryption
and keyring master-key lifecycle.

148 automated tests prove all of this (9 storage-spike regression, 17
Goal-slice/hardening integration, 16 direct Goal action-layer unit, 14
Project integration, 10 direct Project action-layer unit, 4 Project/
storage integrity integration, 3 direct `LifecycleStatus` unit, 23
GoalRelation integration, 5 direct `LinkGoals` action-layer unit, 3
direct `GoalRelationType` unit, 24 Milestone integration covering 32
numbered requirements, 17 direct Milestone/`AddMilestone`/
`UpdateMilestone` action-layer unit, 3 direct `MilestoneStatus` unit).
**Not wired into the running app yet** - no Tauri command layer exists,
so the UI cannot create/see real goals, projects, relations, or
milestones yet.

Repository contents as of this run:

```
LiFeOS/
├── .git/, .gitignore, .vscode/extensions.json, CLAUDE.md, README.md
├── scripts/
│   └── setup-windows-storage-env.ps1   (new - see §4b)
├── docs/
│   ├── LIFEOS_SPEC.md, RESEARCH_DECISIONS.md, PROJECT_STATE.md
│   └── reports/    REPORT_UI_REFINEMENT_..._2026-09-17.md
│                    REPORT_NAVIGATION_CORRECTION_..._2026-09-17.md
│                    REPORT_M2_ARCHITECTURE_FREEZE_2026-09-17.md
│                    REPORT_M2_STORAGE_SPIKE_2026-09-17.md
│                    REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md
│                    REPORT_M2_GOAL_HARDENING_2026-09-17.md
│                    REPORT_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md
│                    REPORT_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md
│                    REPORT_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md
│                    REPORT_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md
│                    REPORT_M2_MILESTONE_VERTICAL_SLICE_2026-09-17.md
├── index.html, vite.config.ts, tsconfig*.json, package.json
├── public/, src/    (React frontend - M1, accepted, untouched)
└── src-tauri/
    ├── migrations/V1__create_goals.sql          (PRODUCTION schema - Goal only, untouched)
    │              V2__create_projects.sql        (PRODUCTION schema - Project only, untouched)
    │              V3__create_goal_relations.sql  (PRODUCTION schema - GoalRelation only, untouched)
    │              V4__create_milestones.sql      (PRODUCTION schema - Milestone only, new)
    ├── src/
    │   ├── main.rs, lib.rs       (run() unchanged; declares the 5 modules, all private)
    │   ├── core_error.rs          CoreError/CoreErrorKind - typed Core-boundary error model
    │   │                          (Storage/Migration/Security/Internal)
    │   ├── domain/
    │   │   ├── mod.rs              Goal, GoalId, Project, ProjectId, GoalRelation,
    │   │   │                       GoalRelationId, Milestone, MilestoneId (new, all UUID
    │   │   │                       v7), LifecycleStatus (shared by Goal.status/
    │   │   │                       Project.status), GoalRelationType (frozen closed
    │   │   │                       11-value enum, NOT the same thing as the Personal
    │   │   │                       Knowledge Layer's separate free-string
    │   │   │                       Relation.relation_type), MilestoneStatus (new -
    │   │   │                       frozen closed 2-value enum, NOT LifecycleStatus),
    │   │   │                       OrderIndex/ORDER_INDEX_MIN (new), Progress, Priority
    │   │   │                       (shared)
    │   │   └── tests.rs             9 round-trip unit tests (3 LifecycleStatus, 3
    │   │                            GoalRelationType - all 11 values/parse round trips,
    │   │                            3 new MilestoneStatus)
    │   ├── actions/
    │   │   ├── mod.rs              Actor, Provenance, ActionMeta, ConfirmationClass,
    │   │   │                       ActionOutcome<T>, ValidationError (+
    │   │   │                       ReferencedGoalNotFound, SelfGoalRelation,
    │   │   │                       RelationFromGoalNotFound, RelationToGoalNotFound,
    │   │   │                       DuplicateGoalRelation, NoteTooLong,
    │   │   │                       ReferencedProjectNotFound, OrderIndexOutOfBounds,
    │   │   │                       DuplicateMilestoneOrderIndex - new), FieldPatch<T>,
    │   │   │                       CreateProjectPayload/UpdateProgressPayload/
    │   │   │                       LinkGoalsPayload/AddMilestonePayload/
    │   │   │                       UpdateMilestonePayload (new), blocks_until_confirmed
    │   │   └── tests.rs             48 direct action-layer unit tests (16 Goal, 10
    │   │                            Project, 5 LinkGoals, 17 new AddMilestone/
    │   │                            UpdateMilestone)
    │   ├── security/mod.rs        OS-keyring master-key lifecycle
    │   └── services/
    │       ├── mod.rs              bootstrap_production_db, create_goal/update_goal,
    │       │                       create_project/update_progress/get_project,
    │       │                       link_goals/get_goal_relation/
    │       │                       get_goal_relations_for_goal, add_milestone/
    │       │                       update_milestone/get_milestone/
    │       │                       get_milestones_for_project (new) - all atomic
    │       │                       (&mut Connection + rusqlite transaction) - capability
    │       │                       API contract's names, no dedicated capability/
    │       │                       module yet (LIFEOS_SPEC.md §5.6/§10.1)
    │       ├── tests.rs            82 integration test functions against a real
    │       │                       encrypted DB (17 Goal, 14 Project, 4 Project/
    │       │                       storage integrity, 23 GoalRelation functions covering
    │       │                       26 numbered requirements, 24 new Milestone functions
    │       │                       covering 32 numbered requirements - incl. 8
    │       │                       forced-audit-failure/raw-SQL-FK-bypass proofs across
    │       │                       all 4 entities, a V3-populated-database
    │       │                       upgrades-to-V4 migration-compatibility test, 5 new
    │       │                       raw-SQL DB-defense-in-depth proofs for Milestone's
    │       │                       FK/UNIQUE/CHECK constraints, and a raw hard-DELETE
    │       │                       RESTRICT proof for Project)
    │       └── storage/            PRIVATE (mod storage; - no pub at all)
    │           ├── mod.rs           generic encrypted-SQLite primitives + prod migrations;
    │           │                    open_encrypted calls enable_foreign_keys (sets +
    │           │                    verifies PRAGMA foreign_keys = ON on every
    │           │                    connection - production, test, and spike alike,
    │           │                    since all go through this one primitive); also
    │           │                    hosts actor_str/provenance_str/
    │           │                    confirmation_class_str/parse_rfc3339 shared by all
    │           │                    4 repo modules (not duplicated per-module)
    │           ├── goal_repo.rs     Goal SQL access, pub(in crate::services) - also
    │           │                    hosts goal_exists (shared by project_repo AND
    │           │                    goal_relation_repo)
    │           ├── project_repo.rs  Project SQL access, pub(in crate::services) - also
    │           │                    now hosts project_exists (new, shared by
    │           │                    milestone_repo)
    │           ├── goal_relation_repo.rs  GoalRelation SQL access, pub(in
    │           │                    crate::services)
    │           ├── milestone_repo.rs  Milestone SQL access, pub(in crate::services)
    │           │                    (new) - incl. order_index_taken, parameterized to
    │           │                    optionally exclude a Milestone from its own
    │           │                    occupancy check
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

Full evidence: `docs/reports/REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`
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
`docs/reports/REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`.

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

Full evidence: `docs/reports/REPORT_M2_GOAL_HARDENING_2026-09-17.md`.

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
`docs/reports/REPORT_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`.

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

Full evidence: `docs/reports/REPORT_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md`.

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
`docs/reports/REPORT_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md`.

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

Full reasoning: `docs/reports/REPORT_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md`.

### 4h. What's still architecturally frozen but not yet implemented

Personal Knowledge Layer, `Task`/`Challenge`/`Plan`/`SolutionAttempt`/
`Outcome`/`CalendarEvent`/`Reminder`/`Question`/`ProgressUpdate`/
`WellbeingCheckin`, the rest of the frozen action list (`AddTask` for
Project - still schema/code-unimplemented, though its sibling
`UpdateTask` is already action-set-frozen, §4g; `AddMilestone`/
`UpdateMilestone` are no longer in this list - implemented, §4i),
`ForgetMemory`'s corrected (hard-delete) semantics, the `knowledge`/
`backup` modules, a real `capability` module, NeXa transport. Full
detail: `docs/LIFEOS_SPEC.md` §5.1-§5.6.

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
`docs/reports/REPORT_M2_MILESTONE_VERTICAL_SLICE_2026-09-17.md`.

### Next implementation step (M2)

**`Task` vertical slice** (frozen order, §4g) - the fifth domain
entity. Reuse `domain → actions → services →
services::storage::<entity>_repo` with the same atomicity/typed-error
discipline applied from the start, and the action set/patch semantics
(corrected per §4i)/confirmation floor/FK policy/creation default
already frozen in §4g/`LIFEOS_SPEC.md` §5.1 - implement `AddTask` and
`UpdateTask` together. `Task` adds two genuinely new complexities
`Milestone` did not have to resolve: two independent optional parent
foreign keys (`project_id`/`goal_id`, both nullable, a Task may be
standalone/Project-only/Goal-only/both - already confirmed
unconstrained) and a four-value status enum (`TODO`/`IN_PROGRESS`/
`DONE`/`CANCELLED`, all peer states, no terminal-state rule). Do not
invent product rules inside the implementation - flag them as open
questions instead (§4c's lesson from the removed terminal-status rule;
§4e's/§4g's action-set-gap documentation-before-code discipline; §4f's
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
  19. (this run) `feat: implement M2 Milestone vertical slice` — see
      this run's report and chat reply for the exact hash.
- Pushed to `origin/main` through commit 7. **Commits 8-19 were not
  pushed** — not authorized in any of those runs' instructions.

### Note on an incident in an earlier run (kept for reference)

`npm create tauri-app@latest -- . --force` briefly deleted `CLAUDE.md`
and all `docs/*.md` files and overwrote `.gitignore` during the original
M1 scaffold. Restored via `git checkout`, no data lost.

---

## 8. What can the user actually do right now?

Launch the real app (`npm run tauri dev`) and use the accepted M1 shell -
unchanged, no real goals, projects, relations, or milestones visible
there yet (no Tauri command wiring). Separately,
`cd src-tauri && cargo test -- --test-threads=1` runs 148 tests proving:
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
+ 5 direct action-layer unit + 3 direct `GoalRelationType` unit), and a
full AddMilestone/UpdateMilestone lifecycle - default-PENDING creation,
reversible PENDING/DONE status, `(project_id, order_index)` UNIQUE
enforced both in Core and the database, all 5 database-level FK/UNIQUE/
CHECK/RESTRICT proofs via raw SQL, and a V3-to-V4 migration-
compatibility check (24 integration test functions covering 32 numbered
requirements + 17 direct action-layer unit + 3 direct `MilestoneStatus`
unit) - all passing against temp databases, never the user's real data.
**Test count rose from 104 to 148 this run** (+44: 24 integration
functions, 17 direct action-layer unit, 3 domain unit) - all 104 prior
tests remain green, unmodified.

---

## 9. Next recommended step

The `Task` vertical slice - see §4i's "Next implementation step" for the
exact reasoning (this duplicates it so it is visible without scrolling
past the M2 detail section). Its action set (`AddTask`/`UpdateTask`),
patch semantics (`title: Option<String>`; `due_at`/`project_id`/
`goal_id: FieldPatch<T>`, corrected per §4i), confirmation floor
(`EXPLICIT_ADD`), and FK policy (`ON DELETE RESTRICT` on both nullable
parent references) are already frozen (§4g) - implementation should not
need to invent any of those. Two genuinely new complexities vs.
`Milestone`: two independent optional parent foreign keys, and a
four-value status enum with no terminal state.

---

## 10. Known problems / risks / unresolved questions

- **Neither Goal, Project, GoalRelation, nor Milestone (nor the storage
  bootstrap) is wired into the running app** — no Tauri command layer
  exists yet; `services`' functions are only reachable from tests.
- **RESOLVED (2026-09-17, M2 Project Child Action Freeze): the Project
  action-set gap is closed.** `UpdateProject` (floor `IMPORTANT_CHANGE`)
  is now frozen - `LIFEOS_SPEC.md` §5.1/§5.4. Not yet implemented in
  code (documentation-only freeze); implementation is expected whenever
  Project editing is actually built, not necessarily the very next run.
  `UpdateTask` (floor `EXPLICIT_ADD`) is also frozen, pre-emptively,
  before `Task` exists in code. `UpdateMilestone` is no longer in this
  category - implemented, §4i.
- **RESOLVED (2026-09-17, M2 Milestone vertical slice, §4i above):
  `Milestone.order_index` uniqueness within a Project.** YES,
  `(project_id, order_index)` must be unique; different Projects may
  freely reuse the same values; enforced both in Core and by a real SQL
  `UNIQUE` constraint.
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
  door.
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
- **Exact NeXa transport, exact confirmation-class UX (modal/inline/
  toast)** — both deferred, unchanged.
- Everything already listed as pending from M1/M2-freeze (local/cloud
  model integration for M4/M5, Goal Intelligence and Personal Knowledge
  UI for M6, LifeOS's own software license, bilingual detection
  mechanism) is unchanged by this run.
