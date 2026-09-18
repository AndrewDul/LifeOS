# R003 — M2 Architecture Freeze: LifeOS Core + Encrypted Local Storage

Date: 2026-09-17
Scope: architecture design only. No implementation. No database created.
No Qwen/Ollama installed. No GPT/Luna integration. M3 not started. No
fake domain functionality added anywhere.

This report is the permanent reasoning record for M2's architecture,
per the reporting policy now codified in `CLAUDE.md`. The frozen,
durable decisions are also written into `docs/LIFEOS_SPEC.md` §5 as the
canonical reference; this document carries the full reasoning, the
options considered, and the tradeoffs behind each decision.

> **Errata (2026-09-17, same day, corrected during the M2 storage-spike
> run before implementation began):** two issues identified during
> review are fixed in place below rather than left as stale text -
> §G's module-boundary claim did not actually hold under Rust's
> visibility rules as originally written, and §E/§F's `ForgetMemory`
> classification conflated it with ordinary soft-deletes. Both are
> corrected in §E/§F/§G directly; see
> `docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md` for the verified
> proof of the corrected module boundary (a real compiler error was
> captured, not just reasoned about) and for the storage-candidate
> verification (§B below) that this same run's Phase B also required.

---

## 0. Context

M1 (visual shell) is accepted (`docs/PROJECT_STATE.md` §3). M2 is "LifeOS
Core + encrypted local storage" - the layer that will own all persistent
domain state and mediate every mutation, for the UI, for the future cloud
LLM chat integration (M5), and for future NeXa integration. This report
freezes that architecture *before* any of it is built, per the explicit
build-start brief: "the next implementation run must prove the chosen
[storage] candidate with an executable spike" - not assume it works.

Everything below is a design decision, not a claim that code exists.
Where something is genuinely unresolved, it's labeled `UNRESOLVED
QUESTION` rather than silently decided.

---

## A. Core ownership boundaries

**Decision:** LifeOS Core (Rust, running inside the Tauri backend
process) is the only code in the system that ever touches SQL or the
encrypted database file. Three consumer categories - the UI (React
frontend, via Tauri IPC), the cloud LLM (its "tools" are function calls
into the same Rust layer, M5), and future NeXa (via the capability API,
§I) - all mutate state exclusively through a typed action/service layer.
None of them ever receive a database handle, a connection string, or the
ability to execute arbitrary SQL.

**Enforcement mechanism (not just convention):** this is enforced with
Rust's module visibility system, not merely documented as a rule. The
`storage` module's functions are `pub(super)` / `pub(crate)`-restricted
so that only `services` can call them - `capability` (which the Tauri
commands and the future LLM tool-handlers both sit behind) cannot import
`storage` directly; the compiler rejects it. This is a stronger guarantee
than a code-review convention: a bypass attempt fails to build. M2
acceptance criterion §J.9 makes this an explicit, checked requirement.

```
UI (Tauri commands)  ─┐
Cloud LLM (tools)     ─┼──▶ capability/ ──▶ services/ ──▶ storage/ ──▶ SQLite (encrypted)
Future NeXa (API)     ─┘         │              │
                                 └── actions/ (validation, pure, no I/O)
```

`services` is the only module that imports both `actions` (for
validation) and `storage` (for persistence) - it's the orchestration
point: validate → check confirmation class (§F) → persist → write to the
audit trail (§H) → return a typed result.

---

## B. Personal Knowledge Layer

Minimum relational model, frozen for M2 (schema below is table shape,
not final column-level SQL - exact types/indexes are an implementation
detail of the spike/schema work, not this freeze):

| Table | Purpose | Key fields |
|---|---|---|
| **Source** | Where a piece of knowledge originated | `id`, `kind` (`USER_INPUT`/`CHAT_MESSAGE`/`JOURNAL_ENTRY`/`LOCAL_MODEL_EXTRACTION`/`IMPORTED_DOCUMENT`/`SYSTEM_OBSERVATION`), `reference` (free text/pointer), `created_at` |
| **Entity** | A thing LifeOS knows about (the user, a person, a place, a concept, or a pointer to a real domain row like a Goal) | `id`, `kind` (`SELF`/`PERSON`/`PLACE`/`ORGANIZATION`/`CONCEPT`/`DOMAIN_REF`/`OTHER`), `label`, `domain_ref_type`+`domain_ref_id` (nullable, e.g. points at a `Goal.id`), `created_at` |
| **Fact** | An atomic piece of knowledge, optionally about an Entity | `id`, `subject_entity_id` (nullable), `category` (the 11 required values, below), `content` (text + optional structured JSON payload), `epistemic_state` (the 5 required values, below), `confidence` (0.0-1.0), `sensitivity`, `valid_from`, `valid_until` (nullable = current), `superseded_by_fact_id` (nullable), `source_id`, `created_at` |
| **Relation** | Typed edges between Facts/Entities | `id`, `from_type` (`ENTITY`\|`FACT`), `from_id`, `relation_type` (free string, intentionally generic - see below), `to_type`, `to_id`, `created_at` |
| **Episode** | A bounded event/experience the layer can reference as an evidentiary unit | `id`, `kind` (`CONVERSATION`/`JOURNAL_ENTRY`/`CHECKIN`/`CHALLENGE_EVENT`/`OTHER`), `occurred_at`, `summary`, `source_id` |
| **Evidence** | Links a Fact to the Episode/material that supports it, for "why does LifeOS believe this" | `id`, `fact_id`, `episode_id` (nullable), `source_id`, `excerpt` (nullable), `created_at` |

**Epistemic states (frozen exactly as specified):** `USER_CONFIRMED`,
`OBSERVED`, `INFERRED`, `DISPUTED`, `STALE`. Transition model: new facts
from the local helper model (M4) start `INFERRED`; the user moving one to
`USER_CONFIRMED` or `DISPUTED` is itself an action (`ConfirmMemory`,
§E); a fact naturally becomes `STALE` when its `valid_until` passes
without renewal or when it's superseded and nothing re-confirms it.

**Categories (frozen exactly as specified):** `FACT`, `PREFERENCE`,
`EXPERIENCE`, `DECISION`, `OBSERVATION`, `HYPOTHESIS`,
`EFFECTIVE_SOLUTION`, `INEFFECTIVE_SOLUTION`, `SKILL`, `HABIT`,
`CURRENT_STATE`.

**Temporal validity, not silent overwrite:** updating a Fact never
`UPDATE`s the row in place. It inserts a new Fact row, sets the old row's
`valid_until = now()` and `superseded_by_fact_id = new.id`. "Current
belief" queries filter `valid_until IS NULL`; "as of a past date" queries
are simply a different filter over the same table - no separate audit
table needed, history is the data model itself. This directly satisfies
M2 acceptance criterion §J.7.

**`relation_type` is deliberately a free string, not a rigid enum** at
this stage - the exact vocabulary of useful relations between arbitrary
facts/entities isn't knowable yet, and over-specifying it now would
violate "do not model every future feature." `UNRESOLVED QUESTION`:
revisit whether a constrained enum is worth introducing once M6 (Personal
Intelligence) is actually being implemented and real usage patterns
exist.

The "scored memory mirrored to a human-readable form" pattern from
`LIFEOS_SPEC.md` §5.1 (OpenHuman-inspired) is **not implemented in M2** -
this schema supports it later (M6) without a breaking change, since the
mirror would just be a read-side projection over `Fact`+`Evidence`.

---

## C. Domain model (minimum v0.1, frozen)

Field lists are the minimum needed to support the action layer (§E) and
the capability API (§I) - not final production schema, not exhaustive.
`UNRESOLVED QUESTION` markers note fields intentionally left loose.

| Entity | Key fields |
|---|---|
| **Goal** | `id`, `title`, `description`, `status` (`ACTIVE`/`PAUSED`/`COMPLETED`/`ABANDONED`), `target_date` (nullable), `priority_user` (nullable, user-declared), `priority_system` (nullable, system-suggested - never auto-applied, §LIFEOS_SPEC §5.2), `progress`, `domain_tag` (free string - `UNRESOLVED QUESTION`: no rigid taxonomy yet), `created_at`, `updated_at` |
| **GoalRelation** | `id`, `from_goal_id`, `to_goal_id`, `relation_type` (§D enum), `note` (nullable), `created_at` |
| **Project** | `id`, `title`, `description`, `status` (same 4 values as Goal), `priority`, `goal_id` (nullable), `created_at`, `updated_at` |
| **Milestone** | `id`, `project_id`, `title`, `target_date` (nullable), `status` (`PENDING`/`DONE`), `order_index` |
| **Task** | `id`, `title`, `status` (`TODO`/`IN_PROGRESS`/`DONE`/`CANCELLED`), `due_at` (nullable), `project_id` (nullable), `goal_id` (nullable), `recurring_rule_id` (nullable, future - `RESEARCH_DECISIONS.md` §4e's Rust `rrule` crate), `created_at`, `updated_at` |
| **Challenge** | `id`, `title`, `description`, `status` (`OPEN`/`IN_PROGRESS`/`RESOLVED`/`ABANDONED`), `domain_tag`, `related_goal_id`/`related_project_id` (nullable), `created_at` |
| **Plan** | `id`, `challenge_id`/`goal_id` (either, nullable), `title`, `description`, `status` (`DRAFT`/`ACTIVE`/`COMPLETED`/`ABANDONED`), `created_at`, `updated_at` |
| **SolutionAttempt** | `id`, `challenge_id`, `description`, `started_at`, `ended_at` (nullable), `outcome_id` (nullable) |
| **Outcome** | `id`, `solution_attempt_id`, `result` (`EFFECTIVE`/`INEFFECTIVE`/`PARTIAL`/`UNKNOWN`), `notes`, `recorded_at` — an `EFFECTIVE`/`INEFFECTIVE` outcome is what feeds the `EFFECTIVE_SOLUTION`/`INEFFECTIVE_SOLUTION` Fact categories (§B) |
| **CalendarEvent** | `id`, `title`, `starts_at`, `ends_at` (nullable), `all_day`, `location` (nullable), `recurring_rule_id` (nullable), `related_goal_id`/`project_id` (nullable), `created_at` |
| **Reminder** | `id`, `calendar_event_id`/`task_id` (nullable), `fires_at`, `message`, `status` (`PENDING`/`FIRED`/`DISMISSED`/`SNOOZED`), `snoozed_until` (nullable) |
| **Question** | `id`, `text`, `context_ref` (nullable - what it's about), `status` (`PENDING`/`ANSWERED`/`SNOOZED`/`DISMISSED`), `answer_text` (nullable), `created_at`, `answered_at` (nullable) — this is the mechanism for "LifeOS asks the user things" from the canonical lifecycle (`LIFEOS_SPEC.md` §1.2) |
| **ProgressUpdate** | `id`, `subject_type` (`GOAL`/`PROJECT`/`TASK`), `subject_id`, `value`, `note` (nullable), `recorded_at` |
| **WellbeingCheckin** *(added this run, see note)* | `id`, `recorded_at`, `mood` (nullable numeric), `energy` (nullable numeric), `stress` (nullable numeric), `note` (nullable) |

**Note on `WellbeingCheckin`:** it was not in the domain-model freeze
list given in the build brief, but `log_wellbeing()` **is** in the
frozen capability API (§I) - a capability function needs something to
persist. Rather than leave a mismatch (a frozen API call with nothing to
call it against), this run adds the smallest possible table for it. This
is flagged explicitly as a deliberate small addition to close a real gap,
not scope creep - full Wellbeing tracking (trends, correlations) remains
an M8 concern.

**Deliberate omission - no hard `DELETE` actions for most domain
objects.** The action list (§E) has `PauseGoal`/`CompleteGoal` but no
`DeleteGoal`; similarly for Project/Task. v0.1 favors soft-state
transitions over deletion for most domain objects - the two real
deletions in the frozen action set are `ForgetMemory` (an explicit
right-to-be-forgotten action) and `CancelEvent`. This is a genuine design
choice worth stating plainly: it keeps history/relations intact and
matches the local-first "your data, your history" posture, at the cost
of the domain tables slowly accumulating abandoned rows (acceptable at
personal-app scale; archival/cleanup is not designed now).

---

## D. Goal relations (frozen exactly as specified)

`SUPPORTS`, `CONFLICTS_WITH`, `DEPENDS_ON`, `BLOCKS`, `CONTRIBUTES_TO`,
`REQUIRES`, `PARENT_OF`, `CHILD_OF`, `DUPLICATES`, `SUPERSEDES`,
`COMPETES_FOR_RESOURCE`.

**Write convention (a minor freeze-time decision, not in the original
list):** `PARENT_OF` and `CHILD_OF` are logical inverses. The convention
is to write only `PARENT_OF` (from the parent Goal to the child Goal) and
derive "is a child of" by querying the reverse direction - avoids
double-writing every hierarchical relation. `CHILD_OF` stays a valid enum
value (as specified) for the rare case of annotating a one-off asymmetric
relationship directly, but application code should default to
`PARENT_OF`. All other relation types are stored as a single directional
row per stated relationship (no auto-generated inverse rows) - reads that
need "what relates to me" query both `from_id = ?` and `to_id = ?`.

---

## E. Action layer

**Decision:** every mutation in the system is a typed action - a Rust
struct/enum variant with a fixed payload shape, run through a
deterministic validation function in `actions/` before `services/` is
allowed to persist it. No LLM is ever in the validation path (matches
`LIFEOS_SPEC.md` §4.1 - anything not requiring judgment must not go
through an LLM, and validation never requires judgment, it requires
rules).

Frozen minimum action set (exactly as specified in the build brief):

**Goal:** `CreateGoal`, `UpdateGoal`, `PauseGoal`, `CompleteGoal`,
`LinkGoals`, `ChangePriority`
**Project:** `CreateProject`, `AddMilestone`, `AddTask`, `UpdateProgress`
**Calendar:** `CreateEvent`, `MoveEvent`, `CancelEvent`, `AddReminder`
**Challenge:** `CreateChallenge`, `AddHypothesis`, `TrySolution`,
`RecordOutcome`
**Memory:** `AddMemory`, `CorrectMemory`, `ForgetMemory`, `ConfirmMemory`
**Question:** `AnswerQuestion`, `SnoozeQuestion`
**Plan:** `CreatePlan`, `UpdatePlan`, `RebalancePlan`

Every action carries a common envelope: `{ actor: Actor, requested_at,
payload }`, where `Actor` is `UI | CloudLLM | NeXa` (used by the
confirmation-class provenance rule, §F, and written to the audit log).
Every action returns a typed `ActionOutcome<T>` - either the persisted
result, a structured validation error (not a bare string), or a
"confirmation required" response the caller must resolve before
resubmitting.

---

## F. Confirmation policy

**Frozen classes (exactly as specified):** `SAFE_READ`, `EXPLICIT_ADD`,
`INFERRED_CHANGE`, `IMPORTANT_CHANGE`, `DELETE`, `SENSITIVE`.

**Decision on how an action maps to a class:** rather than a fixed
one-to-one table (which breaks down because the *same* action type can
be low-risk or high-risk depending on who's asking and why), the
effective class is computed as:

```
effective_class = max(type_floor(action), provenance_class)
```

- `type_floor` is a minimum severity per action *type*, below.
- `provenance_class` is `EXPLICIT_ADD` when the actor directly and
  unambiguously requested this (a form submit, or a chat message that
  literally states the change), and `INFERRED_CHANGE` when the cloud LLM
  or local model is *proposing* something the user didn't literally ask
  for (matches `LIFEOS_SPEC.md` §5.2: AI priority/goal proposals are
  always discussable, never silently applied).
- `SENSITIVE` is an orthogonal overlay, not a rung on the same ladder: if
  the payload touches data tagged sensitive by the Privacy Firewall
  classification (`LIFEOS_SPEC.md` §4.3), the action is *also* SENSITIVE
  regardless of its computed class - this only ever raises logging/
  visibility requirements, never lowers the confirmation requirement.

**Type floor per action:**

| Class | Actions |
|---|---|
| `SAFE_READ` | every `get_*` capability call (§I) |
| `EXPLICIT_ADD` | `CreateGoal`, `LinkGoals`, `CreateProject`, `AddMilestone`, `AddTask`, `UpdateProgress`, `CreateEvent`, `AddReminder`, `CreateChallenge`, `AddHypothesis`, `TrySolution`, `RecordOutcome`, `AddMemory`, `AnswerQuestion`, `SnoozeQuestion`, `CreatePlan`, `ConfirmMemory` |
| `IMPORTANT_CHANGE` | `UpdateGoal`, `PauseGoal`, `CompleteGoal`, `ChangePriority`, `MoveEvent`, `CorrectMemory`, `UpdatePlan`, `RebalancePlan` |
| `DELETE` | `CancelEvent`, `ForgetMemory` |

(No action type has a `SENSITIVE` floor - sensitivity is content-derived,
not type-derived, per the overlay rule above.)

**Default behavior per effective class:**

- **SAFE_READ** — execute silently, no confirmation, no special log.
- **EXPLICIT_ADD** — execute immediately; show an inline confirmation +
  a short Undo window (target ~10s, exact UX is an M3 concern, not
  frozen here).
- **INFERRED_CHANGE** — always block on explicit user confirmation
  before persisting; never silently applied, regardless of underlying
  type floor.
- **IMPORTANT_CHANGE** — execute immediately but requires an explicit
  acknowledgement step (not necessarily a blocking modal - severity-
  dependent, an M3 UX decision) and is always Undo-able and always
  written to the audit log.
- **DELETE** — always a blocking confirmation dialog. **Corrected
  2026-09-17: this is not one uniform storage behavior.** `CancelEvent`
  performs a soft-delete/tombstone (never a hard SQL `DELETE`), so Undo
  remains technically possible for a grace period even after
  confirmation. `ForgetMemory` is different in kind, not just degree: it
  is the user's explicit right-to-be-forgotten action, so it must
  actually, permanently remove the targeted `Fact` payload and any
  dependent sensitive `Evidence` - a real deletion, not a flag flip. Only
  a minimal non-sensitive audit marker (record id/hash, timestamp, action
  type) may remain; the original forgotten text/value must never persist
  anywhere, including in backups taken afterward or in logs. `Undo` is
  therefore not offered for `ForgetMemory` past the confirmation step
  itself - offering it would mean secretly retaining exactly the data the
  user asked to have removed, which defeats the purpose of the action.
- **SENSITIVE** (overlay) — always logged to the user-visible outbound/
  action audit trail (`LIFEOS_SPEC.md` §4.3) regardless of base class,
  even though sensitive data never leaves the device by default; if
  combined with `INFERRED_CHANGE`, the confirmation copy must name the
  sensitivity explicitly rather than using generic wording.

`UNRESOLVED QUESTION`: exact UI treatment (modal vs. inline banner vs.
toast) per class is deferred to M3 - only the architectural classification
and its persistence/logging consequences are frozen now.

---

## G. Storage abstraction (module boundaries)

**Decision: one crate, clear modules - not crate-per-domain, yet.** The
earlier prior-art research (`RESEARCH_DECISIONS.md` §5, Wealthfolio's
crate-per-domain architecture) recommended splitting into independent
crates as a *future* extensibility pattern. The build brief for this
freeze is explicit: "do not create excessive crates... prefer clear
Rust modules/service boundaries initially." This freeze resolves the
tension in favor of the more conservative option for M2: start as
modules inside the existing `lifeos` crate (`src-tauri/`), and treat
splitting any given module into its own crate as a valid future refactor
once a real plugin boundary needs it (post-M2, likely M5+ if/when
third-party extensibility becomes real) - not a contradiction of the
earlier research, a deliberate sequencing of it.

**Corrected 2026-09-17 — the original enforcement claim did not actually
hold.** This report originally described `storage/` as a *sibling* of
`services/`, made callable "only from services" via `pub(crate)`/
`pub(super)`. That does not work under Rust's real visibility rules:
`pub(crate)` is visible to the *whole crate* (including `capability/`,
`domain/`, anything else) - not just `services`; `pub(super)` on a
sibling module only reaches their shared parent (`src-tauri/src/` itself
via `lib.rs`), which is exactly where `capability/` also lives, so it
would have been reachable too. Neither gives the intended guarantee.
Identified during this run's review before any code was written.

**Corrected structure: `storage` nests inside `services` as a private
submodule**, not a sibling:

```
src-tauri/src/
├── domain/       plain data types (Goal, Project, Fact, ...) - no I/O, no SQL
├── actions/      typed action payloads + deterministic validation (pure, no I/O)
├── services/     orchestration: validate → confirmation-class → persist → audit
│   └── storage/  (private submodule - `mod storage;`, no pub at all, plus
│                  pub(super) on its own functions as defense in depth)
│                  SQLite access, repository-style functions per domain
│                  area, migrations via refinery. Reachable ONLY from
│                  services/mod.rs - not from capability/, domain/, or an
│                  external test crate. This is a bare Rust module-privacy
│                  fact, not a keyword choice someone can loosen by
│                  accident without it being an explicit, visible edit.
├── knowledge/    Personal Knowledge Layer (Entity/Fact/Relation/Episode/
│                 Evidence operations, temporal-history helpers)
├── security/     key management (keyring crate), encryption setup,
│                 redaction/PII helpers (future GLiNER integration point),
│                 outbound-audit-trail logging
├── backup/       encrypted export/import, rekey support
├── migrations/   versioned schema migration files (consumed by services/storage/)
└── capability/   the stable API surface - thin wrappers over services/'s
                  own public functions for Tauri commands today, the
                  future NeXa-facing boundary later; cannot import
                  services::storage even if it tried - the compiler
                  rejects it (verified, see the storage-spike report)
```

---

## H. Encryption/key architecture

> **Outcome (2026-09-17, storage spike, same day):** sqlite3mc turned out
> to have no mature native-Rust integration (evidence in the spike
> report's Phase B) - SQLCipher, always the documented fallback below,
> was substituted and is now **PROVEN** against every item in this
> section's checklist, including the Windows Tauri release-build check.
> See `docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md` for the full
> evidence and `docs/LIFEOS_SPEC.md` §5 for the updated canonical
> decision. The target design below is otherwise unchanged.

**Target design (not yet built):**
- **DB master key:** randomly generated (256-bit), created on first run,
  never derived from a user password by default (matches the local-first,
  low-friction product stance) - stored via the `keyring` crate in the OS
  credential store (Windows Credential Manager/DPAPI now; macOS Keychain/
  Linux Secret Service later, same code - `RESEARCH_DECISIONS.md` §4c).
- **Portable/recovery backup key:** a *separate* key path - a future
  user-set password or recovery phrase that can wrap an encrypted export
  so it's restorable on a different machine without that machine's OS
  keystore. Not built in M2; the backup format (§ below) must simply not
  preclude it later.
- **No plaintext secrets in logs, ever** - the master key, any derived
  key, and raw sensitive field values must never appear in `security`/
  `storage` log output, only opaque identifiers where logging is needed.

**Explicit non-claim, per the build brief:** `sqlite3mc`
(`RESEARCH_DECISIONS.md` §4b) is the leading *research* candidate. It is
**not** declared final by this freeze. It remains unproven until an
executable spike (the next implementation run, not this one) passes
every item below. If it fails - especially the Windows release-build
packaging check, which has not been attempted with any encrypted-SQLite
option yet - the documented fallback is `SQLCipher` (the ADAPT-tier
alternative already researched in the same section).

**What the storage spike must prove, exactly as specified:**
1. Create an encrypted database.
2. Reopen it after a process restart (proves the key round-trips
   correctly through the OS keystore, not just that encryption exists).
3. Opening with the *wrong* key fails cleanly - a typed error, not a
   panic, not a silent garbage read.
4. A schema migration (refinery) succeeds against an already-populated
   encrypted database.
5. Rekey (changing the encryption key on an existing database) works
   without data loss.
6. Backup (encrypted export) works.
7. Restore (import into a fresh instance) works and reproduces identical
   data.
8. A corrupted database file fails safely - detected (e.g. via
   `PRAGMA integrity_check` or equivalent) and reported clearly, not
   silently returning corrupt/partial data.
9. **Windows build/package compatibility** - the crate combination
   actually compiles and links inside a Tauri *release* bundle, not just
   `cargo check`/`cargo build` in dev mode. This is explicitly flagged
   because M1's own build history this milestone already surfaced one
   Windows/MSVC-linker surprise (`docs/PROJECT_STATE.md` git history) -
   the storage spike must not repeat that mistake by assuming dev-mode
   success implies release-mode success.

This is a standalone spike (a small test binary or integration test), not
integrated into the running app - it proves the candidate before the real
schema and action code are written against it.

---

## I. LifeOS capability API

**Frozen minimum function set (exactly as specified):**

```
get_current_situation()   get_active_goals()      get_today_plan()
get_relevant_memory()     create_goal()            update_goal()
create_challenge()        record_solution_attempt() record_outcome()
create_calendar_event()   change_priority()        log_wellbeing()
get_progress()
```

These are plain Rust function signatures living in `services`/
`capability` (§G) - not a network API, not yet. Today they're called two
ways: (a) thin `#[tauri::command]` wrappers for the UI, and (b) will be
the cloud LLM's tool-call targets once M5 wires up chat (the LLM's "tool
schema" is generated from these same function signatures, not a separate
definition - this is the mechanical enforcement of `CLAUDE.md` principle
2, "one domain logic layer, three interfaces"). Future NeXa integration
(§10.1 in `LIFEOS_SPEC.md`) reaches the *same* functions through whatever
transport is eventually built for it.

`get_relevant_memory(context)` is the literal implementation point of the
Privacy Firewall's context-minimization requirement
(`LIFEOS_SPEC.md` §4.3) - it returns the minimum bounded memory context
for a given request, not a raw query interface into the `Fact` table.

**Hard constraint, restated:** NeXa (and the cloud LLM) never receive a
database handle, connection string, or raw-SQL capability - only calls
to these typed functions, regardless of what transport eventually carries
the call. `UNRESOLVED QUESTION`: the exact transport for NeXa (local
socket, embedded HTTP server, IPC) is explicitly not decided in this
freeze - out of scope until NeXa integration is actually scheduled.

---

## J. M2 acceptance criteria

M2 cannot be declared complete until all of the following pass, with
evidence (exact commands/output), not just "should work":

1. **Persistence across restart** - write a row, terminate the process,
   restart, the row is present with correct values.
2. **Encrypted-at-rest verification** - the raw database file's bytes on
   disk do not contain any recognizable plaintext substring from known
   test data (grep the raw file for a planted plaintext value; it must
   not be found).
3. **Wrong-key rejection** - opening the database with an incorrect key
   fails cleanly with a typed error; no partial or garbage data is
   returned.
4. **Migration** - a schema migration from version N to N+1 succeeds
   against an existing populated database without data loss, and is
   idempotent (re-running it is a safe no-op).
5. **Backup/restore** - an encrypted backup can be restored into a fresh
   instance, reproducing identical domain data (verified by row-count
   and a content hash/checksum per table, not just "it opened").
6. **Deterministic action validation** - a test suite proving invalid
   action payloads (e.g. `CreateGoal` with an empty title, `ChangePriority`
   referencing a nonexistent goal id) are rejected by Core validation
   *before* any database write occurs, with a structured (not stringly-
   typed) error.
7. **Temporal fact history** - updating/correcting a Fact preserves the
   prior version with `valid_until` set and a `superseded_by_fact_id`
   link; an "as of" query against a past timestamp returns the
   historically-correct value, not the current one.
8. **Relation integrity** - creating a `GoalRelation` or knowledge
   `Relation` referencing a nonexistent id is rejected; the specific
   behavior when a referenced row is later soft-deleted (cascade vs.
   block) is decided and covered by a test - not left ambiguous.
9. **No direct UI/LLM SQL mutation path** - provable at compile time via
   the module-privacy boundary (§A) - `capability` fails to compile if it
   attempts to import `storage` directly. This is the primary check;
   supplement with a written test/manual audit note each run
   (`CLAUDE.md`, "inspect git diff") as a second line of defense.
10. **Unit/integration test coverage** - every action in §E has at least
    one unit test rejecting invalid input and one integration test
    proving the happy path persists correctly, run against a real (in-
    memory or temp-file) encrypted database, not a mock.

---

## Unresolved items (carried into `docs/PROJECT_STATE.md` §10)

- Whether `sqlite3mc` survives the storage spike - genuinely open,
  requires the spike to run.
- `Relation.relation_type` as a free string vs. a constrained enum -
  deferred until M6 usage patterns exist.
- Exact NeXa transport mechanism - deferred, not scheduled yet.
- Exact confirmation-class UI treatment (modal/inline/toast) - deferred
  to M3.
- Cascade-vs-block behavior for relations pointing at soft-deleted rows -
  must be decided *during* the spike/schema work (§J.8), not left open
  past M2.

---

## What must be proven by the storage spike (summary)

See §H for the full list. In one sentence: a standalone Rust program
(not the app) that creates an encrypted `sqlite3mc` database, restarts,
rejects the wrong key, migrates, rekeys, backs up, restores, detects
corruption, and - specifically - builds and runs inside a real Windows
Tauri release bundle, with every step's result recorded as evidence in
that run's own report.

## Exact next implementation step

Run the **M2 storage spike** (§H) as its own small, isolated piece of
work, before writing the domain schema (§C) or any action code (§E)
against it. Do not proceed to schema/action implementation until the
spike's report shows every checklist item passing (or documents exactly
which candidate replaced `sqlite3mc` and why, with the same checklist
re-run against the replacement).
