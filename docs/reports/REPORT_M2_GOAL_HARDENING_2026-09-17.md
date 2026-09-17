# REPORT — M2 Goal Hardening

**Date:** 2026-09-17
**Scope:** Harden the Goal vertical slice (`docs/reports/REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`)
before its pattern is copied to `Project` or any other domain entity.
Explicitly **not** in scope: `GoalRelation`, `Project`, M3, any UI change,
Qwen/GPT/Luna.

---

## 1. Pre-run issue list

Architectural review of the Goal vertical slice found seven issues that
had to be corrected before the Goal pattern could safely be reused:

1. **Non-atomic mutation + audit.** `CreateGoal`/`UpdateGoal` wrote the
   goal row, then called
   `let _ = storage::goal_repo::insert_audit_entry(...)` - a failed
   audit write was silently discarded while `ActionOutcome::Success` was
   still returned. The frozen architecture requires a persisted
   important change to have an audit record; a caller must never see
   `Success` for a partially-recorded mutation.
2. **No test proved atomicity for real** - only code inspection backed
   the (already-broken) assumption that this was safe.
3. **`ActionOutcome::StorageError(String)`** exposed whatever the
   underlying `rusqlite`/`refinery` error's `Display` happened to
   produce as the authoritative Core error API - not a typed, closed
   error model.
4. **`UpdateGoalPayload` could not distinguish "leave unchanged" from
   "explicitly clear to NULL"** for its nullable fields (`Option<T>`
   with `None` meaning "unchanged" only) - already flagged as a
   documented limitation in the vertical-slice report, now fixed before
   the shape is copied elsewhere.
5. **An unapproved terminal-status rule.** The vertical slice invented
   "`COMPLETED`/`ABANDONED` cannot transition further" - never part of
   the frozen Goal status definition, and a real product decision that
   should not have been silently made inside an implementation slice.
6. **`actions` was only tested indirectly**, through `services`-level
   integration tests - the vertical-slice report flagged this explicitly
   as a known gap.
7. **The `Provenance::Inferred` path was implemented but never
   exercised by a test** - deferred to "M5's cloud-LLM integration" in
   the prior report, when the pure logic could be (and now is) tested
   independently of any LLM actually existing.

Plus three documentation inconsistencies (§9 below).

---

## 2. Atomic transaction design

`services::create_goal`/`update_goal` now take `conn: &mut Connection`
(previously `&Connection`) - `Connection::transaction()` requires unique
access. Both functions:

1. Validate (unchanged - validation still happens entirely before any
   database interaction, no SQL, no LLM).
2. Compute confirmation class and block on `ConfirmationRequired` if
   needed (unchanged - still before any write).
3. `let tx = conn.transaction()?;` (mapped to a typed `CoreError` on
   failure).
4. Write the goal row via `storage::goal_repo::insert_goal`/`update_goal`
   against `&tx` (a `rusqlite::Transaction` derefs to `Connection`, so
   the existing repo functions did not need their own signatures
   changed).
5. Write the audit row via `storage::goal_repo::insert_audit_entry`,
   also against `&tx`.
6. `tx.commit()`.

If step 4, 5, or 6 fails, the function returns
`ActionOutcome::StorageError(CoreError::from(e))` immediately. Because
`tx` (the `rusqlite::Transaction`) goes out of scope at that `return`
without ever having been committed, its `Drop` implementation issues a
`ROLLBACK` automatically - this is the idiomatic rusqlite pattern, not a
manual `BEGIN`/`ROLLBACK` string. No `let _ = ...` remains on either
write.

`services/mod.rs` (both functions) - see the file for the exact code;
the important shape is identical in `create_goal` and `update_goal`.

---

## 3. Forced-audit-failure rollback evidence

Code inspection alone was explicitly ruled insufficient by the hardening
brief. The reproducible mechanism used:

```sql
CREATE TRIGGER force_audit_failure
BEFORE INSERT ON goal_audit_log
BEGIN
    SELECT RAISE(FAIL, 'forced audit failure');
END;
```

installed via `conn.execute_batch(...)` only inside the test, on a temp
database, before calling `create_goal`/`update_goal`. `RAISE(FAIL, ...)`
(not `RAISE(ROLLBACK, ...)`) was chosen deliberately: `FAIL` aborts only
the failing statement (the audit `INSERT`) and leaves the SQLite
transaction open with the earlier goal write still pending - so a
passing test proves *our Rust code's* `Transaction::drop` performs the
rollback, not SQLite doing it automatically on our behalf.

**Test A - CreateGoal
(`services::tests::req_h1_create_goal_audit_failure_rolls_back_goal_insert`):**
installs the trigger, calls `create_goal`, asserts the result is
`ActionOutcome::StorageError` with `kind() == CoreErrorKind::Storage`,
asserts `SELECT count(*) FROM goals` is `0` and
`SELECT count(*) FROM goal_audit_log` is `0`, removes the trigger, then
calls `create_goal` again to prove the connection is still fully usable
after the rollback (not left in a broken half-open transaction state).

**Test B - UpdateGoal
(`services::tests::req_h2_update_goal_audit_failure_rolls_back_goal_update`):**
creates a goal (trigger not yet installed), records its exact
post-create state via `get_goal`, installs the trigger, attempts
`update_goal(progress: Some(77))`, asserts the result is
`ActionOutcome::StorageError` with `kind() == CoreErrorKind::Storage`,
removes the trigger, re-fetches the goal and asserts it is `==` (full
`PartialEq` derive, every field) to the pre-update snapshot, and asserts
exactly 1 audit row exists for that goal (the original `CreateGoal`'s
row only - the failed `UpdateGoal`'s attempted row did not survive).

**Measured result:** both tests pass -
`test services::tests::req_h1_create_goal_audit_failure_rolls_back_goal_insert ... ok`,
`test services::tests::req_h2_update_goal_audit_failure_rolls_back_goal_update ... ok`
(full run output in §10).

---

## 4. Typed Core error model

New module `src-tauri/src/core_error.rs`:

```rust
pub(crate) enum CoreErrorKind {
    Storage,
    Migration,
    Security,
    Internal,
}

pub(crate) struct CoreError {
    kind: CoreErrorKind,
    diagnostic: String,   // internal-only, never part of Display
}
```

- `kind()` is the public, matchable, closed-set identity a caller
  programs against.
- `diagnostic()` is `pub(crate)` and carries the underlying driver
  error's own text, for internal logging/debugging - `rusqlite`/
  `refinery` error text names constraints/types, not bound parameter
  values, so it does not carry user-private payload content or key
  material; it is still never exposed through `Display`.
- `Display` prints only `"core error ({kind:?})"` - safe to hand to any
  future external caller without leaking database internals.
- `From<rusqlite::Error>`, `From<refinery::Error>`, and
  `From<crate::security::KeyError>` are implemented (only the first is
  exercised by `create_goal`/`update_goal` today; `Migration`/`Security`
  exist for the model to already be usable when `bootstrap_production_db`
  or a future slice needs them, without inventing a second error type
  later).

`ActionOutcome::StorageError(String)` -> `StorageError(CoreError)` in
`actions/mod.rs`. Every `.to_string()` call on a storage error at the
`create_goal`/`update_goal` call sites was replaced with
`CoreError::from(e)`.

Deliberately not built: no error-code registry, no HTTP-status mapping,
no i18n message table - "a small typed Core/storage-facing error model
appropriate for M2," per the brief, not an enterprise framework.

---

## 5. FieldPatch design

```rust
pub(crate) enum FieldPatch<T> {
    Unchanged,
    Set(T),
    Clear,
}

impl<T> Default for FieldPatch<T> {
    fn default() -> Self { FieldPatch::Unchanged }
}
```

Defined in `actions/mod.rs` (payload shape, not domain data). Applied to
`UpdateGoalPayload`'s nullable `Goal` fields: `description`,
`target_date`, `priority_user`, and `domain_tag` (the last is new to
`UpdateGoalPayload` this run - it existed on `Goal` and in
`CreateGoalPayload` already, but was missing from `UpdateGoalPayload`
entirely). `title`, `status`, and `progress` stay plain `Option<T>`
because those `Goal` fields are **non-nullable** - "clear" has no
meaning for them (an empty title is already rejected by validation, a
goal always has a status and a progress value).

`validate_update_goal` was updated: `touches_any_field` now checks
`!field.is_unchanged()` for `FieldPatch` fields; length/bounds
validation (`validate_description`/`validate_domain_tag`/
`validate_priority`) now runs only on `FieldPatch::Set(x)` - `Clear`
never fails validation (there is nothing to bound-check when clearing),
`Unchanged` is not inspected at all.

`services::update_goal`'s field-application step now `match`es each
`FieldPatch` field: `Unchanged => {}`, `Set(x) => updated.field =
Some(x)`, `Clear => updated.field = None`.

**Tests:**
- `actions::tests::update_goal_field_patch_unchanged_alone_is_not_a_touch`,
  `..field_patch_clear_does_not_trigger_length_validation`,
  `..field_patch_set_triggers_length_validation`,
  `..field_patch_set_priority_out_of_bounds_is_rejected`,
  `field_patch_default_is_unchanged` - pure validation-logic unit tests.
- `services::tests::req_h4_update_goal_field_patch_set_clear_unchanged` -
  end-to-end against a real DB: creates a goal with all four nullable
  fields populated, issues one `UpdateGoalPayload` with
  `description: Set(...)`, `priority_user: Clear`, `domain_tag:
  Unchanged`, `target_date: Unchanged`, asserts the returned `Goal` has
  the new description, `None` priority, and the *original*
  `domain_tag`/`target_date` untouched, then re-fetches from the DB with
  a fresh read and asserts it matches exactly - proving persistence, not
  just the in-memory return value.

---

## 6. Goal status-transition correction

`domain::GoalStatus::can_transition_to` (and the doc comment claiming
`COMPLETED`/`ABANDONED` are terminal) is **removed**.
`actions::ValidationError::InvalidStatusTransition` is **removed** - with
no transition rule left to violate, keeping an unconstructable variant
around would have been dead, misleading code, not a real error case
being preserved for later. `validate_update_goal` no longer inspects
`payload.status` for legality at all; every `GoalStatus` value the type
system already guarantees is valid is accepted.

`domain::GoalStatus`'s doc comment now explicitly records *why* -
future work must not silently reintroduce a terminal-state policy
without `LIFEOS_SPEC.md` freezing one first.

**Tests:**
- `actions::tests::update_goal_status_transition_from_completed_to_active_is_now_valid`,
  `..from_abandoned_to_paused_is_valid`, `..no_op_transition_is_valid` -
  pure validation-logic checks.
- `services::tests::req_h3_completed_goal_can_be_reopened_and_transition_is_audited` -
  end-to-end: creates a goal, marks it `Completed`, then successfully
  transitions it back to `Active` (previously this would have been
  rejected), and asserts exactly 3 audit rows exist for that goal
  (create, complete, reopen) - the transition remains fully audited,
  only the invented restriction is gone.

`services::tests::req11_invalid_update_makes_zero_changes` (one of the
16 originally-required tests) previously proved this exact "zero
changes" guarantee via a rejected `COMPLETED -> ACTIVE` transition -
since that scenario is no longer invalid, the test was rewritten to
prove the same zero-changes guarantee via a still-valid rejection
(out-of-bounds `priority_user`). The guarantee itself was never weakened,
only the example proving it.

---

## 7. Direct action-layer unit tests

New file `src-tauri/src/actions/tests.rs`, 16 tests, no I/O, no temp
files, no keyring - pure calls into `validate_create_goal`/
`validate_update_goal`/`effective_class`/`goal_blocks_until_confirmed`:

- `create_goal_title_is_trimmed`
- `create_goal_rejects_empty_title`
- `create_goal_rejects_title_too_long`
- `create_goal_rejects_priority_out_of_bounds`
- `effective_class_explicit_add_with_explicit_provenance_stays_explicit_add`
- `effective_class_explicit_add_with_inferred_provenance_becomes_inferred_change`
- `effective_class_important_change_with_explicit_provenance_stays_important_change`
- `update_goal_rejects_when_no_fields_touched`
- `update_goal_field_patch_unchanged_alone_is_not_a_touch`
- `update_goal_field_patch_clear_does_not_trigger_length_validation`
- `update_goal_field_patch_set_triggers_length_validation`
- `update_goal_field_patch_set_priority_out_of_bounds_is_rejected`
- `update_goal_status_transition_from_completed_to_active_is_now_valid`
- `update_goal_status_transition_from_abandoned_to_paused_is_valid`
- `update_goal_status_no_op_transition_is_valid`
- `field_patch_default_is_unchanged`

This closes the vertical-slice report's explicitly flagged gap ("No
dedicated `actions`-level unit tests").

---

## 8. InferredChange test evidence

`services::tests::req_h5_inferred_create_goal_requires_confirmation_and_does_not_write`:

1. Builds `ActionMeta::now(Actor::CloudLlm, Provenance::Inferred)`
   (`confirmed: false` by construction) and a valid `CreateGoalPayload`.
2. Calls `create_goal` against a real bootstrapped encrypted DB, asserts
   the result is `ActionOutcome::ConfirmationRequired { confirmation_class:
   ConfirmationClass::InferredChange }`.
3. Asserts `SELECT count(*) FROM goals` is `0` - **no row was written
   before confirmation**.
4. Builds a second `ActionMeta` with the same actor/provenance but
   `confirmed: true`, resubmits the same payload, asserts
   `ActionOutcome::Success` with `confirmation_class ==
   ConfirmationClass::InferredChange`, and that exactly one goal row now
   exists.

This directly satisfies the brief's explicit instruction not to wait for
M5 - the action logic is provider-agnostic and testable without any real
cloud LLM existing.

Also covered at the pure-logic level:
`actions::tests::effective_class_explicit_add_with_inferred_provenance_becomes_inferred_change`
proves `effective_class(ExplicitAdd, Inferred) == InferredChange` and
`goal_blocks_until_confirmed(InferredChange) == true` directly - the
mechanism `req_h5` then proves end-to-end against a real database.

---

## 9. Documentation corrections

All in `docs/LIFEOS_SPEC.md`:

**A. Top-level status banner** (previously "M2 architecture frozen but
not implemented") now states: M1 accepted; M2 storage proven/hardened;
Goal vertical slice implemented and hardened; rest of M2 in progress; M3
not started.

**B. `GoalRelation` wording (§5.2).** Previously claimed "the initial
`Goal` schema has relationship edges from the start rather than needing
a breaking migration later" - false; the actual `V1__create_goals.sql`
migration implements `Goal` only. Corrected to state plainly:
`GoalRelation` is frozen as an M2 domain entity, is **not** implemented
yet, will receive its own vertical slice before M2 is declared complete,
and the V1 migration makes no claim to already contain it. A matching
implementation-status note was added to §5.1's entity list.

**C. Capability wording (§5.6, §10.1).** Previously implied a
`capability/` module exists as "concrete Rust function signatures."
Corrected to separate two things explicitly: the capability API
**contract** (operation names/signatures) is frozen at M2, as design;
the **module implementing it is not built yet** - `services`' functions
are called directly only by `services::tests`, which is a test working
around the architecture (per this repo's testing policy), not an
external caller the "no direct `services`/`storage` access" rule is
meant to permit. Once a real Tauri command, cloud-tool handler, or NeXa
transport exists, it must go through a real `capability/` module and
must never call `services`/`services::storage` directly.

---

## 10. Exact test/build results

All commands run from `D:\Dev\LiFeOS\src-tauri` unless noted, PowerShell,
`cargo 1.98.1`.

| Command | Result |
|---|---|
| `cargo fmt --check` | Initially FAILED (pure formatting diffs across `actions/tests.rs` and `services/tests.rs` - no logic changes); `cargo fmt` applied, re-ran `cargo fmt --check` -> **PASS** (exit clean, no diff) |
| `cargo check --tests` | **PASS** - `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 1.61s`, zero warnings |
| `cargo test -- --test-threads=1` | **PASS** - `running 42 tests` / `test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`. Breakdown: 9 storage-spike regression (`services::storage::spike::tests::*`, unchanged), 17 Goal-slice/hardening integration (`services::tests::*`, includes the 2 new atomicity tests, the FieldPatch test, the reopened-status test, and the 2 InferredChange tests), 16 direct action-layer unit tests (`actions::tests::*`, all new this run). The `sqlcipher_page_cipher: hmac check failed` lines in the output are expected stderr from the wrong-key tests (`req3_wrong_key_fails_cleanly`, `req13_wrong_key_rejected_for_production_db`) succeeding, not failures. |
| `cargo build --release` | **PASS** - `Finished \`release\` profile [optimized] target(s) in 2m 33s` |
| `npm.cmd run build` (repo root) | **PASS** - `tsc && vite build` succeeded, `✓ 27 modules transformed`, `✓ built in 119ms`. No frontend files were touched this run; this reconfirms the untouched M1 build still succeeds after the backend change. |
| `npm.cmd run tauri build` | **Not re-run this pass.** No dependency was added/removed in `Cargo.toml` (unchanged this run) and the release build above already compiles the full crate including the new `core_error` module in release mode - the packaging pipeline itself was proven with a larger dependency graph in the vertical-slice run. Per the brief's own conditional ("If backend dependency/build shape changed enough to justify it"), that condition was not met. |
| Module-boundary probe | Re-run despite no `services::storage` visibility change this pass (nothing about the boundary was touched): temporarily added `fn illegal_boundary_probe() { let _ = services::storage::goal_repo::insert_goal; }` to `lib.rs`, ran `cargo check` -> **`error[E0603]: module 'storage' is private`**, confirmed, then removed the probe and re-ran `cargo check --tests` -> clean. |

---

## 11. Files changed

- `src-tauri/src/core_error.rs` - **new**: `CoreError`/`CoreErrorKind`.
- `src-tauri/src/lib.rs` - declares `mod core_error;`.
- `src-tauri/src/domain/mod.rs` - removed `GoalStatus::can_transition_to`
  and its terminal-state doc comment; replaced with a doc comment
  explaining the removal.
- `src-tauri/src/actions/mod.rs` - added `FieldPatch<T>`; removed
  `ValidationError::InvalidStatusTransition`; `UpdateGoalPayload`'s
  `description`/`target_date`/`priority_user` changed from `Option<T>`
  to `FieldPatch<T>`, added `domain_tag: FieldPatch<String>`;
  `validate_update_goal` rewritten for `FieldPatch` and the removed
  transition check; `ActionOutcome::StorageError` changed from `String`
  to `CoreError`; added `#[cfg(test)] mod tests;`.
- `src-tauri/src/actions/tests.rs` - **new**: 16 direct action-layer unit
  tests.
- `src-tauri/src/services/mod.rs` - `create_goal`/`update_goal` now take
  `&mut Connection`, wrap the goal write + audit write in one
  `rusqlite` transaction, map every storage error to `CoreError`; field
  application in `update_goal` updated for `FieldPatch`; capability
  doc-comment tightened per §9C above.
- `src-tauri/src/services/tests.rs` - all `create_goal`/`update_goal`
  call sites updated for `&mut Connection`; `req11` rewritten (§6); 5 new
  hardening tests (`req_h1`-`req_h5`); new helpers
  (`install_audit_failure_trigger`, `remove_audit_failure_trigger`,
  `goal_row_count`, `audit_row_count`, `mk_create_payload`).
- `docs/LIFEOS_SPEC.md` - status banner, §5.1, §5.2, §5.6, §10.1 (§9
  above).
- `docs/PROJECT_STATE.md` - updated for this run (§4b corrected, new
  §4c, renumbered §4d, file tree, test counts, commit list, known
  problems).
- `docs/reports/REPORT_M2_GOAL_HARDENING_2026-09-17.md` - this report.

No changes to: `Cargo.toml`/`Cargo.lock` (no dependency changes),
`src-tauri/migrations/V1__create_goals.sql` (schema unchanged - all
fixes were in the Rust layer), `services/storage/mod.rs`,
`services/storage/goal_repo.rs` (SQL access unaffected - only its
callers now wrap it in a transaction), `services/storage/spike/*`
(untouched, still passing as regression), any file under `src/` (React
frontend, untouched), `scripts/setup-windows-storage-env.ps1`
(untouched), `CLAUDE.md` (no operating-policy change this run).

---

## 12. Remaining risks

- **Not wired into the running app** - unchanged from the vertical-slice
  report; still no Tauri command layer.
- **`target_date` has no content validation beyond well-formedness** -
  pre-existing, not introduced or expanded here, deliberately left out
  of this run's 7-issue scope.
- **`CoreError`'s `Debug` impl includes the internal `diagnostic`
  string** - safe for internal test/log use, but any future code that
  formats a `CoreError` for an external caller must use `Display`/
  `kind()`, never `{:?}` - documented in `PROJECT_STATE.md` §10 as a
  forward-looking constraint on whoever builds the `capability/` module.
- **`GoalRelation` remains unimplemented** - corrected in documentation
  this run, but the entity itself still does not exist; `Project` (not
  `GoalRelation`) is the next recommended M2 slice per the vertical-slice
  report, and this run did not change that recommendation.
- **Clean-machine runtime is still `dumpbin`-inferred, not an actual
  execution test** - unchanged, carried forward from the vertical-slice
  report.

---

## 13. Is Goal now safe to use as the template for Project?

**Yes**, with one explicit caveat. All seven issues architectural review
raised are fixed and proven:

- Mutation + audit is genuinely atomic, proven via a forced-failure
  trigger, not just inspected.
- Errors returned across the action/service boundary are typed and
  closed, not raw database strings.
- Patch semantics for nullable fields are unambiguous and tested for all
  three states against a real database.
- No unapproved product policy is embedded in the implementation - the
  one that had crept in is removed, and the domain layer's doc comment
  now warns against silently reintroducing one.
- `actions` has direct, fast, DB-free unit test coverage.
- The previously-deferred `Provenance::Inferred` path is proven
  end-to-end, including the "no write before confirmation" guarantee
  that matters most for an AI-initiated change.
- Documentation accurately describes what is and is not implemented,
  removing two sources of future confusion (`GoalRelation`'s status, the
  capability module's status).

**Caveat:** `Project`'s frozen action set (`CreateProject`,
`AddMilestone`, `AddTask`, `UpdateProgress`, §5.4) is a *milestone/task
container*, not a flat single-table entity like `Goal` - its first slice
will need to decide how much of that action set to implement in one pass
(the vertical-slice report already recommended `CreateProject`/
`UpdateProject` only, matching `Goal`'s scope, and that recommendation
still stands). The hardening fixes here (atomic transactions, typed
errors, `FieldPatch`, no invented status rules, direct unit tests,
proven `Inferred` path) are all pattern-level and apply directly
regardless of that scoping decision.
