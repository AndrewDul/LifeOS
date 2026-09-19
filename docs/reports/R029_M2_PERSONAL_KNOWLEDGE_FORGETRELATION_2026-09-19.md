# R029 — M2 Personal Knowledge ForgetRelation

**Type:** Implementation (production Rust + tests). No schema/migration
change — V8's `relation_audit_log.action_type` CHECK already enumerated
`'ForgetRelation'` (R026, forward-looking). Implements exactly the scope
R025 froze for `ForgetRelation` — the final action in the frozen v0.1
Relation action family. Does **not** implement Episode, retrieval,
Tauri commands, or UI.

---

## 1. Pre-run verified checkpoint

CONFIRMED FACT: clean working tree at commit `9e098c9` ("docs: correct
R028 test-fix wording") before this run began. 514 tests passing. V8
unchanged since R026 (`relation_audit_log` only). Read in full before
writing any code: `CLAUDE.md`, `docs/PROJECT_STATE.md`,
`docs/LIFEOS_SPEC.md`, R017–R020, R024 (+ its transaction-isolation
hardening), R025 (+ both post-freeze corrections), R026, R027 (+
transaction/snapshot-integrity hardening), R028 (+ documentation
correction). Inspected the live `forget_memory` implementation in full,
`begin_forget_transaction`, every `ForgetMemory` storage helper
(`scrub_fact_audit_snapshots`, `list_evidence_ids_for_fact`,
`get_source_id_for_evidence`, `delete_evidence_links_for_evidence`,
`delete_evidence`, `delete_source_if_orphan`,
`delete_fact_evidence_links_for_fact`, `delete_fact_forget_target`,
`delete_entity_if_orphan_non_self`), the `relation_audit_log` schema
(confirmed non-FK `relation_id`), `insert_relation_audit_log`, `Relation`/
`RelationId`/`get_relation`, and the existing forced-delete/forced-audit
test-only trigger patterns.

Checked R025 §8's `ForgetRelation` freeze text directly against R029's
brief before writing any code: no conflict found — R025 never requires
an active-state check for `ForgetRelation`, matching this brief's
explicit instruction to treat forgetting as a privacy right, not a
lifecycle validity check (identical to `ForgetMemory`'s own R019
precedent). No STOP condition was triggered.

## 2. Payload/result type

```rust
pub(crate) struct ForgetRelationPayload {
    pub relation_id: RelationId,
}
// -> ActionOutcome<RelationId>
```

Never `ActionOutcome<Relation>` — a successful forget never hands back
the forgotten semantic object.

## 3. Delete confirmation semantics

Floor is `ConfirmationClass::Delete` (severity 4, the highest rung) —
never dominated by either provenance-derived class in the generic
`effective_class` ladder, so it blocks every unconfirmed call regardless
of provenance, with zero Personal-Knowledge-specific overlay needed.
Proven: `Explicit`+unconfirmed and `Inferred`+unconfirmed both return
`ConfirmationRequired { confirmation_class: Delete }`
(`req_fr_conf_a`/`req_fr_conf_b`).

## 4. Gate-before-transaction/lookup proof

MANDATORY, proven with a **nonexistent** `RelationId`: both provenances,
unconfirmed, return `ConfirmationRequired`, never `NotFound` — proving
no target read occurs before deletion consent (`req_fr_conf_a`/
`req_fr_conf_b`, zero audit rows asserted in both).

## 5. Target eligibility

`ForgetRelation` does not call `is_active_relation` anywhere in its
validation or execution path. `USER_CONFIRMED`/`INFERRED`/`OBSERVED`/
`DISPUTED` (all active), `STALE`+`valid_until None`, non-`STALE`+
`valid_until Some`, and `STALE`+`valid_until Some` are ALL forgettable
provided the row exists — proven in one comprehensive matrix,
`req_fr_target_eligibility_active_and_historical_all_forgettable`.

## 6. Immediate transaction boundary

Built correctly from day one via a new `begin_forget_relation_transaction`
helper (`TransactionBehavior::Immediate`), applying `ForgetMemory`'s
final, hardened shape directly rather than reproducing R024's original
race and needing a later fix. Opens immediately after the confirmation
gate; target lookup, candidate Entity capture, Evidence collection, and
Source collection all happen through `&tx`. Proven with the mandatory
two-connection lock test
(`req_fr_uses_immediate_transaction_blocking_concurrent_writer`, calling
the exact production helper) and a deletion-set-specific stability proof
(`req_fr_deletion_set_stability_blocks_concurrent_relation_evidence_write`
— Connection A reads the target and collects its Evidence ids through
`&tx`; Connection B is blocked from inserting a new `relation_evidence`
link for that same target for the whole window).

## 7. Exact Relation-specific deletion algorithm

One `Immediate` transaction, after the confirmation gate:

1. Fetch target Relation through `&tx`. Missing → `NotFound`, zero writes.
2. Candidate endpoint `EntityId`s: `from_entity_id`, `to_entity_id`, deduplicated.
3. Collect every `EvidenceId` linked via `relation_evidence`.
4. Collect each linked Evidence's `SourceId`, deduplicated.
5. Scrub `relation_audit_log.previous_snapshot` for the target (predicate-based).
6. For each collected Evidence: delete ALL its join rows from `fact_evidence`/`relation_evidence`/`episode_evidence` (privacy-first).
7. Delete each collected Evidence row.
8. Delete each collected Source iff now orphaned.
9. Defensive sweep: `DELETE FROM relation_evidence WHERE relation_id = target.id`.
10. Hard-delete the target Relation (`changed == 1` enforced).
11. Candidate endpoint Entity cleanup, iff orphaned and non-SELF.
12. Insert exactly one minimal, payload-free `ForgetRelation` audit marker.
13. `COMMIT`.

## 8. Differences from ForgetMemory

The only structural difference: **there is no step equivalent to
`ForgetMemory`'s `clear_inbound_supersession_refs`.** Relation has no
`superseded_by_relation_id`, no supersession chain — nothing to clear
(R025 §16). Every other step maps directly, and all five
Evidence/Source/Entity helper functions
(`get_source_id_for_evidence`, `delete_evidence_links_for_evidence`,
`delete_evidence`, `delete_source_if_orphan`, `delete_entity_if_orphan_non_self`)
were already fully generic — reused completely **unchanged**, zero
duplication. Only four new narrow functions were needed:
`scrub_relation_audit_snapshots`, `list_evidence_ids_for_relation`,
`delete_relation_evidence_links_for_relation`,
`delete_relation_forget_target` — because `relation_audit_log`/
`relation_evidence`/`relations` are distinct tables from their Fact-family
counterparts.

## 9. Audit-snapshot scrubbing

`scrub_relation_audit_snapshots` uses the exact same predicate-based
approach as `scrub_fact_audit_snapshots` — `WHERE relation_id = ? AND
previous_snapshot IS NOT NULL`, never `WHERE action_type IN (...)`, so
any future payload-bearing action is covered automatically. Mandatory
separate proofs for both snapshot sources:
`req_fr_scrubs_prior_correctrelation_audit_snapshot` and
`req_fr_scrubs_prior_confirmrelation_audit_snapshot` — both assert
`previous_snapshot` becomes exactly `NULL`.

## 10. Structural audit-history preservation

`req_fr_old_audit_structural_metadata_survives_scrub` — captures a prior
`CorrectRelation` audit row's `id`/`relation_id`/`action_type`/`actor`/
`provenance`/`confirmation_class`/`occurred_at` before forgetting,
re-reads the SAME row (by `id`) after, and asserts every one of those
columns is byte-for-byte identical; only `previous_snapshot` changed
(`Some(...)` → `None`).

## 11. Privacy-first Evidence deletion

Every Evidence row linked to the target is deleted even if also linked
to a retained Fact/Relation/Episode — reusing
`delete_evidence_links_for_evidence`/`delete_evidence` completely
unchanged (both already generic over any `EvidenceId`).

## 12. Shared-Evidence matrix (mandatory A–F)

All six proven directly and independently:

- A — private-only Evidence deleted, Relation gone (`req_fr_evidence_private_only_deleted_relation_gone`).
- B — shared with a retained Fact: Evidence deleted, Fact survives, `fact_evidence` link removed (`req_fr_evidence_shared_with_fact_deleted_but_fact_survives`).
- C — shared with a retained second Relation: Evidence deleted, other Relation survives, its link removed (`req_fr_evidence_shared_with_relation_deleted_but_other_relation_survives`).
- D — shared with a retained Episode: Evidence deleted, Episode survives, `episode_evidence` link removed (`req_fr_evidence_shared_with_episode_deleted_but_episode_survives`).
- E — shared across Fact + Relation + Episode simultaneously: Evidence deleted exactly once, all three retained objects survive, all three link types removed (`req_fr_evidence_shared_across_multiple_families_deleted_once_all_survive`).
- F — zero-Evidence target still deletes successfully (`req_fr_zero_evidence_target_deletes_successfully`).

## 13. Source cleanup (A–D)

`req_fr_source_matrix_orphan_and_shared` (one consolidated test,
matching `ForgetMemory`'s own precedent): orphan Source deleted (A);
Source shared with one retained Evidence survives (B); multiple target
Evidence rows sharing one Source cause exactly one delete, no duplicate-
delete error (C); one target + one retained Evidence sharing a Source →
retained (D).

## 14. Endpoint Entity cleanup

Candidates are exactly `target.from_entity_id`/`to_entity_id`,
deduplicated (the DB `CHECK` already forbids `from == to`, deduplicated
defensively anyway). Cleanup reuses `delete_entity_if_orphan_non_self`
**completely unchanged** — it already checked
`relations.from_entity_id`/`to_entity_id` alongside `facts`/
`episode_entities` since R024, so no Relation-specific exception was
ever needed.

## 15. SELF / domain_ref behavior

SELF as an endpoint survives byte-for-byte (structural guard inside
`delete_entity_if_orphan_non_self` itself —
`req_fr_entity_retention_matrix_self_and_orphan_cases` case A). A
domain_ref-only candidate Entity with no other surviving reference is
deleted — `domain_ref` alone never retains an orphan (case C, same test).

## 16. Target hard-delete proof

`req_fr_hard_delete_proof_get_relation_none_raw_count_zero_output_is_relation_id`
— after success, `get_relation` returns `None`, the raw
`SELECT count(*) FROM relations WHERE id = ?` is `0`, and the returned
`Success.value` is the target `RelationId` (never a `Relation`).

## 17. Direct delete runtime enforcement

New `delete_relation_forget_target`: `changed != 1` is a real
`rusqlite::Error::SqliteFailure` in every build profile — never
`debug_assert!`/`assert!`/`panic!`. Direct repo-level proof: existing id
succeeds, missing id is a real `Err`, never silent `Ok`
(`req_fr_repo_delete_relation_forget_target_direct_proof`).

## 18. ForgetRelation audit exactness

`req_fr_audit_marker_fields_and_no_target_snapshot` — fixed
`ActionMeta`, asserts `relation_id`/`action_type = "ForgetRelation"`/
`actor = "CloudLLM"`/`provenance = "Explicit"`/
`confirmation_class = "DELETE"`/`previous_snapshot = None`/
`occurred_at` exactly.

## 19. Inferred provenance

`req_fr_conf_e_inferred_confirmed_succeeds_audit_provenance_inferred` —
a confirmed `Inferred` call succeeds; the audit row's `provenance`
remains `"Inferred"`, never rewritten to `"Explicit"`.

## 20. No Undo / no semantic tombstone

`ActionOutcome` has no Undo variant anywhere in the codebase (structural
fact, re-confirmed by inspection). No hidden Relation payload is
retained anywhere: the new audit marker's `previous_snapshot` is
unconditionally `None`, and every prior payload-bearing audit row for
the target is scrubbed before the target itself is deleted. There is no
semantic tombstone — only payload-free structural audit history
survives.

## 21. Transaction deletion-set stability

Covered by §6/§9 above — `req_fr_deletion_set_stability_blocks_concurrent_relation_evidence_write`
is the dedicated proof.

## 22. Full rollback proofs (mandatory)

- **Final-audit failure** —
  `req_fr_rollback_forced_final_audit_failure_restores_everything`: a
  rich pre-state (target Relation with a prior `CorrectRelation` audit
  snapshot, a candidate orphan endpoint Entity, linked Evidence/Source),
  forced failure on the final `relation_audit_log` insert via the
  existing `install_relation_audit_failure_trigger` (reused unmodified).
  After failure: `relations`/`evidence`/`sources`/`entities`/
  `relation_audit_log` row counts all restored; the specific target
  Source row proven to still exist by id (not merely by count); the
  target Relation reads back byte-for-byte identical to its pre-forget
  state; the candidate orphan Entity still exists; the prior
  `CorrectRelation` snapshot is restored to its original non-`NULL`
  value (the scrub itself rolled back); no `ForgetRelation` marker
  exists; the connection remains usable (a subsequent `add_relation`
  call succeeds).
- **Mid-step failure** —
  `req_fr_rollback_mid_step_forced_relation_delete_failure`: forces the
  final `relations` `DELETE` itself to fail (a new
  `install_relation_delete_failure_trigger`), proving the EARLIER steps
  in the same transaction (audit scrub, Evidence deletion) also roll
  back, not only a final-step failure.

## 23. Logical vs forensic deletion

Same careful language as R024: this is a logical/product-level deletion
from canonical LifeOS Personal Knowledge. No claim of guaranteed SSD
flash-cell erasure, forensic unrecoverability, cloud/provider deletion,
backup deletion, or future sync deletion is made or implied by this run.

## 24. secure_delete regression

`PRAGMA secure_delete = ON` is unchanged by this run — no new `PRAGMA`,
no `VACUUM`, no filesystem-level operation was added. The full 543-test
suite (which includes the existing `secure_delete` regression proof)
passes green, confirming no storage-hardening regression.

## 25. Retained-object non-interference

`req_fr_no_sibling_or_inverse_relation_cascade` — a sibling Relation
(identical semantic edge, different `RelationId`) and an inverse
Relation (opposing direction) both survive byte-for-byte; SELF and the
shared endpoint Entity both survive (retained by the sibling/inverse).
`req_fr_fact_episode_non_interference` — `facts`/`fact_evidence`/
`episodes`/`episode_evidence`/`episode_entities` row counts all
unchanged by a plain forget with no shared Evidence.

## 26. No migration/V9

`git diff -- src-tauri/migrations/` is empty — V1–V8 byte-for-byte
unchanged, no V9. Directly confirmed
`migrations/V8__create_relation_audit_log.sql`'s `action_type` CHECK
already enumerates `'ForgetRelation'` (R026, forward-looking).

## 27. Tests added

- `src/actions/tests.rs`: 1 trivial pure test
  (`forget_relation_payload_validates_trivially_and_passes_relation_id_through`),
  mirroring `validate_forget_memory`'s own precedent.
- `src/services/tests.rs`: 28 new tests spanning the confirmation/gate
  matrix (5), no-audit-on-non-deletion (1), target eligibility (1),
  mandatory concurrency proofs (2), the direct repo delete proof (1),
  the hard-delete/output-type proof (1), audit exactness (1), the
  mandatory snapshot-scrub proofs (3), the shared-Evidence matrix (6),
  the Source matrix (1, consolidated), the Entity retention matrix (2,
  consolidated), no-sibling-cascade (1), Fact/Episode non-interference
  (1), and the two mandatory rollback proofs (2).

## 28. Exact final test count

- Previous: **514**
- New: **29** (1 in `actions::tests`, 28 in `services::tests`)
- Final: **543**
- Failed: **0**

The full 543-test suite passed on the first real run after implementation
(a compile-time `unused_variable` warning was found and fixed during
development — a captured `source_before` value that was not yet used in
an assertion; strengthened into a direct byte-for-byte Source-existence
check in the final-audit-failure rollback proof rather than removed, per
the "never silently hide" discipline of preferring a stronger proof over
a weaker one).

## 29. Build/package results (MEASURED)

- `cargo fmt --check`: clean (after one `cargo fmt` auto-format pass;
  re-verified clean).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **543 passed; 0 failed; 0 ignored**
  (19.63s).
- `cargo build --release`: succeeded (`Finished release profile
  [optimized] target(s) in 2m 09s`).
- `npm.cmd run build` (standalone, repo root, explicit — separate from
  the Tauri-internal frontend build): succeeded, exit 0.
- `npm.cmd run tauri build`: succeeded. Verified actual artifacts on
  disk:
  - `src-tauri/target/release/bundle/msi/LifeOS_0.1.0_x64_en-US.msi`
    (2,973,696 bytes)
  - `src-tauri/target/release/bundle/nsis/LifeOS_0.1.0_x64-setup.exe`
    (1,370,877 bytes)

## 30. Files changed

- `src-tauri/src/actions/mod.rs`
  (`ForgetRelationPayload`, `NormalizedForgetRelation`,
  `validate_forget_relation`)
- `src-tauri/src/services/mod.rs` (`begin_forget_relation_transaction`,
  `forget_relation`, import updates: `ForgetRelationPayload`)
- `src-tauri/src/services/storage/personal_knowledge_repo.rs`
  (`scrub_relation_audit_snapshots`, `list_evidence_ids_for_relation`,
  `delete_relation_evidence_links_for_relation`,
  `delete_relation_forget_target`)
- `src-tauri/src/actions/tests.rs` (+1 test)
- `src-tauri/src/services/tests.rs` (+28 tests)

No `domain/mod.rs` change was needed. `ForgetMemory`/`AddRelation`/
`CorrectRelation`/`ConfirmRelation` and every one of their existing
tests are untouched.

## 31. Relation action-family completion status

**ALL FOUR frozen v0.1 Relation actions now exist:**

- `AddRelation` (R026)
- `CorrectRelation` (R027, + transaction/snapshot-integrity hardening)
- `ConfirmRelation` (R028)
- `ForgetRelation` (R029, this report)

No Tauri command wires any of them into the running app yet. Episode
actions and retrieval/Context Pack remain entirely unstarted.

## 32. Next step recommendation

**R030 — M2 Personal Knowledge Episode Action-Family Architecture
Freeze.** Documentation/architecture only, not implemented this run.
`Episode`/`episode_evidence`/`episode_entities` exist as schema-only
tables (R019 §13) with no Rust domain struct and no action lifecycle
ever frozen. Retrieval/Context Pack should eventually consume the
complete Personal Knowledge substrate — Facts, Relations, and Episodes —
so Episode's own action family needs the same R017-R019/R025-style
freeze pass Fact and Relation each received before any Episode code is
written.

---

## Post-run acceptance-proof hardening (2026-09-19)

A review of this report's own claims against the actual test assertions
found no production `ForgetRelation` defect — the deletion algorithm
itself was never in question — but found three proof/claim gaps where
the report's wording asserted more than the tests actually checked:

1. **SELF existence vs byte-for-byte.** §15 claimed SELF "survives
   byte-for-byte," but the original test
   (`req_fr_entity_retention_matrix_self_and_orphan_cases`, case A) only
   asserted `get_entity(SELF).is_some()` — existence, not exact field
   equality. Fixed: the test now captures the full SELF `Entity` before
   forgetting and asserts full struct equality after.
2. **Incomplete `relations.from`/`to` + reverse-asymmetric Entity-
   retention coverage.** §14/§21's Entity retention matrix proved
   retention via a retaining Relation's `from_entity_id` (case C) and
   the retained-FROM/orphan-TO asymmetric direction (case F), but never
   independently proved retention via a retaining Relation's
   `to_entity_id`, and never proved the REVERSE asymmetric direction
   (orphan-FROM/retained-TO). Fixed:
   `req_fr_entity_retention_matrix_shared_reference_cases` gained case D
   (retention via another Relation's `to_entity_id`, with the retaining
   Relation itself also asserted to survive) and case G (orphan-FROM/
   retained-TO, the exact reverse of case F) — proving candidate cleanup
   is genuinely independent per endpoint, not dependent on endpoint
   position.
3. **Mid-delete rollback under-proved.** §22's mid-step rollback test
   (`req_fr_rollback_mid_step_forced_relation_delete_failure`) only
   asserted aggregate `relations`/`evidence` row counts and that the
   target Relation still existed — it never directly proved that the
   SPECIFIC destructive steps performed earlier in the same transaction
   (the prior `CorrectRelation` audit-snapshot scrub, the target's
   `relation_evidence` link, the exact Evidence row, the exact Source
   row, the endpoint Entities) actually rolled back, as opposed to row
   counts merely coming out coincidentally correct. Fixed: the test now
   builds a richer pre-state (runs `CorrectRelation` on the target first
   so a non-`NULL` prior audit snapshot exists to prove restored) and,
   after the forced failure, asserts eight things individually: (1) the
   target Relation is exactly the PRE-forget state; (2) the prior
   `CorrectRelation` snapshot is restored to its exact original
   non-`NULL` value; (3) the exact `relation_evidence` link (same
   `relation_id`/`evidence_id`/`created_at`) exists again; (4) the exact
   Evidence row (same `source_id`/`content`/`locator`/`sensitivity`/
   `created_at`, via a new test-only `read_evidence_fields` helper
   mirroring `read_source_fields`'s existing precedent) is restored;
   (5) the exact Source row is restored (not merely a matching global
   count); (6) both endpoint Entities are unchanged; (7) zero
   `ForgetRelation` audit markers exist; (8) the connection remains
   usable for a subsequent successful call.

A small documentation-arithmetic error was also found and corrected:
§8 above and the equivalent `PROJECT_STATE.md` paragraph both said "four
of the five" Evidence/Source/Entity helper functions were reused
unchanged, while actually listing and reusing all five
(`get_source_id_for_evidence`, `delete_evidence_links_for_evidence`,
`delete_evidence`, `delete_source_if_orphan`,
`delete_entity_if_orphan_non_self`) — corrected to "all five" in both
places. `LIFEOS_SPEC.md`'s equivalent paragraph never stated a fraction
and needed no correction.

**No production Rust changed.** Every strengthened proof passed on its
first real run — no defect was found or needed fixing;
`src-tauri/src/services/mod.rs`,
`src-tauri/src/services/storage/personal_knowledge_repo.rs`, and
`src-tauri/src/actions/mod.rs` are all untouched by this pass. Only
`src-tauri/src/services/tests.rs` changed (three existing tests
strengthened — no new `#[test]` functions were added, since the new
assertions were added as additional cases inside the same three
existing test functions). `V8` unchanged; no `V9`; no migration touched.

**Test count:** the original R029 historical count (514 → 543, +29)
remains unchanged above. This hardening pass strengthened 3 existing
tests and added 0 new ones: **previous = 543, new = 0, modified = 3,
final = 543, failed = 0.**

All four frozen v0.1 Relation actions (`AddRelation`, `CorrectRelation`,
`ConfirmRelation`, `ForgetRelation`) remain complete, with no behavioral
change from this pass. Episode/retrieval remain unstarted. The next
substantial report remains **R030**.
