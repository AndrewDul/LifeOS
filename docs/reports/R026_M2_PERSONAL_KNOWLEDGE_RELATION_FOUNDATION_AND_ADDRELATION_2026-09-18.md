# R026 — M2 Personal Knowledge Relation Foundation and AddRelation

**Type:** Implementation (schema + production Rust + tests). Implements
exactly the scope R025 (and its two post-freeze addenda) froze for this
slice: V8 migration, `Relation` domain/read-side, and `AddRelation`. Does
**not** implement `CorrectRelation`/`ConfirmRelation`/`ForgetRelation`,
Episode, retrieval, Tauri commands, or UI.

---

## 1. Pre-run verified state

CONFIRMED FACT: clean working tree at commit `c301c0a` ("docs: fix
CorrectRelation sensitivity patch contract") before this run began. 400
tests passing. V7 unchanged since R020. No V8 existed. No Relation
production write path existed. Read in full before writing any code:
`CLAUDE.md`, `docs/PROJECT_STATE.md`, R017–R025 (including both R025
post-freeze addenda), the live `V7__create_personal_knowledge.sql`
(`relations`/`relation_evidence` exact shape), `fact_audit_log`'s exact
schema, `add_memory`'s full implementation (execution order,
`resolve_entity_input_ref`/`resolve_source_ref`, transaction shape),
`validate_add_memory`, `EntityInputRef`/`SourceRef`/`NewEntityData`/
`NewSourceData`/`NewEvidenceData`, `EpistemicState`/`Sensitivity`/
`RelationId`, `personal_knowledge_repo.rs`'s existing functions, the
`fact_audit_log` forced-failure trigger test pattern, and the refinery
`embed_migrations!` registration mechanism (auto-discovers by filename,
no manual registration needed).

## 2. The V8 migration (exact)

`src-tauri/migrations/V8__create_relation_audit_log.sql` — one new,
independent table, nothing else:

```sql
CREATE TABLE relation_audit_log (
    id                  TEXT PRIMARY KEY,
    relation_id         TEXT NOT NULL,
    action_type         TEXT NOT NULL CHECK (action_type IN ('AddRelation', 'CorrectRelation', 'ConfirmRelation', 'ForgetRelation')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI', 'CloudLLM', 'NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit', 'Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);
```

Structurally identical to `fact_audit_log`. `relation_id` is
deliberately **not** a foreign key (proven directly by
`req_mig7_relation_audit_log_relation_id_is_not_a_foreign_key`). No
`ALTER TABLE relations` of any kind — no `updated_at`, no
`superseded_by_relation_id`. V1–V7 untouched (`git diff -- migrations/`
shows the migrations directory has zero modified files, only the new V8
file as untracked/added).

## 3. Proof V8 only adds `relation_audit_log`

- `req_mig8_relations_table_not_altered_by_v8` — `PRAGMA
  table_info(relations)` compared against the exact frozen 11-column V7
  list; fails if `updated_at`/`superseded_by_relation_id` or anything
  else appears.
- `req_mig3_...` — `PRAGMA table_info(relation_audit_log)` compared
  against the exact 8-column frozen list, in order.
- `git diff --stat` (below, §29) confirms no other schema file changed.

## 4. Migration compatibility / reopen (R026 §44)

- `req_mig3` — fresh V1→V8 migration succeeds.
- `req_mig4_populated_v7_database_upgrades_non_destructively_to_v8` — a
  real pre-existing Fact (created via `add_memory` on a V7-only DB) is
  opened at V7, then reopened through the normal `bootstrap_db_at` (which
  runs V8): the Fact survives, `relation_audit_log` exists, and
  `AddRelation` succeeds on the upgraded connection — proving the upgrade
  path is genuinely usable, not merely schema-present.
- `req_mig9_existing_v7_relation_raw_fixtures_remain_compatible_after_v8`
  — a raw V7-shaped `Relation` fixture row round-trips unchanged after
  the V8 migration.
- Existing wrong-key/encryption regression tests are unmodified and
  still pass in the full suite (§13 below) — no key-lifecycle change was
  made.

## 5. `Relation` domain struct (exact, `src/domain/mod.rs`)

```rust
pub(crate) struct Relation {
    pub id: RelationId,
    pub from_entity_id: EntityId,
    pub to_entity_id: EntityId,
    pub relation_type: String,
    pub note: Option<String>,
    pub epistemic_state: EpistemicState,
    pub confidence: Option<f32>,
    pub sensitivity: Sensitivity,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
```

No `updated_at`, no `superseded_by_relation_id`, no fields not present
in V7. Derives `Debug, Clone, PartialEq, Serialize, Deserialize` —
matching `Fact`'s exact derive set.

## 6. `is_active_relation` (exact, `src/domain/mod.rs`)

```rust
pub(crate) fn is_active_relation(relation: &Relation) -> bool {
    relation.valid_until.is_none() && relation.epistemic_state != EpistemicState::Stale
}
```

Byte-for-byte the same shape as `is_active_fact`. `valid_until` is a
pure open/closed marker, never compared to wall-clock time. Proven by
the pure `req_iar_is_active_relation_matrix` test (A–D). Not called by
any production code this run (no `CorrectRelation`/`ConfirmRelation`
exist yet) — frozen future infrastructure, per R025 §16.

## 7. `get_relation` (exact)

`storage::personal_knowledge_repo::get_relation(conn, RelationId) ->
rusqlite::Result<Option<Relation>>`, exposed at `services::get_relation`.
No `get_relations_for_entity`/`get_active_relations_for_entity`/search —
explicitly out of scope (R025 §21).

## 8. `AddRelation` — exact payload/result

```rust
pub(crate) struct AddRelationPayload {
    pub from: EntityInputRef,
    pub to: EntityInputRef,
    pub relation_type: String,
    pub note: Option<String>,
    pub epistemic_state: EpistemicState,
    pub confidence: Option<f32>,
    pub sensitivity: Sensitivity,
    pub source: SourceRef,
    pub evidence: NewEvidenceData,
}
// -> ActionOutcome<Relation>
```

`EntityInputRef`/`SourceRef`/`NewEvidenceData` reused verbatim from the
Memory family — zero new parallel types. No `valid_from`/`valid_until`/
`RelationId`/`created_at`/`updated_at` input field; no inverse/symmetric
flag; no dedup flag.

## 9. Structural validation (`actions::validate_add_relation`)

Pure, no I/O. Reuses `validate_entity_input_ref`/`validate_source_ref`/
`validate_new_evidence_data`/`validate_confidence` unmodified —
zero Relation-specific copies. `relation_type` uses
`validate_pk_required_field` (trim, non-empty, `RELATION_TYPE_MAX_LEN =
100`, open vocabulary, no case-folding/synonym rewriting). `note` uses
`validate_pk_optional_field` (`RELATION_NOTE_MAX_LEN = 4000`, no trim, no
empty-to-`None`, free text preserved exactly). Same-payload duplicate
`domain_ref` between two `New` endpoints rejected here (`actions/tests.rs`:
`add_relation_same_payload_duplicate_domain_ref_rejected`).

One new `ValidationError` variant: `SelfPersonalKnowledgeRelation` —
deliberately its own variant, never `SelfGoalRelation` (a different
entity's error). A PURE self-edge fast path rejects
`SelfEntity`+`SelfEntity` and `Existing(id)`+`Existing(id)` (equal ids)
before any DB access; it cannot catch `SelfEntity` +
`Existing(actual SELF id)` — that needs resolution first.

## 10. Endpoint resolution

`services::add_relation` reuses `resolve_entity_input_ref`/
`resolve_source_ref` **unmodified** — both were already fully generic
over any `EntityInputRef`/`SourceRef`, not Fact-specific, so zero new
resolution code was needed. Both endpoints may be `New` simultaneously;
either may be `SelfEntity`; `Existing` is never mutated on reuse; `New`
validates `domain_ref` authoritatively and checks it against
already-persisted Entities before allocating an id in memory (rollback
if the surrounding transaction fails).

## 11. Self-edge defense (two layers)

1. **Pure** (`validate_add_relation`): catches structurally-provable
   cases only.
2. **Authoritative** (`services::add_relation`, after resolution):
   compares the two RESOLVED `EntityId`s. Catches every case, including
   `SelfEntity` + `Existing(actual SELF id)`
   (`req_ar_endpoint_i_self_to_existing_self_id_rejected_after_resolution`).
   Zero writes in both layers. The live DB `CHECK (from_entity_id <>
   to_entity_id)` remains defense-in-depth, never the first detector.

## 12. `domain_ref` handling

Authoritative existence (`Goal`/`Project`/`Milestone`/`Task`, reusing
`validate_domain_ref_authoritative` unmodified) and persisted-duplicate
detection (`find_entity_by_domain_ref`, unmodified) both reused exactly
as `AddMemory` uses them — zero new domain-ref code. Proven by
`req_ar_dup_domainref_same_payload_both_new_same_ref_rejected`/
`_persisted_on_from_rejected`/`_persisted_on_to_rejected`.

## 13. Source/Evidence lifecycle

`SourceRef::Existing` reused unchanged
(`req_ar_source_existing_reused_unchanged`); `SourceRef::New` created
atomically. Evidence is **always** fresh — two `AddRelation` calls
sharing one Source create two independent Evidence rows
(`req_ar_evidence_always_fresh_and_relation_evidence_link_defense`,
which also directly verifies the `relation_evidence` link's
`relation_id`/`evidence_id`/`created_at` and that exactly one link
exists per Relation).

## 14. `relation_type`/`note`/`confidence` policy — proven exactly as frozen

- `relation_type`: open vocabulary, `"works_on"`/arbitrary strings
  accepted, blank rejected, exact-`RELATION_TYPE_MAX_LEN` accepted,
  N+1 rejected (`req_ar_relation_type_*`).
- `note`: `None`/empty string/whitespace-only all accepted and preserved
  byte-for-byte (no trim, no normalization), exact-`RELATION_NOTE_MAX_LEN`
  accepted, N+1 rejected (`req_ar_note_*`).
- `confidence`: `None`/`0.0`/`1.0`/`0.5` accepted; `<0`, `>1`, `NaN`,
  `+Inf`, `-Inf` all rejected via the existing `ConfidenceOutOfBounds`
  (`req_ar_confidence_matrix`).

## 15. Confirmation ordering (matches the hardened `AddMemory` boundary)

1. Pure `validate_add_relation` (no SQL).
2. `effective_class(ExplicitAdd, meta.provenance)`.
3. Confirmation gate — blocks BEFORE any DB-dependent resolution.
4. Only once satisfied: `from`/`to`/`source` resolution, aggregating
   every reference error.
5. Authoritative self-edge check on resolved ids.
6. Any failure here → `ValidationFailed`, zero writes.
7. Transaction opens only after every check succeeds.

Proven directly by `req_ar_conf_gate_precedes_db_dependent_validation`
(nonexistent `Existing` from/to/source ids under `Inferred`+unconfirmed
still return `ConfirmationRequired`, never a validation error — the
DB-dependent stage was never reached) and
`req_ar_conf_d_structurally_invalid_inferred_unconfirmed_returns_validation_failed_before_gate`.
No PK-specific confirmation overlay is needed — `ExplicitAdd`
(severity 1) is already dominated by `InferredChange` (severity 2) in
the generic ladder, same reasoning as `AddMemory`
(`add_relation_inferred_provenance_effective_class_blocks_with_zero_writes_property`).

## 16. Inferred-confirmed epistemic rule

Identical to `AddMemory`: `Provenance::Inferred && meta.confirmed` forces
`epistemic_state = UserConfirmed` regardless of the payload's originally
proposed state; a direct `Explicit` call persists exactly the caller's
requested state
(`req_ar_conf_c_inferred_confirmed_persists_user_confirmed_and_audit_provenance_inferred`,
`req_ar_explicit_persists_payload_epistemic_state_unchanged`). Audit
`provenance` is stored exactly as given — never silently rewritten to
`Explicit` (verified against the raw `relation_audit_log` row).

## 17. `AddRelation` transaction order (exact)

New `from` Entity (if any) → new `to` Entity (if any) → new Source (if
`SourceRef::New`) → `Relation` → `Evidence` → `relation_evidence` link →
`relation_audit_log`. Any failure drops `tx` without commit; rusqlite
rolls back automatically.

## 18. Audit fields

One `relation_audit_log` row per successful `AddRelation`:
`action_type = "AddRelation"`, `previous_snapshot = NULL` (Create-shaped,
never the mandatory-snapshot helper), `occurred_at = meta.requested_at`.
Exactness proven with fixed `ActionMeta` timestamps in
`req_ar_audit_exact_explicit` (actor `CloudLLM`, provenance `Explicit`,
class `EXPLICIT_ADD`) and `req_ar_audit_exact_inferred_confirmed`
(`INFERRED_CHANGE`, `Relation.epistemic_state = USER_CONFIRMED`).

## 19. Rollback proofs

- **Mandatory** — `req_ar_forced_final_audit_failure_rolls_back_everything`:
  richest combination (New `from`, New `to`, New Source, fresh Evidence,
  Relation, `relation_evidence`), forced failure on the FINAL
  `relation_audit_log` insert via a `RAISE(FAIL, ...)` trigger. Every
  table snapshot is identical before/after; SELF survives; the
  connection remains usable for a subsequent successful call.
- **Earlier-stage** —
  `req_ar_earlier_stage_evidence_failure_rolls_back_entities_source_relation`:
  forces failure on the `evidence` insert (reusing the existing
  `install_evidence_insert_failure_trigger`, shared with the Fact
  family since `evidence` is a shared table); proves the already-created
  Entities/Source/Relation also roll back, not only the final step.

## 20. Duplicate-edge behavior

R025's "no Relation deduplication" is upheld exactly:
`req_ar_no_dedup_two_identical_edges_both_survive_distinct_ids` — two
identical `(from, relation_type, to)` `AddRelation` calls succeed, both
rows survive, distinct `RelationId`s. No `UNIQUE` constraint added.

## 21. No inverse/symmetric behavior

`req_ar_no_inverse_or_symmetric_auto_creation` — asserting `A
--works_on--> B` creates exactly one row; no `B --works_on--> A` or any
transformed inverse appears.

## 22. Timestamps

`valid_from = created_at = meta.requested_at` exactly (proven with a
fixed, non-`Utc::now()` timestamp in `req_ar_timestamps_exact_from_fixed_requested_at`);
`valid_until` always starts `None`
(`req_ar_no_temporal_side_effects_on_pre_existing_relation` further
proves a second `AddRelation` call never touches a pre-existing
identical Relation's own row).

## 23. Zero Fact/Episode side effects

`req_ar_zero_fact_episode_side_effects` — `facts`/`fact_evidence`/
`episodes`/`episode_evidence`/`episode_entities` row counts are
unchanged by `AddRelation`; only `entities`/`sources`/`evidence`/
`relations`/`relation_evidence`/`relation_audit_log` may change, and do
so exactly as expected.

## 24. Immutability proofs

`req_ar_existing_entity_and_source_immutable_after_success` (an
`Existing` Entity/Source used as an endpoint/source is byte-for-byte
unchanged after a successful call) and
`req_ar_self_entity_immutable_after_use_as_endpoint` (SELF specifically,
same guarantee).

## 25. Output/id proofs

`req_ar_relation_id_distinct_and_output_matches_get_relation_readback` —
two calls produce distinct `RelationId`s; the returned `Relation` exactly
equals a subsequent `get_relation` read-back (no partial/in-memory-only
representation).

## 26. Tests added

- `src/actions/tests.rs`: 11 new pure `validate_add_relation` tests
  (baseline accepted; generic-ladder property; pure self-edge
  fast-path×2 plus a non-over-rejection proof; same-payload duplicate
  domain-ref×2; `relation_type` blank/over-length; `note`
  over-length/empty/none; confidence out-of-bounds matrix).
- `src/services/tests.rs`: 50 new tests — 7 V8 migration proofs
  (`req_mig3`–`req_mig9`, covering A–M), 6 `get_relation` round-trip
  proofs (`req_pkrt3`–`req_pkrt8`), 1 `is_active_relation` matrix, and 36
  `AddRelation` service-level tests spanning the confirmation matrix,
  the full A–K endpoint matrix, duplicate-domain-ref (3), no-dedup,
  relation_type/note/confidence policy (7), Source/Evidence lifecycle
  (2), timestamps/audit exactness (3), rollback (2), immutability (2),
  zero-side-effects/no-temporal/no-inverse (3), and id/output proofs
  (1).
- `src/domain/mod.rs`/`src/services/mod.rs`/`personal_knowledge_repo.rs`:
  production code only, no test changes.

## 27. Final test count

- Previous: **400**
- New: **61** (11 in `actions::tests`, 50 in `services::tests`)
- Final: **461**
- Failed: **0**

## 28. Migration/build/package results (MEASURED)

- `cargo fmt --check`: clean (after one `cargo fmt` auto-format pass;
  re-verified clean).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **461 passed; 0 failed; 0 ignored**
  (15.40s).
- `cargo build --release`: succeeded (`Finished release profile
  [optimized] target(s) in 2m 03s`).
- `npm run tauri build` (repo root, `cargo` explicitly on `PATH` for the
  subprocess — the first attempt failed with `program not found` for
  `cargo metadata` because the invoking shell's `PATH` didn't carry
  `cargo`'s directory into the npm/Tauri child process; retried with
  `PATH` exported and succeeded): **succeeded**. Verified actual
  artifacts on disk:
  - `src-tauri/target/release/bundle/msi/LifeOS_0.1.0_x64_en-US.msi`
    (2,973,696 bytes)
  - `src-tauri/target/release/bundle/nsis/LifeOS_0.1.0_x64-setup.exe`
    (1,371,067 bytes)

## 29. Files changed

- `src-tauri/migrations/V8__create_relation_audit_log.sql` (new)
- `src-tauri/src/domain/mod.rs` (`Relation` struct, `is_active_relation`)
- `src-tauri/src/actions/mod.rs` (`AddRelationPayload`,
  `NormalizedAddRelation`, `validate_add_relation`,
  `ValidationError::SelfPersonalKnowledgeRelation`, two new domain
  constant imports)
- `src-tauri/src/services/mod.rs` (`get_relation`, `add_relation`,
  import updates)
- `src-tauri/src/services/storage/personal_knowledge_repo.rs`
  (`row_to_relation`, `insert_relation`, `get_relation`,
  `insert_relation_evidence_link`, `insert_relation_audit_log`)
- `src-tauri/src/actions/tests.rs` (+11 tests)
- `src-tauri/src/services/tests.rs` (+50 tests)

`git diff -- src-tauri/migrations/` confirms zero modified migration
files — V8 is the only change in that directory, added as a new file.
No `V9` exists. No DB files, temp DBs, `target/`/`dist/` artifacts, or
installer binaries are staged.

## 30. Remaining Relation work

`CorrectRelation`, `ConfirmRelation`, `ForgetRelation` — none started.
Episode actions and retrieval/Context Pack — not started. No Tauri
command wires `AddRelation` (or any Memory/Relation action) into the
running app yet.

## 31. Next report recommendation

**R027 — `CorrectRelation`**: the in-place, metadata-only correction
(`note`/`confidence` via `FieldPatch`, `sensitivity` via
`Option<Sensitivity>`, per R025's corrected contract), reusing
`memory_action_requires_confirmation` unmodified for its `ImportantChange`
floor overlay. Followed by R028 `ConfirmRelation`, then R029
`ForgetRelation` (built with `TransactionBehavior::Immediate` from the
start this time, per the lesson from `ForgetMemory`'s own hardening
pass).

---

## Post-run explicit frontend-build verification (2026-09-19)

Post-run review of R026 found that §28's measured results recorded
`npm run tauri build`'s success (which internally runs
`beforeBuildCommand` → the frontend `build` script → then bundles), but
never separately recorded a standalone, explicit run of the frontend
acceptance command itself (`npm.cmd run build`) independent of Tauri
packaging. This is a validation-completeness/acceptance-proof gap in
how R026 was recorded, not a suspected code defect — nothing about the
Rust/schema work is in question.

**Measured result:** `npm.cmd run build` was run explicitly, standalone,
from the repository root, with `cargo` on `PATH` (unrelated to this
script but kept consistent with how prior runs invoked npm in this
environment). Exit code **0**.

```
> lifeos@0.1.0 build
> tsc && vite build

vite v8.3.0 building client environment for production...
transforming...
✓ 27 modules transformed.
rendering chunks...
computing gzip size...
dist/index.html                   0.48 kB │ gzip:  0.31 kB
dist/assets/index-B2qJi2oL.css   13.85 kB │ gzip:  3.53 kB
dist/assets/index-BXGuczAb.js   231.60 kB │ gzip: 72.51 kB

✓ built in 130ms
```

`tsc` (TypeScript project-wide typecheck) and `vite build` both
succeeded. Verified on disk: `dist/index.html`, `dist/assets/index-B2qJi2oL.css`,
`dist/assets/index-BXGuczAb.js` all present with a fresh timestamp
matching this run.

**No production code, schema, or tests changed by this verification
pass** — it is a documentation-only addendum to this report. Test count
remains **461/461**. `V8` (`relation_audit_log` only) is unchanged.
`CorrectRelation`/`ConfirmRelation`/`ForgetRelation` remain
unimplemented; Episode/retrieval remain unstarted. The next substantial
report remains **R027**.
