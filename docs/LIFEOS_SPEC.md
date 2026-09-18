# LifeOS — Product, UX, and Architecture Specification

Canonical, living specification. This document is authoritative for what
LifeOS is and how it is built. Research backing these decisions (with
sources, alternatives, and REUSE/ADAPT/REJECT verdicts) lives in
`RESEARCH_DECISIONS.md`. Current engineering status lives in
`PROJECT_STATE.md`. Operating rules for working in this repo live in
`../CLAUDE.md`.

Status (2026-09-17): M0/M1 done and accepted. M2's storage foundation is
proven and hardened for production use, including genuine SQLite
foreign-key enforcement. Five real domain entities are implemented:
**Goal** (implemented, then hardened - validated atomicity, typed
errors, tri-state patch semantics; see the M2 Goal hardening report),
**Project** (implemented directly with the hardened pattern -
`CreateProject`/`UpdateProgress`, and now `UpdateProject` too (all
mutable fields except `progress`, floor `IMPORTANT_CHANGE`, closing the
Project action-set gap in code - see the M2 UpdateProject report), its
first direct cross-entity foreign key to `Goal` proven; see the M2
Project vertical-slice and Project/storage integrity hardening
reports), **GoalRelation**
(implemented directly with the twice-hardened pattern - `LinkGoals`
only, the first true relationship-*graph* structure, its frozen
11-value relation-type enum and direction convention proven; see the M2
GoalRelation vertical-slice report), **Milestone** (implemented
directly with the pattern - `AddMilestone`/`UpdateMilestone` both, the
first *required-parent* child entity, its `(project_id, order_index)`
UNIQUE rule resolved and enforced both in Core and the database; see the
M2 Milestone vertical-slice report), and **Task** (implemented directly
with the pattern - `AddTask`/`UpdateTask` both, the first entity with
two independent, both-nullable parent references; `recurring_rule_id`
deliberately deferred behind a database-enforced `NULL`-only `CHECK`
until `RecurringRule` itself is designed; see the M2 Task vertical-slice
report). The rest of the M2 domain model (Calendar, `RecurringRule`,
Personal Knowledge, Goal Intelligence *reasoning* over the graph
GoalRelation now makes available) is still in progress - not
implemented yet; the frozen v0.1 action set on existing entities is now
fully implemented. **(2026-09-18)** A dedicated M2 Storage and Audit
Safety Hardening pass (R015) closed two safety gaps without changing
any product/action semantics: the production master-key lifecycle now
follows a canonical four-state DB/key matrix (an existing database with
a missing credential fails safely instead of silently generating a
replacement key), and every mandatory audit `previous_snapshot` across
all five update actions can no longer silently disappear on a
serialization failure. See `PROJECT_STATE.md` §4m and
`docs/reports/R015_M2_STORAGE_AND_AUDIT_SAFETY_HARDENING_2026-09-18.md`.
**(2026-09-18, R016)** A follow-up review found R015's orphan-credential
cleanup could itself strand a freshly-created database without its key
if initialization failed *after* the database file was already written
under that key - corrected so the credential is deleted only when no
database file exists afterward, and proven deterministically
(independent of OS credential-store availability) via a new
`CredentialStore` test seam. See `PROJECT_STATE.md` §4n and
`docs/reports/R016_M2_KEY_LIFECYCLE_DETERMINISM_AND_FAILURE_ATOMICITY_HARDENING_2026-09-18.md`.
M3 is not started. See `PROJECT_STATE.md` for exactly what exists; most
of this document still describes design targets for later milestones,
not built functionality — §5.1-§5.6 in particular are a frozen
*architecture*, only partially code so far (Goal, Project, GoalRelation,
Milestone, and Task).

---

## 1. Product definition

LifeOS is a local-first personal operating system for managing and
understanding a user's life. Built first for the creator's own Windows
laptop and daily use, but designed as a real product that could later be
used by other people and integrate with the separate NeXa project.

Platform targets: Windows first; Linux and macOS are future targets. The
architecture must not be unnecessarily coupled to Windows.

LifeOS should help the user:

- know what to do now
- understand whether they are progressing in the right direction
- manage goals, projects, money, work/income
- manage an internal calendar, deadlines, reminders, recurring plans
- track wellbeing, mood, energy, stress
- journal
- track problems, record attempted solutions, record which solutions
  worked, remember what helps and what harms the user
- understand recurring patterns
- manage personal development, record decisions, manage ideas
- maintain long-term memory
- receive useful AI recommendations
- perform weekly/monthly reviews and monitor progress over time

The system must remain modular and extensible: new life domains (Health,
Learning, Relationships, Career, Travel, Documents, Vehicles, Home, NeXa
integration) must be addable later without rewriting the core.

### 1.1 Not merely a single-purpose app

LifeOS is not merely a mental-health app, a debt tracker, a TODO app, a
calendar, or a habit tracker. It provides **general support in life** —
it should eventually understand what the user is currently dealing with,
whatever domain that belongs to (projects, learning, skills, remembering
something, changing habits, sleep, body/fitness, diet, money, career,
work, business, software projects, personal organisation, decisions,
recurring blockers, personal development, and future domains not yet
known), and increasingly understand what works for *this* user.

### 1.2 Canonical lifecycle

The general shape LifeOS applies across any domain, when the user brings
it a problem, challenge, or goal:

```
PROBLEM / CHALLENGE / GOAL
        -> UNDERSTAND
        -> RETRIEVE RELEVANT USER CONTEXT
        -> RESEARCH IF REQUIRED
        -> OPTIONS
        -> DISCUSS WITH USER
        -> PLAN
        -> EXECUTE
        -> MONITOR
        -> REVIEW RESULT
        -> LEARN WHAT WORKED
        -> UPDATE PERSONAL KNOWLEDGE
```

This lifecycle is a design lens for every domain (§5.2, §5.3), not a
feature to build in one run — later milestones implement pieces of it.

### 1.3 Bilingual (Polish/English) by design

LifeOS is a bilingual Polish/English personal life operating system. It
must support Polish, English, and mixed Polish/English utterances as
**one system**, not two separate language modes — the system detects the
user's language naturally and responds in the same language unless asked
otherwise. Personal knowledge preserves the user's original wording/
language where useful, while structured semantic information (§5.3) is
stored independently of language. Local translation/normalisation may
happen when required for extraction/classification, but LifeOS must not
unnecessarily translate the user's entire private memory database.

---

## 2. Interaction model

LifeOS supports three interfaces to the **same underlying domain logic**:
UI/forms, text chat, voice conversation. These must not implement separate
business logic — all three call the same domain capability/action layer.

```
form  -> CreateGoal(...)
chat  -> CreateGoal(...)
voice -> CreateGoal(...)
```

Example natural-language → action mappings the chat/voice layer must
resolve to calls on the same action layer the forms use:

| Utterance | Resolved action |
|---|---|
| "I spent £42 on fuel" | `AddTransaction` |
| "On Friday at 2 PM I am going to view a van" | `CreateCalendarEvent` |
| "I feel terrible today, energy 2 out of 10" | `LogWellbeingCheckin` |
| "Van is now my highest priority" | `ChangePriority` |
| "My new goal is to get my first paying DeVDul client by December" | `CreateGoal` |
| "Move tomorrow's task to Friday" | `UpdateTask` |
| "That solution fixed the problem" | `ResolveProblem` + record the successful solution |
| "I want to focus less on NeXa this week" | propose a priority/time-allocation change (confirmed with the user, not silently applied) |

Conversation is a first-class operating interface for LifeOS, not a chatbot
bolted onto the application.

### Guarded-action-layer principle

Adapted from prior-art research (see `RESEARCH_DECISIONS.md` §5): the AI
(cloud LLM) must call through the **exact same typed, permissioned action
layer** the UI uses — never a privileged backdoor into raw data. This is
both an architecture pattern and the mechanical enforcement point for the
Privacy Firewall (§4): if the AI can only invoke named actions/functions
with typed arguments, there is no path for it to receive more than what
those functions return.

### AI-optional resilience

Core capture, search, and CRUD operations (create a task, log an event,
write a journal entry, view the calendar) must work with **zero AI or
network dependency**. AI features (chat reasoning, recommendations, later
voice) are additive and must degrade gracefully when the cloud LLM is
unreachable — the user should never be blocked from operating their own
data because a network call failed.

---

## 3. Cloud LLM (user-facing conversation)

The user-facing conversation (final text/voice output, reasoning,
planning, advice, decision support, structured tool/action generation,
research, later voice) is generated by a **cloud LLM**. LifeOS does not
rebuild a local conversational LLM system the way the sibling NeXa project
does.

- **v0.1 final decision: OpenAI GPT-5.6 Luna.** This is a product decision
  for v0.1, superseding the research-stage default of Anthropic Claude
  Sonnet 5 recorded in `RESEARCH_DECISIONS.md` §1 (kept there as the
  research evidence/reasoning, not deleted). Luna is the budget/fast tier
  of the GPT-5.6 family — deliberately **not** the deep-reasoning tier;
  LifeOS does not use expensive/deep reasoning by default for everyday
  conversation.
- **Purpose:** natural user-facing conversation in Polish and English,
  explaining LifeOS Core's local results, discussing options, helping the
  user reason through plans/problems, and (later) optional web research.
- **Never hard-coded, regardless of the current default.** The provider is
  accessed through a replaceable abstraction. Model ID, system prompt, and
  tool definitions are configuration, not code, so switching provider
  requires no architectural change — only an adapter for that vendor's
  wire format (OpenAI's is the reference shape; Anthropic's Messages API
  is not OpenAI-wire-compatible, so the abstraction must not assume any
  one shape is "the" shape).
- **Multilingual requirement:** the assistant must work well in Polish,
  English, and mixed PL/EN input (§1.3) — detecting and responding in the
  user's language without a mode switch. No provider evaluated has a
  public Polish-specific benchmark; this remains an item to validate with
  a small internal eval once chat is implemented (`RESEARCH_DECISIONS.md`
  §1, uncertainty flags).

### 3.1 Hard constraint: the cloud model does not own state

**GPT-5.6 Luna (or whichever cloud model is configured) must NOT have
direct access to the SQLite database, must NOT be the canonical memory
system, and must NOT directly change goals, money, calendar, memory, or
any other LifeOS state.** LifeOS Core (local, deterministic + the local
helper model, §4) owns state and actions. The cloud model only ever
requests an action through the guarded action layer (§2) and only ever
receives the minimum required context assembled by the future Privacy
Firewall (§4.3) — never raw database access. This is `CLAUDE.md`
principle 8, restated here as the binding product requirement for M5
(Conversation, §12).

---

## 4. Local intelligence and the Privacy Firewall

### 4.1 Division of responsibility

- **Cloud LLM**: user-facing conversation, reasoning, planning, structured
  tool calls, research, later natural voice.
- **Local helper model — v0.1 final decision: Qwen3 4B** (Ollama-hosted in
  development, NOT user-facing). This is a product decision superseding
  the research-stage shortlist in `RESEARCH_DECISIONS.md` §2 (kept there
  as evidence; the benchmark plan in that section is still the right way
  to *validate* Qwen3 4B once M4 is reached — a product decision to build
  against it is not the same as it being benchmark-proven yet). Later
  responsibilities: intent extraction, structured extraction, entity
  extraction, candidate memory extraction, goal detection, classification,
  context selection, local summarisation, privacy preprocessing, PL/EN
  normalisation where necessary.
  - **Runtime abstraction requirement:** development runtime is Ollama,
    but the local-model runtime interface must stay abstract enough that
    production LifeOS can later bundle `llama.cpp` as a sidecar instead —
    end users must not be required to install Ollama manually. Treat
    "local model runtime" as a swappable interface from the start (same
    principle as the cloud-provider abstraction, §3), even though only
    the Ollama backend is implemented now.
- **Local deterministic code** (no LLM involved): financial calculations,
  dates, scheduling, rule evaluation, database operations, permissions,
  recurring-task expansion, progress calculations, notifications, and the
  domain command/action layer itself. Anything that does not require
  judgment must not go through an LLM.

### 4.2 Privacy is architectural

**User data belongs to the user.** Finances, journal, wellbeing, personal
problems, memories, preferences, history, goals, private events, and
personal observations are local-first and remain locally stored unless the
user explicitly chooses otherwise.

**The cloud LLM must never automatically receive the complete private
database.** It receives only the minimum necessary context, computed
locally. Example:

Instead of sending: name, address, employer, exact bank data, full
transaction history, private journal —

LifeOS generates locally and sends only what's needed to solve the
specific request, e.g.: `country = UK`, `available project budget ≈ £X`,
`mandatory expenses ≈ £Y`, `deadline = 6 weeks`.

The cloud model solves the reasoning problem using only the minimized
context. This is not a policy promise, it is a mechanical property of the
action layer (§2, guarded-action-layer principle): the cloud LLM only ever
sees what a called tool/function explicitly returns to it.

### 4.3 Privacy Firewall components (design targets, not yet implemented)

- **Data sensitivity classification** — every stored field/entity tagged
  with a sensitivity level (e.g. public-ok, internal, sensitive,
  never-leaves-device) at the schema level, not bolted on later.
- **Context minimisation** — local summarization/aggregation functions
  that turn raw records into the smallest sufficient statistic for a given
  cloud request (the £X/£Y/deadline example above).
- **PII detection & redaction** — a dedicated lightweight local model
  (not the general local helper LLM) purpose-built for this: research
  flagged GLiNER (zero-shot NER/span-tagging, CPU-friendly, 40+ entity
  types) as a better fit than prompting a general-purpose LLM for this
  narrow, high-stakes job. See `RESEARCH_DECISIONS.md` §2.
- **Anonymisation** — replacing identifying values with placeholders/
  categories before anything leaves the device, where redaction alone
  isn't sufficient.
- **Outbound request audit trail** — every payload sent to a cloud
  provider is logged locally (what was sent, to which provider, for what
  purpose, when) so the user can inspect it after the fact.
- **User approval for especially sensitive data** — a confirmation gate
  before any data tagged "sensitive" or above leaves the device, even if
  minimized.
- **Future UI**: a "Data used locally / Data sent to cloud / Provider /
  Purpose" inspector, so the user can audit what left the device and why,
  at any time — not just trust a settings toggle.

This firewall must not rely purely on provider privacy promises (retention
windows, training opt-outs). Those are a second layer of defense, not the
primary one.

---

## 5. Local database and LifeOS Core architecture

Status: **architecture frozen at M2** (2026-09-17); the storage layer is
**PROVEN** (2026-09-17, storage spike) and now **hardened for production
use**, including genuine SQLite foreign-key enforcement. Three real
domain entities are implemented end-to-end: **Goal** (2026-09-17,
vertical slice + hardening), **Project** (2026-09-17, vertical slice,
built directly with the hardened pattern), and **GoalRelation**
(2026-09-17, vertical slice, the first relationship-graph structure,
built directly with the twice-hardened pattern) — the rest of the
domain model (Task, Calendar, Personal Knowledge, Goal Intelligence
*reasoning*) is not yet started. Full reasoning, tradeoffs, and evidence
behind every decision in this section:
`docs/reports/R003_M2_ARCHITECTURE_FREEZE_2026-09-17.md`,
`docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md`,
`docs/reports/R005_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`,
`docs/reports/R006_M2_GOAL_HARDENING_2026-09-17.md`,
`docs/reports/R007_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`,
`docs/reports/R008_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md`, and
`docs/reports/R009_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md`.
Original research: `RESEARCH_DECISIONS.md` §4.

The canonical user database is local SQLite, with an encryption layer and
OS-native key storage.

- **Encryption: SQLCipher, via `rusqlite`'s native `bundled-sqlcipher`
  feature — PROVEN, not `sqlite3mc`.** The architecture freeze proposed
  `sqlite3mc` as the leading candidate; the storage-spike run verified
  that assumption directly (not trusting prior documentation) and found
  **no mature native-Rust integration exists for it** — `rusqlite`
  (latest, 0.40.2) has no `sqlite3mc` feature at all (only SQLCipher-
  family features), the upstream SQLite3MultipleCiphers project
  documents no Rust bindings, and the only native-Rust crate found
  (`walletkit-sqlite`) was 15 days old with 78 downloads at research
  time — not viable to depend on. SQLCipher (always the documented
  fallback) was substituted and passed the full required checklist:
  encrypted creation, persistence across restart, wrong-key rejection,
  encrypted-at-rest verification, migration, rekey, backup, restore,
  corruption handling, OS key-store round trip, and — the specific gap
  the freeze called out — a real Windows Tauri *release* build
  (`cargo build --release` and `npm run tauri build`, producing actual
  MSI and NSIS installers, not just `cargo check`). Full evidence:
  `docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md`.
- **Known Windows build-time dependency, confirmed *not* a runtime one
  (corrected 2026-09-17, Goal vertical slice report Part A2):**
  `bundled-sqlcipher` links against an *external* OpenSSL rather than
  building one from source (the Perl bundled with Git for Windows on
  this machine cannot build OpenSSL — a real, evidenced blocker, not a
  theoretical one). A prebuilt Windows OpenSSL dev package (`winget
  install ShiningLight.OpenSSL.Dev`) is required **to build** `lifeos`.
  `dumpbin /dependents` on the release binary shows it links OpenSSL
  **statically** — no `libcrypto`/`libssl` DLL appears in its import
  table — so a machine that only *runs* the shipped installer has no
  OpenSSL dependency at all; only a machine that *builds* the crate
  does. The path to that dev package is **no longer committed**: the
  prior machine-specific hardcoded `src-tauri/.cargo/config.toml` was
  deleted and replaced with `scripts/setup-windows-storage-env.ps1`,
  which sets `OPENSSL_DIR`/`OPENSSL_LIB_DIR`/`OPENSSL_INCLUDE_DIR` as
  persistent user environment variables (no admin needed) after
  checking the package is actually installed. Revisit
  `bundled-sqlcipher-vendored-openssl` (avoids the external build-time
  dependency entirely) if a working Perl is ever set up on the build
  machine.
- **DB master key**: randomly generated, created on first run, stored via
  the OS-native secure credential store via the `keyring` Rust crate
  (Windows Credential Manager/DPAPI today; macOS Keychain and Linux
  Secret Service later, same code) — the OS round-trip itself proven in
  the storage spike (generate → store → retrieve → use to open the DB →
  clean up), not just assumed. **Not** Tauri's Stronghold plugin —
  deprecated upstream (`RESEARCH_DECISIONS.md` §4). A *separate* future
  password/recovery-key-wrapped portable backup key is a target design
  for later (not built at M2) — the backup format must not preclude it.
  No plaintext secrets (keys, sensitive field values) may ever appear in
  logs. **Lifecycle hardened 2026-09-18** (M2 Storage and Audit Safety
  Hardening, R015): "created on first run" is now precisely a
  four-state DB/key matrix, not a blind "generate if the credential is
  missing" rule - an existing database with a missing credential fails
  safely with a typed recovery-required error rather than silently
  generating a replacement key. **Corrected 2026-09-18** (R016): the
  orphan-credential cleanup on a failed first run now deletes the
  freshly-generated credential only if no database file exists
  afterward - a database file created before a later initialization
  step failed is never left without its key. See `PROJECT_STATE.md`
  §4m-§4n for detail.
- **Driver/migrations**: `rusqlite` 0.37.0 (pinned — `refinery-core`
  0.9.0's `rusqlite` feature caps at `<=0.37`, confirmed via a real
  dependency-resolution failure against the latest 0.40.2) + `refinery`
  0.9.2 for versioned schema migrations, proven end-to-end in the spike
  (apply, re-run safely as a no-op, data survives). Not the official
  `tauri-plugin-sql` (sqlx-based) — it lacks first-class SQLCipher
  support.
- **OS disk encryption** (BitLocker/FileVault/LUKS) is a complementary
  layer, not a substitute — Windows 11 Home's "Device Encryption" requires
  a Microsoft account and is off by default on local accounts (this dev
  machine's actual state), and disk encryption alone doesn't protect a
  copied-out database file (e.g. a backup on a USB stick).
- **Future semantic/vector search**: `sqlite-vec` — a maintained,
  dependency-free SQLite extension, successor to the now-unmaintained
  `sqlite-vss` — reserved for later long-term-memory retrieval features,
  not needed for the initial structured domains.

### 5.1 Domain model (frozen, M2 — minimum v0.1, not exhaustive)

Field-level detail: M2 report §C. Frozen entities: `Goal`, `GoalRelation`
(§5.2), `Project`, `Milestone`, `Task`, `Challenge`, `Plan`,
`SolutionAttempt`, `Outcome`, `CalendarEvent`, `Reminder`, `Question`,
`ProgressUpdate`, and a minimal `WellbeingCheckin` (added at freeze time
specifically because the capability API's `log_wellbeing()`, §10.1,
needed something real to persist — not originally in this list, flagged
in the M2 report as a deliberate small addition, not scope creep).

**`RecurringRule` — reserved, not yet a designed/frozen entity
(2026-09-17, M2 Task vertical-slice report §3).** `Task.recurring_rule_id`
is a frozen field (nullable, FUTURE) referencing an entity that was
never itself added to this frozen list or designed - `RecurringRule`'s
own shape (rrule/RFC5545 representation, recurrence expansion rules,
relationship to Calendar/Reminder) remains completely undesigned. A
typed `RecurringRuleId` newtype exists in `domain::` purely as a
placeholder so `Task.recurring_rule_id` has something real to be typed
as; nothing constructs a real one. The production schema enforces
`CHECK (recurring_rule_id IS NULL)` until `RecurringRule` gets its own
design/freeze pass and migration - do not treat this placeholder as an
implicit freeze of `RecurringRule` itself.
**Implementation status (2026-09-17): `Goal`, `Project`, `GoalRelation`,
`Milestone`, and `Task` are implemented** (Goal with its own hardened
vertical slice; Project built directly with that hardened pattern -
`CreateProject`/`UpdateProgress`, and now `UpdateProject` too (M2
UpdateProject implementation, 2026-09-17 - closes the Project
action-set gap in code, not just design; required an unplanned but
user-authorized V6 migration to widen `project_audit_log.action_type`'s
CHECK constraint, since it had only ever allowed `'CreateProject'`/
`'UpdateProgress'` - see that report §3a); GoalRelation built directly
with the twice-hardened pattern - `LinkGoals` only, its full frozen
relation-type enum and direction convention; Milestone built directly
with the pattern - `AddMilestone`/`UpdateMilestone` both, its
`(project_id, order_index)` UNIQUE rule resolved and enforced both in
Core and the database; Task built directly with the pattern -
`AddTask`/`UpdateTask` both, the first entity with two independent
nullable parent references, `recurring_rule_id` deliberately deferred);
every other entity in this list is schema-frozen only — see §5.2 for
`GoalRelation`'s detail and correction, §5.1 below for `Milestone`'s/
`Task`'s/`UpdateProject`'s detail, and `PROJECT_STATE.md` §9 for what is
next (`RecurringRule`'s design/freeze pass, or the Personal Knowledge
Layer).

**`Project.progress` — canonically resolved (2026-09-17, M2 Project/
storage integrity hardening report).** The Project vertical slice had
added a `progress` field to `Project` as a flagged, open interpretation
- the architecture-freeze report's one-line Project field table did not
list it explicitly, unlike Goal's own row. This is now frozen, not an
open question:

- `Project.progress` = the **current** normalized Project progress
  snapshot, an integer `0..=100`, the same shared `Progress` type/scale
  `Goal.progress` already uses. It is mutated in place by
  `UpdateProgress`, exactly as implemented.
- The separate, still-unimplemented `ProgressUpdate` entity (§5.1's
  table, above: `subject_type`/`subject_id`/`value`/`note`/
  `recorded_at`) is a **temporal history** of progress observations/
  changes over time — e.g. `2026-09-10 → 40%`, `2026-09-15 → 55%`,
  `2026-09-17 → 65%` — not a replacement for the current-state field
  above, and does not remove the need for `Project.progress` to exist.
  Both can coexist the same way a "current value" column and a
  "changelog" table coexist in any ordinary schema.
- `ProgressUpdate` itself remains unimplemented — this resolves only
  what `Project.progress` means, not when/whether `ProgressUpdate` gets
  built.

**M2 ACTION-SET GAP — RESOLVED (2026-09-17, M2 Project Child Action
Freeze report; IMPLEMENTED 2026-09-17, M2 UpdateProject implementation).**
`Project` stores `title`/`description`/`status`/`priority`/`goal_id` in
addition to `progress`; the frozen Project action set (§5.4, below) now
includes **`UpdateProject`** (floor `IMPORTANT_CHANGE`, `FieldPatch<T>`
patch semantics for the nullable fields — same shape as `UpdateGoal`) to
edit all five, as a single typed update action rather than several
narrow ones (`RenameProject`, `ChangeProjectPriority`, etc. were
considered and rejected — see the freeze report §3 for the reasoning).
**`UpdateProgress` stays completely separate** — it still owns
`progress` alone, floor `EXPLICIT_ADD`, unchanged; `UpdateProject` never
touches `progress`, proven by a dedicated regression test. **This is no
longer documentation-only** — `UpdateProject` is real, tested code (see
`docs/reports/R014_M2_UPDATE_PROJECT_IMPLEMENTATION_2026-09-17.md`), closing the gap
in both design and code.

**Project child action set (Milestone, Task) — both implemented
(2026-09-17, M2 Project Child Action Freeze report for the original
freeze; Milestone side implemented in the M2 Milestone vertical slice,
Task side implemented in the M2 Task vertical slice, same date).**
Before `Milestone`/`Task` implementation began, the same mutation gap
`UpdateProject` just closed was identified and closed for both
children, documentation-only, same reasoning. `AddMilestone`/
`UpdateMilestone`/`AddTask`/`UpdateTask` below are now all real, tested
code, not just a frozen target:

**Patch-semantics correction (2026-09-17, M2 Milestone vertical-slice
report §2).** The two bullets immediately below originally used
`FieldPatch<T>` too broadly - for `Milestone.title`/`order_index` and
`Task.title`, all three **non-nullable** fields, where a `Clear` state
is meaningless (there is no `NULL` to clear one of those fields to).
Corrected to the canonical rule this whole pattern has followed since
`UpdateGoal`: **`FieldPatch<T>` only for a persisted field that is
itself nullable** (so `Unchanged`/`Set`/`Clear` are all meaningful
states); every **non-nullable** field uses a plain `Option<T>`
(`None` = unchanged, `Some(value)` = set) with no `Clear` state to be
ambiguous about in the first place. Apply this same rule to
`UpdateProject`'s own fields, above, where relevant (`title`/`status`
are non-nullable → `Option<T>`; `description`/`priority`/`goal_id` are
nullable → `FieldPatch<T>`, already correctly stated there).

- **`UpdateMilestone`** (floor `EXPLICIT_ADD`) — `title: Option<String>`
  and `order_index: Option<i64>` (both non-nullable fields on
  `Milestone` - `None` leaves unchanged, `Some(value)` sets it, no
  `Clear` state exists for either); `target_date: FieldPatch<DateTime>`
  (the one genuinely nullable field); `status: Option<MilestoneStatus>`
  (non-nullable — `PENDING`/`DONE` are peer states, freely reversible;
  no terminal-state rule, matching the correction already applied to
  `Goal.status`/`LifecycleStatus`, never reintroduce one here either).
  This is the only way a `Milestone` created `PENDING` via
  `AddMilestone` can ever become `DONE` (or move back) — without it,
  `AddMilestone` alone leaves no path to completion.
- **`UpdateTask`** (floor `EXPLICIT_ADD`, **implemented** - M2 Task
  vertical slice) — `title: Option<String>` (non-nullable);
  `due_at`/`project_id`/`goal_id` all `FieldPatch<T>` (all three
  genuinely nullable — clearing `project_id`/`goal_id` is a legitimate
  "unlink" operation, distinct from leaving either unchanged);
  `status: Option<TaskStatus>` (non-nullable). `TODO`/`IN_PROGRESS`/
  `DONE`/`CANCELLED` are peer persisted states with **no** frozen
  transition policy — a task may move directly `TODO -> DONE`, reopen
  from `DONE`/`CANCELLED`, etc.; no restriction was invented (proven,
  Task vertical-slice report §7). `recurring_rule_id` mutation is
  deliberately out of scope - `UpdateTaskPayload` has no such field at
  all; recurrence behavior itself is not designed yet (Task
  vertical-slice report §3).
- Both are `EXPLICIT_ADD`, not `IMPORTANT_CHANGE` — `UpdateProgress`
  already established that an *update* action can sit at `EXPLICIT_ADD`
  when the mutation is routine/low-risk; Milestone/Task status changes
  are the routine, frequent case a user makes constantly, unlike
  Project's own structural identity fields.
- **Task ownership stays exactly as frozen — nullable, unconstrained,
  now proven in code.** Neither `project_id` nor `goal_id` is required;
  a `Task` may be standalone, linked to a `Project` only, a `Goal` only,
  or both - all four modes proven directly against a real database (Task
  vertical-slice report §6). No new requirement was introduced.
- **FK/delete policy, matching `GoalRelation`'s precedent exactly:**
  `Milestone.project_id` (required) and `Task.project_id`/`Task.goal_id`
  (both nullable, when present) all use `ON DELETE RESTRICT` against
  their parent, never cascade — neither `Project` nor `Goal` has a hard
  delete action today, so child work must never silently vanish.
- **Creation defaults**: `AddMilestone` creates `PENDING`; `AddTask`
  creates `TODO`. Neither `Add*` action doubles as an update action.
- **`Milestone.order_index` uniqueness within a Project — RESOLVED
  (2026-09-17, M2 Milestone vertical-slice report §3): YES, unique.**
  For a given `Project`, `(project_id, order_index)` must be unique - a
  `Project` may not have two `Milestone`s occupying the same canonical
  position. **Different Projects may reuse the same `order_index`
  values** (Project A's `0, 1, 2` and Project B's `0, 1, 2` are both
  valid and unrelated). Ambiguity is rejected, not silently tie-broken:
  `AddMilestone`/`UpdateMilestone` requesting an already-occupied
  `(project_id, order_index)` return a structured validation error
  before persistence, and `UNIQUE(project_id, order_index)` enforces it
  at the database level too. No automatic renumbering/rebalancing is
  implemented - a future explicit, transactional reorder mechanism can
  add that later if needed.

Full reasoning for every point above:
`docs/reports/R010_M2_PROJECT_CHILD_ACTION_FREEZE_2026-09-17.md` (the
freeze) and
`docs/reports/R011_M2_MILESTONE_VERTICAL_SLICE_2026-09-17.md` (the
`order_index` uniqueness resolution and the patch-semantics correction
above, plus the full `AddMilestone`/`UpdateMilestone` implementation
evidence).

Deliberately **not** modeled yet (future milestones, not a frozen
schema): `UserProfile`/settings, `Transaction`/`IncomeSource`/`Budget`
(Money, M8), `JournalEntry` (folds into the Personal Knowledge Layer,
§5.3, as `Episode`/`Fact` rather than its own table), `Decision`/`Idea`/
`Review` (likely also Personal Knowledge Layer entries rather than
dedicated tables — revisit if that turns out to be the wrong fit),
`Notification` (M7), `ConversationAction` (M5).

Deliberate omission worth stating plainly: v0.1 favors soft-state
transitions (`PauseGoal`, `CompleteGoal`, etc.) over hard deletion for
most domain objects — the only true deletions in the frozen action set
(§5.4) are `ForgetMemory` and `CancelEvent`.

Two prior-art patterns from `RESEARCH_DECISIONS.md` §5 inform this
freeze without being contradicted by it:
- **Rust crate separation by domain** (Wealthfolio-inspired) — refined at
  M2 into a one-crate, clear-modules structure (§5.6) rather than
  multiple crates immediately; splitting any module into its own crate
  remains a valid future step once a real plugin boundary exists.
- **Scored memory mirrored to a human-readable form** (OpenHuman-
  inspired) — the §5.3 schema (`Fact` + `Evidence`) is built to support
  this later (M6) without a breaking change; the actual markdown-mirror
  mechanism is not built at M2.

### 5.2 Goal Intelligence

Goals must not be isolated TODO records. LifeOS will eventually understand
relationships between goals such as: `SUPPORTS`, `CONFLICTS_WITH`,
`DEPENDS_ON`, `BLOCKS`, `CONTRIBUTES_TO`, `REQUIRES`, `PARENT_OF`,
`CHILD_OF`, `DUPLICATES`, `SUPERSEDES`, `COMPETES_FOR_RESOURCE`.

LifeOS may propose goal priorities based on: user-declared importance,
deadlines, cost of delay, dependencies, goals unlocked, resource
conflicts, time, money, current capacity, wellbeing, current situation,
risk, and user motivation.

**LifeOS does not make the final decision.** Priority recommendations are
discussable with the user; the user has final authority. This is a hard
product constraint, not a UI nicety — any future automatic priority
change must be proposed and confirmed, never silently applied (consistent
with the "I want to focus less on NeXa this week" example in §2, and with
the `INFERRED_CHANGE` confirmation class, §5.5).

**`GoalRelation.relation_type` is a frozen, CLOSED 11-value enum** -
exactly the eleven values listed above, no more, no fewer
(`GoalRelationType` in `domain/mod.rs`). This is a completely different
field from the Personal Knowledge Layer's `Relation.relation_type`
(§5.3, below), which is an intentionally *free string*, deferred until
M6 usage patterns exist - the two share a name only by coincidence,
different entities, different design decision, do not conflate them (a
factual mix-up made this mistake once already - see the correction note
at the end of this section).

**Schema frozen AND implemented (2026-09-17, M2 GoalRelation vertical
slice).** `GoalRelation` (`id`, `from_goal_id`, `to_goal_id`,
`relation_type`, `note`, `created_at`) and its one frozen action,
`LinkGoals`, are implemented end to end - production migration V3, both
FKs into `goals(id)` with `ON DELETE RESTRICT` (Goal has no hard
`DeleteGoal` action; a relation must never silently disappear because an
endpoint Goal was removed), a `CHECK` rejecting self-links, and a
`UNIQUE(from_goal_id, to_goal_id, relation_type)` rejecting exact
duplicates - all proven at the database level, not just documented. Rows
are directional and never auto-inverted: `LinkGoals` writes exactly the
one relation requested; `PARENT_OF(parent -> child)` is the canonical
hierarchy convention, with "child of" derived by querying the reverse
direction rather than writing a second `CHILD_OF` row (that value
remains valid and storable for a caller who deliberately requests it
directly - the freeze keeps it, `LinkGoals` simply never auto-generates
it). Full evidence:
`docs/reports/R009_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md`.

The *intelligence* — proposing priorities, surfacing conflicts,
reasoning about dependencies, graph traversal, cycle detection — is
still implemented starting at M6 (Personal Intelligence, §12); M2 only
makes the correct typed graph state available for that reasoning to
consume later, per this run's own explicit scope limit.

**Correction (2026-09-17, M2 GoalRelation vertical-slice report §2):**
the M2 Project/storage integrity hardening report previously claimed,
incorrectly, that `GoalRelation.relation_type`'s taxonomy was an
architecture-freeze `UNRESOLVED QUESTION`. That was a factual mistake,
conflating three distinct things: this frozen 11-value enum (not open at
all); the Personal Knowledge Layer's separate, genuinely-free-string
`Relation.relation_type` (§5.3); and `Goal.domain_tag`, which is the
field the architecture-freeze report's table actually flags as
unresolved ("free string - `UNRESOLVED QUESTION`: no rigid taxonomy
yet"). Corrected in that report and here.

### 5.3 Personal Knowledge / Memory

**Schema frozen at M2** (M2 report §B): `Entity`, `Fact`, `Relation`,
`Episode`, `Evidence`, `Source` — do **not** flatten LifeOS memory into
one text-memory table. **This `Relation`'s `relation_type` is an
intentionally free string, not a constrained enum** — deferred until M6
usage patterns exist (§13's open questions). It is unrelated to, and
must not be confused with, `GoalRelation.relation_type` (§5.2), a
completely different, frozen, closed 11-value enum on a completely
different entity — the two only share a field name. Facts distinguish
at least these categories:
`FACT`, `PREFERENCE`, `EXPERIENCE`, `DECISION`, `OBSERVATION`,
`HYPOTHESIS`, `EFFECTIVE_SOLUTION`, `INEFFECTIVE_SOLUTION`, `SKILL`,
`HABIT`, `CURRENT_STATE`, and at least these epistemic states:
`USER_CONFIRMED`, `OBSERVED`, `INFERRED`, `DISPUTED`, `STALE`.

Provenance is structural, not conventional: every `Fact` carries
`source_id`, `created_at`, `valid_from`, `valid_until`, `confidence`,
`sensitivity`, and an `epistemic_state`. **Facts are never overwritten in
place** — updates insert a new row and set the old row's `valid_until` +
`superseded_by_fact_id`, so full temporal history is the data model
itself, not a bolted-on audit log.

This directly extends the "scored memory mirrored to a human-readable
form" pattern noted in §5.1 — the mirror should be legible enough that a
user can see *why* LifeOS believes something about them, not just *what*
— though the mirror mechanism itself is not built at M2.

The *intelligence* — producing memory candidates from conversation/
journal input via the local helper model (§4), and the UI for reviewing/
confirming them — is implemented starting at M6 (Personal Intelligence,
§12); M2 only freezes the schema those candidates will be written into.

### 5.4 Action layer (frozen, M2)

Every mutation to LifeOS Core state is a typed action, validated
deterministically before persistence — never by an LLM (§4.1: anything
not requiring judgment must not go through an LLM, and validation never
requires judgment). Frozen minimum v0.1 action set (M2 report §E):

- **Goal**: `CreateGoal`, `UpdateGoal`, `PauseGoal`, `CompleteGoal`,
  `LinkGoals`, `ChangePriority`
- **Project** (and its children `Milestone`/`Task`): `CreateProject`,
  `UpdateProject`, `UpdateProgress`, `AddMilestone`, `UpdateMilestone`,
  `AddTask`, `UpdateTask` — **every action in this set is now
  implemented** (2026-09-17): `AddMilestone`/`UpdateMilestone` (M2
  Milestone vertical slice), `AddTask`/`UpdateTask` (M2 Task vertical
  slice), and `UpdateProject` (M2 UpdateProject implementation, closing
  the action-set gap the M2 Project Child Action Freeze report had only
  frozen in documentation)
- **Calendar**: `CreateEvent`, `MoveEvent`, `CancelEvent`, `AddReminder`
- **Challenge**: `CreateChallenge`, `AddHypothesis`, `TrySolution`,
  `RecordOutcome`
- **Memory**: `AddMemory`, `CorrectMemory`, `ForgetMemory`,
  `ConfirmMemory`
- **Question**: `AnswerQuestion`, `SnoozeQuestion`
- **Plan**: `CreatePlan`, `UpdatePlan`, `RebalancePlan`

Every action carries `{actor: UI|CloudLLM|NeXa, requested_at, payload}`
and returns a typed result or a structured validation error — never a
bare string. This is the literal implementation of the guarded-action-
layer principle (§2): forms, chat, and (later) voice/NeXa all resolve to
calls on this exact same set of functions.

**`ForgetMemory` is not a soft-delete (corrected 2026-09-17, M2 storage-
spike report §Errata).** Unlike `CancelEvent` and ordinary domain
removal, which tombstone a row and keep it (§5.5), `ForgetMemory` is the
user's explicit "delete this personal information" instruction and must
actually remove it: the targeted `Fact` payload (and any dependent
sensitive `Evidence`) is permanently deleted, not flagged deleted. Only a
minimal, non-sensitive audit marker may remain — a record identifier/
hash, the deletion timestamp, and the action type. The original
forgotten text/value must never persist anywhere afterward, including in
the audit trail, backups taken after the deletion, or logs.

### 5.5 Confirmation policy (frozen, M2)

Six classes (M2 report §F): `SAFE_READ`, `EXPLICIT_ADD`,
`INFERRED_CHANGE`, `IMPORTANT_CHANGE`, `DELETE`, `SENSITIVE`. An action's
effective class is `max(type_floor(action), provenance_class)`, where
`provenance_class` is `EXPLICIT_ADD` when the user directly and
unambiguously requested the change and `INFERRED_CHANGE` when the AI is
*proposing* something the user didn't literally ask for; `SENSITIVE` is
an orthogonal overlay (Privacy-Firewall-tagged content, §4.3) that only
ever raises logging/visibility requirements, never lowers confirmation.

Default behavior: `SAFE_READ` executes silently; `EXPLICIT_ADD` executes
immediately with inline confirmation + Undo; `INFERRED_CHANGE` always
blocks on explicit user confirmation before persisting; `IMPORTANT_CHANGE`
executes with an acknowledgement step and is always Undo-able and
audit-logged; `DELETE` always requires a blocking confirmation. Exact UI
treatment per class (modal vs. inline vs. toast) is an M3 decision, not
frozen here.

**`DELETE` is not one uniform storage behavior (corrected 2026-09-17).**
`CancelEvent` and ordinary domain removal soft-delete/tombstone the row —
the data stays, only its status changes, so Undo remains trivial.
`ForgetMemory` is different and more severe: because it is the user
exercising the right to have LifeOS actually forget something, it
performs a real, permanent deletion of the personal payload (§5.4) —
Undo is not offered for `ForgetMemory` past the blocking confirmation
step itself, since "Undo" would mean retaining exactly the data the user
asked to have removed.

### 5.6 Module boundaries (Core architecture, M2)

One Rust crate (the existing `lifeos` in `src-tauri/`), organized into
clear modules rather than split into multiple crates yet (M2 report §G —
a deliberate sequencing of the crate-separation idea in §5.1, not a
reversal of it): `domain/`, `actions/`, `services/`, `knowledge/`,
`security/`, `backup/`, `migrations/`, `capability/`.

**Enforcement mechanism, corrected 2026-09-17 (M2 storage-spike report
§Errata).** The original freeze described `storage` as a *sibling* of
`services`, "restricted" to it via `pub(crate)`/`pub(super)` — that does
not actually work: `pub(crate)` is visible to the *entire* crate
(including `capability/`, `domain/`, anything else), and `pub(super)` on
a sibling only reaches their shared parent, not `services` specifically.
Neither gives the intended guarantee. The corrected, verified structure
nests `storage` **inside** `services` as a private submodule:

```
services/
    mod.rs        <- declares `mod storage;` (private - no pub at all)
                     exposes services' own pub(crate) functions, which
                     internally call into storage
    storage/
        mod.rs    <- functions are pub(super): reachable only from
                     services/mod.rs, not even from other submodules
                     of services beyond the immediate parent
        ...
```

Because `storage` is declared as a plain private `mod storage;` inside
`services/mod.rs`, **nothing outside the `services` module tree can even
name `services::storage`** — not `capability/`, not `domain/`, not an
external crate, not an integration test. This is enforced by the Rust
compiler itself (a bare `mod` declaration defaults to private), not by a
visibility keyword someone could loosen without noticing; `pub(super)` on
the individual storage functions is a second, defense-in-depth layer in
case a future edit ever changes the module declaration itself.
**Verified for real** during the M2 storage spike by deliberately adding
an illegal `services::storage::...` call from outside `services` and
capturing the exact compiler rejection — see
`docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md` for the literal
error output, not just this description.

**Re-verified after a deeper nesting (Goal vertical slice, 2026-09-17).**
`storage` now has real internal structure — `storage::goal_repo` (Goal's
SQL access) and, test-only, `storage::spike` (the original spike suite,
relocated so it can never reach a release build; its migrations create
`spike_records`, which must never appear in a production database — a
dedicated test proves this, not just directory placement). A per-entity
repo module like `goal_repo` is a *grandchild* of `services`, not an
immediate child, so `pub(super)` on its functions is not enough to reach
`services/mod.rs` two levels up; it uses `pub(in crate::services)`
instead, which names the exact ancestor module explicitly and still
denies every sibling of `services` outside that path. The "add an
illegal access, capture the compiler error, remove it" proof was
re-run against this deeper shape (not assumed to still hold after the
refactor) and produced the same `error[E0603]: module 'storage' is
private` result — see
`docs/reports/R005_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`.

`capability/` is the thin, stable surface that Tauri commands, the cloud
LLM's tool-handlers, and (later) NeXa all sit behind — it calls only
`services`' public functions, never `storage` — see §10.1.

**Implementation status, made explicit (corrected 2026-09-17, M2 Goal
hardening report §9C).** Two different things were previously blurred
together under "the capability boundary":

- The capability API **contract** — the frozen set of operation names/
  signatures (`create_goal`, `update_goal`, `get_active_goals`, ...,
  §10.1) — is frozen, as design, at M2.
- A dedicated **`capability/` module that is actual code** sitting
  behind that contract is **not implemented yet**. Today,
  `services::create_goal`/`update_goal`/`get_active_goals`/`get_goal`
  are called directly, but only by `services::tests` - there is no
  other caller (no Tauri command, no cloud-tool handler, no NeXa
  transport) to front yet, so adding an empty pass-through `capability/`
  module this early would itself be premature scaffolding.

The constraint this leaves standing, unweakened: once a real Tauri
command, cloud-tool handler, or NeXa transport is wired up, it **must**
go through a real `capability/` module and must **never** call
`services` (or `services::storage`) directly. `services::tests` calling
`services` directly is not an exception to that rule being violated
early - it is a test working around the architecture to exercise it, per
this repo's testing policy, not a caller the rule is meant to constrain.

---

## 6. Calendar

LifeOS has its own internal calendar as the canonical calendar — Google
Calendar/Outlook integration is not required initially. Locally stored:
events, deadlines, recurring events, reminders, planned work blocks,
project milestones.

Recurrence math (RFC5545) is not hand-rolled: `rrule.js` in the React
frontend (calendar rendering/editing) and the Rust `rrule` crate in the
backend (computing next-fire times for reminders/notifications that must
work even when the UI isn't open). Both are spec-driven implementations of
the same RFC, kept independent rather than sharing a native binding, per
`RESEARCH_DECISIONS.md` §5.

External behaviour initially consists primarily of native OS
notifications, not third-party calendar sync.

---

## 7. Notifications

LifeOS sends native OS notifications for: upcoming events, deadline risk,
payment reminders, weekly review, missed priority, planned task reminders,
important goal warnings. Notifications must continue to fire when the main
window is not foregrounded (tray/background execution), and must avoid
spam (batching/priority rules, not yet designed in detail).

Mechanism: official Tauri plugins — `tauri-plugin-notification`,
`tauri-plugin-autostart`, the core tray APIs, `tauri-plugin-updater`, and
`tauri-plugin-single-instance` (so the app doesn't launch twice when
tray-resident). All are genuinely cross-platform; the one platform-specific
note is that Linux tray icons require `libayatana-appindicator` (or
`libappindicator3`) to be present, which becomes a packaging dependency to
declare when Linux support is built. Details: `RESEARCH_DECISIONS.md` §4.

---

## 8. Voice (future — not built yet)

Voice is another input/output interface to the same action/capability
system as chat — it must not gain a second, different "brain." Full
research and comparison: `RESEARCH_DECISIONS.md` §3.

**Planned direction: a cascaded pipeline** (STT → the same cloud LLM
already used for text chat, with the same tool/domain-action bindings →
TTS), not a provider-native speech-to-speech model. Native S2S APIs
(OpenAI Realtime, Gemini Live, Amazon Nova Sonic) are architecturally a
*second brain* — a different model with its own tool-calling schema and
persona behavior that would have to be kept in sync with the text-chat
brain forever. Cascade avoids that by construction: voice is STT-in →
same-LLM-same-tools → TTS-out.

- **Orchestration**: prototype with Pipecat's non-WebRTC transport mode,
  not LiveKit Agents — LiveKit's core value (WebRTC SFU, multi-party/
  telephony transport) solves a network-transport problem LifeOS doesn't
  have (a single local device talking to its own mic/speaker). Revisit
  LiveKit only if a browser/mobile-remote or phone-call channel is ever
  added.
- **NeXa reuse**: no live NeXa git repository is accessible in this
  environment — only Desktop planning documents (docx/txt, not code)
  referencing prior NeXa research into AEC3 echo cancellation, self-echo
  suppression, and barge-in handling. That research area is real and
  reappears here (see the AEC3/echo-cancellation prototyping item below),
  but there is no NeXa source code to pull from; it must be re-solved
  for LifeOS's own Rust/Tauri context.
- **Engineering gaps to prototype before committing**: end-to-end cascade
  latency under real tool-calling load (LLM time-to-first-token will
  dominate, not STT/TTS); the speaker-to-mic self-echo/AEC path on Windows
  (`cpal` + WASAPI loopback + the `webrtc-audio-processing` Rust crate for
  AEC3 — a genuine gap neither Pipecat nor LiveKit solves for a
  non-browser-mediated local loopback); whether to bundle Pipecat as a
  Python sidecar or hand-roll the (comparatively small) STT/LLM/TTS
  orchestration loop directly in Rust; Tauri webview audio-capture
  reliability on Linux (WebKitGTK), which argues for capturing audio
  natively via `cpal` rather than through the webview at all.
- **Privacy**: audio leaves the device in every cloud-brain option
  evaluated (cascade or native S2S) — there is no fully-local path to a
  cloud-quality voice assistant. This must be disclosed to the user, and a
  text-only fallback must remain available for privacy-sensitive use.

---

## 9. UI/UX

No production UI is implemented yet (M1 begins the visual shell only —
navigation and layout, no domain data). This section is the visual
language, information architecture, and text wireframes to build against.

### 9.1 Visual language — Home, face, radial menu

LifeOS shares NeXa's visual language.

**Home:** full-screen dark galaxy background; an animated NeXa face
centered on screen — two white rounded bean-like eyes, a white mouth/
smile, clean and minimal; the galaxy remains visible behind the UI at all
times (the face and menus float over it, nothing fully occludes it).

**Face animation states** (infrastructure required now, M1; real
state-driving logic comes later with each capability): `IDLE`,
`LISTENING`, `LOCAL_PROCESSING`, `WAITING_FOR_CLOUD`, `SPEAKING`,
`ATTENTION`.

**Speaking state:** do not merely swap between smile/straight mouth
shapes — the mouth must visibly open/close with smooth animation. Later,
mouth openness may be driven approximately by output-audio amplitude.
Perfect phoneme lip-sync is explicitly not required.

**Radial menu:** clicking the face smoothly shrinks it slightly, keeps it
centered, and reveals glass-effect tiles around it like numbers around a
clock face. Primary ring (do not overload it):

```
Today | Chat | Calendar | Goals | Projects | Money | Me | Insights
```

Secondary/configuration access (later, not the first ring): Memory, Quick
Forms, Model/AI, Settings.

**Home gestures / edge drawers** (can start as desktop buttons/hit areas —
perfect touch gestures are not required immediately, but the architecture
must allow this interaction model later):

- **Bottom edge** — chat access and chat-history controls (New Chat,
  recent chats, continue chat, delete/archive chat — later).
- **Left edge** — Today/Dashboard drawer.
- **Top edge** — notification/question/decision drawer.

### 9.2 Information architecture

```
Today | Chat | Calendar | Goals | Projects | Money | Me | Insights | Settings
```

LifeOS should feel like a personal command centre, not enterprise
accounting software. Avoid dashboard overload everywhere below — each
screen should answer a small number of specific questions well, not
surface every possible metric.

### 9.3 Today

Answers immediately: What matters today? What should I do next? Am I on
track? What requires attention? What is upcoming?

```
+-------------------------------------------------------------+
| TODAY                                              [date]   |
+-------------------------------------------------------------+
| Current direction: <one-line summary of top goal/focus>     |
+-------------------------------------------------------------+
| TOP 3 TODAY                                                  |
|  1. [ ] ...                                                  |
|  2. [ ] ...                                                  |
|  3. [ ] ...                                                  |
+-------------------------------------------------------------+
| GOALS PROGRESS        | PROJECTS PROGRESS                    |
|  Goal A  [====  ] 62% |  Project X — on track, next: ...     |
|  Goal B  [==    ] 30% |  Project Y — blocked: ...             |
+-------------------------------------------------------------+
| MONEY SNAPSHOT         | UPCOMING                            |
|  Safe-to-spend: £N     |  Fri 14:00  View the van             |
|  Next bill: ... in Nd  |  Mon        Project X milestone       |
+-------------------------------------------------------------+
| WELLBEING SNAPSHOT     | WARNINGS                            |
|  Energy 6/10, mood OK  |  ⚠ Deadline risk: ...                |
+-------------------------------------------------------------+
| INSIGHTS (1-2 surfaced, not a feed)                          |
|  "You tend to skip journaling on high-stress days."          |
+-------------------------------------------------------------+
```

### 9.4 Chat

Full-height conversation view. Natural operation of the system per §2 —
every action the chat performs is visibly the same action a form would
trigger (e.g. a chat-created task appears identically to a form-created
task, with an inline confirmation of what was just done, not just prose).

### 9.5 Calendar

The local LifeOS calendar (§6): month/week/day views, recurring events,
deadlines, reminders, planned work blocks, project milestones layered on
one timeline.

### 9.6 Goals

Per goal: target, deadline, progress, current status, next actions.

```
GOAL: First paying DeVDul client
  Deadline: Dec 2026        Status: On track
  Progress: [======    ] 60%
  Next actions:
   - Follow up with lead #3
   - Publish portfolio update
```

### 9.7 Projects

Active projects; priority; progress; current milestone; blockers.

```
PROJECT: Van purchase
  Priority: High            Progress: [===       ] 30%
  Current milestone: Viewing on Friday
  Blockers: Financing approval pending
```

### 9.8 Money

Eventually: cash, income, expenses, budgets, recurring costs (fuel, food,
work income), savings, protected buffer, safe-to-spend, forecast. Not
built in this phase — domain model only (§5.1).

### 9.9 Me

Eventually: wellbeing, preferences, habits, journal, recurring problems,
successful solutions, personal development, relevant memory.

### 9.10 Insights

Eventually: learned patterns and AI recommendations, surfaced sparingly —
this is the screen most at risk of becoming noise; default to showing few,
high-confidence insights rather than a feed.

### 9.11 Settings

Privacy (the Privacy Firewall inspector, §4.3), AI providers (cloud LLM
selection, §3), local model (§4.1), notifications (§7), data/export,
backups.

---

## 10. Extensibility

LifeOS will evolve while being used; new modules and product ideas are
expected to be discovered during development, not just at design time.

Avoid: a giant monolithic service; hard-coded domain knowledge inside the
chat UI; cloud-model-specific business logic; duplicated logic between
forms/chat/voice; database structures that can't evolve.

Prefer: domain capabilities as the unit of extension; commands/actions as
the interface between UI/chat/voice and domain logic; clear service
boundaries (reinforced by the Rust-crate-per-domain pattern, §5.1); stable
interfaces; versioned migrations; a swappable cloud-LLM-provider interface
(§3). This should allow future capabilities (Health, Learning,
Relationships, Career, Travel, Documents, Vehicles, Home, NeXa
integration) without rebuilding the core.

### 10.1 Future NeXa integration (architectural from day one)

LifeOS is currently a standalone application, but it is intentionally
built so that NeXa can later receive LifeOS as a capability, without the
desktop UI ever being the business-logic owner:

```
NeXa Core -> LifeOS Capability/API -> LifeOS Core
```

NeXa should later be able to call capability operations such as
`get_current_situation`, `get_active_goals`, `get_today_plan`,
`get_relevant_memory`, `create_goal`, `update_goal`, `create_challenge`,
`record_solution_attempt`, `record_outcome`, `create_calendar_event`,
`change_priority`, `log_wellbeing`, `get_progress` — **without directly
manipulating LifeOS database tables.** This is the same guarded action
layer (§2, §5.4-§5.6) the desktop UI and the cloud LLM already use; a
NeXa capability boundary is a third caller of that one layer, not a new
integration point into the database. **The operation names/signatures
above are frozen at M2 as the capability API contract** (M2 report §I) -
this is a design freeze, not a claim that a `capability/` module
implementing them exists yet in code (corrected 2026-09-17, M2 Goal
hardening report §9C: it does not - see §5.6's "Implementation status"
note). What's still undecided, beyond that module simply not being built
yet, is only the transport NeXa will eventually use to reach it (local
socket, embedded HTTP, or otherwise); no NeXa-specific code is
written yet.

---

## 11. Technology stack

Full evaluation, alternatives considered, and sources:
`RESEARCH_DECISIONS.md` §4–§5.

| Layer | Choice | Deviation from initial candidate stack? |
|---|---|---|
| Desktop shell | Tauri 2 | No — confirmed as the right choice |
| Frontend | React + TypeScript | No |
| Backend | Rust | No |
| Local DB | SQLite | No |
| DB encryption | SQLCipher (`rusqlite` `bundled-sqlcipher`) — PROVEN 2026-09-17 | No, net — `sqlite3mc` was tried first (no mature Rust integration found, storage-spike report), SQLCipher was always the documented fallback and is what's actually proven |
| Encryption key storage | `keyring` crate (OS credential store) | **Yes** — Stronghold considered and rejected (deprecated upstream) |
| DB driver/migrations | `rusqlite` + `refinery` | Clarification — not the sqlx-based official plugin |
| Cloud LLM | Provider-agnostic; **v0.1 final: OpenAI GPT-5.6 Luna** | Product decision, §3 — supersedes the research-stage Sonnet-5 default |
| Local helper LLM | **v0.1 final: Qwen3 4B**, Ollama-hosted in dev, runtime kept abstract for a future llama.cpp sidecar | Product decision, §4 — benchmark plan in `RESEARCH_DECISIONS.md` §2 still validates it at M4 |
| Calendar/recurrence | `rrule.js` (frontend) + `rrule` Rust crate (backend) | No — confirms "don't hand-roll RFC5545" |
| Notifications/background | Tauri official plugins (notification, autostart, tray, updater, single-instance) | No |
| Forms/validation | React Hook Form + Zod | No |
| Charts | Recharts | No |
| Calendar UI component | react-big-calendar | Clarification — chosen over FullCalendar to avoid its paid Premium tier |
| Future vector/semantic search | `sqlite-vec` | No — successor to the now-unmaintained `sqlite-vss` |
| Future voice orchestration | Pipecat (non-WebRTC mode) | Clarification — LiveKit Agents considered and deferred |

---

## 12. Development milestones (current build plan)

Vertical milestones — the app is tested continuously, not built to a big
bang. Each milestone should end with a working, tested increment — no
"60% done, half-implemented" states left behind (per `CLAUDE.md`). This
list supersedes the earlier M0–M7 sketch (kept in spirit, expanded and
reordered against `PROJECT_STATE.md`'s current reality).

- **M0 — Environment ready.** Install/verify Windows C++/MSVC
  prerequisites for Tauri; install Rust via `rustup`; verify cargo/rustc;
  verify Node/npm; verify WebView2; verify Tauri prerequisites; document
  exact versions in `PROJECT_STATE.md`.
- **M1 — Visual shell.** Scaffold Tauri 2 + React + TypeScript; launch the
  real LifeOS desktop window; galaxy Home; initial animated NeXa face;
  face-state infrastructure (§9.1); face click interaction; radial glass
  menu; basic screen/navigation shell; left/bottom/top drawer
  architecture. No fake backend functionality. **End of M1: the user must
  be able to launch and see the real app.**
- **M2 — LifeOS Core + encrypted storage.** Architecture frozen
  2026-09-17 (§5.1-§5.6, `docs/reports/R003_M2_ARCHITECTURE_FREEZE_2026-09-17.md`).
  Storage layer PROVEN 2026-09-17 (SQLCipher via `rusqlite`
  `bundled-sqlcipher`, not `sqlite3mc` — see §5 and
  `docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md`) and hardened for
  production use the same day (spike/production separation, no
  committed machine-specific config; SQLite foreign-key enforcement
  genuinely enabled in a later hardening pass —
  `docs/reports/R008_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md`).
  `Goal` is implemented **and** hardened (atomic mutation+audit, typed
  `CoreError`, `FieldPatch` semantics —
  `docs/reports/R005_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`,
  `docs/reports/R006_M2_GOAL_HARDENING_2026-09-17.md`). `Project` is
  also implemented (`CreateProject`/`UpdateProgress`, its `goal_id` link
  to `Goal` proven — `docs/reports/R007_M2_PROJECT_VERTICAL_SLICE_2026-09-17.md`),
  built directly with the hardened pattern. `GoalRelation` is also
  implemented (`LinkGoals`, its 11-value relation-type enum and
  direction convention proven —
  `docs/reports/R009_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md`),
  the first relationship-*graph* structure, built directly with the
  twice-hardened pattern. The remainder of the M2 domain model is still
  in progress — a fourth entity is next (`docs/PROJECT_STATE.md` §9).
  Local encrypted SQLite, secure key handling (`keyring`), migrations
  (`refinery`), core action architecture (§2, §5.4-§5.6), initial
  profile/settings. No AI
  dependency for CRUD (AI-optional resilience, §2).
- **M3 — Core life UI.** Today, Goals, Projects, Calendar; Quick Add /
  schema-driven forms; PL/EN UI architecture (§1.3); backend recurrence
  authority (the Rust `rrule` crate, §6).
- **M4 — Local intelligence.** Ollama development runtime; Qwen3 4B (§4);
  benchmark on the actual laptop (`RESEARCH_DECISIONS.md` §2's plan);
  PL/EN extraction; structured-output reliability; local intent/entity
  extraction; memory candidates; privacy preprocessing.
- **M5 — Conversation.** GPT-5.6 Luna provider (§3); provider abstraction;
  Chat UI; Conversation Brief/context builder; Privacy Firewall (§4.3); no
  direct DB access from cloud (§3.1); actions mediated locally.
- **M6 — Personal intelligence.** Goal Intelligence (§5.2); Challenges;
  Problems/Solutions; Personal Knowledge Layer (§5.3); memory provenance;
  questions from LifeOS; insights;
  confirmed/observed/inferred distinction.
- **M7 — Proactive LifeOS.** Native notifications; reminders; follow-ups;
  check-ins; Today supervision; weekly review; adaptive plan suggestions;
  anti-spam/quiet-hour rules.
- **M8 — Money + Me.** Finances, budgets, safe-to-spend, forecasts; sleep,
  mood, energy, stress; habits, skills; user-configurable tracked
  metrics.
- **M9 — Voice.** `microphone -> local STT -> same LifeOS input pipeline
  -> Qwen/Core/Privacy -> GPT-5.6 Luna -> TTS -> speaker`. Text and voice
  use the same LifeOS domain/action system (§2, §8) — no second,
  independent conversational brain.
- **M10 — Distribution.** `LifeOS-Setup.exe`; clean-machine installation
  test; hardware detection; local-model download/setup; backup; restore;
  diagnostics; update path.

### 12.1 Usable-Alpha definition

LifeOS becomes a **Usable Alpha** when the creator can realistically use
it for at least seven consecutive days for real life management. At
minimum the user must be able to: launch LifeOS normally; use the NeXa
Home interface; see what matters today; create/update goals; manage
projects; use the LifeOS Calendar; chat naturally in Polish and English;
enter changes through conversation; retain relevant local long-term
personal knowledge; record a Challenge/problem; record attempted
solutions; record whether a solution worked; receive useful LifeOS
questions/follow-ups; see progress; receive native reminders/
notifications; inspect important AI-proposed changes; undo/reject
inappropriate changes; keep private data local; see that cloud
conversation receives only approved/minimized context.

Success is **not** "all planned features exist." Success is: LifeOS
genuinely helps the user understand what to do, make plans, remember
important context, adapt when reality changes, and learn what works for
that user. "First run" (a real desktop window launches, M1) and "Usable
Alpha" (the above) are different bars — do not conflate them in a RUN
REPORT.

---

## 13. Open questions

These are genuinely unresolved and should not be treated as decided:

- **Resolved 2026-09-17:** `sqlite3mc` had no mature native-Rust
  integration to test; SQLCipher (via `rusqlite`'s `bundled-sqlcipher`)
  was substituted and is now PROVEN for M2 on Windows — see §5 and
  `docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md`. Still open:
  whether to invest in fixing this machine's Perl to unlock
  `bundled-sqlcipher-vendored-openssl` and drop the machine-specific
  external-OpenSSL build dependency `bundled-sqlcipher` currently needs.
- `Relation.relation_type` (Personal Knowledge Layer, §5.3) is an
  intentionally free string rather than a constrained enum — revisit
  once M6 usage patterns exist.
- Exact NeXa transport mechanism (§10.1) — the capability function
  boundary is frozen, the transport that will carry calls to it is not.
- Exact confirmation-class UI treatment (§5.5: modal vs. inline vs.
  toast per class) — deferred to M3.
- Qwen3 4B and GPT-5.6 Luna are now the chosen v0.1 models (§3, §4) — but
  chosen is not the same as *validated*. The benchmark plan in
  `RESEARCH_DECISIONS.md` §2 still needs to run against Qwen3 4B at M4,
  and a small internal Polish eval still needs to run against GPT-5.6
  Luna once chat exists (M5).
- Exact data-sensitivity classification levels and which specific fields
  fall into each tier (§4.3) — needs to be defined alongside the actual
  schema, not abstractly.
- Multi-device sync strategy — CRDT-based (AFFiNE's y-octo/OctoBase
  pattern) vs. a sidecar tool like Syncthing vs. no sync in v1. Deferred;
  not needed for a single-device initial release.
- Exact notification batching/anti-spam rules (M7).
- Whether/how NeXa integration will actually work in practice, given no
  NeXa codebase is currently accessible from this environment — the
  capability-API shape (§10.1) is a design constraint, not yet a tested
  integration.
- Exact bilingual detection/normalisation mechanism (which layer decides
  the response language, how mixed-language utterances are handled) —
  design target stated in §1.3, mechanism not yet designed.
- **Resolved 2026-09-17 (M2 Project Child Action Freeze report, then
  implemented the same date, M2 UpdateProject implementation):** the
  previously-open Project action-set gap (no frozen action to edit a
  `Project`'s `title`/`description`/`status`/`priority`/`goal_id`) is
  closed both in design and in code — `UpdateProject` (floor
  `IMPORTANT_CHANGE`) is real, tested code. The same gap for
  `Milestone`/`Task` is likewise closed in code —
  `UpdateMilestone`/`UpdateTask`, both floor `EXPLICIT_ADD`. Full detail
  in §5.1.
- **Resolved 2026-09-17 (M2 Milestone vertical-slice report §3):**
  `Milestone.order_index` uniqueness within a Project (§5.1) — YES,
  `(project_id, order_index)` must be unique; different Projects may
  reuse the same values; no automatic renumbering is implemented.
- **Genuinely open (2026-09-17, M2 Task vertical-slice report §3):**
  `RecurringRule`'s own design — representation (rrule/RFC5545 vs. a
  custom scheme), recurrence-expansion semantics, and its relationship
  to the still-unbuilt Calendar/Reminder entities are all undecided.
  `Task.recurring_rule_id` exists as a frozen but deliberately
  unimplemented field (§5.1) specifically so this question can be
  answered later, in its own design/freeze pass, rather than forced by
  the Task vertical slice's implementation needs.
