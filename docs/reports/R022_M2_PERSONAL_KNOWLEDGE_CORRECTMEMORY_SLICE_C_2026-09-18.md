# R022 — M2 Personal Knowledge CorrectMemory Slice C

The second production Personal Knowledge mutation: `CorrectMemory`,
implemented exactly as the frozen R018 payload and R021's corrected
safety-boundary ordering require. "The retained semantic knowledge is
wrong/outdated; replace it while preserving history" — never an in-place
semantic edit. One active predecessor `Fact` becomes historical
(`Stale`); one new successor `Fact` becomes active. **No other Memory
action is implemented this run** — `ConfirmMemory`/`ForgetMemory` remain
entirely unstarted.

---

## 1. Pre-run state

R017→R018→R019 (architecture freeze, then two rounds of hardening,
architecture CLOSED for Slice A/B) → R020 (Slice A: `V7` schema, SELF
bootstrap, narrow reads, `secure_delete`) → R021 (Slice B: `AddMemory`) →
R021's own post-run correctness/verification addendum (confirmation
gate moved ahead of database-dependent resolution; 11-field exact-bound
proof; audit `occurred_at` proof; both duplicate-domain-ref layers given
full zero-write proof). 327/327 tests passing, working tree clean before
this run. `CorrectMemory` did not exist in any form before this run.

---

## 2. Exact `CorrectMemory` payload

```rust
pub(crate) struct CorrectMemoryPayload {
    pub predecessor_fact_id: FactId,
    pub value: FactValueInput,
    pub memory_type: Option<MemoryType>,
    pub confidence: FieldPatch<f32>,
    pub sensitivity: Option<Sensitivity>,
    pub source: SourceRef,
    pub evidence: NewEvidenceData,
}
```

No `subject`/`predicate`/`epistemic_state`/`valid_from`/`valid_until`/
`superseded_by_fact_id`/successor `FactId`/`EvidenceId`/`created_at`/
`updated_at` — all Core-generated or inherited from the predecessor.
Changing subject or predicate is `AddMemory` for a different assertion,
never `CorrectMemory` — the payload has structurally no way to express
either.

---

## 3. `ImportantChange` floor

Type floor is `ConfirmationClass::ImportantChange` (severity 3). For
`Provenance::Explicit`, `ImportantChange` is not a blocking-before-write
class under the existing generic policy — execution proceeds
immediately, `ActionOutcome::Success` carries `confirmation_class =
ImportantChange`, and the mutation is always audited (matching every
other `ImportantChange` action's existing convention).
`ConfirmationClass`/`effective_class`/`blocks_until_confirmed` are
completely unchanged by this run.

---

## 4. Personal-Knowledge inferred confirmation overlay (R018 §6)

`effective_class(ImportantChange, Inferred)` stays `ImportantChange`
(severity 3 already dominates `InferredChange`'s severity 2) — the
generic ladder alone does **not** block an inferred, unconfirmed
correction the way it does for `AddMemory`'s lower `ExplicitAdd` floor.
A new, narrow private helper in `services/mod.rs` restores the required
block without touching `ConfirmationClass`/`effective_class`/
`blocks_until_confirmed`:

```rust
fn memory_action_requires_confirmation(meta: &ActionMeta, class: ConfirmationClass) -> bool {
    if meta.confirmed {
        return false;
    }
    blocks_until_confirmed(class) || meta.provenance == Provenance::Inferred
}
```

`class` returned to the caller as `confirmation_class` stays
`ImportantChange` regardless of provenance — only the blocking decision
differs from the generic ladder's own verdict. Used only by
`correct_memory`; `add_memory`'s own gate (`blocks_until_confirmed(class)
&& !meta.confirmed`, already correct for its floor) was left untouched —
a shared refactor was considered and rejected as unnecessary risk for
zero behavioral benefit (the two expressions are provably equivalent for
`AddMemory`'s floor, but touching tested, working code for no gain
contradicts this run's own "no `AddMemory` regression" instruction).

---

## 5. Exact execution ordering

1. Pure structural validation (`actions::validate_correct_memory` — no
   SQL, no mutation, no external reads).
2. `effective_class(ImportantChange, meta.provenance)`.
3. `memory_action_requires_confirmation` (§4) — the Personal-Knowledge
   overlay.
4. If blocking → `ActionOutcome::ConfirmationRequired`.
5. Only once confirmation is satisfied: read the predecessor
   (`storage::personal_knowledge_repo::get_fact`); if missing →
   `ActionOutcome::NotFound` immediately (matches every Update* action's
   existing convention for a missing target); if found, check
   `domain::is_active_fact` (§6); resolve the value's object `Entity`
   (if `FactValueInput::Entity`) and the `Source`, aggregating every
   reference error found (matches `AddMemory`'s aggregation style)
   rather than stopping at the first.
6. Any aggregated error → `ActionOutcome::ValidationFailed`.
7. The mandatory `previous_snapshot` of the predecessor's PRE-correction
   state is serialized (`serialize_required_snapshot`, never
   `.ok()`-downgraded) — `StorageError` and zero writes if this fails.
8. The atomic transaction opens.

Proven directly: `req_cm_conf_b_inferred_unconfirmed_blocks_before_
predecessor_lookup` uses a **nonexistent** `predecessor_fact_id` with
`Inferred`+unconfirmed and asserts `ConfirmationRequired` (never
`NotFound`) — proving the predecessor is never even fetched while
unconfirmed. `req_cm_conf_d_structurally_invalid_payload_returns_
validation_failed_before_gate` proves pure validation still precedes the
gate (matches R021's corrected boundary, not a regression of it).

---

## 6. Canonical `is_active_fact`

Added to `domain/mod.rs`, directly beside `Fact`, reusable by a future
`ConfirmMemory` (Slice D):

```rust
pub(crate) fn is_active_fact(fact: &Fact) -> bool {
    fact.valid_until.is_none() && fact.epistemic_state != EpistemicState::Stale
}
```

Both conditions are checked — not `valid_until` alone. Proven by the
full four-case matrix, `req_cm_active_predecessor_matrix`: (A) a normal
active predecessor accepted; (B) an already-corrected (now `Stale`)
predecessor rejected; (C) a raw fixture with `epistemic_state = STALE`
but `valid_until = NULL` rejected (the deliberate inconsistent-state
case); (D) a raw fixture with `epistemic_state != STALE` but
`valid_until != NULL` rejected. Zero writes on every rejection.

---

## 7. Predecessor validation

An inactive predecessor produces a new typed variant,
`ValidationError::FactNotActive` — no revival of a historical Fact is
possible. A missing predecessor produces `ActionOutcome::NotFound`,
matching `update_goal`/`update_project`/`update_milestone`/`update_task`'s
existing precedent for a missing update target, rather than inventing a
new error solely for this case.

---

## 8. `FactValueInput` handling

`CorrectMemory` reuses `actions::validate_fact_value_input`/
`validate_entity_input_ref` and `services::resolve_entity_input_ref`
completely unchanged from `AddMemory` — zero duplicated logic. Success
proven for all five required shapes, each needing its own fresh
predecessor (a predecessor becomes inactive after one correction):
`ScalarJson`, `Entity(SelfEntity)`, `Entity(Existing)`, `Entity(New`
without `domain_ref)`, `Entity(New` with a valid `domain_ref = Goal)`.
Rejection proven for malformed JSON, overlong JSON, a missing `Existing`
Entity, a nonexistent authoritative domain object, and a persisted-
duplicate `domain_ref` — all against one reused predecessor (every
attempt is rejected, so it is never consumed), with the predecessor
proven byte-for-byte unchanged at the end. Currently-supported
`domain_ref_type` set is unchanged from R021: **`Goal`/`Project`/
`Milestone`/`Task`** — no new type added this run (the full four-type
matrix is already exhaustively proven against the shared validation
function by `AddMemory`'s own tests; re-proving it here would be pure
duplication).

---

## 9. Source/Evidence behavior

`SourceRef` reuses `validate_source_ref`/`resolve_source_ref` unchanged.
`Existing` must exist and is never mutated (proven,
`req_cm_existing_entity_and_source_untouched_on_reuse`); `New` is
validated and created inside the same transaction, `created_at =
meta.requested_at`, `captured_at` preserved exactly as supplied. Every
correction creates a **brand-new** `Evidence` row for the successor —
never reused. The predecessor's original `Evidence`/`fact_evidence`
link/`Source` remain completely untouched and queryable
(`req_cm_predecessor_evidence_and_source_remain_linked_successor_gets_
new_evidence`): the successor's evidence link is a distinct row from
the predecessor's, `evidence`/`fact_evidence` row counts are exactly 2
after one correction (one per Fact), and no sharing exists.

---

## 10. `memory_type` inheritance/override

`None` → successor inherits `predecessor.memory_type` exactly; `Some(x)`
→ successor's `memory_type = x`, predecessor's own `memory_type` never
changes either way (it is not one of the four frozen predecessor-
mutable fields). No `FieldPatch` was introduced for `memory_type` — the
column is non-nullable, matching the frozen contract precisely. Proven
directly, `req_cm_memory_type_sensitivity_confidence_inherit_and_
override`.

---

## 11. `confidence` `FieldPatch` semantics

`Unchanged` → successor inherits `predecessor.confidence` exactly;
`Set(v)` → successor's `confidence = Some(v)` (bound-checked: finite,
`[0.0, 1.0]`, `NaN`/`±Infinity`/out-of-range rejected via
`validate_confidence_patch`, sharing the exact bound check `AddMemory`'s
`validate_confidence` uses — `confidence_out_of_bounds`, extracted once,
used by both); `Clear` → successor's `confidence = None`. Predecessor's
own `confidence` never changes under any patch mode — proven by the
dedicated three-case matrix, `req_cm_confidence_fieldpatch_matrix_
unchanged_set_clear`, and the rejected-`Set`-value pure tests in
`actions::tests` (`-0.1`/`1.1`/`NaN`/`+Infinity`/`-Infinity`).

---

## 12. `sensitivity` inheritance/override

`None` → successor inherits `predecessor.sensitivity` exactly; `Some(x)`
→ successor's `sensitivity = x`; predecessor's own `sensitivity` never
changes either way. No `Clear` exists — the column is non-nullable.
Proven directly alongside `memory_type` in the same inherit/override
test (§10).

---

## 13. Successor construction

```rust
Fact {
    id: FactId::new(),
    subject_entity_id: predecessor.subject_entity_id,
    predicate: predecessor.predicate.clone(),
    value: <resolved from FactValueInput>,
    memory_type: normalized.memory_type.unwrap_or(predecessor.memory_type),
    epistemic_state: EpistemicState::UserConfirmed,
    confidence: <FieldPatch applied to predecessor.confidence>,
    sensitivity: normalized.sensitivity.unwrap_or(predecessor.sensitivity),
    valid_from: now,
    valid_until: None,
    superseded_by_fact_id: None,
    created_at: now,
    updated_at: now,
}
```

`epistemic_state` is unconditionally `UserConfirmed` — every successful
`CorrectMemory` produces it, with no v0.1 path to `INFERRED`/
`OBSERVED`/`DISPUTED`/`STALE` through this action (R018), including a
confirmed `Inferred` correction (`req_cm_conf_c_inferred_confirmed_
executes_and_audit_provenance_remains_inferred`). No semantic field on
the predecessor is ever read-then-rewritten to build the successor —
only copied where inheritance applies.

---

## 14. Predecessor update

A new, deliberately narrow repo function,
`storage::personal_knowledge_repo::update_fact_as_superseded(conn,
predecessor_fact_id, successor_fact_id, correction_timestamp)`, is the
*only* path that can ever mutate an existing `facts` row. Its SQL
touches exactly the four frozen-allowed fields:

```sql
UPDATE facts SET valid_until = ?2, superseded_by_fact_id = ?3,
                  epistemic_state = 'STALE', updated_at = ?4
WHERE id = ?1
```

No generic `update_fact`/arbitrary patch function exists — the storage
API makes the frozen mutability contract structurally hard to violate,
not merely documented. `debug_assert_eq!(changed, 1, ...)` matches the
existing `update_project`/`update_project_progress` precedent exactly.
Proven via full struct comparison before/after
(`req_cm_successor_and_predecessor_full_field_proof_no_gap_no_overlap`):
`id`/`subject_entity_id`/`predicate`/`value`/`memory_type`/`confidence`/
`sensitivity`/`valid_from`/`created_at` are asserted identical;
`valid_until`/`superseded_by_fact_id`/`epistemic_state`/`updated_at` are
asserted to have changed, to the exact expected new values.

---

## 15. Temporal no-gap/no-overlap proof

Using a deliberately fixed `ActionMeta.requested_at` (not `Utc::now()`)
for both the predecessor's creation and the correction call, the same
test above proves:

```
predecessor.valid_until == successor.valid_from == meta.requested_at (t2)
```

exactly — no gap, no overlap. `successor.superseded_by_fact_id == None`.

---

## 16. Supersession-chain semantics

`predecessor.superseded_by_fact_id == Some(successor.id)` after success;
the predecessor row is never deleted, never rewritten beyond its four
allowed fields. `req_cm_correction_chain_a_to_b_to_c` proves a full
A→B→C chain: A becomes `Stale` (`valid_until = t1`, `superseded_by =
B`); B becomes `Stale` in turn (`valid_from = t1`, `valid_until = t2`,
`superseded_by = C`); C remains active (`UserConfirmed`, `valid_from =
t2`, `valid_until = None`, `superseded_by = None`). Attempting
`CorrectMemory(A)` or `CorrectMemory(B)` again is rejected with
`FactNotActive` — history forms a chain, never gets overwritten, and no
intermediate Fact can be skipped or an older ancestor mutated.

---

## 17. Mandatory `previous_snapshot`

`serialize_required_snapshot(&predecessor)` runs after all validation/
confirmation succeeds but strictly before the transaction opens — never
`.ok()`-downgraded; a serialization failure returns `StorageError` with
zero writes (the same helper `AddMemory`'s Update*-action precedent
already established, reused unchanged). Proven to deserialize exactly
back to the predecessor's PRE-correction state:
`req_cm_previous_snapshot_mandatory_and_deserializes_exactly_to_pre_
correction_predecessor` captures the predecessor `Fact` before calling
`correct_memory`, then `serde_json::from_str::<Fact>` on the persisted
`previous_snapshot` column and asserts full equality against that
captured pre-state.

---

## 18. Audit `fact_id` rationale (predecessor id)

The single `fact_audit_log` row `CorrectMemory` inserts uses the
**predecessor's** `FactId`, never the successor's — the row records the
mutation of the targeted predecessor and carries that predecessor's
`previous_snapshot`, preserving the frozen future `ForgetMemory` rule
that it can scrub `fact_audit_log.previous_snapshot WHERE fact_id =
<target>` before deleting that exact Fact. Proven directly,
`req_cm_audit_fact_id_is_predecessor_id_with_correct_fields`:
`fact_id == predecessor.id` (and explicitly `!= successor.id`),
`action_type == "CorrectMemory"`, `confirmation_class ==
"IMPORTANT_CHANGE"`. `provenance` is never rewritten — a confirmed
`Inferred` correction's audit row keeps `provenance = "Inferred"` while
the successor's `epistemic_state` is still `USER_CONFIRMED`
(confirmation and provenance remain distinct concepts, matching
`AddMemory`'s identical guarantee).

---

## 19. Atomic transaction order

New object `Entity` (if `FactValueInput::Entity(New(..))`) → new
`Source` (if `SourceRef::New`) → successor `Fact` → new `Evidence` →
successor's `fact_evidence` link → predecessor update
(`update_fact_as_superseded`) → one `fact_audit_log` row for the
predecessor → `COMMIT`. All reused, unmodified `personal_knowledge_repo`
insert functions except the one new `update_fact_as_superseded`. No V8.

---

## 20. Rollback proofs

**Central proof**, `req_cm_rollback_forced_audit_failure_rolls_back_
everything`: a predecessor is created, a correction payload with a
**new** object `Entity` and a **new** `Source` is built, the
`fact_audit_log` forced-failure trigger (reused unchanged from
`AddMemory`'s own suite) fires on the final insert. After the
`StorageError`: every `AddMemory`/`CorrectMemory`-mutated table
(`entities`/`sources`/`evidence`/`facts`/`fact_evidence`/
`fact_audit_log`) is confirmed via `am_full_table_snapshot` to be
exactly as it was before the failed call; the predecessor is confirmed
byte-for-byte unchanged (full struct equality) **and** still active
(`domain::is_active_fact`); the connection remains usable (a subsequent
`add_memory` call succeeds).

**Earlier-stage proof**, `req_cm_rollback_earlier_stage_forced_evidence_
failure`: the same `evidence`-table forced-failure trigger `AddMemory`'s
suite already defines (fully reused, generic to the table not the
action) fires *before* the predecessor-update/audit steps run. Same
full-snapshot and predecessor-unchanged assertions hold — proving the
rollback guarantee independent of which insert in the sequence fails.

---

## 21. Confirmation test matrix

Four scenarios (`req_cm_conf_a`-`req_cm_conf_d`), matching the brief's
own lettering:

- **A** (Explicit, unconfirmed): executes immediately, `Success`,
  `confirmation_class = ImportantChange`, successor `UserConfirmed`.
- **B** (Inferred, unconfirmed): `ConfirmationRequired {
  ImportantChange }`, using a nonexistent `predecessor_fact_id` to prove
  the gate precedes the fetch — zero writes.
- **C** (Inferred, confirmed): executes, successor `UserConfirmed`,
  audit `provenance` remains `Inferred`.
- **D** (structurally invalid payload, Inferred/unconfirmed):
  `ValidationFailed` — pure structural validation still precedes the
  gate, matching R021's corrected boundary rather than regressing it.

---

## 22. Active/inactive predecessor matrix

See §6 — `req_cm_active_predecessor_matrix`'s four cases (A/B/C/D),
proving the full `is_active_fact` two-part predicate rather than
`valid_until` alone, with zero writes on every rejection.

---

## 23. Inheritance/patch tests

`req_cm_memory_type_sensitivity_confidence_inherit_and_override` (two
predecessors: one all-inherit, one all-override, asserting both
successor and predecessor state in each case) and
`req_cm_confidence_fieldpatch_matrix_unchanged_set_clear` (three fresh
predecessors, one per `FieldPatch` mode) together prove every inherit-
vs-override combination §10-§12 describe, plus the pure `Set`-rejection
tests in `actions::tests`.

---

## 24. Correction-chain tests

`req_cm_correction_chain_a_to_b_to_c` (§16) plus
`req_cm_sibling_fact_with_same_subject_predicate_remains_untouched`:
two Facts sharing the same `subject_entity_id`+`predicate` (created via
two explicit `AddMemory` calls, matching `AddMemory`'s own no-dedup
precedent) — correcting one leaves the other byte-for-byte unchanged.
No contradiction detection, no sibling search, no auto-`DISPUTED`
anywhere in `correct_memory`.

---

## 25. Persistence/read-side proofs

`req_cm_read_side_persisted_state_via_get_fact`: after a successful
correction, `get_fact(predecessor.id)` returns the historical `Stale`
predecessor with `valid_until` set; `get_fact(successor.id)` returns the
active `UserConfirmed` successor with `valid_until = None`, and its
persisted form matches the in-memory returned `Fact` exactly — not
relying only on the returned struct.

---

## 26. No Relation/Episode side effects

`req_cm_no_relation_episode_side_effects`: after a successful
correction, `relations`/`episodes`/`relation_evidence`/
`episode_evidence`/`episode_entities` all remain at zero rows — the
same proof `AddMemory` already established, now also holding for
`CorrectMemory`.

---

## 27. Exact tests added

**29 new tests**: 8 pure, no-I/O tests in `actions::tests`
(`correct_memory_valid_baseline_payload_is_accepted`,
`correct_memory_confidence_fieldpatch_unchanged_and_clear_always_valid`,
`correct_memory_confidence_fieldpatch_set_accepted_values`,
`correct_memory_confidence_fieldpatch_set_rejected_values`,
`correct_memory_scalar_json_accepted_types_and_rejected_malformed`,
`correct_memory_entity_value_reuses_addmemory_structural_validation`,
`correct_memory_evidence_requires_content_or_locator`,
`correct_memory_normalized_output_has_no_subject_or_predicate_field`)
and 21 database-integration tests in `services::tests` (§21-§26 above,
plus `req_cm_subject_predicate_inherited_exactly`,
`req_cm_success_value_variants_scalar_self_existing_new_domain_ref`,
`req_cm_rejects_malformed_value_and_missing_references_zero_writes`,
`req_cm_previous_snapshot_mandatory_and_deserializes_exactly_to_pre_
correction_predecessor`,
`req_cm_audit_fact_id_is_predecessor_id_with_correct_fields`,
`req_cm_existing_entity_and_source_untouched_on_reuse`,
`req_cm_predecessor_evidence_and_source_remain_linked_successor_gets_
new_evidence`).

---

## 28. Final total test count

**Previous: 327. New: 29 (8 + 21). Final: 356. Failed: 0.** Confirmed by
running the full suite (`cargo test -- --test-threads=1`) both
immediately before this run's first edit and after every subsequent
change, not estimated.

---

## 29. Build/package results

- `cargo fmt --check`: clean (after one `cargo fmt` pass).
- `cargo check --tests`: clean, zero warnings, at every intermediate
  edit and the final state.
- `cargo test -- --test-threads=1`: **356 passed; 0 failed; 0 ignored**.
- `cargo build --release`: succeeded (2m38s).
- `npm run build`: succeeded.
- `npm run tauri build`: run this pass (required — new production Core
  action and storage mutation path) — see the chat reply for the
  measured MSI/NSIS result, verified from the captured log and the
  actual bundle files on disk, not the wrapper's exit code alone. No
  Tauri command was added (`CorrectMemory` is Core-only) — this build
  only proves packaging/regression.

---

## 30. Files changed

- `src-tauri/src/domain/mod.rs` — `is_active_fact`; `Fact`'s own doc
  comment updated to describe `CorrectMemory`'s predecessor mutation
  path instead of calling it unimplemented.
- `src-tauri/src/actions/mod.rs` — `CorrectMemoryPayload`/
  `NormalizedCorrectMemory`, one new `ValidationError` variant
  (`FactNotActive`), `validate_correct_memory` and its own new helpers
  (`confidence_out_of_bounds` extracted and shared,
  `validate_confidence_patch`), new `FactId` import.
- `src-tauri/src/actions/tests.rs` — 8 new direct unit tests (§27).
- `src-tauri/src/services/mod.rs` — `correct_memory` (the action),
  `memory_action_requires_confirmation` (§4), new imports
  (`CorrectMemoryPayload` from `actions`; `is_active_fact` from
  `domain`).
- `src-tauri/src/services/storage/personal_knowledge_repo.rs` — new
  `update_fact_as_superseded` (§14) — the only function that can mutate
  an existing `facts` row.
- `src-tauri/src/services/tests.rs` — 21 new `CorrectMemory` integration
  tests (§27), new fixtures/helpers (`mk_cm_payload`/`mk_cm_predecessor`),
  new import (`CorrectMemoryPayload`).
- `docs/reports/R022_M2_PERSONAL_KNOWLEDGE_CORRECTMEMORY_SLICE_C_2026-09-18.md`
  (this report).
- `docs/PROJECT_STATE.md`/`docs/LIFEOS_SPEC.md` — updated (see chat
  reply/commit for detail).

**`V7__create_personal_knowledge.sql` was not modified** — it already
supported `CorrectMemory` completely (`fact_audit_log.action_type`
already included `'CorrectMemory'` in its `CHECK`); no V8 was created.

---

## 31. Remaining Personal Knowledge work

Slice D (`ConfirmMemory`), E (`ForgetMemory`'s full 14-step deletion
algorithm, R019 §10), F (`Relation` actions), G (`Episode` actions), H
(retrieval/Context Pack) all remain entirely unstarted, unchanged from
R019 §29/R020 §23/R021 §24's recommended order. `Relation`/`Episode` and
their four join tables remain schema-only.

---

## 32. Exact recommendation for Slice D — ConfirmMemory

**STOP after Slice C — `ConfirmMemory` was explicitly not started this
run**, per the run's own instruction. `ConfirmMemory`'s job is narrower
than `CorrectMemory`'s: it changes only `epistemic_state` on an existing
*active* Fact (e.g. `Inferred`/`Observed` → `UserConfirmed`, or clearing
`Disputed`) — no successor Fact, no predecessor closure, no new
Entity/Source/Evidence. It should reuse `domain::is_active_fact` (§6,
built exactly for this reuse) for its own active-Fact precondition, and
will need its own careful review of exactly which `epistemic_state`
transitions are frozen as valid (R017 §8's confirmation-floor rules) —
this is the next concrete design question, not yet resolved by this
run.

---

## Post-run temporal-integrity and audit hardening (2026-09-18)

A small, deliberate pass conducted before Slice D, prompted by a review
of `update_fact_as_superseded`'s actual enforcement mechanism against
what a release build genuinely runs. Found and corrected **one genuine
release-build correctness gap** (not a data-integrity defect that has
occurred — every original R022 test already exercised only active
predecessors, so `debug_assert_eq!` never actually fired incorrectly in
this run's own test suite) and closed several verification gaps in
R022's own coverage. **Does not create R023** — appended here as
instructed; the original §1-§32 above are otherwise unedited.

### Release-build enforcement correction

**Before this pass**: `update_fact_as_superseded`'s only defense against
an unexpected affected-row count was `debug_assert_eq!(changed, 1,
...)` — a check that is compiled out entirely in release builds
(`cargo build --release`, the exact build this run's own npm/Tauri
packaging step produces). A row-count anomaly in a release binary would
have silently returned `Ok(())` despite not having performed the
intended mutation.

**After this pass**: the check is a real runtime `if changed != 1`
returning a typed `rusqlite::Error::SqliteFailure` (matching the exact
`enable_foreign_keys`/`enable_secure_delete`/`ensure_self_entity`
integrity-error-synthesis pattern already established elsewhere in this
codebase) — present in every build profile, propagated by `services::
correct_memory` through its existing `CoreError::from(e)` conversion,
same as every other fallible step in the action. The diagnostic string
is fixed (`"CorrectMemory predecessor update did not affect exactly one
row - predecessor missing or no longer active"`) and carries no Fact
content, value, or evidence text.

### Defense-in-depth active guard in the UPDATE statement itself

The guarded SQL is now:

```sql
UPDATE facts SET valid_until = ?2, superseded_by_fact_id = ?3,
                  epistemic_state = 'STALE', updated_at = ?4
WHERE id = ?1 AND valid_until IS NULL AND epistemic_state <> 'STALE'
```

The `WHERE` clause re-checks the exact `domain::is_active_fact`
predicate at mutation time, not only at `services::correct_memory`'s
earlier preflight — a row that became inactive between the preflight
read and this statement (impossible today within one single-threaded
transaction, but the guard does not rely on that being permanently
true) cannot silently be superseded twice. No schema/migration change -
this is a `WHERE`-clause addition to an existing hand-written `UPDATE`,
not a `CHECK` constraint.

### Deterministic repo-level proof

Four new tests call `storage::personal_knowledge_repo::
update_fact_as_superseded` directly (not only through `correct_memory`)
and assert on its returned `Result`, proving release-build behavior
rather than relying on a debug assertion or a panic-catching harness:

- `req_cm_repo_update_fact_as_superseded_active_predecessor_succeeds` -
  a real active predecessor (plus a real successor row, required by the
  `superseded_by_fact_id` foreign key) updates exactly one row and
  returns `Ok`.
- `req_cm_repo_update_fact_as_superseded_stale_predecessor_rejected` - a
  raw `STALE`-with-`NULL`-`valid_until` fixture returns `Err`, zero
  fields changed.
- `req_cm_repo_update_fact_as_superseded_closed_predecessor_rejected` -
  a raw `valid_until`-already-set fixture returns `Err`, zero fields
  changed.
- `req_cm_repo_update_fact_as_superseded_nonexistent_fact_rejected` - a
  nonexistent `FactId` returns `Err`, never a silent `Ok`.

### Service-level rollback proof for this exact failure

`req_cm_rollback_predecessor_update_guard_rejects_and_rolls_back`: a
test-only trigger (no production failure flag) closes the exact
predecessor row immediately after the `Evidence` insert - the step
immediately preceding `update_fact_as_superseded` in the real
transaction order - so the guarded `UPDATE` genuinely matches zero rows
inside a real `correct_memory` call. Proves the whole transaction,
including the trigger's own side effect, rolls back: the new object
`Entity`, `Source`, successor `Fact`, `Evidence`, link, and audit row
are all absent afterward, and the predecessor is confirmed byte-for-byte
unchanged. The two pre-existing rollback tests
(`req_cm_rollback_forced_audit_failure_rolls_back_everything`,
`req_cm_rollback_earlier_stage_forced_evidence_failure`) remain green,
unmodified.

### Audit field completeness

`req_cm_audit_fact_id_is_predecessor_id_with_correct_fields` now
constructs a deliberately fixed `ActionMeta` (`Actor::CloudLlm`,
`Provenance::Explicit`, `requested_at = "2026-09-18T15:20:00Z"`, not
`ui_explicit()`'s implicit `Actor::Ui`/call-time timestamp) and asserts,
in one query, all seven `fact_audit_log` columns: `fact_id ==
predecessor.id` (and `!= successor.id`), `action_type ==
"CorrectMemory"`, `actor == "CloudLLM"` (the exact supplied actor),
`provenance == "Explicit"`, `confirmation_class ==
"IMPORTANT_CHANGE"`, `previous_snapshot` non-`NULL` and deserializing
exactly to the predecessor's captured pre-correction state, and
`occurred_at == requested_at.to_rfc3339()` exactly. The separate
inferred-confirmed test
(`req_cm_conf_c_inferred_confirmed_executes_and_audit_provenance_
remains_inferred`, proving `provenance` stays `Inferred` while the
successor's `epistemic_state` becomes `USER_CONFIRMED`) was left as its
own dedicated test, not merged into this one - confirmation and
provenance remain distinct concepts, tested separately.

### `Fact` doc comment clarity

`domain/mod.rs`'s `Fact` doc comment previously listed
`epistemic_state`/`confidence`/`sensitivity`/`valid_until`/
`superseded_by_fact_id`/`updated_at` together as fields that "may ever
change," which left it ambiguous whether `confidence`/`sensitivity`
could ever be mutated on an *existing* Fact row. Reworded to state
explicitly that exactly two mutation paths exist (a future
`ConfirmMemory`: `epistemic_state` only; `CorrectMemory`'s predecessor
side-effect: `valid_until`/`superseded_by_fact_id`/`epistemic_state`/
`updated_at` only), and that **`confidence`/`sensitivity` are never
mutated on an existing Fact row by either path** - a `CorrectMemory`
payload's overrides apply only to the brand-new successor. Documentation
only; no semantic change (`CorrectMemory`'s actual behavior was already
correct - only the doc comment was ambiguous).

### Exact new test count

**Previous (R022 original): 356. New this pass: 5**
(`req_cm_repo_update_fact_as_superseded_active_predecessor_succeeds`,
`..._stale_predecessor_rejected`, `..._closed_predecessor_rejected`,
`..._nonexistent_fact_rejected`,
`req_cm_rollback_predecessor_update_guard_rejects_and_rolls_back` - the
audit-completeness strengthening extended a PRE-EXISTING test rather
than adding a new one). **Final: 361. Failed: 0.** The original R022
§28 result — **327 → 356 (+29)** — remains this report's unedited
historical record; **361** is this pass's own separately-reported
measured result, not a retroactive edit to §28.

### Build/package results (this pass)

- `cargo fmt --check`: clean (after one `cargo fmt` pass).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **361 passed; 0 failed; 0 ignored**.
- `cargo build --release`: succeeded.
- `npm run build`: succeeded.
- `npm run tauri build`: run this pass (required — production
  `CorrectMemory` storage behavior changed) — see the chat reply for
  the measured MSI/NSIS result, verified from the captured log and the
  actual bundle files on disk.

### Files changed (this pass)

- `src-tauri/src/services/storage/personal_knowledge_repo.rs` -
  `update_fact_as_superseded`'s `debug_assert_eq!` replaced with a real
  runtime `Result`-returning check; `WHERE` clause hardened with the
  active-Fact predicate; doc comment updated.
- `src-tauri/src/domain/mod.rs` - `Fact`'s mutability-contract doc
  comment reworded for clarity (§7 above); no semantic change.
- `src-tauri/src/services/tests.rs` - 5 new tests, 1 existing test
  strengthened (audit field completeness).
- `docs/reports/R022_M2_PERSONAL_KNOWLEDGE_CORRECTMEMORY_SLICE_C_2026-09-18.md`
  (this addendum).

**No `V7` migration change. No V8 created. `CorrectMemory`'s observable
semantics are unchanged** - this pass only replaced a debug-only check
with an equivalent-but-real runtime one and added a redundant defense-
in-depth `WHERE`-clause guard; every existing R022 test (including the
two pre-existing rollback tests) remained green throughout, unmodified.
No `ConfirmMemory`/`ForgetMemory` code. No UI/Tauri command changes.

### Next substantial report

Unchanged: **R023**, allocated to whichever of Slice D (`ConfirmMemory`)
or another explicitly-instructed run comes next. `ConfirmMemory` was
**not** started by this pass.
