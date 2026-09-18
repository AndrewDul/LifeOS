# R024 — M2 Personal Knowledge ForgetMemory Slice E

The fourth production Personal Knowledge mutation: `ForgetMemory`, and
the first HARD-DELETE action in M2. "The user wants this one exact Fact
removed from LifeOS's canonical Personal Knowledge" — implements R019's
final 14-step privacy-safe algorithm exactly. **No other action is
implemented this run** — Relation actions, Episode actions, and
retrieval/Context Pack all remain entirely unstarted.

---

## 1. Pre-run state

R017→R018→R019 (architecture freeze, then two rounds of hardening,
architecture CLOSED for Slice A/B) → R020 (Slice A) → R021 (Slice B:
`AddMemory`, then its own post-run hardening) → R022 (Slice C:
`CorrectMemory`, then its own post-run temporal-integrity/audit
hardening) → R023 (Slice D: `ConfirmMemory`). 375/375 tests passing,
working tree clean before this run. `ForgetMemory` did not exist in any
form before this run.

---

## 2. Exact payload/result type

```rust
pub(crate) struct ForgetMemoryPayload {
    pub fact_id: FactId,
}
```

No `reason`/`value`/`entity id`/`Source`/`Evidence`/snapshot input/
delete-mode/cascade option — targets exactly one Fact, never a bulk
"forget all about X."

**Result is deliberately `ActionOutcome<FactId>`, never `ActionOutcome<
Fact>`.** A success result must never hand back the semantic object
that was just forgotten. `Success.value` is only the deleted `FactId` —
the frozen non-semantic residual audit marker — proven at the type
level: `forget_memory`'s signature makes returning a `Fact` a compile
error, not merely a convention. No structural obstacle existed to this
choice; no STOP was needed.

---

## 3. `Delete` confirmation semantics

Type floor is `ConfirmationClass::Delete` (severity 4, the highest
rung). `effective_class(Delete, provenance)` stays `Delete` regardless
of provenance — never dominated by either provenance-derived class — so
`blocks_until_confirmed(Delete) == true` blocks **every** unconfirmed
call, `Explicit` included. Unlike every other Memory action, a direct
explicit delete request is not itself treated as sufficient
confirmation; only `meta.confirmed == true` executes it. No new
confirmation helper — the exact same generic `effective_class`/
`blocks_until_confirmed` gate every other action already uses, no change
to `ConfirmationClass` or the generic ladder.

---

## 4. No-Undo contract

After a confirmed `ForgetMemory` succeeds there is **no Undo**: no undo
token is generated, no hidden semantic tombstone is retained, the new
audit row's `previous_snapshot` is unconditionally `None`, and no
deleted Evidence is kept as a recovery copy. `ActionOutcome<T>` itself
has no Undo concept for any action to opt out of — confirmed directly
by inspecting the enum (`Success`/`ConfirmationRequired`/
`ValidationFailed`/`NotFound`/`StorageError`, nothing else) — so there
was nothing to disable.

---

## 5. Execution boundary/gate

1. Pure structural validation (`actions::validate_forget_memory` —
   trivial, one `FactId`).
2. `effective_class(Delete, meta.provenance)`.
3. The confirmation gate — blocks unconditionally while `!meta.confirmed`.
4. Only once confirmed does the target Fact get read — missing →
   `ActionOutcome::NotFound`, zero writes, no audit marker ever created
   for a Fact that never existed.

Proven directly with a **nonexistent** `FactId`: both `Explicit`+
unconfirmed and `Inferred`+unconfirmed return `ConfirmationRequired`,
never `NotFound` — the target is never fetched before consent.

---

## 6. Target eligibility (active or historical)

**`domain::is_active_fact` is deliberately never called anywhere in
this action** — unlike `CorrectMemory`/`ConfirmMemory`, `ForgetMemory`
may delete an active `USER_CONFIRMED`/`INFERRED`/`OBSERVED`/`DISPUTED`
Fact, a historical `STALE`/closed Fact, or any Fact at any position
(predecessor/middle/successor/terminal) in a supersession chain — the
right to forget applies to historical knowledge too (R019). Proven
directly for all four active epistemic states plus a raw `STALE`
fixture, and separately across the full supersession-chain matrix (§9).

---

## 7. Exact R019 14-step transaction

Implemented exactly, no alternative ordering:

1. Validate target Fact T exists (service-stage, after the gate; zero
   writes if missing).
2. Capture/deduplicate candidate subject/object `EntityId`s from T.
3. Collect every `EvidenceId` linked to T through `fact_evidence`.
4. Collect the `SourceId` for every collected Evidence row, deduplicated.
5. Scrub existing audit payloads (`previous_snapshot = NULL WHERE
   fact_id = T.id AND previous_snapshot IS NOT NULL`).
6. Clear every inbound `superseded_by_fact_id` reference pointing at T,
   without rewiring.
7. For every collected `EvidenceId`, delete all references from
   `fact_evidence`/`relation_evidence`/`episode_evidence`.
8. Delete all collected Evidence rows.
9. For every collected `SourceId`, delete it only if zero surviving
   Evidence rows reference it.
10. A defensive `DELETE FROM fact_evidence WHERE fact_id = T.id` sweep.
11. `DELETE` the target Fact T (runtime-enforced exactly-one).
12. For every candidate Entity from step 2, delete it iff non-SELF and
    zero surviving references remain.
13. Insert exactly one minimal `ForgetMemory` audit marker
    (`previous_snapshot = NULL`).
14. `COMMIT`.

Steps 1-4 (pure reads) run against `&Connection` before the transaction
opens; steps 5-13 (all writes) run inside one `rusqlite::Transaction`,
matching `AddMemory`/`CorrectMemory`/`ConfirmMemory`'s exact "resolve
first, mutate inside one transaction" precedent. FK ordering is safe by
construction: step 6 (clearing inbound `superseded_by_fact_id`) must
precede step 11 (deleting T) since `facts.superseded_by_fact_id
REFERENCES facts(id) ON DELETE RESTRICT`; step 7 (deleting join rows)
must precede step 8 (deleting Evidence) since `fact_evidence`/
`relation_evidence`/`episode_evidence`.`evidence_id REFERENCES
evidence(id) ON DELETE RESTRICT`; step 11 (deleting T) must precede
step 12 (Entity cleanup) since T's own subject/object reference would
otherwise always count as "surviving."

---

## 8. Audit snapshot scrubbing

Step 5 scrubs **every** `fact_audit_log.previous_snapshot` for rows
where `fact_id = T.id` and the snapshot is non-`NULL` — covering
payloads left by `CorrectMemory`/`ConfirmMemory` (or any future
snapshot-bearing action targeting the same `fact_id`). Only
`previous_snapshot` is touched; every other column (`id`/`action_type`/
`actor`/`provenance`/`confirmation_class`/`occurred_at`) survives
untouched — old audit rows become payload-free structural history, never
deleted outright. Proven directly for both a `ConfirmMemory`-sourced
snapshot (`req_fm_snapshot_scrub_confirmmemory_audit`) and a
`CorrectMemory`-sourced snapshot (`req_fm_snapshot_scrub_
correctmemory_audit`, using an A→B→C chain where B's own audit row from
correcting B→C carries B's pre-correction snapshot), plus a dedicated
full-column-except-snapshot equality proof
(`req_fm_old_audit_structural_metadata_survives_scrub`). After success,
every `fact_audit_log` row for that `fact_id` — including the new
`ForgetMemory` marker itself — shows `previous_snapshot IS NULL`.

---

## 9. Inbound supersession handling

Step 6 (`storage::personal_knowledge_repo::clear_inbound_supersession_
refs`) sets `superseded_by_fact_id = NULL`/`updated_at = <forget's
requested_at>` for every Fact whose `superseded_by_fact_id` pointed at
T — **never rewired** to some other Fact further down the chain (that
would falsify history; ForgetMemory deliberately creates a privacy-
driven hole). May affect 0..N rows (a standalone or terminal Fact has
none) — unlike `CorrectMemory`'s exactly-one-row guarantee. Proven
across the full chain matrix (§22 test list below): forgetting a
standalone Fact; forgetting the **middle** of A→B→C (A's `superseded_by`
clears to `NULL`, never becomes C; A's `valid_until`/`epistemic_state`
stay exactly as they were; C is untouched); forgetting a **predecessor**
of A→B (B is completely unaffected); forgetting the **terminal** Fact of
A→B→C (B's `superseded_by` clears, B stays `Stale`/closed; A further up
the chain is untouched). No historical row is ever revived.

---

## 10. Evidence privacy-first deletion

Every Evidence row linked to T is **always** deleted, even if that
Evidence was also linked to another retained Fact, Relation, or Episode
— privacy of the forgotten Evidence outranks preservation of shared
supporting Evidence (R018/R019). The join rows are removed first (step
7), then the Evidence rows themselves (step 8), so the retained
Fact/Relation/Episode OBJECT survives — it merely loses that one piece
of Evidence support. No semantic inspection of Evidence content, no AI
judgment, no conditional retention. `V7` does not enforce "exactly one
Evidence per Fact," so 0..N Evidence rows are all supported and tested:
zero (a raw fixture with its `fact_evidence` link removed), one (the
normal case, exercised throughout), and two sharing one Source (§11's
case C).

---

## 11. Source orphan/shared lifecycle

Step 9 (`delete_source_if_orphan`) is one guarded, atomic `DELETE ...
WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM evidence WHERE source_id =
?1)` — no separate check-then-delete, naturally idempotent if ever
called twice for the same id. Proven by a three-case matrix
(`req_fm_source_matrix_orphan_and_shared`): (A) a unique Source becomes
orphaned and is deleted; (B) a Source shared with another retained
Fact's Evidence (via `SourceRef::Existing`) remains byte-for-byte
unchanged, and the retained Fact's own Evidence link is untouched; (C)
two Evidence rows belonging to the same target share one orphan Source
— deleted exactly once, safely, no duplicate-delete failure.

---

## 12. Entity orphan/shared lifecycle

Step 12 (`delete_entity_if_orphan_non_self`) is one guarded, atomic
`DELETE` enforcing the exact retention predicate (§13) — `is_self = 0`
is part of the guard itself, so this function can **never** delete
SELF regardless of what id is passed. Only the target's own candidate
subject/object Entities are ever considered — no arbitrary scan of
other Entities. Proven across ten lettered cases (§25 below): SELF
survives; an orphaned non-SELF subject/object is deleted; a self-edge
(same Entity as both subject and object, deduplicated) is deleted once;
an Entity referenced by another Fact's subject/object, a `Relation.from`/
`Relation.to`, or `episode_entities` is retained; an Entity with a
`domain_ref` but zero surviving Personal Knowledge references is
deleted anyway, while the authoritative Goal/Project/Milestone/Task row
itself remains completely untouched.

---

## 13. Exact Entity retention predicate

A candidate Entity is retained if **any** surviving row references it
via `facts.subject_entity_id`, `facts.object_entity_id`,
`relations.from_entity_id`, `relations.to_entity_id`, or
`episode_entities.entity_id` — otherwise deleted, provided `is_self ==
false`. `domain_ref` alone never retains an otherwise-orphaned Entity —
proven directly (case J, §12/§25).

---

## 14. Hard Fact deletion

`storage::personal_knowledge_repo::delete_fact_forget_target` is the
only hard-delete path for a `facts` row: `DELETE FROM facts WHERE id =
?1`, with `changed != 1` a real runtime error in every build profile
(matching `update_fact_as_superseded`/`confirm_fact_epistemic_state`'s
established discipline — never a `debug_assert!`). Proven:
`get_fact(T.id)` returns `None`, a raw `SELECT count(*) FROM facts
WHERE id = T.id` returns `0`, and the total `facts` row count decreases
by exactly one — no tombstone row, no `deleted_at` marker, no `Stale`
replacement, no successor Fact of any kind.

---

## 15. Minimal audit marker

Exactly one `fact_audit_log` row is inserted after the target is
deleted: `fact_id = T.id`, `action_type = "ForgetMemory"`, `actor =
meta.actor`, `provenance = meta.provenance`, `confirmation_class =
"DELETE"`, `previous_snapshot = NULL` unconditionally, `occurred_at =
meta.requested_at`. This works precisely because `fact_audit_log.fact_id`
intentionally has no foreign key to `facts` (R017 §20) — no FK was
added, none is needed. Proven with a deliberately fixed `ActionMeta`
(`Actor::CloudLlm`, `Provenance::Explicit`, a fixed RFC3339
`requested_at`), asserting all seven columns exactly.

---

## 16. Audit provenance behavior

For `provenance = Inferred`/`confirmed = true`, the deletion executes
and the audit row's `provenance` stays `"Inferred"` — never rewritten to
`"Explicit"`. Confirmation approval and proposal origin remain distinct
concepts, the same guarantee `AddMemory`/`CorrectMemory`/`ConfirmMemory`
already established.

---

## 17. Logical deletion guarantee

After a successful `ForgetMemory`, every reachable product-level
guarantee holds (proven across the full test suite, not asserted in the
abstract): the target Fact is absent; every Evidence row linked to it is
absent; every join row referencing those Evidence ids is absent; every
orphaned collected Source is absent while every shared collected Source
is retained; every orphaned candidate non-SELF Entity is absent while
every shared candidate Entity is retained; SELF is retained; every old
audit row targeting this Fact has `previous_snapshot = NULL`; the new
`ForgetMemory` marker has `previous_snapshot = NULL`; the target can
never again be returned by `get_fact`. This is **"true deletion from
LifeOS's canonical Personal Knowledge model"** — a logical/product-level
guarantee, never described as forensic/physical-media erasure.

---

## 18. Forensic-storage boundary

`PRAGMA secure_delete = ON` (R020) is unchanged and unverified-again by
this run only in the sense that no new test re-proves it — it was
already proven in R020/R021/R022's suites and nothing in this run
alters `open_encrypted`. No `VACUUM`, no journal-mode change, no WAL
change, no SSD-erase command was added. `ForgetMemory`'s guarantee
remains exactly logical/product-level deletion from the canonical
database plus the existing SQLite-level `secure_delete` hardening —
never a claim of impossible SSD/filesystem/snapshot erasure.

---

## 19. Backup/sync boundary

There is no production managed backup/sync/export subsystem in this
codebase, and none was implemented this run. `ForgetMemory` applies
only to the active canonical database it operates on directly. A future
LifeOS-managed backup/sync system **must** propagate forgetting and
prevent an ordinary restore from silently resurrecting forgotten data —
that is a requirement on a not-yet-built future subsystem, not a
guarantee this action can make about copies LifeOS does not itself
control.

---

## 20. Transaction rollback proof

**Mandatory central proof**,
`req_fm_rollback_forced_final_audit_failure_restores_everything`: a rich
pre-call state is constructed — predecessor P, target T created as a
`CorrectMemory` successor of P (so T carries an inbound predecessor and
its own object Entity created fresh at that correction), T further
corrected into U (giving T its own payload-bearing `CorrectMemory` audit
snapshot targeting `fact_id = T.id`, and making T historical/`Stale`).
The `fact_audit_log` forced-failure trigger (reused unchanged from
`AddMemory`/`CorrectMemory`/`ConfirmMemory`'s own suite) fires on the
final `ForgetMemory` audit insert. After the `StorageError`: every table
is confirmed via `am_full_table_snapshot` to be exactly as before the
failed call; T still exists byte-for-byte unchanged; P's
`superseded_by_fact_id` is restored; T's Evidence/Source/`fact_evidence`
link all still exist; the candidate orphan Entity still exists; T's own
`CorrectMemory` audit snapshot is restored (the scrub rolled back too);
no `ForgetMemory` audit marker exists; the connection remains usable
afterward.

---

## 21. Mid-step rollback proof

`req_fm_rollback_mid_step_forced_fact_delete_failure`: a new test-only
trigger (`BEFORE DELETE ON facts`, `RAISE(FAIL, ...)`) forces step 11
itself (deleting the target Fact) to fail, **after** steps 5-10 (audit
scrub, inbound-supersession clearing, Evidence/Source deletion) have
already executed within the same transaction. Proves the earlier steps
roll back too, not only a final-step failure: every table is restored
exactly, and predecessor P's `superseded_by_fact_id` clearing is
reverted.

---

## 22. Supersession-chain test matrix

`req_fm_supersession_chain_forgetting_middle_fact` (A→B→C, forget B: A's
`superseded_by` clears to `NULL`, never C; A's `valid_until`/
`epistemic_state` unchanged; A's `updated_at` equals the forget call's
`requested_at`; C untouched), `req_fm_forgetting_predecessor_leaves_
successor_unchanged` (A→B, forget A: B completely unaffected),
`req_fm_forgetting_terminal_fact_clears_immediate_predecessor_only`
(A→B→C, forget C: B's `superseded_by` clears, B stays `Stale`/closed; A
further up the chain untouched).

---

## 23. Evidence sharing tests

`req_fm_relation_episode_shared_evidence_privacy_proof` (raw Relation/
Episode fixtures sharing the target's Evidence — Evidence and every
join row deleted, Relation/Episode rows themselves survive),
`req_fm_other_fact_shared_evidence_deleted_but_other_fact_survives` (a
raw fixture linking a retained Fact K to the target's own Evidence — the
shared Evidence is still deleted, K survives with zero Evidence links
in this unreachable-by-production fixture, matching R019's explicit
"privacy outranks shared-Evidence preservation" instruction),
`req_fm_zero_evidence_fixture_forgotten_safely` (a raw fixture with no
linked Evidence at all).

---

## 24. Source sharing tests

See §11 — `req_fm_source_matrix_orphan_and_shared`'s three cases (A/B/C).

---

## 25. Entity retention test matrix

`req_fm_entity_retention_matrix_self_and_orphan_cases` (A: SELF subject
only, survives; B: orphaned non-SELF subject, deleted; C: orphaned
non-SELF object, deleted; D: a raw self-edge fixture — same Entity as
both subject and object, deduplicated, deleted once, no double-delete
failure) and `req_fm_entity_retention_matrix_shared_reference_cases` (E:
retained via another Fact's subject; F: retained via another Fact's
object; G: retained via `Relation.from`; H: retained via `Relation.to`;
I: retained via `episode_entities`; J: `domain_ref` alone does not
retain an orphan, the authoritative Goal row survives untouched).

---

## 26. Audit scrub tests

See §8 — the two snapshot-scrub tests plus the dedicated structural-
metadata-survives test.

---

## 27. Confirmation matrix

Five scenarios, matching the brief's own lettering: **A** (Explicit,
unconfirmed, nonexistent `FactId`) → `ConfirmationRequired(Delete)`,
zero writes; **B** (Explicit, confirmed, missing Fact) → `NotFound`;
**C** (Explicit, confirmed, existing Fact) → `Success(FactId)`; **D**
(Inferred, unconfirmed, nonexistent `FactId`) → `ConfirmationRequired
(Delete)`, zero writes; **E** (Inferred, confirmed) → `Success(FactId)`,
audit `provenance = Inferred`, `confirmation_class = Delete`.

---

## 28. Storage helpers added

All `pub(in crate::services)`, each doing exactly one thing (no generic
`delete_entity`/`update_fact`/destructive public API):
`scrub_fact_audit_snapshots`, `clear_inbound_supersession_refs`,
`list_evidence_ids_for_fact`, `get_source_id_for_evidence`,
`delete_evidence_links_for_evidence`, `delete_evidence`,
`delete_source_if_orphan`, `delete_fact_evidence_links_for_fact`,
`delete_fact_forget_target`, `delete_entity_if_orphan_non_self`.

---

## 29. Tests added

**23 new tests**: 1 pure, no-I/O test in `actions::tests`
(`forget_memory_payload_validates_trivially_and_passes_fact_id_
through`) and 22 database-integration tests in `services::tests`
(§20-§27 above): the five confirmation-matrix tests, target-eligibility,
audit-marker-fields, hard-delete-proof, three chain tests, zero-evidence
fixture, two Evidence-sharing tests, the source matrix, the two entity-
retention matrices, the two snapshot-scrub tests plus the structural-
metadata test, and the two rollback proofs.

---

## 30. Final total test count

**Previous: 375. New: 23. Final: 398. Failed: 0.** Confirmed by running
the full suite (`cargo test -- --test-threads=1`) both immediately
before this run's first edit and after every subsequent change, not
estimated.

---

## 31. Build/package results

- `cargo fmt --check`: clean (after one `cargo fmt` pass).
- `cargo check --tests`: clean, zero warnings, at every intermediate
  edit and the final state.
- `cargo test -- --test-threads=1`: **398 passed; 0 failed; 0 ignored**.
- `cargo build --release`: succeeded (2m33s).
- `npm run build`: succeeded.
- `npm run tauri build`: run this pass (required — new production Core
  action and hard-deletion storage path) — see the chat reply for the
  measured MSI/NSIS result, verified from the captured log and the
  actual bundle files on disk. No Tauri command was added
  (`ForgetMemory` is Core-only) — this build only proves packaging/
  regression.

---

## 32. Files changed

- `src-tauri/src/actions/mod.rs` — `ForgetMemoryPayload`/
  `NormalizedForgetMemory`, `validate_forget_memory` (trivial, no new
  `ValidationError` variant needed at all).
- `src-tauri/src/actions/tests.rs` — 1 new direct unit test.
- `src-tauri/src/services/mod.rs` — `forget_memory` (the action), new
  import (`ForgetMemoryPayload` from `actions`). Reuses `effective_
  class`/`blocks_until_confirmed` unchanged — no new confirmation
  helper.
- `src-tauri/src/services/storage/personal_knowledge_repo.rs` — 10 new
  narrow functions (§28).
- `src-tauri/src/services/tests.rs` — 22 new `ForgetMemory` integration
  tests (§29), new fixtures/helpers (`mk_fm_payload`/`confirmed_
  explicit`/`mk_fm_target`/`mk_fm_second_entity`/`insert_raw_relation`/
  `insert_raw_episode`/`install_fact_delete_failure_trigger`), new
  import (`ForgetMemoryPayload`).
- `docs/reports/R024_M2_PERSONAL_KNOWLEDGE_FORGETMEMORY_SLICE_E_2026-09-18.md`
  (this report).
- `docs/PROJECT_STATE.md`/`docs/LIFEOS_SPEC.md` — updated (see chat
  reply/commit for detail).

**`V7__create_personal_knowledge.sql` was not modified** — it already
supported `ForgetMemory` completely (non-FK `fact_audit_log.fact_id`,
`ForgetMemory` already in the `action_type` `CHECK`, `ON DELETE
RESTRICT` everywhere it needed to be); no V8 was created. All 375 prior
tests remained green throughout — `AddMemory`/`CorrectMemory`/
`ConfirmMemory`'s semantics and `secure_delete` are byte-for-byte
unchanged.

---

## 33. Remaining Personal Knowledge work

All four frozen Memory actions (`AddMemory`/`CorrectMemory`/
`ConfirmMemory`/`ForgetMemory`) are now implemented and proven. Relation
actions (Slice F), Episode actions (Slice G), and retrieval/
`get_relevant_memory`/Context Pack (Slice H) all remain entirely
unstarted, unchanged from R019 §29's recommended order — `Relation`/
`Episode` and their four join tables remain schema-only, with zero
production write path for either.

---

## 34. Exact recommendation for the next slice

**STOP after Slice E — Relation actions, Episode actions, and
retrieval/Context Pack were NOT started this run**, per the run's own
instruction; this section only recommends, it does not begin any of
them. Of the three frozen-roadmap candidates:

- **Relation action-family freeze/implementation** — `relations` is
  schema-complete (R017 §8/R018 §8) but its action set (create/update/
  confirm/correct/forget equivalents for a Relation, and exactly which
  of `ConfirmMemory`/`CorrectMemory`/`ForgetMemory`'s patterns transfer
  unchanged versus need their own design decision) was never frozen —
  it needs its own dedicated freeze pass first, the same discipline
  R017-R019 applied to Personal Knowledge before any Fact-action code
  was written.
- **Episode action-family freeze/implementation** — same reasoning;
  `episodes`/`episode_evidence`/`episode_entities` are schema-complete
  but have no frozen action set either.
- **Retrieval/`get_relevant_memory`/Context Pack** — this is the first
  slice that makes Personal Knowledge actually *useful* to Qwen/Luna
  (R017 §27's read-only proposal path depends on some retrieval
  mechanism existing), but it depends on having real Fact data to
  retrieve meaningfully, and its own privacy-firewall shape (what a
  Context Pack may include, how sensitivity gates it) was only ever
  sketched at the architecture level, never frozen to the same
  field-level precision the Fact actions received.

Given all four Memory actions are now done, **Relation** is the most
natural next step (it reuses the largest fraction of already-proven
machinery — Entity resolution, the same confirmation-class/audit
patterns, `Evidence`'s already-implemented lifecycle), but it explicitly
requires its own freeze pass before any code, exactly as this run's
instructions require. This recommendation is not a decision to proceed.
