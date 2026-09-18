# R023 — M2 Personal Knowledge ConfirmMemory Slice D

The third production Personal Knowledge mutation: `ConfirmMemory`. "The
semantic assertion already stored in this Fact is correct according to
the user" — a pure epistemic confirmation, never a semantic-value
correction, never a new/successor Fact, never a contradiction-resolution
workflow, never an Evidence-creation operation. Mutates the SAME `Fact`
row in place. **No other Memory action is implemented this run** —
`ForgetMemory` remains entirely unstarted.

---

## 1. Pre-run state

R017→R018→R019 (architecture freeze, then two rounds of hardening,
architecture CLOSED for Slice A/B) → R020 (Slice A: `V7` schema, SELF
bootstrap, narrow reads, `secure_delete`) → R021 (Slice B: `AddMemory`,
then its own post-run correctness/verification hardening) → R022 (Slice
C: `CorrectMemory`, then its own post-run temporal-integrity/audit
hardening). 361/361 tests passing, working tree clean before this run.
`ConfirmMemory` did not exist in any form before this run.

---

## 2. Exact payload

```rust
pub(crate) struct ConfirmMemoryPayload {
    pub fact_id: FactId,
}
```

No other field — no `value`/`subject`/`predicate`/`confidence`/
`sensitivity`/`memory_type`/`Source`/`Evidence`/`epistemic_state`
input/timestamp. Core decides the one permitted state transition
(`epistemic_state → UserConfirmed`) unconditionally; there is nothing
else for a caller to supply.

---

## 3. `ExplicitAdd` floor

Type floor is `ConfirmationClass::ExplicitAdd` — the same floor
`AddMemory` has. For `Provenance::Explicit`, the action executes
immediately under the existing `ExplicitAdd` policy: a direct "yes,
that's correct" from the user IS the confirmation, and no second
blocking confirmation is introduced for it.

---

## 4. Inferred confirmation behavior

For `Provenance::Inferred` and `meta.confirmed == false`,
`effective_class(ExplicitAdd, Inferred) = InferredChange`, and the
*generic* `blocks_until_confirmed(InferredChange) = true` already
blocks — exactly the same mechanism `AddMemory` relies on, and for the
same reason: `ExplicitAdd`'s severity (1) is dominated by
`InferredChange`'s severity (2). **`ConfirmMemory` therefore needs no
Personal-Knowledge-specific confirmation overlay** (unlike
`CorrectMemory`'s `ImportantChange` floor, which is NOT dominated and
needed `memory_action_requires_confirmation`). `services::confirm_memory`
uses the plain `blocks_until_confirmed(class) && !meta.confirmed` check,
byte-for-byte the same shape `add_memory` already uses — no new helper,
no change to `ConfirmationClass`/`effective_class`/
`blocks_until_confirmed` themselves.

---

## 5. Execution order

1. Pure structural validation (`actions::validate_confirm_memory` —
   trivial, the payload is one `FactId`, nothing to check without a
   database read).
2. `effective_class(ExplicitAdd, meta.provenance)`.
3. The confirmation gate (§4).
4. Only once confirmation is satisfied: read the target `Fact`
   (`storage::personal_knowledge_repo::get_fact`); missing →
   `ActionOutcome::NotFound` immediately, matching `CorrectMemory`'s
   precedent for a missing update target.
5. `domain::is_active_fact` rejects an inactive target
   (`ValidationError::FactNotActive`, reused unchanged); an active
   target already `UserConfirmed` is rejected as a no-op
   (`ValidationError::FactAlreadyUserConfirmed`).
6. The mandatory `previous_snapshot` of the target's PRE-confirmation
   state is serialized (`serialize_required_snapshot`, never
   `.ok()`-downgraded).
7. The atomic transaction opens.

Proven directly: `req_cfm_conf_b_inferred_unconfirmed_blocks_before_
fact_lookup` uses a **nonexistent** `FactId` with `Inferred`+unconfirmed
and asserts `ConfirmationRequired` (never `NotFound`) — the target is
never even fetched while unconfirmed.

---

## 6. Active-Fact reuse

`domain::is_active_fact` (frozen R022, unchanged) is reused verbatim —
`fact.valid_until.is_none() && fact.epistemic_state != EpistemicState::
Stale`. No duplicated expression anywhere in `ConfirmMemory`'s own code.

---

## 7. Valid state transitions

For an active Fact: `INFERRED → USER_CONFIRMED`, `OBSERVED →
USER_CONFIRMED`, `DISPUTED → USER_CONFIRMED` — all confirmable, all
proven directly in one table-driven test
(`req_cfm_state_transition_matrix`). No other frozen `EpistemicState`
value exists for an active Fact to transition from (an active Fact by
definition is never `Stale`; `UserConfirmed` is the no-op case, §8).

---

## 8. Already-confirmed no-op rejection

Confirming a Fact already `epistemic_state = UserConfirmed` returns
`ValidationError::FactAlreadyUserConfirmed` — a new, narrow variant,
distinct from `FactNotActive` (an already-confirmed Fact IS active; it
is simply not a valid *transition* target). Zero writes, no new audit
row, never a silently-returned `Success`.

---

## 9. `DISPUTED` semantics (R018 critical clarification)

Confirming one `DISPUTED` Fact changes only that Fact's own
`epistemic_state` to `USER_CONFIRMED`. No sibling Fact sharing the same
`subject_entity_id`+`predicate` is ever read, searched, changed,
deleted, or marked `Stale`/auto-confirmed — no contradiction set is
"resolved" globally. `req_cfm_disputed_sibling_untouched` creates two
`DISPUTED` Facts sharing subject+predicate (different values), confirms
one, and proves the other remains byte-for-byte unchanged.
`ConfirmMemory` confirms exactly one assertion — contradiction detection
is explicitly not implemented, and never will be by this action.

---

## 10. Exact in-place mutation contract

Exactly two fields change on the same row: `epistemic_state` (always to
`UserConfirmed`) and `updated_at` (to `meta.requested_at`). Everything
else is proven identical before/after in
`req_cfm_mutation_contract_full_field_proof`: `id`, `subject_entity_id`,
`predicate`, `value`, `memory_type`, `confidence`, `sensitivity`,
`valid_from`, `valid_until`, `superseded_by_fact_id`, `created_at` — a
full struct-field comparison, not merely "the action reports success."

---

## 11. Guarded storage `UPDATE`

A new, deliberately narrow repo function,
`storage::personal_knowledge_repo::confirm_fact_epistemic_state(conn,
fact_id, confirmed_at)`, matching `update_fact_as_superseded`'s exact
narrow-storage-function discipline (R022):

```sql
UPDATE facts SET epistemic_state = 'USER_CONFIRMED', updated_at = ?2
WHERE id = ?1 AND valid_until IS NULL AND epistemic_state <> 'STALE'
  AND epistemic_state <> 'USER_CONFIRMED'
```

No generic `update_fact()` — the `WHERE` clause re-checks, at mutation
time, both preconditions the service layer already checked at
preflight (active, not already confirmed), as defense-in-depth,
independent of whether the service's own preflight is later changed.

---

## 12. Runtime affected-row enforcement

`changed != 1` is a real runtime error in every build profile — never a
`debug_assert!` — returning a typed `rusqlite::Error::SqliteFailure`
with a fixed, non-sensitive diagnostic
(`"ConfirmMemory update did not affect exactly one row - Fact missing,
no longer active, or already confirmed"`), matching the exact R022
post-run hardening precedent for `update_fact_as_superseded`. Proven
directly by `req_cfm_repo_confirm_fact_epistemic_state_matrix` (calling
the repo function directly, not only through `confirm_memory`): active
`INFERRED`/`OBSERVED`/`DISPUTED` all succeed with exactly one row
changed; `USER_CONFIRMED`, `STALE`-with-`NULL`-`valid_until`, a
`valid_until`-set-but-not-`STALE` fixture, and a nonexistent `FactId`
all return `Err` with zero fields changed.

---

## 13. Mandatory `previous_snapshot`

`serialize_required_snapshot(&target)` runs after all preflight checks
succeed but strictly before the transaction opens — never
`.ok()`-downgraded; a serialization failure returns `StorageError` with
zero writes. Proven to deserialize exactly back to the target's
PRE-confirmation state in `req_cfm_audit_fields_and_previous_snapshot_
exactness`.

---

## 14. Audit fields

One `fact_audit_log` row per confirmation: `fact_id = target.id`,
`action_type = "ConfirmMemory"`, `actor = meta.actor`, `provenance =
meta.provenance`, `confirmation_class = <effective class>`,
`previous_snapshot = Some(<serialized pre-confirmation Fact>)`,
`occurred_at = meta.requested_at`. Proven with a deliberately fixed
`ActionMeta` (`Actor::CloudLlm`, `Provenance::Explicit`, a fixed RFC3339
`requested_at`) asserting all seven columns exactly, including
`confirmation_class == "EXPLICIT_ADD"` and `occurred_at ==
requested_at.to_rfc3339()`.

---

## 15. Inferred provenance behavior

For a confirmed `Inferred` proposal, the target's `epistemic_state`
becomes `UserConfirmed` while the audit row's `provenance` stays
`"Inferred"` and `confirmation_class` stays `"INFERRED_CHANGE"` —
confirmation and provenance remain distinct concepts, matching
`AddMemory`/`CorrectMemory`'s identical guarantee. Proven directly in
`req_cfm_conf_c_inferred_confirmed_executes_and_audit_provenance_
remains_inferred`.

---

## 16. Atomic transaction

After preflight and mandatory snapshot serialization succeed: `BEGIN` →
`confirm_fact_epistemic_state` (the guarded update) → one
`fact_audit_log` row → `COMMIT`. Only two steps — no Entity/Source/
Evidence/`fact_evidence` row is ever created by this action.

---

## 17. Rollback proof

**Mandatory central proof**,
`req_cfm_rollback_forced_audit_failure_rolls_back_fact_state`: the
`fact_audit_log` forced-failure trigger (reused unchanged from
`AddMemory`/`CorrectMemory`'s own suite) fires on the audit insert.
After the `StorageError`: the target Fact is confirmed byte-for-byte
unchanged (including `epistemic_state` and `updated_at` — no partial
confirmation), zero `ConfirmMemory` audit rows exist, and the connection
remains usable (a subsequent `add_memory` call succeeds).

A genuine service-level "guard fires mid-transaction" race proof (R022
§22's style, hooking a trigger onto an earlier insert step) was
considered and explicitly **not attempted**: `ConfirmMemory`'s
transaction has only two steps (the guarded update itself is the FIRST
write), so there is no earlier insert to hook a same-transaction trigger
onto before the guarded update runs, unlike `CorrectMemory`'s six-step
transaction. This is documented directly in the test file rather than
forcing an artificial seam. The direct repo-level matrix (§12) already
proves the guard's own `Result`-returning behavior exhaustively.

---

## 18. Guarded-update test matrix

See §12 — `req_cfm_repo_confirm_fact_epistemic_state_matrix`'s seven
cases (active `INFERRED`/`OBSERVED`/`DISPUTED` succeed;
`USER_CONFIRMED`/`STALE`-with-`NULL`-`valid_until`/closed-but-not-
`STALE`/nonexistent-id all `Err` with zero fields changed).

---

## 19. Confirmation matrix

Four scenarios, matching the brief's own lettering:

- **A** (Explicit, unconfirmed): executes immediately, `Success`,
  `confirmation_class = ExplicitAdd`.
- **B** (Inferred, unconfirmed): `ConfirmationRequired {
  InferredChange }`, using a nonexistent `FactId` to prove the gate
  precedes the lookup — zero writes.
- **C** (Inferred, confirmed): executes, `UserConfirmed`, audit
  `provenance` stays `Inferred`, `confirmation_class = InferredChange`.
- **D** (Explicit, missing Fact): `ActionOutcome::NotFound`.

---

## 20. Epistemic transition matrix

See §7/§8 — `req_cfm_state_transition_matrix` covers `INFERRED`/
`OBSERVED`/`DISPUTED → USER_CONFIRMED`, the `USER_CONFIRMED` no-op
rejection, `STALE` rejection, and the closed-active-looking raw-fixture
rejection, all in one table-driven test. No new `FactId` is ever
produced by any of these.

---

## 21. Identity/no-successor proof

`req_cfm_identity_no_temporal_successor`: `returned.id ==
payload.fact_id`; the `facts` row count is identical before/after
(in-place mutation, no successor); `valid_from`/`valid_until`/
`superseded_by_fact_id` are all identical before/after — this is
metadata confirmation, not temporal correction, and `ConfirmMemory`
never touches any of the three temporal/supersession fields
`CorrectMemory` owns.

---

## 22. No Entity/Source/Evidence side effects

`req_cfm_no_entity_source_evidence_side_effects`: `entities`/`sources`/
`evidence`/`fact_evidence` row counts are all identical before/after;
`facts` row count is unchanged (in-place mutation); `fact_audit_log`
gains exactly one row. The target's own pre-existing Evidence/Source
(from whatever created it) remain completely untouched.

---

## 23. Sibling `DISPUTED` proof

See §9 — `req_cfm_disputed_sibling_untouched`.

---

## 24. Timestamp proof

Using a deliberately fixed `ActionMeta.requested_at` (not `Utc::now()`),
`req_cfm_audit_fields_and_previous_snapshot_exactness` proves
`current.updated_at == requested_at` exactly and
`fact_audit_log.occurred_at == requested_at.to_rfc3339()` exactly, in
the same test as the audit-field and snapshot-exactness proofs (§13-§14)
— no separate narrower timestamp-only test was added, since it would
have duplicated this one's assertions with zero additional coverage.

---

## 25. Exact tests added

**14 new tests**: 1 pure, no-I/O test in `actions::tests`
(`confirm_memory_payload_validates_trivially_and_passes_fact_id_
through`) and 13 database-integration tests in `services::tests`
(§19-§23 above): `req_cfm_conf_a`-`req_cfm_conf_d` (confirmation
matrix), `req_cfm_state_transition_matrix`, `req_cfm_identity_no_
temporal_successor`, `req_cfm_mutation_contract_full_field_proof`,
`req_cfm_no_entity_source_evidence_side_effects`, `req_cfm_no_
relation_episode_side_effects`, `req_cfm_audit_fields_and_previous_
snapshot_exactness`, `req_cfm_disputed_sibling_untouched`,
`req_cfm_rollback_forced_audit_failure_rolls_back_fact_state`,
`req_cfm_repo_confirm_fact_epistemic_state_matrix`.

---

## 26. Final test count

**Previous: 361. New: 14. Final: 375. Failed: 0.** Confirmed by running
the full suite (`cargo test -- --test-threads=1`) both immediately
before this run's first edit and after every subsequent change, not
estimated.

---

## 27. Builds/package results

- `cargo fmt --check`: clean (after one `cargo fmt` pass).
- `cargo check --tests`: clean, zero warnings, at every intermediate
  edit and the final state.
- `cargo test -- --test-threads=1`: **375 passed; 0 failed; 0 ignored**.
- `cargo build --release`: succeeded (2m9s).
- `npm run build`: succeeded.
- `npm run tauri build`: run this pass (required — new production Core
  action and storage mutation path) — see the chat reply for the
  measured MSI/NSIS result, verified from the captured log and the
  actual bundle files on disk. No Tauri command was added
  (`ConfirmMemory` is Core-only) — this build only proves packaging/
  regression.

---

## 28. Files changed

- `src-tauri/src/domain/mod.rs` — `Fact`'s mutability-contract doc
  comment updated to mark `ConfirmMemory` implemented (§34 of the
  brief), listing its exact two mutated fields.
- `src-tauri/src/actions/mod.rs` — `ConfirmMemoryPayload`/
  `NormalizedConfirmMemory`, one new `ValidationError` variant
  (`FactAlreadyUserConfirmed`), `validate_confirm_memory` (trivial, no
  database read).
- `src-tauri/src/actions/tests.rs` — 1 new direct unit test.
- `src-tauri/src/services/mod.rs` — `confirm_memory` (the action), new
  import (`ConfirmMemoryPayload` from `actions`). Reuses `is_active_
  fact`/`serialize_required_snapshot`/`blocks_until_confirmed`/
  `effective_class` unchanged - no new shared helper was needed (unlike
  `CorrectMemory`'s `memory_action_requires_confirmation`).
- `src-tauri/src/services/storage/personal_knowledge_repo.rs` — new
  `confirm_fact_epistemic_state` — the only function that can mutate an
  existing Fact's `epistemic_state` to `UserConfirmed`.
- `src-tauri/src/services/tests.rs` — 13 new `ConfirmMemory` integration
  tests (§25), new fixtures/helpers (`mk_cfm_payload`/`mk_cfm_target`),
  new import (`ConfirmMemoryPayload`).
- `docs/reports/R023_M2_PERSONAL_KNOWLEDGE_CONFIRMMEMORY_SLICE_D_2026-09-18.md`
  (this report).
- `docs/PROJECT_STATE.md`/`docs/LIFEOS_SPEC.md` — updated (see chat
  reply/commit for detail).

**`V7__create_personal_knowledge.sql` was not modified** — it already
supported `ConfirmMemory` completely (`fact_audit_log.action_type`
already included `'ConfirmMemory'` in its `CHECK`); no V8 was created.
All 361 prior tests remained green throughout — `AddMemory`'s
confirmation behavior and `CorrectMemory`'s temporal behavior/storage
guard are byte-for-byte unchanged.

---

## 29. Remaining Personal Knowledge work

Slice E (`ForgetMemory`'s full 14-step deletion algorithm, R019 §10),
F (`Relation` actions), G (`Episode` actions), H (retrieval/Context
Pack) all remain entirely unstarted, unchanged from R019 §29/R020 §23/
R021 §24/R022 §32's recommended order. `Relation`/`Episode` and their
four join tables remain schema-only.

---

## 30. Exact recommendation for Slice E — ForgetMemory

**STOP after Slice D — `ForgetMemory` was explicitly not started this
run**, per the run's own instruction. `ForgetMemory` is qualitatively
different from every action implemented so far: it is the first
*deletion* action (`ConfirmationClass::Delete`, the one severity rung
above `ImportantChange` that no Memory action has exercised yet), it
must implement R019 §10's full multi-step deletion algorithm (true
deletion of the target Fact, orphan-Entity cleanup, and — critically —
scrubbing `fact_audit_log.previous_snapshot` for every audit row
targeting the deleted Fact, exactly the property R017 §20/R021/R022's
non-enforced-FK `fact_id` column was designed to make possible), and it
needs its own careful review of exactly which rows besides the Fact
itself (Evidence? `fact_evidence` links? an orphaned Entity with no
other referencing Fact?) are safe to delete versus must be preserved.
This is a substantially larger design/implementation surface than
Slices B-D and deserves its own dedicated read-first pass over R019 §8-
§11 before any code, not a direct continuation of this run's pattern.
