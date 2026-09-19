# R027 — M2 Personal Knowledge CorrectRelation

**Type:** Implementation (production Rust + tests). No schema/migration
change. Implements exactly the scope R025 (and its two post-freeze
addenda) froze for `CorrectRelation`. Does **not** implement
`ConfirmRelation`/`ForgetRelation`, Episode, retrieval, Tauri commands,
or UI.

---

## 1. Pre-run verified state

CONFIRMED FACT: clean working tree at commit `a2b08d7` ("docs: record
R026 frontend build verification") before this run began. 461 tests
passing. V8 unchanged since R026 (`relation_audit_log` only). Read in
full before writing any code: `CLAUDE.md`, `docs/PROJECT_STATE.md`,
`docs/LIFEOS_SPEC.md`, R017–R019, R024 (+ its transaction-isolation
addendum), R025 (+ both post-freeze corrections), R026 (+ its
frontend-build addendum). Inspected the live `Relation` struct,
`is_active_relation`, `get_relation`, `add_relation`, `FieldPatch<T>`,
`correct_memory`/`confirm_memory`'s full implementations,
`memory_action_requires_confirmation`, `effective_class`,
`blocks_until_confirmed`, `serialize_required_snapshot`,
`relation_audit_log`'s schema, `insert_relation_audit_log`, the existing
guarded-update runtime affected-row pattern
(`update_fact_as_superseded`/`confirm_fact_epistemic_state`), and the
existing forced-audit-failure rollback trigger pattern.

## 2. Exact `CorrectRelationPayload`

```rust
pub(crate) struct CorrectRelationPayload {
    pub relation_id: RelationId,
    pub note: FieldPatch<String>,
    pub confidence: FieldPatch<f32>,
    pub sensitivity: Option<Sensitivity>,
}
// -> ActionOutcome<Relation>
```

Exactly as R025's post-freeze patch-shape addendum froze it. No `from`/
`to`/`relation_type`/`epistemic_state`/`valid_from`/`valid_until`/
`created_at`/`updated_at`/`Source`/`Evidence` field.

## 3. Exact patch semantics

- `note` (`FieldPatch<String>`): `Unchanged` → retain; `Set(v)` →
  `Some(v)`; `Clear` → `None`.
- `confidence` (`FieldPatch<f32>`): `Unchanged` → retain; `Set(v)` →
  `Some(v)`; `Clear` → `None`.
- `sensitivity` (`Option<Sensitivity>`): `None` → retain; `Some(v)` →
  replace. No `Clear` state exists — `relations.sensitivity` is
  `NOT NULL`.

## 4. Pure validation / NoFieldsToUpdate rule

`actions::validate_correct_relation` is pure, no I/O. A payload where
`note == Unchanged` AND `confidence == Unchanged` AND `sensitivity ==
None` is rejected with the existing, generic
`ValidationError::NoFieldsToUpdate` — no new Relation-specific variant.
This check runs before confirmation and before any DB read. Proven
directly by `correct_relation_all_fields_absent_rejected_as_no_fields_to_update`
(pure) and, at the service level, by
`req_cr_conf_e_structural_no_op_rejected_before_gate_and_lookup` — using
`Inferred`+unconfirmed with a **nonexistent** `RelationId`, the result is
`ValidationFailed(NoFieldsToUpdate)`, never `ConfirmationRequired` or
`NotFound`, proving this check precedes both.

## 5. Note validation

`FieldPatch::Set(note)` bound-checked via `validate_pk_optional_field`
against `RELATION_NOTE_MAX_LEN = 4000` — no trim, no normalization,
empty and whitespace-only both valid and preserved exactly. `Clear`/
`Unchanged` always valid regardless of length. Proven by the full
A–G matrix, `req_cr_note_patch_matrix`.

## 6. Confidence validation

`FieldPatch::Set(c)` requires `c.is_finite() && 0.0 <= c <= 1.0`
(`ValidationError::ConfidenceOutOfBounds`, reused unchanged).
`Clear`/`Unchanged` always valid. Proven by `req_cr_confidence_patch_matrix`
(A–F) and the pure `correct_relation_confidence_set_out_of_bounds_rejected`/
`_clear_always_valid_regardless_of_numeric_bounds`.

## 7. Sensitivity semantics

`None` → retain; `Some(v)` → replace, for all 4 frozen values. Proven by
`req_cr_sensitivity_patch_matrix` (A–E).

## 8. `ImportantChange` confirmation behavior

Floor is `ConfirmationClass::ImportantChange`, same as `CorrectMemory`.
`effective_class(ImportantChange, Explicit)` stays `ImportantChange`;
existing global policy lets a direct Explicit `ImportantChange` action
execute without a second confirmation — unchanged, not altered by this
run. Proven by `req_cr_conf_a`/`req_cr_conf_b`.

## 9. Inferred blocking overlay

`ImportantChange` (severity 3) already dominates `InferredChange`
(severity 2) in the generic `effective_class` ladder, so
`blocks_until_confirmed` alone would NOT block an inferred, unconfirmed
correction. `memory_action_requires_confirmation` — already fully
generic, no Fact-specific logic inside it — is reused **completely
unmodified** to restore the block. Proven by
`req_cr_conf_c_inferred_unconfirmed_blocks_gate_precedes_relation_lookup`
and the pure
`correct_relation_inferred_provenance_effective_class_does_not_block_alone`.

## 10. Gate-before-lookup proof

Using a **nonexistent** `RelationId` with `Inferred`+unconfirmed returns
`ConfirmationRequired { confirmation_class: ImportantChange }`, never
`NotFound` — proving the confirmation gate runs strictly before the
target Relation is ever fetched (`req_cr_conf_c`, mandatory ordering
proof).

## 11. Active Relation requirement

Reuses `domain::is_active_relation` unmodified:
`valid_until.is_none() && epistemic_state != Stale`. No wall-clock
comparison. Correctable active states: `USER_CONFIRMED`, `OBSERVED`,
`INFERRED`, `DISPUTED`. Proven by `req_cr_active_matrix_correctable_epistemic_states_unchanged_after`.

## 12. `RelationNotActive` behavior

New variant `ValidationError::RelationNotActive` — deliberately never
reused from `FactNotActive` (`Relation`/`Fact` are distinct domain
types). All three inactive combinations (STALE+`None`, non-STALE+`Some`,
STALE+`Some`) rejected with zero writes/audits, proven by
`req_cr_inactive_matrix_rejected_zero_writes`. A missing id remains
`NotFound` (`req_cr_missing_target_returns_not_found_after_gate_satisfied`).

## 13. Same-row identity model

`CorrectRelation` mutates the SAME `Relation` row — no successor, no
`superseded_by_relation_id` (deliberately never invented, matching
R025's frozen resolution). `relations` row count is unchanged before/
after; returned `id == predecessor.id`. Proven by
`req_cr_no_new_relation_row_same_id_and_output_matches_readback`.

## 14. Exact mutable / immutable fields

Mutable: `note`, `confidence`, `sensitivity`. Everything else —
`id`/`from_entity_id`/`to_entity_id`/`relation_type`/`epistemic_state`/
`valid_from`/`valid_until`/`created_at` — is preserved byte-for-byte.
There is no `updated_at` to simulate, and none is added. Proven by
`req_cr_identity_fields_immutable`, `req_cr_temporal_fields_immutable`,
and the epistemic-immutability assertions folded into
`req_cr_active_matrix_correctable_epistemic_states_unchanged_after` and
`req_cr_combined_patch_all_three_fields_at_once` (which asserts every
immutable field at once).

## 15. Guarded storage `UPDATE`

New narrow private repo function,
`update_relation_metadata_if_active(conn, relation_id, note, confidence,
sensitivity)`:

```sql
UPDATE relations SET note = ?2, confidence = ?3, sensitivity = ?4
WHERE id = ?1 AND valid_until IS NULL AND epistemic_state <> 'STALE'
```

Touches only `note`/`confidence`/`sensitivity` — structurally impossible
to touch anything else through this function's parameter list, matching
`update_fact_as_superseded`/`confirm_fact_epistemic_state`'s established
discipline.

## 16. Runtime affected-row enforcement

`changed != 1` is a real `rusqlite::Error::SqliteFailure` (SQLITE_MISUSE,
fixed non-sensitive diagnostic) in every build profile — never
`debug_assert!`/`assert!`/`panic!`. Direct repo-level proof matrix
(A–F), `req_cr_repo_guarded_update_matrix`: active `USER_CONFIRMED`/
`INFERRED` succeed exactly one row; STALE+`NULL`, non-STALE+`Some`,
STALE+`Some`, and a nonexistent id all `Err` with zero mutation (full
field-equality checked before/after for the rejected cases).

## 17. Mandatory previous snapshot

`serialize_required_snapshot` (existing, generic, unmodified) serializes
the complete PRE-correction `Relation` before the transaction opens —
never `.ok()`-downgraded; a serialization failure means zero writes.
Snapshot exactness (not merely "non-NULL" or "contains note") proven by
`req_cr_snapshot_deserializes_exactly_to_pre_correction_relation`
(deserializes `previous_snapshot` back into a `Relation` and asserts
full struct equality against the captured pre-state) and again inside
`req_cr_audit_exact_explicit`.

## 18. Audit exactness

One `relation_audit_log` row per successful `CorrectRelation`, via the
existing `insert_relation_audit_log` (R026, reused unmodified — no
second audit helper created): `action_type = "CorrectRelation"`,
`previous_snapshot = Some(serialized_pre_relation)`,
`occurred_at = meta.requested_at`. Exactness proven with fixed
`ActionMeta` timestamps in `req_cr_audit_exact_explicit` (actor
`CloudLLM`, provenance `Explicit`, class `IMPORTANT_CHANGE`) and
`req_cr_audit_exact_inferred_confirmed`.

## 19. Inferred provenance behavior

Audit `provenance` is stored exactly as given — an inferred, confirmed
correction keeps `provenance = "Inferred"` in the audit row while the
Relation's `epistemic_state` remains **completely untouched**
(`CorrectRelation` never forces `USER_CONFIRMED`, unlike `AddRelation`
— correction and epistemic confirmation are distinct concepts, the
latter reserved for the not-yet-implemented `ConfirmRelation`). Proven
by `req_cr_conf_d`/`req_cr_audit_exact_inferred_confirmed`.

## 20. Transaction order

`BEGIN` → guarded `update_relation_metadata_if_active` → one
`relation_audit_log` row → `COMMIT`. Any failure drops `tx` without
commit; rusqlite rolls back automatically — no partial correction.

## 21. Rollback proof

**Mandatory** — `req_cr_forced_audit_failure_rolls_back_metadata_update`:
forces the `relation_audit_log` insert to fail via the existing
`install_relation_audit_failure_trigger`/`remove_relation_audit_failure_trigger`
(R026, reused unmodified — same shared table as `AddRelation`'s own
proof). The metadata `UPDATE` rolls back too: `get_relation` returns
EXACTLY the pre-correction Relation, no `CorrectRelation` audit row
exists, and the connection remains usable for a subsequent successful
call.

## 22. Patch matrices

- **Note** (§5 above): A–G all proven, `req_cr_note_patch_matrix`.
- **Confidence** (§6 above): A–F all proven, `req_cr_confidence_patch_matrix`.
- **Sensitivity** (§7 above): A–E all proven, `req_cr_sensitivity_patch_matrix`.
- **Combined** — `req_cr_combined_patch_all_three_fields_at_once`:
  patches all three simultaneously (`note Set`, `confidence Clear`,
  `sensitivity Some`), asserting the three new values AND every
  immutable field unchanged.
- **Same-value resupply** —
  `req_cr_same_value_resupply_still_succeeds_and_writes_audit`:
  explicitly resupplying the Relation's current `note` still succeeds,
  writes an audit row, and is never coerced into `NoFieldsToUpdate` — no
  database-dependent same-value optimization exists.

## 23. Active/inactive state matrix

Active (`USER_CONFIRMED`/`OBSERVED`/`INFERRED`/`DISPUTED`) all
correctable with `epistemic_state` unchanged
(`req_cr_active_matrix_correctable_epistemic_states_unchanged_after`).
Inactive (STALE+`None`, non-STALE+`Some`, STALE+`Some`) all rejected
with `RelationNotActive` and zero writes/audits
(`req_cr_inactive_matrix_rejected_zero_writes`).

## 24. Entity/Source/Evidence and Fact/Episode non-interference

`req_cr_entity_source_evidence_non_interference` — `entities`/`sources`/
`evidence`/`relation_evidence` row counts unchanged by `CorrectRelation`
itself (only the two fixture Entities the test creates for setup exist).
`req_cr_fact_episode_non_interference` — `facts`/`fact_evidence`/
`episodes`/`episode_evidence`/`episode_entities` all unchanged.

## 25. Row-count / id / output proofs

`req_cr_no_new_relation_row_same_id_and_output_matches_readback` — the
`relations` table's row count is unchanged before/after; the returned
`Relation` exactly equals a subsequent `get_relation` read-back (no
stale in-memory result).

## 26. Tests added

- `src/actions/tests.rs`: 8 new pure `validate_correct_relation` tests
  (baseline accepted; generic-ladder non-blocking property;
  all-fields-absent rejected; sensitivity-only accepted; note
  Clear/Unchanged always valid; note bounds; confidence out-of-bounds
  matrix; confidence Clear always valid).
- `src/services/tests.rs`: 23 new tests — the 6-case direct guarded-
  update matrix, 5 confirmation/gate tests, 2 active/inactive matrices,
  2 identity/temporal immutability tests, 3 patch matrices (note/
  confidence/sensitivity), combined-patch + same-value-resupply, 3
  snapshot/audit-exactness tests, the mandatory forced-rollback test, 2
  non-interference tests, and the row-count/output test.

## 27. Exact final test count

- Previous: **461**
- New: **31** (8 in `actions::tests`, 23 in `services::tests`)
- Final: **492**
- Failed: **0**

## 28. Build/package results (MEASURED)

- `cargo fmt --check`: clean (after one `cargo fmt` auto-format pass;
  re-verified clean).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **492 passed; 0 failed; 0 ignored**
  (20.00s).
- `cargo build --release`: succeeded (`Finished release profile
  [optimized] target(s) in 2m 22s`).
- `npm.cmd run build` (standalone, repo root, explicit — not the
  Tauri-internal frontend build): succeeded, exit 0. `tsc` and `vite
  build` both succeeded; `dist/` regenerated.
- `npm.cmd run tauri build`: succeeded. Verified actual artifacts on
  disk:
  - `src-tauri/target/release/bundle/msi/LifeOS_0.1.0_x64_en-US.msi`
    (2,973,696 bytes)
  - `src-tauri/target/release/bundle/nsis/LifeOS_0.1.0_x64-setup.exe`
    (1,370,474 bytes)

## 29. Files changed

- `src-tauri/src/actions/mod.rs` (`ValidationError::RelationNotActive`,
  `CorrectRelationPayload`, `NormalizedCorrectRelation`,
  `validate_correct_relation`, one new domain import: `RelationId`)
- `src-tauri/src/services/mod.rs` (`correct_relation`, import updates:
  `CorrectRelationPayload`, `is_active_relation`)
- `src-tauri/src/services/storage/personal_knowledge_repo.rs`
  (`update_relation_metadata_if_active`)
- `src-tauri/src/actions/tests.rs` (+8 tests)
- `src-tauri/src/services/tests.rs` (+23 tests)

No `docs/domain/mod.rs` change was needed — `Relation`/`is_active_relation`
already existed exactly as R026 built them.

## 30. Confirmation: no migration

`git diff -- src-tauri/migrations/` is empty — V1–V8 byte-for-byte
unchanged, no V9. `relation_audit_log`'s existing schema already
supported `CorrectRelation`'s audit row from R026's own forward-looking
`action_type` CHECK.

## 31. Remaining Relation work

`ConfirmRelation`, `ForgetRelation` — not started. Episode actions and
retrieval/Context Pack — not started. No Tauri command wires any
Relation action into the running app yet.

## 32. Next report recommendation

**R028 — `ConfirmRelation`**: pure epistemic confirmation in place
(`INFERRED`/`OBSERVED`/`DISPUTED` → `USER_CONFIRMED`, already-confirmed
rejected as a no-op), mirroring `ConfirmMemory`'s exact shape — floor
`ExplicitAdd`, no PK-specific overlay needed (dominated by
`InferredChange` in the generic ladder, same reasoning `AddRelation`/
`ConfirmMemory` already established). Followed by R029 `ForgetRelation`,
built with `TransactionBehavior::Immediate` from the start.
