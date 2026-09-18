# R020 — M2 Personal Knowledge Slice A Foundation

The first Personal Knowledge implementation slice: the `V7` full
eleven-table relational foundation, the six frozen domain types Slice A
needs, the `ensure_self_entity` bootstrap, a narrow `get_entity`/
`get_fact` read side, and `secure_delete = ON` storage hardening —
exactly R019 §21's corrected Slice A scope, no more. **`AddMemory` is
NOT implemented in this run** — Slice B remains a future, separate
pass.

---

## 1. Pre-run state

- R017/R018/R019 (Personal Knowledge architecture freeze, then two
  rounds of consistency/privacy hardening) all completed, architecture
  explicitly declared CLOSED for Slice A/B implementation (R019 §23).
  Local checkpoint `413b61b` — `docs: finalize Personal Knowledge
  privacy foundation`. 262/262 existing implementation tests passing.
  Working tree clean. R001-R019 exist; this is **R020**, the next
  allocation.

---

## 2. R019 secure_delete terminology correction

R019 §17 conceptually said `PRAGMA secure_delete = ON (or the stronger
FULL)` — **SQLite has no `FULL` value for `secure_delete`; its
canonical values are `OFF`, `ON`, `FAST`.** Corrected via a clearly
labelled "Implementation correction (R020, 2026-09-18)" addendum
appended to R019 (its original historical text is otherwise unedited):
the canonical LifeOS choice is **`ON`**; `FAST` is deliberately not
chosen, since it can still leave deleted content on freelist pages,
weaker than the privacy-oriented guarantee this hardening exists to
provide. This run implements and verifies `ON`.

---

## 3. `V7` table list

Exactly the eleven tables R019 §12 froze — no more, no fewer, no split
across multiple migration files:

1. `entities` 2. `sources` 3. `evidence` 4. `facts` 5. `relations`
6. `episodes` 7. `fact_evidence` 8. `relation_evidence`
9. `episode_evidence` 10. `episode_entities` 11. `fact_audit_log`

`migrations/V7__create_personal_knowledge.sql`. V1-V6 are unedited.

---

## 4. Exact migration structure

A direct, unmodified transcription of R017/R018/R019's frozen field-
level schema (no field redesigned) — full text in the migration file
itself; summary per table:

- **`entities`**: `entity_type`/`canonical_name`/`is_self`/
  `sensitivity` (all `NOT NULL`, `sensitivity` `CHECK IN` the 4 frozen
  values), `domain_ref_type`/`domain_ref_id` (nullable, `CHECK` both-or-
  neither), `created_at`/`updated_at`. Two partial unique indexes:
  `idx_entities_single_self` (`WHERE is_self = 1`, R019 §3's DB half of
  the SELF invariant) and `idx_entities_domain_ref` (`WHERE
  domain_ref_type IS NOT NULL`, R017 §4).
- **`sources`**: `source_type`/`sensitivity` `NOT NULL`, `label`/
  `reference` nullable, `captured_at`/`created_at`. No FK, no `updated_at`
  (Source rows are never mutated).
- **`evidence`**: `source_id REFERENCES sources(id) ON DELETE
  RESTRICT`, `content`/`locator` nullable with `CHECK (content IS NOT
  NULL OR locator IS NOT NULL)`, `sensitivity NOT NULL`, `created_at`.
- **`facts`**: `subject_entity_id NOT NULL REFERENCES entities(id) ON
  DELETE RESTRICT` (corrected vs. R003, R017 §2 item 3),
  `predicate NOT NULL`, `value_json`/`object_entity_id` with the
  one-semantic-object `CHECK ((value_json IS NOT NULL) <>
  (object_entity_id IS NOT NULL))`, `memory_type`/`epistemic_state`
  `CHECK IN` their 11/5 frozen values, `confidence REAL CHECK (NULL OR
  0.0-1.0)`, `sensitivity NOT NULL`, `valid_from`/`valid_until`,
  `superseded_by_fact_id REFERENCES facts(id) ON DELETE RESTRICT` with
  `CHECK (<> id)`, `created_at`/`updated_at`. **No `source_id` column**
  (R017 §2 item 4/§5).
- **`relations`**: `from_entity_id`/`to_entity_id NOT NULL REFERENCES
  entities(id) ON DELETE RESTRICT`, open `relation_type` (`CHECK
  (length(trim(...)) > 0)` only, no closed list — deliberately NOT
  `GoalRelation`'s enum), `note` nullable, `epistemic_state`/
  `sensitivity`/`confidence` same frozen shape as `facts`,
  `valid_from`/`valid_until`, `CHECK (from_entity_id <> to_entity_id)`.
  Schema only — no write path this slice (R019 §13).
- **`episodes`**: `title` nullable, `summary NOT NULL`, `started_at`,
  `ended_at` nullable with `CHECK (ended_at IS NULL OR ended_at >=
  started_at)`, `sensitivity NOT NULL`, `created_at`/`updated_at`.
  Schema only.
- **`fact_evidence`/`relation_evidence`/`episode_evidence`/
  `episode_entities`**: pure many-to-many join tables, composite
  `PRIMARY KEY`, both sides `ON DELETE RESTRICT`, `created_at`. No
  independent semantic payload.
- **`fact_audit_log`**: `fact_id` **deliberately NOT an enforced FK**
  (R017 §20 — a future `ForgetMemory` genuinely deletes the `facts` row
  while retaining this table's minimal marker), `action_type CHECK IN`
  the four Memory action names, `actor`/`provenance CHECK`, same shape
  as every other M2 audit table, `occurred_at` (not `created_at` —
  **naming alignment, not a semantic change**, see §22).

**Deliberately no plain (non-`UNIQUE`) indexes added this slice** —
Slice A's own reads (`get_entity`/`get_fact`, both by primary key) need
none; matching the established "no speculative indexes" discipline
(`V4__create_milestones.sql`'s own precedent) rather than the R017/
R018/R019 reports' non-binding illustrative field lists. Deferred to
whichever future slice's read function actually needs one.

---

## 5. Domain types added

`src-tauri/src/domain/mod.rs`:

- **Length constants** (15, R018 §15, unchanged values): `ENTITY_TYPE_MAX_LEN`
  = 60, `CANONICAL_NAME_MAX_LEN` = 200, `DOMAIN_REF_TYPE_MAX_LEN` = 60,
  `DOMAIN_REF_ID_MAX_LEN` = 200, `PREDICATE_MAX_LEN` = 100,
  `VALUE_JSON_MAX_LEN` = 2000, `RELATION_TYPE_MAX_LEN` = 100,
  `RELATION_NOTE_MAX_LEN` = 4000, `EPISODE_TITLE_MAX_LEN` = 200,
  `EPISODE_SUMMARY_MAX_LEN` = 4000, `SOURCE_TYPE_MAX_LEN` = 60,
  `SOURCE_LABEL_MAX_LEN` = 200, `SOURCE_REFERENCE_MAX_LEN` = 500,
  `EVIDENCE_CONTENT_MAX_LEN` = 4000, `EVIDENCE_LOCATOR_MAX_LEN` = 200.
- `validate_personal_knowledge_text_len(value, max_len) -> Result<(),
  usize>` — one generic reusable pure length check (§7 below).
- `Sensitivity` (4 variants), `MemoryType` (11 variants),
  `EpistemicState` (5 variants) — closed enums, `as_str()`/`parse()`,
  matching `LifecycleStatus`/`TaskStatus`'s exact established pattern.
- `EntityId`/`FactId`/`RelationId`/`EpisodeId`/`SourceId`/`EvidenceId` —
  six UUIDv7 newtypes, matching `GoalId`'s exact established pattern
  (`new()`/`parse()`/`Display`).
- `DomainRef { domain_ref_type: String, domain_ref_id: String }`.
- `Entity`, `Source`, `Evidence`, `Fact` (with `FactValue { Scalar(String),
  EntityRef(EntityId) }`) — the four domain structs Slice A's read side
  actually needs.

**`Relation`/`Episode` domain structs deliberately NOT added** — no
`get_relation`/`get_episode` function exists this slice, and their raw-
SQL defense-in-depth tests operate directly against the schema without
needing a Rust struct; adding one now would be speculative scaffolding
for an action family that remains deferred (R019 §13). `RelationId`/
`EpisodeId` typed IDs ARE added, since the join-table shapes already
reference them by type.

---

## 6. Enum mappings

All three new enums follow the exact `LifecycleStatus`/`TaskStatus`
convention — a closed Rust enum, `as_str()` to the frozen uppercase
`TEXT` value, `parse()` back, both sides proven by the raw-SQL defense
matrix (§16) rejecting every value outside the set and the round-trip
tests (§17) proving every value that IS accepted parses back correctly.
No renumbering, no additional variants, no renaming from R017's frozen
names.

---

## 7. Length constants / validation

`domain::validate_personal_knowledge_text_len` is the one reusable
pure helper — Slice A has no action payload yet to attach a per-field
`ValidationError` variant to (`AddMemory` does not exist), so this is
deliberately generic rather than 15 speculative error variants for an
action that doesn't exist (R018 §28's "Core validators should support
future Slice B without requiring semantic changes" — the length-check
*logic* is proven now; Slice B decides the exact per-field error-
variant shape once `AddMemory`'s real payload exists). Proven table-
driven against all 15 constants (`req_pklen1`, §18).

---

## 8. DB integrity constraints

Every constraint R019 §11 (defense-in-depth review) and R017-R019's
frozen field tables call for is implemented exactly as specified —
closed vocabularies (`Sensitivity`/`MemoryType`/`EpistemicState`) via
`CHECK IN`, open vocabularies (`entity_type`/`predicate`/
`relation_type`/`source_type`/`domain_ref_type`) left as plain `TEXT`
with only a non-empty/trim `CHECK`, never a fake closed list. Full
matrix proven in §16.

---

## 9. secure_delete implementation

`services::storage::mod::enable_secure_delete` (new): `PRAGMA
secure_delete = ON`, then reads it back immediately, returning a typed
`rusqlite::Error::SqliteFailure` if verification fails — the exact
"set, then verify, typed failure on mismatch" discipline already
established for `enable_foreign_keys` (R008/R015 §4e), never trusting
an unverified SQLite default for a privacy-relevant setting. Called
from `open_encrypted` immediately after `enable_foreign_keys`, before
the key is ever used for a real schema read (`verify_key` still runs
separately, afterward, as it always has) — ordering preserved: key
pragma → `enable_foreign_keys` → `enable_secure_delete` → return the
connection; `verify_key`/migrations/`ensure_self_entity` all run after,
unchanged. Key lifecycle, wrong-key detection, and `rekey` behavior are
completely untouched (proven unchanged in §18's `req_sd3`, and every
pre-existing R015/R016 key-lifecycle test remains green, §20).

---

## 10. Measured journal_mode

**MEASURED**, not assumed: `PRAGMA journal_mode` on a fresh encrypted
connection (`req_jm1`, run with `--nocapture`) returns:

```
[req_jm1] MEASURED - PRAGMA journal_mode = delete
```

Matches SQLite's own historical default (R019 §17's expectation,
confirmed rather than assumed) — no `journal_mode` PRAGMA is set
anywhere in this codebase, observation only, per this run's explicit
instruction. **Not forced to WAL or DELETE** — this test only reads the
current value.

---

## 11. SELF bootstrap implementation

`services::ensure_self_entity(conn) -> Result<Entity, BootstrapError>`
— 0 rows → generates a fresh `EntityId`, inserts the canonical row
(`entity_type = "person"`, `canonical_name = "SELF"`, `is_self = true`,
`sensitivity = Internal`, `domain_ref = None`); 1 row → reads and
returns it unchanged; >1 rows → a typed `BootstrapError::Storage`
integrity-violation error (constructed the same way
`enable_foreign_keys`/`enable_secure_delete` synthesize a typed
failure) — **never silently picks the first row and continues.**

Wired into `bootstrap_db_at` (the function both `bootstrap_production_db`
and every test bootstrap already shared) immediately after
`run_production_migrations`, **not** into `bootstrap_db_at_version`
(test-only, may deliberately freeze at a pre-V7 schema where `entities`
doesn't exist yet). Runs on every bootstrap, proven idempotent
(`req_self3`, three repeated calls). **Not placed inside the V7
migration itself** — a migration is schema, Core bootstrap is logic
(R019 §3).

---

## 12. Read-side functions

`services::get_entity(conn, EntityId) -> Result<Option<Entity>,
rusqlite::Error>` and `services::get_fact(conn, FactId) ->
Result<Option<Fact>, rusqlite::Error>` — thin wrappers over
`storage::personal_knowledge_repo`, matching `get_goal`/`get_task`'s
exact existing pattern precisely. No `get_self_entity` was added
separately — `ensure_self_entity` is itself directly callable by tests
(it's `fn`, visible to `services::tests` via `use super::*`) and
already returns the `Entity`, so a second narrow read would have been
redundant surface for no new capability. No `get_relevant_memory`,
graph traversal, history retrieval, or semantic search — unchanged
scope, R019 §29's Slice A boundary.

---

## 13. Relation/Episode schema-only safety

No production function anywhere writes to `relations`/`episodes`/
`relation_evidence`/`episode_evidence`/`episode_entities` — no
`insert_relation`/`insert_episode` exists in
`personal_knowledge_repo.rs`. Every raw-SQL row these tables receive in
this run's test suite is inserted directly via `conn.execute(...)` at
the test boundary (`req_pk_relation_defense_matrix`/
`req_pk_episode_defense_matrix`/`req_pk_join_defense_matrix`), never
through a repo function — proving their constraints without creating an
accidental mutation API (R019 §13's explicit concern).

---

## 14. Migration tests

- `req_mig1_fresh_v1_through_v7_migration_succeeds` — a fresh bootstrap
  creates all eleven Personal Knowledge tables.
- `req_mig2_populated_v6_database_upgrades_non_destructively_to_v7` —
  real `Goal`/`Project`/`Milestone`/`Task`/`GoalRelation` rows created
  against a database frozen at V6 (`bootstrap_db_at_version(.., 6)`),
  then a normal `bootstrap_db_at` upgrade to V7: every pre-existing row
  is unchanged (`get_goal`/`get_project`/`get_milestone`/`get_task`/
  `get_goal_relation` all still resolve to the same data), Personal
  Knowledge tables exist, and the upgrade's own bootstrap call created
  exactly one SELF entity.

---

## 15. Raw SQL defense tests

Seven grouped test functions (one per table family, matching the
established "group related raw-SQL proofs into one function" precedent
already used throughout M2), all real rejections against a real
encrypted, fully-migrated database — never inferred from Core
validation alone:

- **`req_pk_entity_defense_matrix`**: invalid `sensitivity`;
  half-populated `domain_ref` pair; duplicate non-`NULL`
  `(domain_ref_type, domain_ref_id)` pair. (Duplicate SELF is proven
  separately, `req_self4`.)
- **`req_pk_source_defense_matrix`**: invalid `sensitivity`.
- **`req_pk_evidence_defense_matrix`**: invalid `sensitivity`;
  nonexistent `source_id` FK; `content` NULL + `locator` NULL together.
- **`req_pk_fact_defense_matrix`**: nonexistent `subject_entity_id` FK;
  nonexistent `object_entity_id` FK; neither `value_json` nor
  `object_entity_id`; both together; invalid `memory_type`; invalid
  `epistemic_state`; invalid `sensitivity`; `confidence < 0`;
  `confidence > 1`; self-supersession (`superseded_by_fact_id = id`);
  nonexistent `superseded_by_fact_id` FK — 11 checks in one function.
- **`req_pk_relation_defense_matrix`**: nonexistent `from_entity_id`;
  nonexistent `to_entity_id`; invalid `epistemic_state`; invalid
  `sensitivity`; `confidence` out of bounds (both directions);
  self-edge (`from == to`) — 6 rejections — **plus** a positive proof
  that a `relation_type` genuinely outside `GoalRelation`'s 11 closed
  values (`"THIS_IS_NOT_A_GOALRELATION_VALUE_AT_ALL"`) is **accepted**,
  demonstrating the vocabulary is genuinely open (R019 §13's explicit
  requirement).
- **`req_pk_episode_defense_matrix`**: invalid `sensitivity`;
  `ended_at` before `started_at`.
- **`req_pk_join_defense_matrix`**: nonexistent `fact_id` in
  `fact_evidence`; nonexistent `evidence_id`; a duplicate
  `(fact_id, evidence_id)` link rejected by the composite primary key.

---

## 16. Length-bound tests

`req_pklen1_all_frozen_length_constants_enforced` — one table-driven
test iterating all 15 R018-frozen constants: exactly `max_len`
characters accepted, `max_len + 1` rejected, for every one, via
`domain::validate_personal_knowledge_text_len` directly (no DB, no
action — matching §7's design). Deliberately one function, not 15, per
this run's own "do not bloat the suite with unnecessary duplicate test
functions" instruction.

---

## 17. Read round-trip tests

- **`req_pkrt1_get_entity_round_trip_all_fields`**: an Entity with no
  `domain_ref`, an Entity WITH `domain_ref`, and the SELF entity (via
  `ensure_self_entity`) — all three round-trip through `get_entity`
  with full struct equality (`PartialEq`), proving `sensitivity`,
  timestamps, and the `is_self` flag all survive exactly.
- **`req_pkrt2_get_fact_round_trip_scalar_and_entity_ref`**: a
  `Scalar`-value Fact (with `confidence = Some(0.75)`), an
  `EntityRef`-value Fact (with `confidence = None`, proving `NULL`
  round-trips as `None`), and a closed/superseded predecessor Fact
  (`valid_until` set, `superseded_by_fact_id` pointing at a real
  successor, `epistemic_state = Stale`) — all round-trip through
  `get_fact` correctly, proving every enum's `parse()`, both
  `FactValue` variants, nullable `confidence`, and the temporal/
  supersession fields.

---

## 18. Encryption-at-rest regression

`req_sd4_encryption_at_rest_unchanged` (part of the secure_delete
suite, §9) — after the `secure_delete` hardening, the raw database file
still shows no plaintext `SQLite` header, matching R016's established
`req_kl_bootstrap_regression`-style proof exactly. `req_sd3` separately
proves wrong-key rejection is unchanged. No real user data or key
material is ever exposed by either test.

---

## 19. Storage privacy proof

The established "attempt the illegal access, capture the real compiler
error" probe was re-run against the new module shape: a temporary
`#[cfg(test)] fn` in `domain/mod.rs` (a sibling module) attempting
`crate::services::storage::personal_knowledge_repo::get_entity` failed
with:

```
error[E0603]: module `storage` is private
```

— confirmed directly via `cargo check --tests`, then the probe was
**removed immediately** (never committed, never left in the tree; `git
diff --stat`, §24, shows no trace of it). `services::storage`'s privacy
boundary holds unchanged after this slice's restructuring (new
`personal_knowledge_repo` submodule included).

---

## 20. Exact total test count

**Previous: 262. New: 23. Final: 285. Failed: 0.**

23 new tests, exactly: `req_sd1`-`req_sd5` (5, secure_delete),
`req_jm1` (1, journal_mode observation), `req_self1`-`req_self5` (5,
SELF bootstrap), `req_mig1`-`req_mig2` (2, migration), `req_pk_entity`/
`source`/`evidence`/`fact`/`relation`/`episode`/`join_defense_matrix`
(7, raw SQL defense, covering 20+ individual sub-assertions),
`req_pklen1` (1, all 15 length constants table-driven), `req_pkrt1`-
`req_pkrt2` (2, read round-trip). All 262 prior tests remain green,
unmodified — confirmed by running the full suite (`cargo test --
--test-threads=1`) both before and after this slice's changes.

---

## 21. Build/package results

- `cargo fmt --check`: clean (after one `cargo fmt` pass — recorded for
  completeness, not silently corrected away).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **285 passed; 0 failed; 0 ignored**.
- `cargo build --release`: succeeded (2m36s).
- `npm run build`: succeeded.
- `npm run tauri build`: run this pass (required — this run changes
  production migrations, encrypted-connection configuration, and
  bootstrap behavior) — see the chat reply for the measured MSI/NSIS
  result, verified from the captured log file directly.

---

## 22. Files changed

- `src-tauri/migrations/V7__create_personal_knowledge.sql` (new) — the
  full 11-table Personal Knowledge schema.
- `src-tauri/src/domain/mod.rs` — 15 length constants, 1 generic
  length-validator, `Sensitivity`/`MemoryType`/`EpistemicState`, six
  typed IDs, `DomainRef`, `Entity`/`Source`/`Evidence`/`Fact`/
  `FactValue`.
- `src-tauri/src/services/storage/mod.rs` — new `enable_secure_delete`
  (called from `open_encrypted`); `pub(super) mod
  personal_knowledge_repo;`.
- `src-tauri/src/services/storage/personal_knowledge_repo.rs` (new) —
  `Entity`/`Fact` insert/get, `Source`/`Evidence`/`fact_evidence`-link
  insert (test-fixture only, `#[cfg(test)]`), `count_self_entities`/
  `get_self_entity`.
- `src-tauri/src/services/mod.rs` — `ensure_self_entity` (wired into
  `bootstrap_db_at`, after migrations), `get_entity`/`get_fact`, new
  imports (`Entity`/`EntityId`/`Fact`/`FactId`/`Sensitivity`/
  `chrono::Utc`).
- `src-tauri/src/services/tests.rs` — 23 new tests (§20), new imports.
- `docs/reports/R019_...md` — one-section "Implementation correction
  (R020)" addendum appended (secure_delete `FULL`→`ON` terminology
  fix); original body otherwise unedited.
- `docs/reports/R020_M2_PERSONAL_KNOWLEDGE_SLICE_A_FOUNDATION_2026-09-18.md`
  (this report).
- `docs/PROJECT_STATE.md`/`docs/LIFEOS_SPEC.md` — updated (see chat
  reply/commit for detail).

**One incidental naming alignment, not a semantic change**: R017-R019's
prose used `created_at` for `fact_audit_log`'s timestamp column; every
other M2 audit table (`goal_audit_log`, `project_audit_log`,
`goal_relation_audit_log`, `milestone_audit_log`, `task_audit_log`)
actually uses `occurred_at`. This run's own instruction to "use the
repo's actual migration naming convention" was followed — the column is
named `occurred_at` in `V7`, matching the unbroken existing convention;
this is a column name only, not a semantic or architectural change
(R017-R019 never froze the literal SQL identifier, only that the column
exists and is a timestamp).

---

## 23. Remaining Personal Knowledge work

Slice B (`AddMemory`) is the next slice — creates `Fact`+`Source`(new-
or-existing)+`Evidence`+`fact_evidence`+audit atomically, with the
`EntityInputRef`/`DuplicateDomainReference` machinery R018 §4/R019 §7
froze. Slices C (`CorrectMemory`), D (`ConfirmMemory`), E
(`ForgetMemory`'s full 14-step algorithm, R019 §10), F (`Relation`
actions), G (`Episode` actions), H (retrieval/Context Pack) all remain
entirely unstarted, unchanged from R019 §29's recommended order.

---

## 24. Exact recommendation for Slice B

Implement `AddMemory` next, exactly as R018 §4-§7/R019 §4-§7 froze it:
`EntityInputRef` (`SelfEntity`/`Existing`/`New`) for both subject and
object, `SourceRef` (`Existing`/`New` with required `sensitivity`),
always-new `Evidence`, one atomic transaction, the
`DuplicateDomainReference` same-payload preflight, and the
Personal-Knowledge-specific inferred-confirmation overlay (persist
`USER_CONFIRMED` only after real confirmation, R018 §5-§6). This is the
one Memory action with no dependency on another Memory action already
existing (`CorrectMemory`/`ConfirmMemory`/`ForgetMemory` all require an
existing Fact to target) — the correct next real implementation step.
**`AddMemory` does not exist after this run.**
