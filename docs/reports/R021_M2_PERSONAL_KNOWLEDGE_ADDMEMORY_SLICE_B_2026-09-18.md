# R021 — M2 Personal Knowledge AddMemory Slice B

The first production Personal Knowledge **mutation**: `AddMemory`,
implemented exactly as R018 §4-§7/R019 §4-§7 froze it and R020 §24
recommended next. Creates a `Fact` (with a new-or-existing subject
`Entity`, an optional new-or-existing object `Entity`, a new-or-existing
`Source`, an always-new `Evidence`, the `fact_evidence` link, and one
`fact_audit_log` row) in a single atomic transaction. **No other Memory
action is implemented this run** — `CorrectMemory`/`ConfirmMemory`/
`ForgetMemory` remain entirely unstarted.

---

## 1. Pre-run state

- R017 (architecture freeze) → R018 (consistency hardening, 11 findings)
  → R019 (privacy/foundation finalization, 6 findings, architecture
  declared CLOSED for Slice A/B) → R020 (Slice A: `V7` schema, domain
  types, SELF bootstrap, narrow reads, `secure_delete` hardening) → R020
  post-run verification hardening (8 more tests, no code change). 293/293
  tests passing, working tree clean before this run. R001-R020 exist;
  this is **R021**, the next allocation.
- `AddMemory` did not exist in any form before this run — no override,
  no partial implementation, no stub.

---

## 2. Confirmation floor and semantics (R018 §3, restated)

`AddMemory`'s type floor is `ExplicitAdd` (severity 1). Unlike
`CorrectMemory` (`ImportantChange`, severity 3, which already dominates
`InferredChange`'s severity 2 and therefore needed a dedicated
Personal-Knowledge-specific override to force a pre-write block), the
*generic* `effective_class = max(type_floor, provenance_class)` formula
alone already produces the correct blocking behavior for `AddMemory`:
`Provenance::Inferred` → `provenance_class = InferredChange` (severity
2) → `effective_class = InferredChange` → `blocks_until_confirmed` is
`true`. **No override code was added to `add_memory`** — only a
dedicated test (`req_am_conf_b_inferred_unconfirmed_blocks_with_zero_writes`
in `services::tests`, and a pure-layer companion in `actions::tests`)
proving this frozen safety property holds, since it is safety-critical
even though it "falls out for free" from shared infrastructure.

`epistemic_state` persistence (R018 §5): an `Inferred`, now-confirmed
resubmission (`meta.provenance == Inferred && meta.confirmed`) always
persists `EpistemicState::UserConfirmed`, regardless of the payload's
originally-proposed `epistemic_state`. A direct `Explicit` call persists
exactly the caller's requested `epistemic_state` unchanged — the only
path that can produce a deliberate `HYPOTHESIS` (`memory_type`) +
`INFERRED` (`epistemic_state`) memory. `provenance` itself, stored in
`fact_audit_log`, is never rewritten by this override — confirmation and
provenance are distinct concepts (proven directly,
`req_am_audit_provenance_not_rewritten_when_inferred_confirmed`).

---

## 3. Frozen input types (R018 §4/R019 §4-§7, unchanged)

Added to `src-tauri/src/actions/mod.rs`: `EntityInputRef {SelfEntity,
Existing(EntityId), New(NewEntityData)}`, `NewEntityData {entity_type,
canonical_name, sensitivity, domain_ref: Option<DomainRef>}`,
`FactValueInput {ScalarJson(String), Entity(EntityInputRef)}`, `SourceRef
{Existing(SourceId), New(NewSourceData)}`, `NewSourceData {source_type,
label, reference, sensitivity, captured_at}`, `NewEvidenceData {content,
locator, sensitivity}`, `AddMemoryPayload {subject, predicate, value,
memory_type, epistemic_state, confidence, sensitivity, source,
evidence}`. Exactly the frozen shape — **no `valid_from`/`valid_until`/
`superseded_by_fact_id`/`created_at`/`updated_at`/`EvidenceId`/`FactId`
anywhere in the payload**; all Core-generated. No new public DTO exposing
internal Entity/Source/Evidence IDs was needed — the output type is
plain `ActionOutcome<Fact>`, matching every other Create* action.

---

## 4. Validation-before-write ordering

`actions::validate_add_memory` (pure, no I/O) runs first: predicate
(trim/non-empty/bound), subject structural validation, value structural
validation (`ScalarJson` length+JSON syntax, or `Entity` recursing into
the same structural rules), confidence (finite, `[0.0, 1.0]`), source
structural validation, evidence (bounds + at-least-one-of-content-
locator), then the same-payload duplicate-domain-ref preflight (two
`New` candidates, subject and/or object, sharing one `domain_ref`).
Every structural error is aggregated into one `Vec<ValidationError>`
before returning — no short-circuiting on the first failure.

`services::add_memory` then performs every database-dependent check,
also aggregated into one `Vec<ValidationError>` before any write: `Self`/
`Existing`/`New` resolution for subject and (if the value is
`FactValueInput::Entity`) object, `SourceRef` resolution, each
`New` Entity's `domain_ref` authoritative existence (§6) and persisted-
duplicate check (§7). **Nothing is written to any table until every
check above has succeeded.** Only then does the confirmation gate run,
and only after that does the transaction open (§2/§8).

---

## 5. Resolution helpers (`services/mod.rs`)

`resolve_entity_input_ref`/`resolve_source_ref` (private) each return
`Result<Option<Resolved{Entity,Source}Ref>, CoreError>`: `Ok(None)` means
one or more `ValidationError`s were pushed into the caller's shared
`errors` vec (continue aggregating); `Err` is a genuine storage failure
that aborts the whole action immediately. `ResolvedEntityRef`/
`ResolvedSourceRef` carry `id` (always populated) and `to_insert: Option<
Entity/Source>` — `Some` only for `New` (the caller inserts it inside the
transaction), `None` for `SelfEntity`/`Existing` (never mutated,
regardless of the rest of the payload — proven directly, §16).

---

## 6. Authoritative domain-ref validation (R021, resolving the open
question R020 §24 deferred)

`validate_domain_ref_authoritative` reuses each domain module's own
`*_exists` read — `goal_repo::goal_exists`, `project_repo::
project_exists`, and two new functions added this run,
`milestone_repo::milestone_exists`/`task_repo::task_exists` (exact
`project_exists` template: `SELECT count(*) FROM <table> WHERE id = ?1`).
**Personal Knowledge's own repo contains zero SQL against Goal/Project/
Milestone/Task tables.** The exact currently-supported `domain_ref_type`
set is **`Goal`/`Project`/`Milestone`/`Task`** — the minimum the brief
required; this does **not** close the open `domain_ref_type` vocabulary
at the schema level, it only limits what Core can validate today. An
unsupported type (e.g. `"Vehicle"`) → `UnsupportedDomainReferenceType`.
A supported type whose id fails to parse, or parses but matches no row,
both produce the same `ReferencedDomainObjectNotFound` — a deliberate
non-split (both are "cannot be validated as an existing X"; documented
in `ValidationError`'s own doc comment rather than invented as two
near-identical variants).

---

## 7. Duplicate domain-ref detection (two layers, R018 §9/R019 §9)

**Layer 1 (same-payload, pure):** `actions::validate_add_memory`
compares the normalized `domain_ref` of a `New` subject against a `New`
object's `domain_ref` (when both are present) — equal → `Duplicate
DomainReference`, before any SQL runs at all.

**Layer 2 (persisted, database-dependent):** a new repo read,
`storage::personal_knowledge_repo::find_entity_by_domain_ref(conn,
domain_ref_type, domain_ref_id) -> Option<Entity>`, checked for every
`New` candidate independently inside `resolve_entity_input_ref` — a hit
→ `DuplicateDomainReference`, zero writes. Proven across two *separate*
`add_memory` calls (`req_am_dup_persisted_duplicate_domain_ref_rejected_
across_two_calls_zero_writes`), not just within one payload.

The real `idx_entities_domain_ref` UNIQUE index (`V7`, unchanged)
remains defense-in-depth only — neither layer above relies on catching
its failure.

---

## 8. Atomic transaction (single `conn.transaction()`)

Insert order: new subject `Entity` (if any) → new object `Entity` (if
any) → new `Source` (if `SourceRef::New`) → `Fact` → `Evidence` →
`fact_evidence` link → `fact_audit_log`. Any failure at any step means
`tx` drops without a commit and rusqlite rolls back everything
automatically — proven for real via two forced-failure triggers, not
just code inspection: `req_am_rollback_forced_audit_failure_rolls_back_
everything` (the last step fails) and `req_am_rollback_earlier_stage_
forced_evidence_failure_rolls_back_new_entity_and_source` (an earlier
step fails) — both confirm **zero rows across every new table** (new
Entities, Source, Fact, Evidence, `fact_evidence`, `fact_audit_log`)
after rollback, that the pre-existing SELF entity is untouched, and that
the connection remains usable for a subsequent successful call
afterward.

---

## 9. Fact/audit construction

`Fact.valid_from`/`created_at`/`updated_at` are all set to `meta.
requested_at` (the canonical action timestamp, matching every existing
Create* action's convention); `valid_until = None`,
`superseded_by_fact_id = None` — no correction/supersession logic exists
in this action. Exactly one `fact_audit_log` row: `action_type =
"AddMemory"`, correct `fact_id`/`actor`/`provenance`/
`confirmation_class`, **`previous_snapshot` always `None`** (this
creates a Fact, it never updates one — `serialize_required_snapshot`,
the Update* actions' mandatory-snapshot helper, is never called here),
correct `occurred_at`. Proven directly
(`req_am_audit_row_fields_correct_and_previous_snapshot_always_null`).

---

## 10. Storage functions promoted from `#[cfg(test)]` to production

`insert_fact`/`insert_source`/`insert_evidence`/
`insert_fact_evidence_link` in `personal_knowledge_repo.rs` (all
previously test-fixture-only in R020, now real write paths) — their
bodies are unchanged, only the `#[cfg(test)]` gate (and, for
`insert_source`/`insert_evidence`, the now-production-needed
`Evidence`/`Source`/`chrono` imports' own `#[cfg(test)]` gates) was
removed. New functions: `find_entity_by_domain_ref` (§7),
`entity_exists`/`source_exists` (lightweight `count(*)` checks,
matching the `goal_exists`/`project_exists`/`milestone_exists`/
`task_exists` precedent rather than reusing the heavier `get_entity(...)
.is_some()` — nothing needs the full row, only existence), and
`insert_fact_audit_log` (the first production writer of
`fact_audit_log`). All remain `pub(in crate::services)`;
`services::storage` itself is still a private (non-`pub`) module —
unchanged, so the R020 E0603 module-privacy probe was not re-run (no
material visibility change occurred, only new functions at the same
existing visibility level).

---

## 11. New `ValidationError` variants

Ten new variants, inspecting the existing 16-variant enum first to avoid
duplication: `ReferencedEntityNotFound`, `ReferencedSourceNotFound`,
`DuplicateDomainReference`, `UnsupportedDomainReferenceType`,
`ReferencedDomainObjectNotFound`, `InvalidValueJson`,
`ConfidenceOutOfBounds {min, max, actual}`,
`EvidenceRequiresContentOrLocator`, and two **generic, field-
parameterized** variants — `PersonalKnowledgeFieldEmpty {field:
&'static str}` / `PersonalKnowledgeFieldTooLong {field, max, actual}` —
covering all eleven bounded Personal Knowledge free-text fields
(`entity_type`/`canonical_name`/`domain_ref_type`/`domain_ref_id`/
`predicate`/`value_json`/`source_type`/`label`/`reference`/`content`/
`locator`) instead of eleven near-identical `*Empty`/`*TooLong` variants
in the style `EmptyTitle`/`NoteTooLong` established. `field` names
exactly which one failed, so nothing is lost versus the per-field style
— a deliberate, documented deviation given the number of fields
involved. No bare strings anywhere; no personal payload content is ever
embedded in an error variant.

---

## 12. Field validation applied by `AddMemory`

Required (trim, reject-empty, bound; returns the normalized value):
`entity_type`, `canonical_name`, `domain_ref_type`/`domain_ref_id` (only
when `domain_ref` is `Some`), `predicate`, `source_type`. Optional
(bound only, no trim/emptiness requirement, matching
`validate_description`'s existing precedent that free text is preserved
exactly as supplied): `label`, `reference`, `content`, `locator`,
`value_json`. `value_json` accepts **any** JSON type
(string/number/bool/null/object/array) — never "scalar-only" despite the
field's name — checked only for length and `serde_json::from_str`
syntactic validity, never executed/evaluated, never silently altered
(stored byte-for-byte as supplied). All eleven constants are reused
unchanged from `domain::mod.rs` (R018 §15/R020 §5) — no new length
constant was added.

---

## 13. Confidence validation

`None`, or `Some(c)` with `c.is_finite() && (0.0..=1.0).contains(&c)`.
`is_finite()` is checked explicitly rather than relying on the range
comparison alone, since `NaN` compares `false` to every ordering
comparison and would otherwise silently pass a naive `c < 0.0 || c >
1.0` check. `+Infinity`/`-Infinity`/`NaN`/`-0.0001`/`1.0001` are all
rejected; `None`/`0.0`/`1.0`/`0.75` are all accepted — proven in both
`actions::tests` (pure) and exercised end-to-end through `services::
add_memory`.

---

## 14. Source and Evidence handling

`SourceRef::Existing` requires the row to already exist
(`ReferencedSourceNotFound` otherwise, zero writes) and is never
mutated on reuse (proven by full-field comparison before/after, §16).
`SourceRef::New` validates all fields and creates the row inside the
same transaction, preserving the supplied `captured_at` exactly (it may
legitimately predate `created_at`). `Evidence` is **always** newly
created — never reused across Facts, matching R018 §11 — with
independent `sensitivity`, never inherited from the Fact/Source/Entity
it is attached to. At least one of `content`/`locator` is required,
mirroring the `evidence` table's own DB `CHECK`.

---

## 15. Success-combination coverage (10 combinations, grouped)

Grouped into four table-driven-style test functions rather than ten
near-identical ones, per the explicit "table-driven, not one test per
trivial combination" instruction:
`req_am_success_self_subject_scalar_value_evidence_content_or_locator`
(Self subject + ScalarJson + New Source, Evidence as content-only and
locator-only); `req_am_success_entity_valued_facts_new_existing_and_
self_edge_objects` (new object with no `domain_ref`, an Existing object,
and a genuine self-edge — subject == object, both `Existing` referencing
the same `EntityId`, explicitly allowed, no invented restriction);
`req_am_success_subject_variants_existing_and_new_without_domain_ref`
(Existing subject, New subject with no `domain_ref`);
`req_am_success_domain_ref_backed_new_entities_both_roles_and_existing_
source` (subject `domain_ref = Goal` + object `domain_ref = Project` in
one call, subject `domain_ref = Milestone` + object `domain_ref = Task`
+ `SourceRef::Existing` reuse in a second call).

---

## 16. Not-found, duplicate, and immutability coverage

`req_am_not_found_existing_subject_object_source_rejected_zero_writes` —
a nonexistent `EntityId`/`SourceId` for subject, object, and source
respectively, each rejected with the correct typed error and zero
writes. `req_am_dref_matrix_supported_types_accepted_invalid_and_
unsupported_rejected` — one test proving, for all four supported types:
a real existing row accepted; a well-formed-but-nonexistent id rejected;
a syntactically invalid id rejected; plus one unsupported-type rejection
— zero writes across every rejected attempt.
`req_am_existing_entity_and_source_untouched_on_reuse` — full struct
equality (`Entity`, via `get_entity`) and full-field tuple equality
(`Source`, read via raw SQL since no production `get_source` exists — 
nothing else needs one) before and after a reuse call.

---

## 17. Audit and side-effect coverage

`req_am_audit_row_fields_correct_and_previous_snapshot_always_null`;
`req_am_audit_provenance_not_rewritten_when_inferred_confirmed` (§2/§9);
`req_am_no_side_effects_on_relation_episode_tables` — after a successful
`AddMemory`, `relations`/`episodes`/`relation_evidence`/
`episode_evidence`/`episode_entities` all remain at zero rows, proving
`AddMemory` never writes to any of the five schema-only tables.

---

## 18. No implicit dedup/correction/alias-merge (R021, explicit
negative proof)

`req_am_no_implicit_dedup_two_identical_calls_create_two_facts` — two
explicit, identical `AddMemory` calls create two distinct `Fact` rows;
no similarity detection, no auto-`STALE` transition, no correction.
`req_am_no_entity_alias_merge_same_canonical_name_creates_two_entities`
— two `New` Entities sharing a `canonical_name` (no `domain_ref`) create
two distinct `Entity` rows; no name-based alias merging.

---

## 19. Direct action-layer unit tests (`actions::tests`)

Twelve pure, no-I/O tests directly against `validate_add_memory` and
`effective_class`/`blocks_until_confirmed`: baseline-accepted,
confidence accepted/rejected value sets, JSON accepted-types/rejected
malformed-and-overlong, evidence-requires-content-or-locator, required-
field-empty, required-and-optional field bounds (table-driven over
`entity_type`/`canonical_name`/`predicate`/`source_type`/`content`),
same-payload duplicate-domain-ref, self-edge structural allowance,
predicate trimming, and the dedicated `effective_class`/
`blocks_until_confirmed` safety-property test (§2). Matches the existing
`actions::tests` precedent of testing pure validation directly rather
than only indirectly through `services`-level integration tests.

---

## 20. Exact total test count

**Previous: 293. New: 31 (12 in `actions::tests`, 19 in
`services::tests`). Final: 324. Failed: 0.** Confirmed by running the
full suite (`cargo test -- --test-threads=1`) both immediately before
this run's first edit and after every change, not estimated.

---

## 21. Build/package results

- `cargo fmt --check`: clean (after one `cargo fmt` pass — recorded for
  completeness).
- `cargo check --tests`: clean, zero warnings, on every intermediate
  edit as well as the final state.
- `cargo test -- --test-threads=1`: **324 passed; 0 failed; 0 ignored**.
- `cargo build --release`: succeeded (2m10s).
- `npm run build`: succeeded.
- `npm run tauri build`: run this pass (required — this run adds a new
  production Core action and a new storage mutation path) — see the
  chat reply for the measured MSI/NSIS result, verified from the
  captured log file directly, not the wrapper's exit code alone. No
  Tauri command was added this run (`AddMemory` is Core-only, no UI/IPC
  wiring) — this build only proves packaging/regression.

---

## 22. Files changed

- `src-tauri/src/actions/mod.rs` — `EntityInputRef`/`NewEntityData`/
  `FactValueInput`/`SourceRef`/`NewSourceData`/`NewEvidenceData`/
  `AddMemoryPayload`/`NormalizedAddMemory`, ten new `ValidationError`
  variants (§11), `validate_add_memory` and its private helpers
  (`validate_pk_required_field`/`validate_pk_optional_field`/
  `validate_entity_input_ref`/`validate_fact_value_input`/
  `validate_source_ref`/`validate_new_evidence_data`/
  `validate_confidence`), new imports.
- `src-tauri/src/actions/tests.rs` — 12 new direct unit tests (§19).
- `src-tauri/src/services/mod.rs` — `add_memory` (the action), private
  `resolve_entity_input_ref`/`resolve_source_ref`/
  `validate_domain_ref_authoritative` helpers, `ResolvedEntityRef`/
  `ResolvedSourceRef`, new imports (`AddMemoryPayload`/
  `EntityInputRef`/`FactValueInput`/`Provenance`/`SourceRef` from
  `actions`; `EpistemicState`/`Evidence`/`EvidenceId`/`FactValue`/
  `Source`/`SourceId` from `domain`).
- `src-tauri/src/services/storage/milestone_repo.rs` — new
  `milestone_exists`.
- `src-tauri/src/services/storage/task_repo.rs` — new `task_exists`.
- `src-tauri/src/services/storage/personal_knowledge_repo.rs` —
  `insert_fact`/`insert_source`/`insert_evidence`/
  `insert_fact_evidence_link` promoted to production; new
  `find_entity_by_domain_ref`/`entity_exists`/`source_exists`/
  `insert_fact_audit_log`; updated module doc.
- `src-tauri/src/services/tests.rs` — 19 new `AddMemory` integration
  tests (§15-§18), new fixtures/helpers, new imports.
- `docs/reports/R021_M2_PERSONAL_KNOWLEDGE_ADDMEMORY_SLICE_B_2026-09-18.md`
  (this report).
- `docs/PROJECT_STATE.md`/`docs/LIFEOS_SPEC.md` — updated (see chat
  reply/commit for detail).

**`V7__create_personal_knowledge.sql` was not modified** — Slice A's
schema was already sufficient for everything Slice B needed; no V8 was
created.

---

## 23. Known problems / risks / unresolved questions

- `ReferencedEntityNotFound`/`DuplicateDomainReference`/
  `PersonalKnowledgeFieldEmpty`/`PersonalKnowledgeFieldTooLong` do not
  distinguish *which role* (subject vs. object, or which of the two New
  candidates) produced the error when both happen to fail the same way
  in one call — matching the existing minimalist precedent
  (`ReferencedGoalNotFound` etc. also carry no extra context), but
  worth flagging explicitly as a known granularity limit rather than
  silently accepting it. Not fixed this run — would require a larger,
  not-yet-justified error-context redesign across the whole
  `ValidationError` enum, out of scope for one action.
- No `get_source` production read function exists — `source_exists`
  covers everything `AddMemory` itself needs; the immutability test
  reads `Source` fields via raw SQL instead. If a future slice needs a
  real `get_source`, add it then rather than now.
- `CorrectMemory`/`ConfirmMemory`/`ForgetMemory` remain unimplemented —
  Facts created by `AddMemory` can never yet be corrected, confirmed
  from a lower epistemic state, or deleted through the action layer.

---

## 24. Remaining Personal Knowledge work

Slice C (`CorrectMemory`), D (`ConfirmMemory`), E (`ForgetMemory`'s full
14-step deletion algorithm, R019 §10), F (`Relation` actions), G
(`Episode` actions), H (retrieval/Context Pack) all remain entirely
unstarted, unchanged from R019 §29/R020 §23's recommended order.
`Relation`/`Episode` and their four join tables remain schema-only — no
production write path exists for any of them; `AddMemory` does not
touch them (§17).

---

## 25. Exact recommendation for the next slice

**STOP after Slice B — `CorrectMemory` was explicitly not started this
run**, per the run's own instruction. The correct next step is Slice C
(`CorrectMemory`): the first Memory action to target an existing `Fact`,
requiring the `ImportantChange`-floor Personal-Knowledge-specific
confirmation override R018 §6 already froze (unlike `AddMemory`, its
generic `effective_class` alone does *not* force a block for
`Provenance::Inferred` — `ImportantChange`, severity 3, already
dominates `InferredChange`, severity 2), plus `serialize_required_
snapshot`-backed `previous_snapshot` population (this is an update, not
a create) and the supersession-vs-in-place-correction distinction R018/
R019 froze.

---

## Post-run correctness and verification hardening (2026-09-18)

A small, deliberate pass conducted before Slice C, prompted by a review
of `services::add_memory`'s actual execution order against the intended
`AddMemory` safety boundary. Found and corrected **one genuine ordering
deviation** (not a data-integrity defect — no write ever happened before
confirmation either way) and closed several verification gaps in R021's
own test coverage. **Does not create R022** — appended here as
instructed; the original §1-§25 above are otherwise unedited.

### Confirmation-gate ordering correction

**Before this pass**, `add_memory` ran: pure structural validation →
subject/object/source database-dependent resolution (Existing-existence,
domain-ref authoritative existence, persisted-duplicate-domain-ref) →
confirmation gate → transaction. An inferred, unconfirmed proposal could
therefore cause real database reads against Personal Knowledge and
authoritative domain tables (Goal/Project/Milestone/Task) before the
user had approved anything — never a write, but a semantically wrong
place for those reads to happen relative to the safety boundary the
confirmation gate exists to enforce.

**After this pass**, the order is corrected to:

1. pure structural validation (`actions::validate_add_memory` — no SQL,
   no mutation, no external reads)
2. `effective_class`/`blocks_until_confirmed` confirmation-class
   calculation
3. the blocking confirmation gate itself — `ConfirmationRequired`
   returned here, before step 4, for any inferred/unconfirmed call
4. database-dependent validation/resolution (subject/object/source
   existence, domain-ref authoritative existence, persisted-duplicate-
   domain-ref)
5. zero-write validation result (`ValidationFailed` if step 4 found
   anything)
6. the atomic transaction

Implemented as a straightforward reorder inside `services::add_memory`
— the confirmation-gate block now runs immediately after `normalized =
validate_add_memory(&payload)` succeeds, before `resolve_entity_input_
ref`/`resolve_source_ref` are ever called. No change to `actions::
validate_add_memory` itself (pure structural validation correctly stays
*before* the gate, per this pass's own explicit instruction — rejecting
a malformed payload before asking for confirmation is desired, not a
deviation). No change to `effective_class`/`blocks_until_confirmed`
themselves, and no Personal-Knowledge-specific override was added — the
existing generic formula still does all the work (R021 §2, unchanged).
The function's own doc comment was updated to describe the corrected
order and explain why it is deliberately this way round.

**New test**, `req_am_conf_gate_precedes_db_dependent_validation`:
an unconfirmed `Inferred` call with a *structurally valid* payload whose
`subject`/`source` are each `Existing(<nonexistent id>)` (tried
independently) now returns `ConfirmationRequired { InferredChange }` in
both cases — never `ValidationFailed([ReferencedEntityNotFound])` or
`ValidationFailed([ReferencedSourceNotFound])` — proving the database-
dependent stage is never reached at all while unconfirmed. All six
`AddMemory`-mutated table counts (`entities`/`sources`/`evidence`/
`facts`/`fact_evidence`/`fact_audit_log`) confirmed unchanged
afterward.

**Explicit-provenance behavior is unchanged and re-confirmed**: for a
direct `Explicit` call (which never blocks at `AddMemory`'s
`ExplicitAdd` floor), the same nonexistent `Existing` references still
reach database-dependent resolution and still return the correct typed
`ValidationError` — `req_am_not_found_existing_subject_object_source_
rejected_zero_writes` (pre-existing, unmodified) remains green,
confirming normal validation was not weakened by the reorder.

### Action-level length-bound completeness

R021's original `add_memory_required_and_optional_field_bounds_enforced`
covered only 4 of `AddMemory`'s 11 bounded fields, and only at `N+1`
(never proving `N`, the exact maximum, is actually *accepted*). A new
test, `add_memory_all_bounded_fields_exact_max_accepted_and_over_max_
rejected`, drives `validate_add_memory` directly (not the generic
`validate_personal_knowledge_text_len` helper in isolation) for all
eleven bounded fields — `entity_type`/`canonical_name`/
`domain_ref_type`/`domain_ref_id` (`NewEntityData`), `predicate`/
`value_json` (`Fact`), `source_type`/`label`/`reference`
(`NewSourceData`), `content`/`locator` (`NewEvidenceData`) — proving
the exact frozen maximum is structurally accepted and one character more
is rejected with the correct `field`/`max`, for every one. `value_json`
is handled as its own special case: the total JSON string, *including*
its wrapping quote characters, is constructed to be exactly
`VALUE_JSON_MAX_LEN`/`VALUE_JSON_MAX_LEN + 1` characters while remaining
syntactically valid JSON (`"` + `x` × (`VALUE_JSON_MAX_LEN - 2`) + `"`).
`domain_ref_type`/`domain_ref_id` are proven as pure structural bounds
only (a trivial well-formed companion value on the other side of the
pair) — authoritative domain-ref *existence* remains a `services::tests`
concern (R021 §6/§15), not retested here. The original narrower test was
kept alongside (not deleted) since it still adds coverage of the
`content`-and-`source_type`-together interaction it originally proved.

### Audit `occurred_at` direct proof

`req_am_audit_row_fields_correct_and_previous_snapshot_always_null` now
constructs `ActionMeta` with a deliberately fixed, non-`Utc::now()`
`requested_at` (`"2026-09-18T12:34:56Z"`) instead of `ui_explicit()`'s
call-time timestamp, `SELECT`s `fact_audit_log.occurred_at` in addition
to the fields already checked, and asserts it equals `requested_at.
to_rfc3339()` exactly — the project's canonical RFC3339 timestamp
representation, matching `insert_fact_audit_log`'s own
`occurred_at.to_rfc3339()` call. `previous_snapshot IS NULL` remains
asserted, unchanged. No audit schema change.

### Duplicate-domain-ref: full zero-write proof, both layers

**Same-payload layer** (previously proven only at the pure-validator
level, `actions::tests::add_memory_same_payload_duplicate_domain_ref_
rejected_subject_and_object`): a new service-level test,
`req_am_dup_same_payload_full_zero_write_proof`, calls `add_memory`
itself with a subject `New` + object `New` payload sharing one
`domain_ref`, confirms `ValidationFailed([DuplicateDomainReference])`,
and confirms all six `AddMemory`-mutated table counts plus the SELF
entity's own id are completely unchanged from immediately before the
call — proving the full action respects the preflight, not only that
the pure validator function itself does.

**Persisted (cross-call) layer**: `req_am_dup_persisted_duplicate_
domain_ref_rejected_across_two_calls_zero_writes` previously snapshotted
only `entities`/`facts` row counts around the rejected second call. A
new shared helper, `am_full_table_snapshot`, returns
`(entities, sources, evidence, facts, fact_evidence, fact_audit_log)` in
one call; the test now asserts this full six-table tuple (plus the SELF
entity's own id) is byte-for-byte unchanged by the rejected second call,
while the first call's own successful writes remain the — now
explicitly captured — baseline.

### Exact new test count

**Previous (R021 original): 324. New this pass: 3
(`req_am_conf_gate_precedes_db_dependent_validation`,
`add_memory_all_bounded_fields_exact_max_accepted_and_over_max_rejected`,
`req_am_dup_same_payload_full_zero_write_proof` — the audit-`occurred_
at` and cross-call-duplicate strengthenings extended two PRE-EXISTING
tests rather than adding new ones). Final: 327. Failed: 0.** The
original R021 §20 result — **293 → 324 (+31)** — remains this report's
unedited historical record; **327** is this pass's own separately-
reported measured result, not a retroactive edit to §20.

### Build/package results (this pass)

- `cargo fmt --check`: clean (after one `cargo fmt` pass).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **327 passed; 0 failed; 0 ignored**.
- `cargo build --release`: succeeded (2m37s).
- `npm run build`: succeeded.
- `npm run tauri build`: run this pass (required — production
  `AddMemory` execution order changed) — see the chat reply for the
  measured MSI/NSIS result, verified from the captured log and the
  actual bundle files on disk, not the wrapper's exit code alone.

### Files changed (this pass)

- `src-tauri/src/services/mod.rs` — `add_memory`'s confirmation-gate
  reordered ahead of database-dependent resolution; doc comment updated
  to match.
- `src-tauri/src/services/tests.rs` — 1 new test
  (`req_am_conf_gate_precedes_db_dependent_validation`), 1 new test
  (`req_am_dup_same_payload_full_zero_write_proof`), 1 new shared helper
  (`am_full_table_snapshot`), 2 existing tests strengthened
  (`req_am_audit_row_fields_correct_and_previous_snapshot_always_null`,
  `req_am_dup_persisted_duplicate_domain_ref_rejected_across_two_calls_
  zero_writes`).
- `src-tauri/src/actions/tests.rs` — 1 new test
  (`add_memory_all_bounded_fields_exact_max_accepted_and_over_max_
  rejected`); the original narrower bounds test was kept, not removed.
- `docs/reports/R021_M2_PERSONAL_KNOWLEDGE_ADDMEMORY_SLICE_B_2026-09-18.md`
  (this addendum).

**No `V7` migration change. No V8 created. No `CorrectMemory`/
`ConfirmMemory`/`ForgetMemory` code. No UI/Tauri command changes. No
retrieval/Qwen/Luna work.** `docs/PROJECT_STATE.md`/`docs/LIFEOS_SPEC.md`
were reviewed for wording describing `AddMemory`'s internal confirmation-
vs-database-dependent-validation ordering; neither document made a
specific claim about that ordering, so neither required a correction —
left unedited by this pass.

### Next substantial report

Unchanged: **R022**, allocated to whichever of Slice C (`CorrectMemory`)
or another explicitly-instructed run comes next. `CorrectMemory` was
**not** started by this pass.
