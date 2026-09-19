# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it
whenever project state actually changes (`CLAUDE.md`, Documentation
policy) — not only as an end-of-run formality.

Last updated: 2026-09-18 (M2 Personal Knowledge Privacy and Foundation
Finalization, R019 - **documentation/architecture only, no Rust or
React code changed, no migration created.** The final pre-`V7`
consistency pass, closing the two-round review cycle R018 began.
Found and corrected six further gaps: `Entity` gains a `sensitivity`
field (R018 had only fixed this for `Source`, despite the identical
reasoning applying to `Entity.canonical_name`/`domain_ref`);
**`ForgetMemory` gained a deterministic orphan-Entity cleanup step it
was entirely missing** - a brand-new Entity `AddMemory` creates solely
for one Fact could otherwise survive that Fact being forgotten,
undermining the whole guarantee; a same-payload duplicate-`domain_ref`
input now fails in Core before any SQL; the exact `V7` table scope is
resolved to all eleven already-frozen tables (R017 and R018 had
disagreed - R018's Slice A only listed six); `Source`/`Evidence`'s
creation payload shapes are fully specified now that both carry
`sensitivity`; and, most consequentially, **"true deletion" is now
explicitly split into a logical/product guarantee (fully upheld - no
live row, no query path, no payload-bearing audit snapshot anywhere
LifeOS's own code can reach) versus a forensic-storage guarantee
(honestly evaluated by direct inspection: this codebase sets/queries
only `key`/`foreign_keys`/`rekey`/`integrity_check` - no
`journal_mode`, no `secure_delete` - so no physical-media-level
guarantee is or can be claimed; a concrete future hardening step,
`PRAGMA secure_delete = ON` set-and-verified, is frozen but not
implemented)**. R017's and R018's original text is unedited - a
one-line forward pointer was added to R017's addendum, a full addendum
appended to R018; R019 is authoritative wherever any of the three
disagree. **Architecture is now explicitly declared CLOSED for Slice
A/B** (R019 §23) - the recommended next slice is R019 §21/§22's final
corrected Slice A+B, ready to implement directly. See §4q (§4p for
R018's first hardening pass, §4o for R017's original freeze). On top of
2026-09-18's M2 Key Lifecycle Determinism and Failure Atomicity
Hardening (R016, §4n) and M2 Storage and Audit Safety Hardening (R015,
§4m), and 2026-09-17's Goal vertical slice/storage hardening, Goal
hardening pass, Project vertical slice, Project/storage integrity
hardening, GoalRelation vertical slice, Project Child Action Freeze
pass, Milestone vertical slice, Task vertical slice, Task
parent-validation hardening, and UpdateProject implementation).

---

## 1. Milestone status

| Milestone | Status |
|---|---|
| M0 — Environment ready | **DONE** (§5) |
| M1 — Visual shell | **ACCEPTED BY USER on 2026-09-17** (§3) |
| M2 — LifeOS Core + encrypted storage | Architecture frozen; storage PROVEN + hardened; **Goal, Project, GoalRelation, Milestone, and Task all implemented; the Project action-set gap is now fully closed (`UpdateProject` implemented)** 2026-09-17 (§4); **master-key lifecycle + mandatory audit-snapshot safety hardened** 2026-09-18 (§4m), **then verified deterministically + corrected for a post-file-creation cleanup edge case** 2026-09-18 (§4n); **Personal Knowledge Layer v0.1 architecture fully frozen at field level (docs-only, not implemented)** 2026-09-18 (§4o), **then two rounds of post-freeze consistency review found and fixed 17 gaps total (four critical), architecture declared CLOSED for implementation** 2026-09-18 (§4p, §4q), **then Slice A (V7 foundation, SELF bootstrap, narrow reads, secure_delete hardening) actually implemented in code** 2026-09-18 (§4r); rest of domain model not started |
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

398 automated tests prove all of this (262 of them covering the
CRUD/relation M2 entities below plus storage/key-lifecycle hardening;
136 more covering M2 Personal Knowledge Slices A/B/C/D/E,
§4r/§4s/§4t/§4u/§4v - the full
breakdown by category: 9 storage-spike regression, 17
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
availability), plus (R020, 2026-09-18) 23 new M2 Personal Knowledge
Slice A tests - `secure_delete`/`journal_mode` storage-hardening
proofs, SELF-bootstrap proofs, `V1`-`V7`/populated-`V6`-to-`V7`
migration-compatibility proofs, a full raw-SQL defense-in-depth matrix
across all six Personal Knowledge tables (including a positive proof
that Personal Knowledge `Relation.relation_type` genuinely accepts a
value outside `GoalRelation`'s 11 closed values), all 15 frozen
length-bound proofs, and `get_entity`/`get_fact` read-round-trip
proofs, plus (R020 post-run verification hardening, same date) 8 more
tests closing four evidence gaps (full join-table/`ON DELETE RESTRICT`
proofs via real parent `DELETE` attempts, `fact_audit_log`'s own
constraints, full enum round-trip/DB-acceptance proofs, and a
distinctive-plaintext-marker encryption-at-rest proof), plus (R021,
2026-09-18) 31 new M2 Personal Knowledge AddMemory Slice B tests - the
confirmation gate (4 scenarios), a 10-combination success matrix, both
layers of duplicate-domain-ref detection, the full authoritative
domain-ref matrix for `Goal`/`Project`/`Milestone`/`Task`, Existing-
reference not-found and immutability-on-reuse, two independent forced-
failure atomic-rollback proofs, audit-row correctness including
provenance-never-rewritten, zero Relation/Episode side effects, and
explicit no-dedup/no-alias-merge proofs, plus (R021's own post-run
correctness/verification hardening, same date) 3 more tests correcting
the confirmation-gate ordering to precede all database-dependent
resolution and closing action-level bound/audit-timestamp/duplicate-
domain-ref evidence gaps, plus (R022, 2026-09-18) 29 new M2 Personal
Knowledge CorrectMemory Slice C tests - the confirmation matrix (4
scenarios, including the Personal-Knowledge inferred-confirmation
overlay `AddMemory` did not need), the active/inactive predecessor
matrix (4 cases proving the full `is_active_fact` predicate), all five
`FactValueInput` success shapes plus five rejection paths, inherit-vs-
override for `memory_type`/`sensitivity`/`confidence`, the full
successor/predecessor field proof with exact no-gap/no-overlap
timestamps, an A->B->C correction chain, sibling-Fact non-interference,
mandatory-snapshot exact-deserialization, audit `fact_id`-is-
predecessor-id, two independent rollback proofs, and zero Relation/
Episode side effects, plus (R022's own post-run temporal-integrity/
audit hardening, same date) 5 more tests replacing a `debug_assert!`
with a real runtime enforcement of `update_fact_as_superseded`'s
affected-row count, hardening the `UPDATE` itself with an active-Fact
guard, and strengthening the audit proof to directly assert actor/
`occurred_at`, plus (R023, 2026-09-18) 14 new M2 Personal Knowledge
ConfirmMemory Slice D tests - the confirmation matrix (4 scenarios,
needing no Personal-Knowledge-specific overlay since `ExplicitAdd` is
already dominated generically), the full epistemic-transition matrix
(`INFERRED`/`OBSERVED`/`DISPUTED` -> `USER_CONFIRMED`, an already-
confirmed no-op rejection, inactive-Fact rejection via the reused
`is_active_fact` predicate), identity/no-successor proof, the full
in-place two-field mutation contract, no Entity/Source/Evidence/
Relation/Episode side effects, audit-field/snapshot/timestamp
exactness, a `DISPUTED`-sibling non-interference proof, a forced-audit-
failure rollback proof, and a direct guarded-update repo-level matrix,
plus (R024, 2026-09-18) 23 new M2 Personal Knowledge ForgetMemory Slice
E tests - the first hard-DELETE action's five-scenario confirmation
matrix (`Delete` blocks every unconfirmed call, `Explicit` included),
active/historical target eligibility, the exact 14-step transaction
(audit-snapshot scrubbing, inbound-supersession clearing without
rewiring, Evidence privacy-first deletion even when shared with a
retained Fact/Relation/Episode, Source/Entity orphan-vs-shared
lifecycle, hard Fact deletion), a full supersession-chain matrix, a
ten-case Entity retention matrix, and two independent rollback proofs.
**The Goal/Project/GoalRelation/Milestone/Task action layer is still
not wired into the running app** - no Tauri command layer exists, so
the UI cannot create/see real goals, projects, relations, milestones,
or tasks yet. **The Personal Knowledge Layer now has a real `V7`
schema, a `SELF` entity bootstrap, narrow `get_entity`/`get_fact` reads
(R020), and all four frozen Memory actions implemented: `AddMemory`
(R021), `CorrectMemory` (R022), `ConfirmMemory` (R023), and
`ForgetMemory` (R024)**, and no Tauri command layer exists for any of
them, so no Personal Knowledge data can be created, corrected,
confirmed, or forgotten through the running app yet, only through
direct Core calls in tests (Relation/Episode/retrieval + UI wiring both
still pending).

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
│                    R017_M2_PERSONAL_KNOWLEDGE_LAYER_ARCHITECTURE_FREEZE_2026-09-18.md
│                    R018_M2_PERSONAL_KNOWLEDGE_ARCHITECTURE_CONSISTENCY_HARDENING_2026-09-18.md
│                    R019_M2_PERSONAL_KNOWLEDGE_PRIVACY_AND_FOUNDATION_FINALIZATION_2026-09-18.md
│                    R020_M2_PERSONAL_KNOWLEDGE_SLICE_A_FOUNDATION_2026-09-18.md
├── index.html, vite.config.ts, tsconfig*.json, package.json
├── public/, src/    (React frontend - M1, accepted, untouched)
└── src-tauri/
    ├── migrations/V1__create_goals.sql          (PRODUCTION schema - Goal only, untouched)
    │              V2__create_projects.sql        (PRODUCTION schema - Project only, untouched)
    │              V3__create_goal_relations.sql  (PRODUCTION schema - GoalRelation only, untouched)
    │              V4__create_milestones.sql      (PRODUCTION schema - Milestone only, untouched)
    │              V5__create_tasks.sql           (PRODUCTION schema - Task only, untouched)
    │              V6__widen_project_audit_log_action_types.sql (widens
    │                             project_audit_log.action_type's CHECK to allow
    │                             'UpdateProject'; projects itself untouched, see
    │                             R014_M2_UPDATE_PROJECT_IMPLEMENTATION_2026-09-17.md §3a)
    │              V7__create_personal_knowledge.sql (new, R020 - the full 11-table
    │                             Personal Knowledge foundation: entities/sources/
    │                             evidence/facts/relations/episodes/fact_evidence/
    │                             relation_evidence/episode_evidence/episode_entities/
    │                             fact_audit_log; V1-V6 untouched)
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
    │   │   │                       Priority (shared). (R020, new) EntityId/FactId/
    │   │   │                       RelationId/EpisodeId/SourceId/EvidenceId (UUID v7),
    │   │   │                       Sensitivity (4-value closed), MemoryType (11-value
    │   │   │                       closed), EpistemicState (5-value closed), DomainRef,
    │   │   │                       Entity/Source/Evidence/Fact (+ FactValue) - the
    │   │   │                       Personal Knowledge domain structs Slice A's read
    │   │   │                       side needs (Relation/Episode structs deliberately
    │   │   │                       not added - no read function needs them yet), plus
    │   │   │                       all 15 R018-frozen length constants and
    │   │   │                       validate_personal_knowledge_text_len
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
    │       │                       capability/module yet (LIFEOS_SPEC.md §5.6/§10.1).
    │       │                       (R020, new) ensure_self_entity (0/1/>1 SELF dispatch,
    │       │                       wired into bootstrap_db_at right after migrations,
    │       │                       NOT into the test-only bootstrap_db_at_version),
    │       │                       get_entity/get_fact - no AddMemory/CorrectMemory/
    │       │                       ConfirmMemory/ForgetMemory exists yet
    │       ├── tests.rs            190 integration test functions against a real
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
    │       │                       GoalRelation non-side-effect proof, and (R020,
    │       │                       2026-09-18) 23 new Personal Knowledge Slice A
    │       │                       functions: 5 secure_delete proofs, 1 journal_mode
    │       │                       measurement (MEASURED: delete), 5 SELF-bootstrap
    │       │                       proofs, 2 migration proofs (fresh V1-V7, populated
    │       │                       V6-to-V7 non-destructive), 7 raw-SQL defense-matrix
    │       │                       functions across all 6 Personal Knowledge tables
    │       │                       (20+ sub-assertions, incl. proving Relation.
    │       │                       relation_type is genuinely open), 1 table-driven
    │       │                       length-bound proof (all 15 constants), 2 get_entity/
    │       │                       get_fact read-round-trip proofs, and (post-run
    │       │                       verification hardening, same date) 8 more functions
    │       │                       completing join-table FK/duplicate coverage for
    │       │                       relation_evidence/episode_evidence/episode_entities,
    │       │                       2 ON DELETE RESTRICT matrices proven via a real
    │       │                       parent DELETE attempt (14 relationships total,
    │       │                       matching the operation a future ForgetMemory's
    │       │                       deletion order depends on), fact_audit_log's own
    │       │                       constraints (incl. proving fact_id genuinely accepts
    │       │                       a nonexistent FactId, since it is deliberately not an
    │       │                       FK), 3 table-driven proofs that every Sensitivity/
    │       │                       MemoryType/EpistemicState value round-trips and is
    │       │                       DB-accepted, and a strengthened encryption-at-rest
    │       │                       proof that a distinctive Personal Knowledge value is
    │       │                       absent from the raw file, not just the SQLite header)
    │       └── storage/            PRIVATE (mod storage; - no pub at all)
    │           ├── mod.rs           generic encrypted-SQLite primitives + prod migrations;
    │           │                    open_encrypted calls enable_foreign_keys (sets +
    │           │                    verifies PRAGMA foreign_keys = ON on every
    │           │                    connection - production, test, and spike alike,
    │           │                    since all go through this one primitive), then (R020,
    │           │                    new) enable_secure_delete (same set-and-verify
    │           │                    discipline - PRAGMA secure_delete = ON, never FAST,
    │           │                    never the nonexistent FULL R019 had misnamed); also
    │           │                    hosts actor_str/provenance_str/
    │           │                    confirmation_class_str/parse_rfc3339 shared by all
    │           │                    6 repo modules (not duplicated per-module)
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
    │           ├── personal_knowledge_repo.rs  (new, R020) Entity/Fact insert+get,
    │           │                    Source/Evidence/fact_evidence-link insert
    │           │                    (#[cfg(test)]-only fixtures - no AddMemory exists to
    │           │                    create these for real yet), count_self_entities/
    │           │                    get_self_entity. NO insert_relation/insert_episode -
    │           │                    those tables are schema-only (R019 §13), touched
    │           │                    only by deliberate raw SQL at the test boundary
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
   confirms the resulting state re-resolves as Case B (`Reused`), never
   Case C. This proves the DB/key ASSOCIATION is retained and a
   subsequent bootstrap attempt can retry with the same key - it does
   NOT prove the partially-created database file itself is valid, or
   that SQLCipher/migrations can actually complete against it; that is
   a separate recovery question `req_dkl7`'s synthetic-bytes initializer
   does not exercise.
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
4. **12 new deterministic key-lifecycle tests** (`req_dkl1`-`req_dkl12`,
   in-memory credential store) - aggregate count rose 250 → 262 because
   only these 12 new functions increase the test count. One
   PRE-EXISTING test, `req_snap1`, was additionally strengthened (§3
   above) to assert diagnostic redaction, not added as new - all other
   previously-covered behavior remains green, unmodified. An incidental
   fix found along the way: `ResolvedMasterKey`
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

### 4o. M2 Personal Knowledge Layer Architecture Freeze (2026-09-18, R017, documentation/architecture only)

**Eleven points below were found inconsistent/under-specified by a
dedicated post-freeze review and corrected in R018 (§4p below) before
any implementation - most importantly the SELF-entity invariant, the
lack of any way for `AddMemory` to create a new Entity, and three
privacy/integrity gaps in `ForgetMemory` (old audit snapshots, an
inbound FK that would have blocked deletion outright, and Source having
no sensitivity field). This section is left as originally written for
history; §4p is authoritative wherever it disagrees with this section.**

**No Rust or React code changed. No migration created.** A dedicated
design/freeze pass - the kind of planning pass §4n above said was
needed before starting either `RecurringRule` or the Personal Knowledge
Layer - for the Personal Knowledge Layer specifically: LifeOS's central
differentiator (facts, preferences, decisions, experiences, what
worked/didn't, skills, habits, current state, relationships, history,
provenance about the user - not just a planner/calendar/chatbot). R003
had already named the six-table minimum (`Entity`/`Fact`/`Relation`/
`Episode`/`Evidence`/`Source`) and the 11 memory-type/5 epistemic-state
closed sets, but only as a one-paragraph-per-table sketch ("table
shape, not final column-level SQL"). R017 freezes the actual field-level
design and the four Memory actions' exact semantics:

- **Genuine contradictions found against R003's original sketch,
  identified explicitly and resolved (not silently overwritten) -
  R017 §2, seven items:** `Entity.kind` (closed enum) → `entity_type`
  (open string) + `is_self: bool`; `Fact.content` (opaque blob) → a
  structured subject-predicate-object triple
  (`subject_entity_id`/`predicate`/exactly one of `value_json` or
  `object_entity_id`); `Fact.subject_entity_id` nullable → `NOT NULL`
  (tightened, now that a frozen SELF entity always exists);
  `Fact.source_id` (direct FK) removed entirely - provenance now flows
  exclusively through `fact_evidence` → `Evidence.source_id` → `Source`;
  `Relation`'s polymorphic `ENTITY|FACT` endpoints → Entity-to-Entity
  only, both sides real FKs; `Evidence.fact_id` (embedded single-parent
  FK) → normalized `fact_evidence`/`relation_evidence`/
  `episode_evidence` link tables, so one `Evidence` row can support more
  than one `Fact`/`Relation`/`Episode`; `Source.kind` (ambiguously
  closed) → explicitly frozen OPEN (`source_type`, plain validated
  `TEXT`, R003's six values plus three more as a documented starting
  vocabulary, not an enforced ceiling).
- **`Entity`**: `id`, `entity_type` (open vocabulary), `canonical_name`,
  `is_self: bool`, `domain_ref_type`/`domain_ref_id` (nullable, NOT an
  enforced FK - deliberately, since the referenced domain table varies
  and this module must not need schema-time knowledge of every domain
  table that will ever exist; validity is a Core-level check reusing
  the existing domain module's own `*_exists` functions, same pattern
  as `Project.goal_id`), `created_at`/`updated_at`. **Exactly one SELF
  entity**, enforced via a partial unique index on `is_self = 1`.
  Aliases/entity-resolution explicitly deferred.
- **`Fact`**: the central assertion, subject-predicate-object shape
  above, `memory_type` (11 frozen values, unchanged/unrenamed) +
  `epistemic_state` (5 frozen values, unchanged/unrenamed) both closed
  `CHECK`-constrained enums, `confidence` (`Option<f32>`, 0.0-1.0, `NULL`
  = no fabricated percentage - never conflated with "user confirmed
  saying this"), `sensitivity` (new 4-value closed set, below),
  `valid_from`/`valid_until`/`superseded_by_fact_id` for temporal
  supersession (never in-place semantic overwrite). **Mutability
  contract frozen**: after creation, only `epistemic_state`/
  `confidence`/`sensitivity`/`valid_until`/`superseded_by_fact_id`/
  `updated_at` may ever change - everything else is immutable for the
  row's life; changing what a Fact asserts always means a new Fact.
- **11 memory types preserved exactly**, each given a precise,
  non-overlapping definition (R017 §6) - `FACT` (stable/general) vs.
  `OBSERVATION` (a single noticed data point) vs. `HYPOTHESIS`
  (unconfirmed candidate pattern); `EXPERIENCE` (neutral, no verdict) vs.
  `EFFECTIVE_SOLUTION`/`INEFFECTIVE_SOLUTION` (same family, with a
  positive/negative efficacy verdict); `CURRENT_STATE` vs. a long-lived
  `FACT` (same mechanism, distinguished only by expected churn rate and
  framing, not a different technical mechanism).
- **5 epistemic states preserved exactly**, each precisely defined
  (R017 §7) - `INFERRED` is never equivalent to `USER_CONFIRMED` and
  must never silently become it (only `ConfirmMemory` can make that
  transition); `DISPUTED` means unresolved/conflicting, never silently
  treated as settled; `STALE` means historically retained, excluded
  from active-knowledge retrieval by default. Kept structurally separate
  from `confidence` and `sensitivity` - three independent dimensions,
  never derived from one another.
- **`Relation` (Personal Knowledge) reaffirmed completely distinct from
  `GoalRelation`** - open `relation_type` string (never
  `GoalRelation`'s closed 11-value enum), Entity-to-Entity only, no
  automatic generation in either direction between the two layers. No
  action mutates `Relation` yet in this freeze - schema only, its own
  action family needs a dedicated freeze pass before implementation
  (slice F, below), mirroring how Milestone/Task's schema was frozen
  ahead of their actions (R010's precedent).
- **`Episode`** (a bounded historical event/context unit - not chat
  history, not a `Fact`, not a `CalendarEvent`) and **`Source`**
  (provenance metadata only - `source_type` open, `label`/`reference`
  bounded locator strings, deliberately no `sensitivity` field since it
  structurally never holds sensitive payload) and **`Evidence`** (the
  specific supporting payload/snippet, `content`/`locator`, its own
  independent `sensitivity` - never inherited from the Fact it
  supports) all schema-frozen, with normalized join tables
  (`fact_evidence`/`relation_evidence`/`episode_evidence`/
  `episode_entities`) rather than embedded polymorphic FKs.
- **Sensitivity: new frozen 4-value closed set** -
  `PUBLIC`/`INTERNAL`/`SENSITIVE`/`NEVER_LEAVES_DEVICE` (promoted from
  `LIFEOS_SPEC.md` §4.3's illustrative example list to the actual
  frozen v0.1 set) - resolves the `LIFEOS_SPEC.md` §13 open question
  "exact data-sensitivity classification levels... needs to be defined
  alongside the actual schema." Applied to `Fact`/`Relation`/`Episode`/
  `Evidence`, deliberately not `Source`.
- **Four Memory actions, exact semantics frozen, confirmation floors
  CONFIRMED unchanged from R003 §F/`LIFEOS_SPEC.md` §5.5 (not
  re-decided this run):** `AddMemory` (`EXPLICIT_ADD` - creates
  Fact+Source(new-or-existing)+Evidence+link atomically; inferred
  candidates block with zero writes until confirmed, exactly like every
  other `INFERRED_CHANGE` action already proven for `CreateGoal`),
  `CorrectMemory` (`IMPORTANT_CHANGE` - creates a successor Fact,
  closes/supersedes the predecessor atomically, mandatory
  `previous_snapshot` via the existing `serialize_required_snapshot`
  discipline), `ConfirmMemory` (`EXPLICIT_ADD` - updates the SAME Fact's
  `epistemic_state` in place, never creates a duplicate temporal
  successor; rejects an already-`USER_CONFIRMED` or non-active target),
  `ForgetMemory` (`DELETE`, true permanent deletion - a fully specified,
  deterministic, privacy-safe deletion algorithm: removes the Fact,
  removes now-orphaned Evidence, never touches shared Entities, never
  touches Source metadata at all, `previous_snapshot` is UNCONDITIONALLY
  `NULL` for this action - it must never route through the ordinary
  mandatory-snapshot helper, or the "forgotten" content would survive in
  the audit log; the residual marker is the Fact's own random UUIDv7 id,
  already non-sensitive with no hashing needed).
- **Retrieval/Context Pack contract frozen as an architectural target
  only** - no embeddings, no vector DB, no retrieval code this run;
  the encrypted relational model remains the sole source of truth,
  any future embedding index must be rebuildable from it and
  invalidated when knowledge is forgotten.
- **Qwen/Luna boundary reaffirmed** - neither ever mutates Personal
  Knowledge directly; both only ever propose typed, `Provenance::
  Inferred` candidates through the same Core validation/confirmation
  path as any other actor.
- **Explicitly deferred, not speculatively scaffolded** (R017 §26):
  locked/pinned memory, `last_used`/usage tracking, memory importance/
  weight, entity alias/resolution, ontology management for
  `predicate`/`relation_type`, embeddings/semantic retrieval,
  `Relation`/`Episode`'s own action families, bulk "forget everything
  about X", backdating `valid_from`, the Memory inspection UI.
- **Recommended implementation order** (not started): Slice A (`Entity`
  + `Source` + `Evidence` + `Fact` foundation, schema + narrow read-side)
  → Slice B (`AddMemory`) → Slice C (`CorrectMemory`) → Slice D
  (`ConfirmMemory`) → Slice E (`ForgetMemory`) → Slice F (`Relation`,
  needs its own action-freeze pass first) → Slice G (`Episode`, same) →
  Slice H (retrieval/Context Pack foundation). A+B are the recommended
  *next* slice specifically - nothing else in the family can exist
  before them, and `AddMemory` is the only Memory action with no
  dependency on another Memory action already having run.

Full evidence, exact field-by-field schemas, and every design decision:
`docs/reports/R017_M2_PERSONAL_KNOWLEDGE_LAYER_ARCHITECTURE_FREEZE_2026-09-18.md`.

### 4p. M2 Personal Knowledge Architecture Consistency Hardening (2026-09-18, R018, documentation/architecture only)

**Six further points below were found inconsistent/under-specified by
a final pre-`V7` review and corrected in R019 (§4q below) - most
importantly, `Entity` also needed a `sensitivity` field, `ForgetMemory`
needed an orphan-Entity cleanup step, and "true deletion" needed to be
split into a logical guarantee (upheld) versus a forensic-storage
guarantee (honestly evaluated, not assumed). This section is left as
originally written for history; §4q is authoritative wherever it
disagrees with this section.**

**No Rust or React code changed. No migration created.** A dedicated
post-freeze review of R017 (§4o) before any `V7` migration or
implementation slice was written - found and corrected eleven
consistency/privacy gaps, two of them critical (an inbound foreign key
that would have blocked `ForgetMemory` outright on any Fact that was
ever superseded; pre-existing `CorrectMemory`/`ConfirmMemory` audit
snapshots that could have retained the exact text `ForgetMemory` exists
to erase). R017's original text is unedited - a clearly labelled
addendum was appended to it, and R018 is authoritative wherever it
corrects R017:

- **SELF-entity invariant corrected to two cooperating layers.** R017's
  partial unique index only ever guaranteed *at most one* SELF Entity,
  never existence. R018 freezes a Core-level `ensure_self_entity`
  bootstrap call - invoked eagerly, from the normal trusted bootstrap
  path (lazy-vs-eager, left undecided by R017, is now resolved to
  eager) - guaranteeing *at least one* exists before any Personal
  Knowledge action/read-side becomes reachable. DB + Core together now
  actually guarantee *exactly one*. Canonical initial row frozen:
  `entity_type = "person"`, `canonical_name = "SELF"`, `is_self =
  true`.
- **`AddMemory` can now create a NEW Entity, not only reference an
  existing one.** R017 had no path to record the first fact about a
  brand-new person/place/vehicle/device/concept at all. A new typed
  `EntityInputRef` (`SelfEntity` / `Existing(EntityId)` /
  `New(NewEntityData)`) replaces `AddMemory`'s plain `Option<EntityId>`
  subject and its `FactValue::EntityRef(EntityId)` object - usable on
  *both* the subject and object side, and inside `CorrectMemory`'s
  value too. A new Entity created this way is created **inside the
  same atomic `AddMemory` transaction** as the Fact/Source/Evidence/
  audit - a forced-failure rollback must now also prove the
  newly-created Entity disappears, not just the Fact/Evidence. No
  entity aliasing/merging - a `New()` that collides with an existing
  Entity's `domain_ref` is a structured `DuplicateDomainReference`
  rejection, never a silent merge. Still exactly four Memory actions -
  no new `CreateEntity` action was added.
- **`CorrectMemory`'s payload/prose contradiction resolved.** R017's
  payload had no `epistemic_state` field, but its prose said the
  successor's epistemic state was "the caller's requested starting
  state." R018 freezes: a `CorrectMemory` successor is **always**
  `USER_CONFIRMED` - there is no v0.1 path where a model silently
  replaces current knowledge with a durable `INFERRED` successor.
  **A genuine gap was also found by checking R017's design against the
  actual, unmodified `effective_class = max(type_floor,
  provenance_class)` code** (`actions/mod.rs`): `CorrectMemory`'s
  `IMPORTANT_CHANGE` floor (severity 3) already dominates
  `InferredChange`'s severity (2), so the generic formula alone would
  *not* have forced a pre-write block for a model-proposed correction
  - only `IMPORTANT_CHANGE`'s own default "execute immediately with an
  acknowledgement step," not a real block. R018 freezes a narrow,
  explicitly-documented Personal-Knowledge-specific overlay: any of the
  four Memory actions, when `Provenance::Inferred`, unconditionally
  blocks with zero durable writes before confirmation, regardless of
  the action's own type-floor - **no change to `ConfirmationClass`/
  `effective_class`/any other M2 action anywhere else in the system.**
  (`AddMemory`/`ConfirmMemory`/`ForgetMemory` were checked too and need
  no such override - their own floors already interact correctly with
  the generic formula.)
- **`CorrectMemory`'s nullable `confidence` field is now `FieldPatch<f32>`**,
  not `Option<f32>` - the existing M2 nullable-field convention
  (R006), since `Option<f32>` could never express "replace the Fact and
  clear the old confidence back to `NULL`" once `None` already meant
  "inherit." `memory_type`/`sensitivity` correctly stay plain
  `Option<T>` (non-nullable columns, no `Clear` invented for either).
- **One canonical `is_active_fact` predicate** (`valid_until IS NULL
  AND epistemic_state <> 'STALE'`) now explicitly reused by
  `CorrectMemory`'s predecessor check, `ConfirmMemory`'s target check,
  and the retrieval contract's active-knowledge scope - R017 had
  validated only half the predicate (`valid_until IS NULL` alone) at
  the two action call sites.
- **`ConfirmMemory` on a `DISPUTED` Fact clarified**: confirms only the
  one target assertion; sibling conflicting Facts are untouched and
  retrieval must keep surfacing them - not a full contradiction
  resolution, corrected from R017's overstated "resolution path"
  framing.
- **CRITICAL: `ForgetMemory` now scrubs pre-existing audit snapshots.**
  R017 only guaranteed `ForgetMemory`'s *own new* audit row had
  `previous_snapshot = NULL` - it never accounted for a Fact that had
  previously been corrected or confirmed, whose `CorrectMemory`/
  `ConfirmMemory` audit rows carry a *mandatory, non-NULL* payload-
  bearing snapshot by R017's own rules. Before deleting the target
  Fact, `ForgetMemory` now runs `UPDATE fact_audit_log SET
  previous_snapshot = NULL WHERE fact_id = ... AND previous_snapshot IS
  NOT NULL` - every other column in those older rows (actor,
  provenance, action_type, timestamps) is left untouched; only the
  payload-bearing column is scrubbed.
- **CRITICAL: the inbound `superseded_by_fact_id` foreign key no longer
  blocks `ForgetMemory`.** R017 froze `facts.superseded_by_fact_id ...
  REFERENCES facts(id)` with no `ON DELETE` clause (SQLite default:
  `RESTRICT`) - meaning forgetting any Fact that was ever someone's
  successor would have been rejected by the database outright, a gap
  R017's own algorithm never accounted for. `ForgetMemory` now
  explicitly clears every inbound reference (`superseded_by_fact_id =
  NULL` on the predecessor, `valid_until`/`epistemic_state` left
  unchanged) before deleting the target - deliberately never rewiring
  the predecessor to point at the forgotten Fact's own successor, since
  that would misrepresent history.
- **`ForgetMemory`'s Evidence handling is now privacy-first, not
  conditional.** R017 retained Evidence still linked to another Fact/
  Relation/Episode - but that Evidence could itself contain the exact
  forgotten payload (Evidence is where personal excerpts live). All
  Evidence directly linked to the forgotten Fact is now unconditionally
  deleted, even if still referenced elsewhere - privacy beats evidence
  retention; the other knowledge objects survive, just with less
  supporting evidence.
- **`Source` gains a `sensitivity` field; "never delete Source" is
  removed.** R017 claimed Source retention "carries no privacy risk"
  since it is metadata-only - too strong, since metadata itself
  (a label, a document path, a conversation reference) can be
  sensitive on its own. `Source.sensitivity` (same 4-value closed set)
  is now frozen. `ForgetMemory` now deletes a Source only once it has
  zero remaining Evidence references; genuinely shared Source metadata
  survives.
- **Exact field-length bounds frozen for every bounded Personal
  Knowledge text/JSON field** (a canonical constant table, e.g.
  `VALUE_JSON_MAX_LEN = 2000`, `PREDICATE_MAX_LEN = 100`,
  `EVIDENCE_CONTENT_MAX_LEN = 4000` - 15 constants total), reusing
  M2's existing `TITLE_MAX_LEN`/`DESCRIPTION_MAX_LEN`/
  `DOMAIN_TAG_MAX_LEN` tiers where they fit, plus two new intermediate
  tiers - so the eventual `V7`/Core implementation copies frozen values
  rather than inventing schema policy mid-implementation, which R017
  had left an open gap for (`Fact.value_json`'s bound explicitly
  deferred "to implementation time").
- **`ForgetMemory`'s final atomic transaction algorithm rewritten** into
  one exact 13-step order incorporating every correction above (scrub
  audit snapshots → clear inbound supersession links → delete Evidence
  links/rows → delete orphaned Source rows → delete the Fact → insert
  the minimal audit marker) - supersedes R017's original 9-step
  version in full.
- **Slice A/B acceptance criteria updated** to prove the corrected
  invariants (combined Core+DB SELF-uniqueness proof, rollback proving
  newly-created Entity rows also vanish, all 15 exact length bounds
  proven, etc.) - nothing else about the recommended implementation
  order (Slice A then B, §4o) changed.

Full evidence, exact corrected algorithms, and the complete review
findings:
`docs/reports/R018_M2_PERSONAL_KNOWLEDGE_ARCHITECTURE_CONSISTENCY_HARDENING_2026-09-18.md`.

### 4q. M2 Personal Knowledge Privacy and Foundation Finalization (2026-09-18, R019, documentation/architecture only)

**No Rust or React code changed. No migration created.** The final
pre-`V7` consistency pass - closes a privacy gap R018 (§4p) itself left
open, specifies an orphan-Entity cleanup step `ForgetMemory` was
missing entirely, resolves a genuine ambiguity between R017's and
R018's stated `V7` table scope, freezes the exact `Source`/`Evidence`
creation payload shapes now that both carry `sensitivity`, and - the
most consequential correction - separates "true deletion" into two
explicitly distinct claims: a logical/product privacy guarantee
(required, fully upheld) and a forensic-storage guarantee (honestly
evaluated against this project's actual, inspected storage
configuration, not assumed). R017's and R018's original text is
unedited - a one-line forward pointer was added to R017's existing
addendum, and a full addendum was appended to R018; this report is
authoritative wherever any of the three disagree:

- **`Entity` gains a `sensitivity` field.** R018 had corrected `Source`
  to carry `sensitivity` ("metadata itself can still be
  privacy-sensitive") but froze `Entity` as "the one table without
  sensitivity... structural identity metadata." The same reasoning
  R018 applied to `Source.label`/`reference` applies directly to
  `Entity.canonical_name`/`domain_ref` (a name, an organization, a
  private project pointer can each be sensitive on their own) - R018's
  own stated reasoning contradicted its own stated conclusion for
  `Entity` specifically. Now corrected: same four-value closed set,
  structurally independent of every other table's own `sensitivity`
  (no automatic equality assumed in either direction). Canonical SELF
  bootstrap row now includes `sensitivity = INTERNAL`.
- **CRITICAL: `ForgetMemory` now prunes orphaned, non-SELF Entities.**
  R018's "never delete subject/object Entity rows" was too broad once
  `AddMemory` could create brand-new Entities (§4p) - a forgotten
  Fact's only mention of a newly-created, sensitive Entity could
  otherwise survive forever, directly undermining the privacy guarantee
  `ForgetMemory` exists to provide. A deterministic (no AI judgment)
  cleanup step is now frozen: after the target Fact is deleted, a
  captured non-SELF Entity is deleted only if zero surviving
  `Fact`/`Relation`/`episode_entities` references remain - a
  `domain_ref` alone never forces retention (the authoritative domain
  object lives elsewhere and is unaffected). All Entity FKs stay
  `RESTRICT`, no `CASCADE` - legal by construction because the Fact
  itself is deleted first, and the reference-count check runs
  immediately before the Entity delete.
- **Same-payload duplicate `domain_ref` now fails in Core before any
  SQL.** Two `New()` Entity inputs in one `AddMemory` call (subject and
  object) requesting the identical domain reference previously relied
  on the real `UNIQUE` DB constraint alone (R018 §4). A Core preflight
  check across the whole payload now rejects this deterministically-
  predictable-in-advance conflict with the same typed
  `DuplicateDomainReference` error, zero writes - the DB constraint
  remains as defense-in-depth for the cross-call race case it alone
  can catch.
- **`V7` table scope resolved - all eleven tables, not six.** R017 had
  frozen all six entity schemas plus four join tables as "schema ahead
  of their own action family"; R018's corrected Slice A acceptance then
  listed only six of the eleven, while R018's own `ForgetMemory`
  algorithm already referenced `relation_evidence`/`episode_evidence`
  by name - a real disagreement between the two reports, now resolved:
  `V7` creates `entities`/`sources`/`evidence`/`facts`/`relations`/
  `episodes`/`fact_evidence`/`relation_evidence`/`episode_evidence`/
  `episode_entities`/`fact_audit_log` from day one, so `ForgetMemory`
  has one stable schema immediately and slices F/G need zero further
  schema migration later. **Table existence is explicitly not
  permission to mutate it** - no `Relation`/`Episode` write action
  exists, no capability path can touch those five tables, only
  deliberate raw-SQL fixture-row tests may (proving `CHECK`/FK
  constraints, never a production write path).
- **Final `NewSourceData`/`NewEvidenceData` shapes frozen**, now that
  `Source`/`Evidence` both require `sensitivity` explicitly (no
  inheritance from `Fact`/`Entity`/each other, ever) -
  `SourceRef::Existing` reuses an existing Source completely unchanged,
  never silently mutating its metadata/sensitivity as a side effect.
- **Privacy Firewall sensitivity composition frozen as an architectural
  target**: a future Context Pack item's effective outbound
  classification must be at least as strict as the single most
  restrictive `sensitivity` among every included component (Entity +
  Fact/Relation/Episode + Source + Evidence) - never merely the
  enclosing Fact's own value in isolation. No Privacy Firewall
  implementation this run; the four sensitivity values are unchanged.
- **`ForgetMemory`'s final transaction is now 14 steps** (up from
  R018's 13) - adds the orphan-Entity cleanup as its own explicit step,
  positioned after the target Fact itself is deleted.
- **"True deletion" terminology corrected, the most consequential
  finding.** Direct inspection of `src/services/storage/mod.rs`
  (read-only) confirmed this project currently sets/queries only
  `key`/`foreign_keys`/`rekey`/`integrity_check` - **no `journal_mode`
  and no `secure_delete` PRAGMA anywhere**. R019 freezes two explicitly
  separate guarantees: (A) a **logical/product guarantee** - no live
  row, no query path, no payload-bearing audit snapshot for the
  forgotten Fact anywhere LifeOS's own code can reach - fully upheld
  and the correct thing to tell a user; (B) a **forensic-storage
  guarantee** - honestly evaluated as currently UNVERIFIED: absent an
  explicit `secure_delete` setting, a freed database page's old
  (still-encrypted) bytes can persist until reused, meaning a *future*
  master-key compromise could, in principle, still expose "forgotten"
  ciphertext - SQLCipher encrypts the file itself (proven, R004/R005),
  but this project has not verified or configured the freed-page
  behavior beyond that. No physical-media/SSD-level guarantee is or
  can ever be claimed. A concrete, actionable future hardening step is
  frozen (not implemented): explicitly set-and-verify `PRAGMA
  secure_delete = ON`, matching the exact `enable_foreign_keys`
  precedent (R008/R015) of never trusting an unverified default.
- **No production backup/sync/export subsystem exists** (confirmed by
  inspection - only a `#[cfg(test)]`-only proof of the `rusqlite`
  backup API exists, in the storage spike). A future rule is frozen for
  whenever one is built: it must define how `ForgetMemory` propagates,
  so restoring an ordinary backup cannot silently resurrect forgotten
  Personal Knowledge - not solved now, named so it cannot be
  overlooked later. LifeOS cannot control removal from copies it does
  not itself manage (a user's manual file copy, a third-party cloud
  sync) - never claimed otherwise.
- **Architecture is now explicitly CLOSED for Slice A/B** (R019 §23) -
  every finding raised across R017, R018, and R019 has been identified
  and resolved; a future implementer should not need to invent further
  product/privacy semantics, only genuine implementation detail (exact
  Rust error-enum shapes, the SELF-bootstrap call site, the
  `secure_delete` hardening step).

Full evidence, the complete logical-vs-forensic deletion analysis, and
every corrected algorithm/acceptance criterion:
`docs/reports/R019_M2_PERSONAL_KNOWLEDGE_PRIVACY_AND_FOUNDATION_FINALIZATION_2026-09-18.md`.

### 4r. M2 Personal Knowledge Slice A Foundation (2026-09-18, R020 — first Personal Knowledge CODE, not docs-only)

**The first real Personal Knowledge implementation slice.** `V7`
migration (`src-tauri/migrations/V7__create_personal_knowledge.sql`) -
the full eleven-table foundation R019 §12 froze (`entities`/`sources`/
`evidence`/`facts`/`relations`/`episodes`/`fact_evidence`/
`relation_evidence`/`episode_evidence`/`episode_entities`/
`fact_audit_log`), a direct transcription of R017→R018→R019's frozen
field-level schema, no field redesigned. `Sensitivity`/`MemoryType`/
`EpistemicState` closed enums, six typed UUIDv7 IDs
(`EntityId`/`FactId`/`RelationId`/`EpisodeId`/`SourceId`/`EvidenceId`),
`DomainRef`, and `Entity`/`Source`/`Evidence`/`Fact` (with `FactValue`)
domain structs added to `domain/mod.rs`, all 15 R018-frozen length
constants. `services::ensure_self_entity` (0 rows → create the
canonical SELF row `entity_type="person"`/`canonical_name="SELF"`/
`sensitivity=INTERNAL`; 1 row → reuse unchanged; >1 → typed integrity
error, never silently pick one) wired into `bootstrap_db_at` right
after migrations - runs on every bootstrap, proven idempotent. Narrow
`services::get_entity`/`get_fact` reads. **`secure_delete = ON`**
storage hardening: `enable_secure_delete` (set-and-verify, matching
`enable_foreign_keys`'s exact discipline) called from `open_encrypted`
on every encrypted connection - closes the R019 §17 gap this codebase
had left genuinely unverified. **A factual correction to R019 §17 was
also made**: SQLite's `secure_delete` has no `FULL` value (canonical
values are `OFF`/`ON`/`FAST`) - `ON` is the correct, now-implemented
choice; a labelled addendum was appended to R019, its original text
otherwise unedited.

- **`AddMemory`/`CorrectMemory`/`ConfirmMemory`/`ForgetMemory` do NOT
  exist yet** - Slice B+ remains entirely unstarted. `Relation`/
  `Episode` have zero production write paths - schema only, proven
  unreachable through any public function; only deliberate raw-SQL
  fixture-row tests touch those five tables, never a repo function
  (R019 §13's explicit concern, closed).
- **23 new tests** (285 total, up from 262 - all 262 prior tests remain
  green, unmodified): 5 `secure_delete` proofs (fresh-connection ON,
  ON-after-reopen, wrong-key behavior unchanged, encryption-at-rest
  unchanged, simultaneous with `foreign_keys`), 1 `journal_mode`
  measurement (**MEASURED: `delete`** - SQLite's own historical
  default, observation only, not forced), 5 SELF-bootstrap proofs
  (exactly one on fresh bootstrap, id survives reopen, idempotent
  across 3+ calls, a raw duplicate `is_self=1` insert genuinely
  rejected by the real DB unique index, canonical row's exact shape),
  2 migration tests (fresh V1-V7, and a populated V6 database with
  real Goal/Project/Milestone/Task/GoalRelation rows upgrading to V7
  non-destructively), 7 raw-SQL defense-in-depth functions covering
  20+ individual constraint proofs across all six tables (including a
  positive proof that a `relation_type` far outside `GoalRelation`'s
  11 closed values is genuinely *accepted* - proving Personal Knowledge
  `Relation` stays a truly open vocabulary, never confused with
  `GoalRelation`), 1 table-driven test proving all 15 frozen length
  constants, and 2 `get_entity`/`get_fact` read-round-trip tests
  (`Scalar` and `EntityRef` `FactValue`, nullable confidence, a valid
  `superseded_by_fact_id` link, SELF/non-SELF, with/without
  `domain_ref`). The established illegal-cross-module-access privacy
  probe was re-run against the new `personal_knowledge_repo` submodule
  (`error[E0603]: module 'storage' is private`, confirmed, then
  removed - never left in the tree).
- **Post-run verification hardening (same date, R020 report's own
  labelled addendum, no new numbered report):** a dedicated review of
  R020's own test coverage before Slice B found four verification gaps
  - not evidence of a production defect, confirmed since every new test
  passes against the unchanged `V7` schema/Slice A code - and closed
  them with **8 more tests (293 total, up from 285)**: completed
  join-table FK/duplicate coverage for `relation_evidence`/
  `episode_evidence`/`episode_entities` (previously only
  `fact_evidence` had been proven); `ON DELETE RESTRICT` proven via an
  actual parent `DELETE` attempt for all 14 non-join and join-table
  parent-child relationships (previously only inferred from failed
  child `INSERT`s - directly relevant to a future `ForgetMemory`'s
  explicit deletion order); `fact_audit_log`'s own constraints proven,
  including that `fact_id` genuinely accepts a nonexistent `FactId`
  (confirming it is not an FK, the exact property a future
  `ForgetMemory` depends on); every `Sensitivity`/`MemoryType`/
  `EpistemicState` value proven to round-trip and be DB-accepted (not
  merely rejection of an invalid value); and the encryption-at-rest
  regression strengthened to prove a distinctive Personal Knowledge
  value (not just the SQLite header) is absent from the raw file. No
  `V7` migration change - the `is_self IN (0, 1)` `CHECK` this pass
  also proved was already present in the originally-committed
  migration.
- **No schema outside the frozen 11-table list; V1-V6 untouched.** No
  new dependency. One incidental naming alignment (not a semantic
  change): `fact_audit_log`'s timestamp column is `occurred_at`,
  matching every other M2 audit table's actual established convention
  (R017-R019's prose had said `created_at` in passing; the frozen
  reports never fixed the literal SQL identifier, only that a
  timestamp column exists).

Full evidence, exact schema, and every test result:
`docs/reports/R020_M2_PERSONAL_KNOWLEDGE_SLICE_A_FOUNDATION_2026-09-18.md`.

### 4s. M2 Personal Knowledge Slice B — AddMemory (2026-09-18, R021)

**The first production Personal Knowledge *mutation*.** `services::
add_memory(conn, meta, AddMemoryPayload) -> ActionOutcome<Fact>` creates
a `Fact` (new-or-existing subject `Entity`, optional new-or-existing
object `Entity`, new-or-existing `Source`, always-new `Evidence`, the
`fact_evidence` link, one `fact_audit_log` row) in a single atomic
transaction. `EntityInputRef {SelfEntity, Existing, New}`/
`FactValueInput {ScalarJson, Entity}`/`SourceRef {Existing, New}`/
`NewEntityData`/`NewSourceData`/`NewEvidenceData`/`AddMemoryPayload` -
the exact frozen R018 §4/R019 §4-§7 shapes, no `valid_from`/
`created_at`/`FactId`/etc. in the payload (Core-generated). Confirmation
floor `ExplicitAdd`; unlike `CorrectMemory`'s frozen `ImportantChange`
floor, the *generic* `effective_class` formula alone already blocks
`Provenance::Inferred` before any write - no Personal-Knowledge-specific
override was needed, only a dedicated test proving it. A confirmed
`Inferred` resubmission always persists `epistemic_state =
USER_CONFIRMED`; a direct `Explicit` call persists exactly the caller's
requested `epistemic_state` (the only path to a deliberate
`HYPOTHESIS`+`INFERRED` memory) - `provenance` itself is never silently
rewritten.

- **Authoritative `domain_ref` validation** (the open question R020 §24
  deferred) reuses each domain module's own `*_exists` read -
  `goal_exists`/`project_exists`, plus two new functions this run,
  `milestone_repo::milestone_exists`/`task_repo::task_exists` - zero SQL
  against Goal/Project/Milestone/Task tables inside Personal Knowledge.
  Currently-supported `domain_ref_type` set: **`Goal`/`Project`/
  `Milestone`/`Task`** (does not close the open vocabulary, only limits
  what Core validates today); unsupported type ->
  `UnsupportedDomainReferenceType`; malformed-or-nonexistent id (both
  cases, deliberately not split) -> `ReferencedDomainObjectNotFound`.
- **Duplicate-domain-ref detection, two layers**: a same-payload pure
  preflight (subject `New` vs. object `New` sharing one `domain_ref`)
  plus a new persisted-duplicate repo read,
  `find_entity_by_domain_ref` - checked across *separate* `AddMemory`
  calls too, not just within one payload. The real `UNIQUE` index stays
  defense-in-depth only.
- **Full atomic rollback proven twice** - a forced `fact_audit_log`
  insert failure (last step) and a forced `evidence` insert failure
  (an earlier step) both confirm zero rows across every new table
  (Entities/Source/Fact/Evidence/link/audit), SELF untouched, connection
  still usable afterward - one of this slice's central acceptance
  tests.
- **Existing Entity/Source are never mutated on reuse** - proven by full
  field equality before/after a reuse call. **No implicit
  deduplication/correction/auto-`STALE`/alias-merge** - two identical
  explicit `AddMemory` calls create two Facts; two `New` Entities
  sharing a `canonical_name` (no `domain_ref`) create two Entities -
  both proven directly as explicit negative tests.
- Storage functions promoted from `#[cfg(test)]` to production:
  `insert_fact`/`insert_source`/`insert_evidence`/
  `insert_fact_evidence_link`; new: `find_entity_by_domain_ref`/
  `entity_exists`/`source_exists`/`insert_fact_audit_log`. Ten new
  `ValidationError` variants, including two generic field-parameterized
  ones (`PersonalKnowledgeFieldEmpty`/`PersonalKnowledgeFieldTooLong {
  field, .. }`) covering all eleven bounded free-text fields instead of
  eleven near-identical per-field variants.
- **No `V7` migration change** - Slice A's schema was already
  sufficient; no V8 was created. `Relation`/`Episode` and their four
  join tables remain untouched, proven zero-side-effect after a
  successful `AddMemory`.
- **31 new tests** (324 total, up from 293 - all 293 prior tests remain
  green, unmodified): 12 direct pure-layer tests (`actions::tests`,
  `validate_add_memory`/confirmation-safety-property) + 19 database-
  integration tests (`services::tests`) covering the confirmation gate
  (4 scenarios), a 10-combination success matrix (grouped into 4
  table-driven test functions), both duplicate-domain-ref layers, the
  full domain-ref authoritative matrix (accepted/nonexistent/malformed/
  unsupported for all 4 supported types), Existing-not-found for
  subject/object/source, Existing immutability-on-reuse, both rollback
  proofs, audit-row correctness (including provenance-never-rewritten),
  zero Relation/Episode side effects, and no-dedup/no-alias-merge.
- **`CorrectMemory`/`ConfirmMemory`/`ForgetMemory` still do NOT exist**
  - explicitly not started this run, per instruction.

Full evidence, exact validation ordering, and every test result:
`docs/reports/R021_M2_PERSONAL_KNOWLEDGE_ADDMEMORY_SLICE_B_2026-09-18.md`.

### 4t. M2 Personal Knowledge Slice C — CorrectMemory (2026-09-18, R022)

**The second production Personal Knowledge mutation.** `services::
correct_memory(conn, meta, CorrectMemoryPayload) -> ActionOutcome<Fact>`
- "the retained semantic knowledge is wrong/outdated; replace it while
preserving history", never an in-place semantic edit. One active
predecessor `Fact` becomes historical (`Stale`); one new successor
`Fact` becomes active, in one atomic transaction.
`CorrectMemoryPayload {predecessor_fact_id, value, memory_type,
confidence: FieldPatch<f32>, sensitivity, source, evidence}` - the
exact frozen R018 shape; `subject_entity_id`/`predicate` are always
inherited from the predecessor exactly, never caller-supplied.

- **Type floor `ImportantChange`** - executes immediately for
  `Explicit` provenance (not blocking, matching the existing generic
  policy). **Personal-Knowledge inferred-confirmation overlay** (R018
  §6): unlike `AddMemory`, `effective_class(ImportantChange, Inferred)`
  stays `ImportantChange` (does not fall out for free from the generic
  ladder) - a new narrow helper, `memory_action_requires_confirmation`,
  restores the required block without changing `ConfirmationClass`/
  `effective_class`/`blocks_until_confirmed` or `AddMemory`'s own gate.
  Proven directly with a **nonexistent** `predecessor_fact_id`: an
  inferred, unconfirmed call returns `ConfirmationRequired`, never
  `NotFound` - the predecessor is never even fetched while unconfirmed.
- **Canonical `domain::is_active_fact(fact) = valid_until.is_none() &&
  epistemic_state != Stale`** - checked, not `valid_until` alone; a
  four-case matrix proves a normal active predecessor accepted, an
  already-corrected one rejected, and two deliberate inconsistent-state
  raw fixtures (STALE-with-NULL-valid_until; valid_until-set-but-not-
  STALE) both rejected. Reusable by a future `ConfirmMemory` (Slice D).
- **Subject/predicate always inherited exactly** - the payload has no
  field to change either. `memory_type`/`sensitivity` follow an
  inherit-when-`None`/override-when-`Some` pattern; `confidence` uses
  `FieldPatch<f32>` (`Unchanged`/`Set`/`Clear`, bound-checked on `Set`
  sharing `AddMemory`'s exact confidence-bound logic). The predecessor's
  own `memory_type`/`confidence`/`sensitivity` never change either way.
- **Successor `epistemic_state` is unconditionally `USER_CONFIRMED`** -
  including a confirmed `Inferred` correction, whose audit row still
  keeps `provenance = Inferred` (confirmation and provenance stay
  distinct, matching `AddMemory`'s identical guarantee).
- **A correction chain A->B->C is proven**: each predecessor closes
  with `valid_until` exactly equal to the successor's `valid_from` (no
  gap, no overlap), `superseded_by_fact_id` pointing at the real
  successor; correcting an already-superseded Fact again is rejected.
  A sibling Fact sharing the same subject+predicate is proven
  byte-for-byte untouched - no contradiction detection, no sibling
  search, ever.
- **Mandatory `previous_snapshot`** (`serialize_required_snapshot`,
  never `.ok()`-downgraded) is proven to deserialize exactly back to the
  predecessor's PRE-correction state. **The audit row's `fact_id` is the
  PREDECESSOR's id, not the successor's** - preserving the frozen future
  `ForgetMemory` rule that it can scrub `previous_snapshot WHERE fact_id
  = target` before deleting that Fact.
- **Full atomic rollback proven twice** (a forced final-audit-insert
  failure, and a forced earlier evidence-insert failure) - every new
  row (object Entity/Source/successor Fact/Evidence/link/audit) and the
  predecessor's own update all roll back together; the predecessor
  remains active and byte-for-byte unchanged; the connection stays
  usable afterward.
- Predecessor's original Evidence/`fact_evidence` link/Source remain
  completely untouched; the successor always receives a brand-new
  Evidence row, never shared. Existing Entity/Source reused via
  `Existing(...)` are never mutated. New repo function:
  `update_fact_as_superseded` - deliberately the *only* function that
  can mutate an existing `facts` row, its SQL touching exactly the four
  frozen-allowed fields; no generic Fact-patch function exists.
- **No `V7` migration change** - the schema already supported
  `CorrectMemory` completely; no V8 was created. `Relation`/`Episode`
  and their join tables proven untouched after a successful correction.
- **29 new tests** (356 total, up from 327 - all 327 prior tests remain
  green, unmodified): 8 direct pure-layer tests (`actions::tests`) + 21
  database-integration tests (`services::tests`) covering the
  confirmation matrix (4 scenarios), the active/inactive predecessor
  matrix (4 cases), all five `FactValueInput` success shapes plus five
  rejection paths, inherit-vs-override for all three patchable fields,
  the full successor/predecessor field proof with exact no-gap/no-
  overlap timestamps, the A->B->C correction chain, sibling-Fact
  non-interference, mandatory-snapshot exact-deserialization, audit
  `fact_id`-is-predecessor-id, both rollback proofs, Existing-reuse
  immutability, Evidence/Source non-sharing, read-side persistence via
  `get_fact`, and zero Relation/Episode side effects.
- **`ConfirmMemory`/`ForgetMemory` still do NOT exist** - explicitly not
  started this run, per instruction.

Full evidence, exact execution ordering, and every test result:
`docs/reports/R022_M2_PERSONAL_KNOWLEDGE_CORRECTMEMORY_SLICE_C_2026-09-18.md`.

### 4u. M2 Personal Knowledge Slice D — ConfirmMemory (2026-09-18, R023)

**The third production Personal Knowledge mutation, and the narrowest
one so far.** `services::confirm_memory(conn, meta,
ConfirmMemoryPayload) -> ActionOutcome<Fact>` - "the semantic assertion
already stored in this Fact is correct according to the user," a pure
epistemic confirmation. `ConfirmMemoryPayload { fact_id: FactId }` - a
single field, nothing else; Core decides the one permitted transition
(`epistemic_state -> USER_CONFIRMED`) unconditionally. Mutates the SAME
Fact row in place - no successor, no new Entity/Source/Evidence.

- **Floor `ExplicitAdd`** - the same floor `AddMemory` has, and for the
  same reason needs **no** Personal-Knowledge-specific confirmation
  overlay: `effective_class(ExplicitAdd, Inferred) = InferredChange`
  already blocks generically (unlike `CorrectMemory`'s `ImportantChange`
  floor, which needed `memory_action_requires_confirmation`). Proven
  with a nonexistent `FactId`: an inferred, unconfirmed call returns
  `ConfirmationRequired`, never `NotFound` - the target is never
  fetched while unconfirmed.
- **Valid transitions**: an active Fact's `INFERRED`/`OBSERVED`/
  `DISPUTED` all confirm to `USER_CONFIRMED`. An active Fact already
  `USER_CONFIRMED` is rejected as a no-op
  (`ValidationError::FactAlreadyUserConfirmed`, new) - zero writes, no
  audit row, never silently `Success`. An inactive Fact (`Stale`, or a
  closed active-looking raw fixture) is rejected via
  `domain::is_active_fact`, reused unchanged from `CorrectMemory`
  (`FactNotActive`).
- **`DISPUTED` confirmation touches only the ONE targeted Fact** (R018
  critical clarification) - no sibling Fact sharing the same
  subject+predicate is ever read, searched, changed, or globally
  "resolved." Proven directly with two `DISPUTED` siblings, confirming
  one, proving the other byte-for-byte unchanged.
- **Exactly two fields mutate**: `epistemic_state` and `updated_at`.
  `id`/`subject_entity_id`/`predicate`/`value`/`memory_type`/
  `confidence`/`sensitivity`/`valid_from`/`valid_until`/
  `superseded_by_fact_id`/`created_at` are all proven identical
  before/after via full struct comparison. New narrow repo function
  `confirm_fact_epistemic_state` - the only path that can mutate an
  existing Fact's `epistemic_state` to `USER_CONFIRMED` - with the same
  guarded-`WHERE`-clause + real-runtime-`Result` discipline R022
  established for `update_fact_as_superseded` (active AND not-already-
  confirmed re-checked at mutation time; `changed != 1` is a real error
  in every build profile, never `debug_assert!`).
- **Mandatory `previous_snapshot`** deserializes exactly to the target's
  PRE-confirmation state. Audit `provenance` is never rewritten - a
  confirmed `Inferred` proposal's audit row keeps `provenance =
  Inferred` while the Fact's own `epistemic_state` becomes
  `UserConfirmed`.
- **Forced audit-failure rollback proven** - the Fact retains its
  original `epistemic_state`/`updated_at` completely, no `ConfirmMemory`
  audit row exists, connection remains usable. A genuine mid-transaction
  race proof (R022's style) was explicitly not attempted and
  documented why: `ConfirmMemory`'s transaction has only two steps (the
  guarded update is the FIRST write), so there is no earlier insert to
  hook a same-transaction trigger onto - the direct repo-level guarded-
  update matrix already proves the guard's own behavior exhaustively.
- **No `V7` migration change** - the schema already supported
  `ConfirmMemory` completely; no V8 was created. `Relation`/`Episode`
  and all Entity/Source/Evidence/`fact_evidence` row counts proven
  unchanged after a successful confirmation.
- **14 new tests** (375 total, up from 361 - all 361 prior tests remain
  green, unmodified): 1 direct pure-layer test (`actions::tests`) + 13
  database-integration tests (`services::tests`) covering the
  confirmation matrix (4 scenarios), the full epistemic-transition
  matrix (valid transitions, already-confirmed no-op, inactive
  rejection), identity/no-successor proof, the full in-place mutation
  contract, no Entity/Source/Evidence/Relation/Episode side effects,
  audit-field/snapshot/timestamp exactness, the `DISPUTED`-sibling
  proof, the forced-audit-failure rollback proof, and the direct
  guarded-update repo-level matrix.
- **`ForgetMemory` still does NOT exist** - explicitly not started this
  run, per instruction.

Full evidence, exact execution ordering, and every test result:
`docs/reports/R023_M2_PERSONAL_KNOWLEDGE_CONFIRMMEMORY_SLICE_D_2026-09-18.md`.

### 4v. M2 Personal Knowledge Slice E — ForgetMemory (2026-09-18, R024)

**The fourth production Personal Knowledge mutation, and the first
HARD-DELETE action.** `services::forget_memory(conn, meta,
ForgetMemoryPayload) -> ActionOutcome<FactId>` - "the user wants this
one exact Fact removed from LifeOS's canonical Personal Knowledge,"
implementing R019's final 14-step privacy-safe algorithm exactly.
`ForgetMemoryPayload { fact_id: FactId }` - a single field. **Result is
`ActionOutcome<FactId>`, never `ActionOutcome<Fact>`** - a success
result never hands back the semantic object just forgotten, proven at
the type level.

- **Floor `Delete`** (severity 4) - `effective_class(Delete,
  provenance)` stays `Delete` regardless of provenance, so
  `blocks_until_confirmed` blocks EVERY unconfirmed call, `Explicit`
  included - unlike every other Memory action, a direct explicit
  delete request is not itself sufficient confirmation. **No Undo**:
  `ActionOutcome<T>` has no Undo concept for any action to opt out of;
  the new audit marker's `previous_snapshot` is unconditionally `None`.
- **`domain::is_active_fact` is deliberately never called** - active
  and historical/`Stale` Facts, at any position in a supersession
  chain, are all eligible to be forgotten (R019 - the right to forget
  applies to historical knowledge too).
- **The exact 14 steps**: validate target exists -> capture/dedupe
  candidate subject/object Entity ids -> collect linked Evidence ids ->
  collect their Source ids (deduped) -> scrub every existing
  `fact_audit_log.previous_snapshot` for this Fact -> clear every
  inbound `superseded_by_fact_id` reference WITHOUT rewiring the chain
  -> delete every `fact_evidence`/`relation_evidence`/`episode_evidence`
  row referencing the collected Evidence (even if shared with another
  retained Fact/Relation/Episode - privacy-first, R018/R019) -> delete
  the collected Evidence rows -> delete each collected Source only if
  now orphaned -> a defensive `fact_evidence` sweep -> hard-delete the
  target Fact (runtime-enforced exactly-one, never `debug_assert!`) ->
  delete each candidate Entity only if now orphaned across every
  referencing table (SELF never deleted, `domain_ref` alone never
  retains an orphan) -> insert exactly one minimal, payload-free
  `ForgetMemory` audit marker -> `COMMIT`.
- **Entity retention predicate** (reused for `ForgetMemory` only, no
  new global rule): retained if any surviving row references it via
  `facts.subject_entity_id`/`facts.object_entity_id`/
  `relations.from_entity_id`/`relations.to_entity_id`/
  `episode_entities.entity_id` - otherwise deleted (never SELF).
- **Full atomic rollback proven twice** (a forced final-audit-insert
  failure against a rich state - inbound predecessor, the target's own
  payload-bearing `CorrectMemory` snapshot, Evidence, Source, a
  candidate orphan Entity - and a forced mid-transaction Fact-delete
  failure after earlier scrub/clear steps already ran) - every prior
  scrub/clear/delete in the same call rolls back completely.
- **No `V7` migration change** - the schema already supported
  `ForgetMemory` completely (the deliberately non-FK `fact_audit_log.
  fact_id`, R017 §20, is exactly what makes step 13 possible); no V8
  was created.
- **23 new tests** (398 total, up from 375 - all 375 prior tests remain
  green, unmodified): 1 direct pure-layer test (`actions::tests`) + 22
  database-integration tests (`services::tests`) covering the five-
  scenario confirmation matrix, active/historical target eligibility,
  the audit marker's fields and NULL snapshot, hard-delete proof, the
  full supersession-chain matrix (middle/predecessor/terminal), Evidence
  privacy-first deletion (including raw Relation/Episode/other-Fact
  shared-Evidence fixtures and a zero-Evidence fixture), the Source
  orphan/shared matrix, a ten-case Entity retention matrix, both
  audit-snapshot-scrub proofs (`ConfirmMemory`- and `CorrectMemory`-
  sourced) plus a structural-metadata-survives proof, and both rollback
  proofs.
- **All four frozen Memory actions (`AddMemory`/`CorrectMemory`/
  `ConfirmMemory`/`ForgetMemory`) now exist.** Relation actions, Episode
  actions, and retrieval/Context Pack all remain entirely unstarted,
  explicitly not started this run.

Full evidence, exact execution ordering, and every test result:
`docs/reports/R024_M2_PERSONAL_KNOWLEDGE_FORGETMEMORY_SLICE_E_2026-09-18.md`.

### Next implementation step (M2)

With `UpdateProject` implemented, every frozen, shovel-ready CRUD action
on an existing M2 entity is now built. The Personal Knowledge Layer's
v0.1 architecture was frozen (R017 §4o), hardened through two rounds of
post-freeze review (R018 §4p/R019 §4q, 17 gaps found and fixed, four
critical), and **all four frozen Memory actions are now implemented and
proven**: **Slice A** (2026-09-18, R020 §4r), **Slice B (`AddMemory`)**
(2026-09-18, R021 §4s), **Slice C (`CorrectMemory`)** (2026-09-18, R022
§4t), **Slice D (`ConfirmMemory`)** (2026-09-18, R023 §4u), and **Slice
E (`ForgetMemory`) - the first hard-delete action** (2026-09-18, R024
§4v; 398/398 tests passing). The next *implementation* step is **not
automatically chosen** - R024 §34 recommends, without deciding, among
three candidates: the **Relation action-family freeze/implementation**
(Slice F - `relations` is schema-complete but its action set was never
frozen), the **Episode action-family freeze/implementation** (Slice G -
same reasoning), or **retrieval/`get_relevant_memory`/Context Pack**
(Slice H - depends on real Fact data existing to retrieve, and its own
privacy-firewall shape was only ever sketched, never frozen to
field-level precision). Relation is the most natural next step (reuses
the largest fraction of already-proven machinery), but explicitly
requires its own dedicated freeze pass before any code - the same
discipline R017-R019 applied to Personal Knowledge before any Fact-
action code was written. `RecurringRule` remains a separate, still-
deferred option, still needing its own design/freeze pass before any
code - `Task.recurring_rule_id` stays `NULL`-only until then, §4j.

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
`cd src-tauri && cargo test -- --test-threads=1` runs 398 tests proving:
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
Display/Debug (`req_snap1`, strengthened - a PRE-EXISTING test, not
new), and (as of 2026-09-18, R020, §4r) that the new Personal
Knowledge `V7` foundation is fully sound: `secure_delete`/
`foreign_keys` both provably ON simultaneously on every connection,
`journal_mode` measured (not forced) at `delete`, exactly one SELF
entity created on fresh bootstrap and surviving reopen/repeated
bootstrap unchanged, a raw duplicate SELF insert genuinely rejected, a
populated V6 database upgrading to V7 non-destructively, a full raw-SQL
defense-in-depth matrix across all six Personal Knowledge tables
(closed enums, FKs, the one-semantic-object XOR, self-supersession,
self-edge rejection, and Personal Knowledge `Relation.relation_type`
proven genuinely open by accepting a value far outside `GoalRelation`'s
11 closed values), all 15 frozen length bounds, and `get_entity`/
`get_fact` full round-trips (23 new tests), then (post-run verification
hardening, same date) that all four `V7` join tables reject nonexistent
parents/duplicate links (not only `fact_evidence`), that `ON DELETE
RESTRICT` genuinely rejects a real parent `DELETE` for all 14
non-join/join relationships (not merely inferred from failed child
inserts), that `fact_audit_log` accepts a valid marker with `fact_id`
pointing at a nonexistent Fact (proving it is deliberately not an FK)
while rejecting invalid `action_type`/`actor`/`provenance`, that every
`Sensitivity`/`MemoryType`/`EpistemicState` value round-trips and is
DB-accepted, and that a distinctive Personal Knowledge value (not just
the SQLite header) is absent from the raw encrypted file (8 more
tests), and (as of 2026-09-18, R021, §4s) that `AddMemory` - the first
production Personal Knowledge mutation - correctly gates on the
confirmation floor, resolves Self/Existing/New subjects and objects,
validates domain references authoritatively against `Goal`/`Project`/
`Milestone`/`Task`, rejects duplicate domain references at both the
same-payload and persisted-database layers, never mutates an Existing
Entity/Source on reuse, atomically rolls back every new row (proven via
two independent forced-failure triggers) on any mid-transaction
failure, records a correct audit row with `previous_snapshot` always
`NULL` and `provenance` never silently rewritten, never touches
`relations`/`episodes` or their join tables, and never implicitly
deduplicates Facts or merges Entities by name (31 new tests), then (R021
post-run correctness/verification hardening, same date) that the
confirmation gate now precedes ALL database-dependent resolution (not
merely the write) - an inferred/unconfirmed call gates before even a
nonexistent reference would be looked up - and that all eleven bounded
`AddMemory` fields, the audit `occurred_at` timestamp, and both
duplicate-domain-ref layers now each have exact/full-table proof (3 more
tests), and (as of 2026-09-18, R022, §4t) that `CorrectMemory` - the
second production Personal Knowledge mutation - correctly gates
inferred/unconfirmed corrections before the predecessor is even fetched
(the Personal-Knowledge inferred-confirmation overlay `AddMemory` did
not need), rejects an inactive predecessor via the full `is_active_fact`
predicate, inherits `subject_entity_id`/`predicate` exactly, applies
`memory_type`/`sensitivity`/`confidence` inherit-vs-override correctly,
persists a mandatory `previous_snapshot` that deserializes exactly to
the predecessor's pre-correction state, targets its audit row at the
predecessor's own id, proves a full A->B->C correction chain with exact
no-gap/no-overlap timestamps, and atomically rolls back every new row
plus the predecessor's own update on any mid-transaction failure (29 new
tests), then (R022 post-run temporal-integrity/audit hardening, same
date) that `update_fact_as_superseded`'s affected-row check is now a
real runtime error in every build profile (not a `debug_assert!` that
compiles out of release builds), defended further by an active-Fact
guard inside the `UPDATE` statement itself, proven directly against
active/`STALE`/closed/nonexistent predecessors, with a full-transaction
rollback proof for this exact failure and a strengthened audit-field
proof (actor/`occurred_at` now directly asserted) (5 more tests), and
(as of 2026-09-18, R023, §4u) that `ConfirmMemory` - the third
production Personal Knowledge mutation - correctly gates inferred/
unconfirmed confirmations before the target Fact is even fetched
(needing no Personal-Knowledge-specific overlay, unlike `CorrectMemory`,
since its `ExplicitAdd` floor is already dominated generically),
confirms `INFERRED`/`OBSERVED`/`DISPUTED` Facts to `USER_CONFIRMED` in
place with no successor Fact and no Entity/Source/Evidence side
effects, rejects an already-`USER_CONFIRMED` Fact as a no-op and an
inactive Fact via the reused `is_active_fact` predicate, leaves a
`DISPUTED` sibling completely untouched (no contradiction resolution),
mutates exactly `epistemic_state`/`updated_at` (proven via full
struct comparison), and atomically rolls back on a forced audit
failure (14 new tests), and (as of 2026-09-18, R024, §4v) that
`ForgetMemory` - the fourth production Personal Knowledge mutation and
the first hard-delete action - blocks EVERY unconfirmed call regardless
of provenance, may target an active OR historical Fact at any position
in a supersession chain, executes R019's exact 14-step transaction
(scrubbing every existing audit snapshot for the target, clearing
inbound supersession references without rewiring the chain, always
deleting linked Evidence even when shared with a retained Fact/
Relation/Episode, deleting an orphaned Source/Entity only when truly
orphaned, hard-deleting the target Fact itself), inserts exactly one
payload-free audit marker, and atomically rolls back on both a
final-step and a mid-step forced failure (23 new tests). **Test count
rose from 200 to 233 to 250 to 262 to 285 to 293 to 324 to 327 to 356 to
361 to 375 to 398 across these three days' runs** (375→398 this run,
+23) - all previously-covered behavior remains green.

---

## 9. Next recommended step

See §4l's "Next implementation step" for the exact reasoning (this
duplicates it so it is visible without scrolling past the M2 detail
section). With Goal/Project/GoalRelation/Milestone/Task all
implemented, `UpdateProject` closing out the last frozen, shovel-ready
action, (2026-09-18, R015 §4m then R016 §4n) the master-key lifecycle
and mandatory audit-snapshot safety gaps closed and then verified
deterministically/corrected for a post-file-creation failure edge case,
and (2026-09-18, R017 §4o → R018 §4p → R019 §4q → R020 §4r) the
Personal Knowledge Layer's v0.1 architecture was fully frozen, hardened
through two rounds of post-freeze review (17 gaps found and fixed, four
critical - an inbound FK that would have blocked `ForgetMemory`
outright, pre-existing audit snapshots that could have retained
forgotten content, no orphan-Entity cleanup, and an unverified "true
deletion" claim now correctly split into a logical guarantee and an
honestly-evaluated forensic-storage guarantee), **Slice A (the full
11-table `V7` foundation, `ensure_self_entity` bootstrap, narrow
`get_entity`/`get_fact` reads, `secure_delete = ON` storage hardening)
is implemented and proven (293/293 tests after post-run verification
hardening), (2026-09-18, R021 §4s) Slice B (`AddMemory`) - the first
production Personal Knowledge mutation - is implemented and proven
(327/327 after R021's own post-run correctness/verification hardening),
(2026-09-18, R022 §4t) Slice C (`CorrectMemory`) - the second
production Personal Knowledge mutation - is implemented and proven
(361/361 after R022's own post-run temporal-integrity/audit
hardening), (2026-09-18, R023 §4u) Slice D (`ConfirmMemory`) - the
third production Personal Knowledge mutation - is implemented and
proven (375/375 tests), and (2026-09-18, R024 §4v) Slice E
(`ForgetMemory`) - the fourth production Personal Knowledge mutation
and the first hard-delete action - is also implemented and proven
(398/398 tests)**, all four frozen Memory actions now exist, and
(2026-09-18, R025, then corrected the same day by R025's own post-freeze
schema-scope consistency addendum) the Relation action family's
architecture is now **CLOSED for v0.1 implementation, with zero
schema decisions left open** - `AddRelation`/`CorrectRelation`/
`ConfirmRelation`/`ForgetRelation` names, payloads, confirmation floors,
provenance rules, and privacy/deletion semantics are all fixed, with
`CorrectRelation` deliberately designed as an in-place metadata-only
correction (not a Fact-style successor chain), so no
`superseded_by_relation_id`-equivalent is needed. R025 found current `V7`
insufficient by one narrow, additive gap - **no `relation_audit_log`
table exists**, and every M2 mutating action in this codebase writes to
its own dedicated audit table with no exception - so a small V8
(`relation_audit_log` ONLY) must be created before any Relation action
code is written. `relations.updated_at` is **not part of Relation v0.1**
(the original freeze had left it "recommended, not required," which
contradicted a CLOSED verdict; the addendum resolved this by excluding it
entirely - `CorrectRelation`/`ConfirmRelation` mutation timestamps are
fully carried by `relation_audit_log.occurred_at`). No schema/code/tests
changed in R025 or its addendum - both are documentation only. **(2026-09-18,
R026) V8 (`relation_audit_log` only) is now real, migrated, and tested,
and `AddRelation` - the first production Relation mutation - is implemented
and proven (461/461 tests)**: `Relation`'s domain struct and
`is_active_relation` exist, `get_relation` is the minimum read, and
`AddRelation` reuses the Memory family's `EntityInputRef`/`SourceRef`/
`resolve_entity_input_ref`/`resolve_source_ref`/confirmation-gate-before-DB
ordering verbatim, adding only a two-layer self-edge defense (a pure
structural fast path plus an authoritative post-resolution check) and the
new `ValidationError::SelfPersonalKnowledgeRelation` variant. **(2026-09-19,
R027) `CorrectRelation` - the second production Relation mutation - is
now implemented and proven (492/492 tests)**: an IN-PLACE metadata
correction of exactly `note`/`confidence`/`sensitivity`, never the
frozen `(from_entity_id, relation_type, to_entity_id)` identity triple,
never `epistemic_state` (that stays `ConfirmRelation`'s exclusive job).
No schema change was needed - `relation_audit_log`'s existing R026
schema already covered `CorrectRelation`'s audit row. It reuses
`memory_action_requires_confirmation`/`serialize_required_snapshot`
completely unmodified for its `ImportantChange` floor's inferred-
blocking overlay and mandatory previous-snapshot, matching
`CorrectMemory`'s exact safety boundary (then further hardened the same
day, R027 addendum, to open its own `TransactionBehavior::Immediate`
transaction before the target read/snapshot, closing a read/snapshot/
write race - see §10 below). **(2026-09-19, R028) `ConfirmRelation` -
the third production Relation mutation - is now implemented and proven
(514/514 tests)**: a pure epistemic confirmation in place
(`INFERRED`/`OBSERVED`/`DISPUTED` -> `USER_CONFIRMED`, already-confirmed
rejected as a no-op), mirroring `ConfirmMemory`'s exact shape - floor
`ExplicitAdd`, no PK-specific overlay needed. Built with
`TransactionBehavior::Immediate` from the start this time, applying
R024's/R027's hardening lesson proactively rather than reproducing the
same race a third time. `ForgetRelation` remains unimplemented; the
recommended next step is R029 `ForgetRelation`, the last of the four
frozen v0.1 Relation actions. `RecurringRule`
remains deliberately deferred (§4j), still needing its own design/
freeze pass before any code. M2's storage/security foundation is safe
to proceed into implementation - see R016 §15 and R019 §23.

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
  **Reaffirmed unchanged 2026-09-18 (R017 §8)** during the full
  field-level architecture freeze — not a stale note, still the current
  decision.
- **RESOLVED (2026-09-18, M2 Personal Knowledge Layer Architecture
  Freeze, R017 §2 above): R003's original Personal Knowledge sketch had
  several under-specified or genuinely contradictory points** (a closed
  `Entity.kind` enum instead of an open, extensible `entity_type`; an
  opaque `Fact.content` blob instead of a structured
  subject/predicate/value triple; a polymorphic, FK-unenforceable
  `Relation`; an embedded single-parent `Evidence.fact_id` instead of
  normalized link tables; an ambiguously-closed `Source.kind`). All
  identified explicitly and corrected in R017, not silently carried
  forward into an eventual implementation that would have had to guess.
- **RESOLVED (2026-09-18, M2 Personal Knowledge Architecture
  Consistency Hardening, R018 §4p above): a dedicated post-freeze
  review of R017 found eleven further consistency/privacy gaps before
  any implementation - two critical.** The SELF-entity invariant only
  ever guaranteed at-most-one via a DB index, never existence
  (corrected with a Core-level `ensure_self_entity` bootstrap);
  `AddMemory` had no path to create a new Entity at all (corrected with
  a typed `EntityInputRef`); `ForgetMemory` never scrubbed pre-existing
  `CorrectMemory`/`ConfirmMemory` audit snapshots, which could have
  retained the exact forgotten text (corrected - now scrubbed before
  deletion); the `superseded_by_fact_id` foreign key had no `ON DELETE`
  clause and would have blocked `ForgetMemory` outright on any
  previously-superseded Fact (corrected - inbound references are now
  explicitly cleared first). All identified explicitly and corrected in
  R018, not silently carried forward into an eventual implementation
  that would have had to discover them the hard way.
- **RESOLVED (2026-09-18, M2 Personal Knowledge Privacy and Foundation
  Finalization, R019 §4q above): a final pre-`V7` review found six more
  gaps, closing the Personal Knowledge review cycle - most notably,
  `Entity` metadata needed a `sensitivity` field too (R018 had only
  fixed this for `Source`), `ForgetMemory` needed a deterministic
  orphan-Entity cleanup step it was entirely missing, and "true
  deletion" needed to be explicitly split into a logical guarantee
  (fully upheld) versus a forensic-storage guarantee (honestly
  evaluated - this codebase sets neither `journal_mode` nor
  `secure_delete`, confirmed by direct inspection, so no
  physical-media-level guarantee is or can be claimed).** All
  identified explicitly and corrected in R019; architecture is now
  explicitly declared CLOSED for Slice A/B implementation (R019 §23).
- **RESOLVED in part (2026-09-18, M2 Personal Knowledge Slice A
  Foundation, R020 §4r above): the `V7` schema, `SELF` bootstrap, and
  narrow `get_entity`/`get_fact` reads are now real, implemented, and
  tested (293/293, after post-run verification hardening the same
  date).** `Entity`/`Source`/`Evidence`/`Fact`/`Relation`/
  `Episode` all exist as real tables now (`Relation`/`Episode` schema
  only, no write path).
- **RESOLVED in part (2026-09-18, M2 Personal Knowledge AddMemory Slice
  B, R021 §4s above, then R021's own post-run correctness/verification
  hardening): `AddMemory` - the first production Personal Knowledge
  mutation - is real, implemented, and tested (327/327).**
- **RESOLVED in part (2026-09-18, M2 Personal Knowledge CorrectMemory
  Slice C, R022 §4t above, then R022's own post-run temporal-integrity/
  audit hardening): `CorrectMemory` - the second production Personal
  Knowledge mutation - is real, implemented, and tested (361/361).**
- **RESOLVED (2026-09-18, M2 Personal Knowledge ConfirmMemory Slice D,
  R023 §4u above): `ConfirmMemory` - the third production Personal
  Knowledge mutation - is real, implemented, and tested (375/375).**
- **RESOLVED (2026-09-18, M2 Personal Knowledge ForgetMemory Slice E,
  R024 §4v above): `ForgetMemory` - the fourth production Personal
  Knowledge mutation and the first hard-delete action - is now real,
  implemented, and tested (398/398). All four frozen Memory actions
  now exist.** No Tauri command layer wires any of them into the
  running app yet. Episode actions and retrieval/Context Pack remain
  entirely unstarted.
- **RESOLVED (2026-09-18, M2 Personal Knowledge Relation Action-Family
  Architecture Freeze, R025 above, then corrected the same day by R025's
  own post-freeze schema-scope consistency addendum): the Relation
  action family (`AddRelation`/`CorrectRelation`/`ConfirmRelation`/
  `ForgetRelation`) is now CLOSED for v0.1 implementation, with zero
  schema decisions left open - no code, schema, or tests changed.**
  Current `V7` was found insufficient by exactly one narrow, additive
  gap: no `relation_audit_log` table exists, and this codebase has an
  unbroken six-table precedent (`goal_audit_log`/`project_audit_log`/
  `goal_relation_audit_log`/`milestone_audit_log`/`task_audit_log`/
  `fact_audit_log`) of every M2 write path getting its own dedicated
  audit table. **The exact, final V8 scope is `relation_audit_log`
  ONLY** - the original freeze had left `relations.updated_at` as
  "recommended, not required," which contradicted its own "CLOSED"
  verdict (an implementation-time schema choice cannot coexist with a
  closed freeze); the post-freeze addendum resolved this by excluding
  `relations.updated_at` entirely. It is **not part of Relation v0.1**:
  `CorrectRelation`/`ConfirmRelation` mutation timestamps are fully
  carried by `relation_audit_log.occurred_at`, and `milestones` (V4)
  already proves a mutable M2 table needs no row-level `updated_at` to
  support correct in-place updates. `CorrectRelation` is deliberately
  designed as an in-place metadata-only correction (`note`/`confidence`
  via `FieldPatch`; `sensitivity` via `Option<Sensitivity>`, since
  `relations.sensitivity` is `NOT NULL` and has no legal `Clear` state -
  corrected 2026-09-18 by R025's own post-freeze patch-shape addendum
  after the original freeze incorrectly modeled it as
  `FieldPatch<Sensitivity>`; never `from`/`to`/`relation_type`), so no
  `superseded_by_relation_id`-
  equivalent column is needed either - a genuine divergence from
  `CorrectMemory`'s successor-Fact pattern, justified by Relation's
  identity being the full `(from, relation_type, to)` triple with no
  separate correctable "value" the way Fact has. Recommended next
  implementation slices: R026 (V8 `relation_audit_log` + Relation domain
  struct/read-side + `AddRelation`, combined), R027 `CorrectRelation`,
  R028 `ConfirmRelation`, R029 `ForgetRelation`.
- **RESOLVED (2026-09-18, M2 Personal Knowledge Relation Foundation and
  AddRelation, R026 above): `V8` (`relation_audit_log` only, exactly as
  R025's addenda froze it) is real, migrated, and tested; `Relation`'s
  domain struct, `is_active_relation`, and the minimum `get_relation`
  read all exist; `AddRelation` - the first production Relation mutation
  - is real, implemented, and tested (461/461).** `AddRelation` reuses
  `EntityInputRef`/`SourceRef`/`resolve_entity_input_ref`/
  `resolve_source_ref`/the confirmation-gate-before-DB-dependent-
  validation ordering all verbatim from the Memory family; the only new
  logic is a two-layer self-edge defense (a pure structural fast path in
  `validate_add_relation`, catching `SelfEntity`+`SelfEntity` and
  `Existing(id)`+`Existing(id)`, plus an authoritative post-resolution
  check in `services::add_relation` catching every remaining case
  including `SelfEntity` + `Existing(actual SELF id)`) and one new
  `ValidationError::SelfPersonalKnowledgeRelation` variant.
  `CorrectRelation`/`ConfirmRelation`/`ForgetRelation` remain
  unimplemented; no Tauri command wires `AddRelation` into the running
  app yet; Episode actions and retrieval/Context Pack remain entirely
  unstarted.
- **RESOLVED (2026-09-19, M2 Personal Knowledge CorrectRelation, R027
  above): `CorrectRelation` - the second production Relation mutation -
  is real, implemented, and tested (492/492).** An IN-PLACE metadata
  correction of exactly `note`/`confidence`/`sensitivity` - the frozen
  `(from_entity_id, relation_type, to_entity_id)` identity triple and
  `epistemic_state` are never touched, and there is no successor row (no
  `superseded_by_relation_id` was needed, matching R025's frozen
  resolution). No schema change: `relation_audit_log`'s existing R026
  shape already covered `CorrectRelation`'s audit row via its
  forward-looking `action_type` CHECK. Reuses
  `memory_action_requires_confirmation`/`serialize_required_snapshot`
  completely unmodified for the `ImportantChange` floor's inferred-
  blocking overlay and mandatory previous-snapshot - proven by a direct
  gate-precedes-lookup test using a nonexistent `RelationId`. One new
  narrow guarded-update repo function
  (`update_relation_metadata_if_active`) with real runtime
  affected-row enforcement (never `debug_assert!`), and one new
  `ValidationError::RelationNotActive` variant (never reused from
  `FactNotActive`). `ConfirmRelation`/`ForgetRelation` remain
  unimplemented; no Tauri command wires any Relation action into the
  running app yet; Episode actions and retrieval/Context Pack remain
  entirely unstarted.
- **RESOLVED (2026-09-19, R027 post-run transaction/snapshot-integrity
  hardening addendum): `CorrectRelation` originally read its target and
  serialized `previous_snapshot` before opening a plain `Deferred`
  transaction, leaving a read/snapshot/write race a concurrent writer
  could exploit for a stale snapshot and a lost update.** Fixed with a
  new `begin_correct_relation_transaction` helper
  (`TransactionBehavior::Immediate`, opened right after the confirmation
  gate, before the target is ever read) - target lookup, the
  active-state check, patch resolution, snapshot serialization, the
  guarded update, and the audit insert all now happen inside one stable
  write-locked transaction. Proven with a mandatory two-connection lock
  test using the exact production helper, plus a target-row-specific
  lost-update proof. 494/494 tests (492 + 2). No semantic contract
  changed; V8 unchanged; no V9.
- **RESOLVED (2026-09-19, M2 Personal Knowledge ConfirmRelation, R028
  above): `ConfirmRelation` - the third production Relation mutation -
  is real, implemented, and tested (514/514).** A pure epistemic
  confirmation in place (`INFERRED`/`OBSERVED`/`DISPUTED` ->
  `USER_CONFIRMED`, already-confirmed rejected via a new
  `ValidationError::RelationAlreadyUserConfirmed` variant, never reused
  from `FactAlreadyUserConfirmed`), mirroring `ConfirmMemory`'s exact
  shape - floor `ExplicitAdd`, no PK-specific confirmation overlay
  needed (the generic ladder already blocks `Inferred`). Built with
  `TransactionBehavior::Immediate` from the start via a new
  `begin_confirm_relation_transaction` helper, applying R024's/R027's
  hardening lesson proactively instead of needing a later fix - target
  lookup, the active/already-confirmed state checks, and mandatory
  `previous_snapshot` serialization all happen inside that one
  transaction, proven by a mandatory two-connection lock test plus a
  target-row snapshot-stability proof. State-check ordering matches
  `ConfirmMemory` exactly (active check before already-confirmed check).
  Confirming a `DISPUTED` Relation resolves only that one edge - no
  sibling Relation is ever read, searched, or changed, proven directly.
  No schema change: V8's `action_type` CHECK already enumerated
  `'ConfirmRelation'` (R026, forward-looking). `ForgetRelation` remains
  unimplemented - the last of the four frozen v0.1 Relation actions; no
  Tauri command wires any Relation action into the running app yet;
  Episode actions and retrieval/Context Pack remain entirely unstarted.
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
  resulting state correctly re-resolves as Case B (`Reused`), never
  Case C. This proves the DB/key association is retained and a
  subsequent bootstrap can retry with the same key - it does NOT prove
  the partially-created database file is itself valid or that
  SQLCipher/migrations can actually complete against it, a separate
  recovery question outside this test's synthetic-bytes initializer.
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
| R017 | M2 Personal Knowledge Layer Architecture Freeze | `docs/reports/R017_M2_PERSONAL_KNOWLEDGE_LAYER_ARCHITECTURE_FREEZE_2026-09-18.md` |
| R018 | M2 Personal Knowledge Architecture Consistency Hardening | `docs/reports/R018_M2_PERSONAL_KNOWLEDGE_ARCHITECTURE_CONSISTENCY_HARDENING_2026-09-18.md` |
| R019 | M2 Personal Knowledge Privacy and Foundation Finalization | `docs/reports/R019_M2_PERSONAL_KNOWLEDGE_PRIVACY_AND_FOUNDATION_FINALIZATION_2026-09-18.md` |
| R020 | M2 Personal Knowledge Slice A Foundation | `docs/reports/R020_M2_PERSONAL_KNOWLEDGE_SLICE_A_FOUNDATION_2026-09-18.md` |
| R021 | M2 Personal Knowledge AddMemory Slice B | `docs/reports/R021_M2_PERSONAL_KNOWLEDGE_ADDMEMORY_SLICE_B_2026-09-18.md` |
| R022 | M2 Personal Knowledge CorrectMemory Slice C | `docs/reports/R022_M2_PERSONAL_KNOWLEDGE_CORRECTMEMORY_SLICE_C_2026-09-18.md` |
| R023 | M2 Personal Knowledge ConfirmMemory Slice D | `docs/reports/R023_M2_PERSONAL_KNOWLEDGE_CONFIRMMEMORY_SLICE_D_2026-09-18.md` |
| R024 | M2 Personal Knowledge ForgetMemory Slice E | `docs/reports/R024_M2_PERSONAL_KNOWLEDGE_FORGETMEMORY_SLICE_E_2026-09-18.md` |
| R025 | M2 Personal Knowledge Relation Action-Family Architecture Freeze | `docs/reports/R025_M2_PERSONAL_KNOWLEDGE_RELATION_ACTION_FAMILY_ARCHITECTURE_FREEZE_2026-09-18.md` |
| R026 | M2 Personal Knowledge Relation Foundation and AddRelation | `docs/reports/R026_M2_PERSONAL_KNOWLEDGE_RELATION_FOUNDATION_AND_ADDRELATION_2026-09-18.md` |
| R027 | M2 Personal Knowledge CorrectRelation | `docs/reports/R027_M2_PERSONAL_KNOWLEDGE_CORRECTRELATION_2026-09-19.md` |
| R028 | M2 Personal Knowledge ConfirmRelation | `docs/reports/R028_M2_PERSONAL_KNOWLEDGE_CONFIRMRELATION_2026-09-19.md` |

Next report allocated will be **R029**.
