# R028 — M2 Personal Knowledge ConfirmRelation

**Type:** Implementation (production Rust + tests). No schema/migration
change — V8's `relation_audit_log.action_type` CHECK already enumerated
`'ConfirmRelation'` (R026, forward-looking). Implements exactly the
scope R025 froze for `ConfirmRelation`. Does **not** implement
`ForgetRelation`, Episode, retrieval, Tauri commands, or UI.

---

## 1. Pre-run verified checkpoint

CONFIRMED FACT: clean working tree at commit `a64e599` ("fix: harden
CorrectRelation snapshot transaction") before this run began. 494 tests
passing. V8 unchanged since R026 (`relation_audit_log` only). Read in
full before writing any code: `CLAUDE.md`, `docs/PROJECT_STATE.md`,
`docs/LIFEOS_SPEC.md`, R017–R019, R023 (`ConfirmMemory`), R025 (+ both
post-freeze corrections), R026 (+ frontend-build addendum), R027 (+
transaction/snapshot-integrity hardening addendum). Inspected the live
`Relation`/`RelationId`/`EpistemicState`/`is_active_relation`/
`get_relation`/`AddRelation`/`CorrectRelation`/
`begin_correct_relation_transaction`, `ConfirmMemory`/
`ConfirmMemoryPayload`/`confirm_fact_epistemic_state`/
`FactAlreadyUserConfirmed`, `RelationNotActive`, `effective_class`,
`blocks_until_confirmed`, `memory_action_requires_confirmation`,
`serialize_required_snapshot`, `insert_relation_audit_log`, the
`relation_audit_log` schema, and the forced relation-audit-failure
trigger tests. Confirmed `CorrectRelation`'s post-hardening transaction
boundary (target lookup/state checks/snapshot all inside its own
`Immediate` transaction) as the pattern to apply from the start, not
`ConfirmMemory`'s original pre-Deferred-transaction reads.

## 2. Exact payload/result

```rust
pub(crate) struct ConfirmRelationPayload {
    pub relation_id: RelationId,
}
// -> ActionOutcome<Relation>
```

No caller-provided `epistemic_state`/timestamp/`note`/`confidence`/
`sensitivity`/endpoints/`relation_type`/`Source`/`Evidence`.

## 3. Semantic purpose

"The user confirms that this existing Personal Knowledge Relation
should be treated as USER_CONFIRMED knowledge." Not metadata
correction, not creation, not deletion, not contradiction resolution,
not Evidence/Source mutation, not endpoint/`relation_type` mutation.
`CorrectRelation` and `ConfirmRelation` remain fully separate actions.

## 4. Confirmation floor

`ConfirmationClass::ExplicitAdd` — same floor `AddMemory`/`AddRelation`/
`ConfirmMemory` have.

## 5. Generic inferred-blocking behavior (no PK-specific overlay)

`effective_class(ExplicitAdd, Inferred) = InferredChange` already
dominates and `blocks_until_confirmed` already returns `true` for it —
the exact same generic-ladder reasoning `AddRelation`/`ConfirmMemory`
established. `ConfirmRelation` uses the plain `blocks_until_confirmed(class)
&& !meta.confirmed` gate, **not** `memory_action_requires_confirmation`
(that overlay exists specifically because `CorrectRelation`'s
`ImportantChange` floor is NOT dominated by `InferredChange` — a
different floor, a different need). Proven by
`req_cf_conf_a`/`req_cf_conf_b`/`req_cf_conf_c`/`req_cf_conf_d`.

## 6. Gate-before-transaction/lookup proof

MANDATORY, proven directly by
`req_cf_conf_c_inferred_unconfirmed_blocks_gate_precedes_transaction_and_lookup`:
a **nonexistent** `RelationId` with `Inferred`+unconfirmed returns
`ConfirmationRequired { confirmation_class: InferredChange }`, never
`NotFound` — proving the gate precedes both the `Immediate` transaction
open and the target lookup.

## 7. `TransactionBehavior::Immediate` decision — built in from the start

Unlike `CorrectMemory`/`CorrectRelation` (both originally shipped with a
`Deferred` transaction and needed a later hardening pass),
`ConfirmRelation` opens `begin_confirm_relation_transaction`
(`conn.transaction_with_behavior(TransactionBehavior::Immediate)`) from
day one — applying R024's and R027's hardening lesson proactively
instead of reproducing the same read/snapshot/write race a third time.
Scoped to `ConfirmRelation` only; `begin_correct_relation_transaction`
is untouched, not renamed, not generalized.

## 8. Target lookup / state checks inside the transaction

`get_relation(&tx, relation_id)` runs through the `Immediate`
transaction. Missing → `ActionOutcome::NotFound`, transaction drops,
zero writes. `is_active_relation(&target)` and the already-confirmed
check both run on that same transaction-read target.

## 9. Active Relation requirement

Reuses `domain::is_active_relation` unmodified — `valid_until.is_none()
&& epistemic_state != Stale`, no wall-clock comparison. Inactive →
`ValidationFailed(RelationNotActive)`, the existing R027 variant, reused
unchanged.

## 10. `RelationAlreadyUserConfirmed`

New variant — deliberately never reused from `FactAlreadyUserConfirmed`
(`Relation`/`Fact` are distinct domain types). An active target already
`USER_CONFIRMED` is rejected explicitly rather than silently returning
`Success` with zero writes.

## 11. Valid transition matrix

`INFERRED`/`OBSERVED`/`DISPUTED` → `USER_CONFIRMED`. No other
transition. Proven by
`req_cf_service_valid_transitions_produce_user_confirmed_and_one_audit_row`
and the direct repo-level `req_cf_repo_guarded_update_matrix` (A–C).

## 12. State-check ordering

Active check runs **before** the already-confirmed check — matching
`ConfirmMemory`'s established ordering exactly. A Relation that is BOTH
closed AND already `USER_CONFIRMED` reports `RelationNotActive`, never
`RelationAlreadyUserConfirmed`. Proven by
`req_cf_inactive_and_already_confirmed_ordering_inactive_wins`.

## 13. Guarded repo update

New narrow private repo function, `confirm_relation_epistemic_state`:

```sql
UPDATE relations SET epistemic_state = 'USER_CONFIRMED'
WHERE id = ?1 AND valid_until IS NULL
  AND epistemic_state IN ('INFERRED', 'OBSERVED', 'DISPUTED')
```

Touches only `epistemic_state` — structurally impossible to touch
anything else through this function's parameter list, matching
`confirm_fact_epistemic_state`/`update_relation_metadata_if_active`'s
established discipline.

## 14. Runtime affected-row enforcement

`changed != 1` is a real `rusqlite::Error::SqliteFailure` (SQLITE_MISUSE,
fixed non-sensitive diagnostic) in every build profile — never
`debug_assert!`/`assert!`/`panic!`. Full A–H matrix proven directly:
INFERRED/OBSERVED/DISPUTED succeed exactly one row; USER_CONFIRMED,
STALE+`NULL`, non-STALE+`Some`, STALE+`Some`, and a nonexistent id all
`Err` with zero mutation (`req_cf_repo_guarded_update_matrix`).

## 15. Mandatory `previous_snapshot`

`serialize_required_snapshot` (existing, generic, unmodified) serializes
the complete PRE-confirmation `Relation` — read through the SAME
`Immediate` transaction, never `.ok()`-downgraded. Exactness (full
struct equality, not merely non-NULL) proven by
`req_cf_snapshot_deserializes_exactly_to_pre_confirmation_relation` and
again inside `req_cf_audit_exact_explicit`.

## 16. Audit exactness

One `relation_audit_log` row per successful `ConfirmRelation`, via the
existing `insert_relation_audit_log` (reused unmodified — no second
audit helper): `action_type = "ConfirmRelation"`, `previous_snapshot =
Some(serialized_pre_relation)`, `occurred_at = meta.requested_at`.
Exactness proven with fixed `ActionMeta` timestamps in
`req_cf_audit_exact_explicit` (actor `CloudLLM`, provenance `Explicit`,
class `EXPLICIT_ADD`) and `req_cf_audit_exact_inferred_confirmed`.

## 17. Inferred provenance behavior

Audit `provenance` stored exactly as given — a confirmed `Inferred`
call keeps `provenance = "Inferred"` in the audit row while the
Relation's `epistemic_state` becomes `USER_CONFIRMED`. Proven by
`req_cf_conf_d`/`req_cf_audit_exact_inferred_confirmed`.

## 18. Transaction order

`BEGIN IMMEDIATE` → `get_relation(&tx)` → active check → already-
confirmed check → serialize `previous_snapshot` → guarded
`confirm_relation_epistemic_state` → one `relation_audit_log` row →
`COMMIT`. Any failure drops `tx` without commit; rusqlite rolls back
automatically.

## 19. Two-connection writer-lock proof (MANDATORY)

`req_cf_uses_immediate_transaction_blocking_concurrent_writer` — calls
`begin_confirm_relation_transaction` directly (the exact helper/path
production uses). Connection B, `busy_timeout(0)`, is rejected
`SQLITE_BUSY`/`SQLITE_LOCKED` for a harmless unrelated write while
Connection A holds the transaction open, then succeeds once A's
transaction drops.

## 20. Target-row snapshot-stability proof

`req_cf_immediate_transaction_blocks_concurrent_relation_metadata_write`
— Connection A opens the transaction, reads the TARGET Relation through
`&tx` (asserting the read matches the fixture exactly), then Connection
B specifically attempts to `UPDATE` that same Relation's
`epistemic_state` — blocked for the whole window, succeeding only after
A's transaction drops.

## 21. Rollback proof

MANDATORY —
`req_cf_forced_audit_failure_rolls_back_epistemic_update_then_recovers`:
forces the `relation_audit_log` insert to fail via the existing
`install_relation_audit_failure_trigger` (reused unmodified). The
epistemic `UPDATE` rolls back too: `get_relation` returns EXACTLY the
pre-confirmation Relation (`epistemic_state` unchanged), no
`ConfirmRelation` audit row exists, and — after the trigger is removed —
a genuine subsequent call succeeds, proving the connection remains
usable.

## 22. Same-row / row-count behavior

No new `RelationId`, no successor, no `valid_until` mutation, no
supersession pointer. `returned.id == target.id`; `relations` row count
unchanged before/after
(`req_cf_only_epistemic_state_changes_everything_else_immutable_row_count_unchanged`).

## 23. Exact immutable fields

Only `epistemic_state` changes. Proven unchanged: `id`,
`from_entity_id`, `to_entity_id`, `relation_type`, `note`, `confidence`,
`sensitivity`, `valid_from`, `valid_until`, `created_at`. No
`relations.updated_at` exists or is simulated — the confirmation
timestamp lives only in `relation_audit_log.occurred_at`.

## 24. Sibling Relation non-interference

`req_cf_sibling_relation_non_interference_when_confirming_disputed` —
confirming a `DISPUTED` target Relation never touches a sibling
Relation sharing the same endpoints/`relation_type`: no lookup, no
mutation, no deletion, no contradiction resolution. Sibling row proven
byte-for-byte unchanged.

## 25. Entity/Source/Evidence non-interference

`req_cf_entity_source_evidence_non_interference` — `entities`/
`sources`/`evidence`/`relation_evidence` row counts unchanged by
`ConfirmRelation` itself (only the two fixture Entities the test
creates exist).

## 26. Fact/Episode non-interference

`req_cf_fact_episode_non_interference` — `facts`/`fact_evidence`/
`episodes`/`episode_evidence`/`episode_entities` all unchanged; `relations`
row count itself also asserted unchanged (same row).

## 27. Output/readback proof

`req_cf_output_matches_get_relation_readback` — the returned `Relation`
exactly equals a subsequent `get_relation` read-back, no stale
in-memory result.

## 28. Tests added

- `src/actions/tests.rs`: 1 trivial pure test
  (`confirm_relation_payload_validates_trivially_and_passes_relation_id_through`),
  mirroring `validate_confirm_memory`'s own precedent (no dedicated file
  split — nothing structural to test purely).
- `src/services/tests.rs`: 19 new tests — the 8-case direct guarded-
  update matrix, valid-transition and rejection matrices, state-check
  ordering, 4 confirmation/gate tests, 2 audit-exactness tests, snapshot
  exactness, 2 mandatory concurrency proofs, the mandatory rollback
  proof, same-row/immutable-fields/row-count, sibling non-interference,
  Entity/Source/Evidence non-interference, Fact/Episode non-
  interference, and the output/readback proof.

One pre-existing test-ordering bug was found and fixed during this run:
`req_cf_fact_episode_non_interference` originally captured its "before"
row-count snapshot BEFORE creating the fixture Relation, so the
post-confirmation count legitimately differed by the fixture's own
creation, not by any real `ConfirmRelation` side effect — a test bug,
not a production bug. Fixed by moving the snapshot to after fixture
creation; the full suite is green.

## 29. Exact final test count

- Previous: **494**
- New: **20** (1 in `actions::tests`, 19 in `services::tests`)
- Final: **514**
- Failed: **0**

## 30. Build/package results (MEASURED)

- `cargo fmt --check`: clean (after one `cargo fmt` auto-format pass;
  re-verified clean).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **514 passed; 0 failed; 0 ignored**
  (17.27s) — after fixing the one test-ordering bug found (§28); a
  first run correctly caught it (513 passed, 1 failed).
- `cargo build --release`: succeeded (`Finished release profile
  [optimized] target(s) in 2m 05s`).
- `npm.cmd run build` (standalone, repo root, explicit — separate from
  the Tauri-internal frontend build, per the acceptance-gap fix closed
  in R026): succeeded, exit 0.
- `npm.cmd run tauri build`: succeeded. Verified actual artifacts on
  disk:
  - `src-tauri/target/release/bundle/msi/LifeOS_0.1.0_x64_en-US.msi`
    (2,973,696 bytes)
  - `src-tauri/target/release/bundle/nsis/LifeOS_0.1.0_x64-setup.exe`
    (1,370,911 bytes)

## 31. Files changed

- `src-tauri/src/actions/mod.rs`
  (`ValidationError::RelationAlreadyUserConfirmed`,
  `ConfirmRelationPayload`, `NormalizedConfirmRelation`,
  `validate_confirm_relation`)
- `src-tauri/src/services/mod.rs` (`begin_confirm_relation_transaction`,
  `confirm_relation`, import updates: `ConfirmRelationPayload`)
- `src-tauri/src/services/storage/personal_knowledge_repo.rs`
  (`confirm_relation_epistemic_state`)
- `src-tauri/src/actions/tests.rs` (+1 test)
- `src-tauri/src/services/tests.rs` (+19 tests)

No `domain/mod.rs` change was needed — `Relation`/`EpistemicState`/
`is_active_relation` already existed exactly as R026/R027 built them.
`begin_correct_relation_transaction` and every R027 test are untouched.

## 32. Confirmation: no migration/V9

`git diff -- src-tauri/migrations/` is empty — V1–V8 byte-for-byte
unchanged, no V9. Directly confirmed
`migrations/V8__create_relation_audit_log.sql`'s `action_type` CHECK
already enumerates `'ConfirmRelation'` (frozen forward-looking in R026).

## 33. Remaining Relation work

`ForgetRelation` — not started. Episode actions and retrieval/Context
Pack — not started. No Tauri command wires any Relation action into the
running app yet.

## 34. Next report recommendation

**R029 — `ForgetRelation`**: the privacy-safe hard-delete action, the
last of the four frozen v0.1 Relation actions. Built with
`TransactionBehavior::Immediate` from the start (per R024's/R027's
lesson, now applied proactively a second time by `ConfirmRelation`
itself), reusing `ForgetMemory`'s exact privacy-first algorithm shape:
Evidence deleted even if shared with another retained Fact/Relation/
Episode, orphaned Source cleanup, candidate `from`/`to` Entity orphan
cleanup reusing `delete_entity_if_orphan_non_self` unchanged (already
covers `relations.from_entity_id`/`to_entity_id`), audit-snapshot
scrubbing for any prior `CorrectRelation`/`ConfirmRelation` audit rows
referencing the target, `Delete` confirmation floor (blocks
unconditionally regardless of provenance, no overlay needed), and
`ActionOutcome<RelationId>` never `ActionOutcome<Relation>`.
