# REPORT — M2 Task Parent-Validation Hardening (2026-09-17)

A small, focused correctness hardening pass on the M2 Task vertical
slice (`ace5ab1`). Fixes a validation-order mismatch found during
review: `add_task`/`update_task` short-circuited on the first missing
parent reference instead of independently checking both, as the
frozen validation contract (and the vertical-slice report itself)
claimed. **No new entity, action, or product behavior is introduced.**
`UpdateProject`, `RecurringRule`, Calendar, Reminder, Challenge,
Personal Knowledge, capability transport, and M3 all remain untouched
and out of scope.

---

## 1. Issue found

The M2 Task vertical-slice report (§10) claimed:

> "neither is ever skipped just because the other one ran or failed"

But the actual `services::add_task`/`services::update_task`
implementation returned immediately on the first missing reference:

```rust
if let Some(project_id) = payload.project_id {
    match storage::project_repo::project_exists(conn, project_id) {
        Ok(true) => {}
        Ok(false) => return ActionOutcome::ValidationFailed(vec![
            ValidationError::ReferencedProjectNotFound,
        ]),
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    }
}
if let Some(goal_id) = payload.goal_id {
    // never reached if the block above already returned
    ...
}
```

If both `project_id` and `goal_id` were supplied and both were invalid,
the `goal_id` check never ran, and only `ReferencedProjectNotFound` was
ever returned - the `goal_id` reference was never actually validated in
that call, even though the frozen contract (Task vertical-slice brief
§10/§12) required both to be checked independently whenever both are
supplied.

---

## 2. Root cause

Each parent check used an early `return` on the first `Ok(false)`
result instead of accumulating into a shared error list and returning
only after every applicable check had run. This is a straightforward
control-flow mistake, not a design flaw: the intent (independent
validation of both parents) was already correctly described in the
brief and in the original report's prose - the code just didn't
implement it that way. It never permitted an invalid write (a single
`ReferencedProjectNotFound` was still a correct rejection with zero
persistence), so this was a **validation-completeness** defect, not a
security or data-integrity defect.

---

## 3. Exact behavior before

- `AddTask`/`UpdateTask` with **only** `project_id` invalid →
  `ReferencedProjectNotFound`. Correct.
- `AddTask`/`UpdateTask` with **only** `goal_id` invalid →
  `ReferencedGoalNotFound`. Correct.
- `AddTask`/`UpdateTask` with **both** `project_id` and `goal_id`
  invalid → **only** `ReferencedProjectNotFound` returned;
  `ReferencedGoalNotFound` silently never checked or reported.

---

## 4. Exact behavior after

`add_task` and `update_task` (`src-tauri/src/services/mod.rs`) both now
accumulate into a local `reference_errors: Vec<ValidationError>`:

```rust
let mut reference_errors: Vec<ValidationError> = Vec::new();
if let Some(project_id) = payload.project_id {           // AddTask
    match storage::project_repo::project_exists(conn, project_id) {
        Ok(true) => {}
        Ok(false) => reference_errors.push(ValidationError::ReferencedProjectNotFound),
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    }
}
if let Some(goal_id) = payload.goal_id {
    match storage::goal_repo::goal_exists(conn, goal_id) {
        Ok(true) => {}
        Ok(false) => reference_errors.push(ValidationError::ReferencedGoalNotFound),
        Err(e) => return ActionOutcome::StorageError(CoreError::from(e)),
    }
}
if !reference_errors.is_empty() {
    return ActionOutcome::ValidationFailed(reference_errors);
}
```

(`update_task` is identical except it matches on
`FieldPatch::Set(project_id)`/`FieldPatch::Set(goal_id)` instead of
`Some(...)` - `Clear`/`Unchanged` never trigger a lookup, unchanged
from before.)

- Both `project_id` and `goal_id`, when applicable, are now **always**
  independently checked - neither is ever skipped because the other
  check ran, passed, or failed.
- If both checks fail, both errors are returned together in one
  `ValidationFailed(vec![...])`, in **stable, deterministic order**:
  `ReferencedProjectNotFound` always precedes `ReferencedGoalNotFound`
  (guaranteed by push order, not by any sort - Project is always
  checked and pushed first).
- If only one check fails, only that one error is returned (unchanged
  from before) - a single-error `Vec` is not distinguishable from the
  old behavior in that case, which is exactly why this fix required
  *both-invalid* test cases to actually exercise the previously-broken
  path.
- A real storage error from either lookup still aborts immediately as a
  typed `StorageError`, exactly as before - a storage failure is never
  silently downgraded into a validation failure or allowed to let the
  other check proceed and mask it.
- Zero writes/mutation in every case - this was already true before the
  fix and remains true after; this pass is purely about *which errors
  are reported*, not about persistence safety.

---

## 5. Tests added

Six new integration tests in `src-tauri/src/services/tests.rs`, under a
new "M2 Task parent-validation hardening" section, immediately following
`req_t12`:

- `req_tpv_a_add_task_both_parents_invalid_returns_both_errors_in_stable_order`
  — both `project_id`/`goal_id` nonexistent → both errors, in
  `[ReferencedProjectNotFound, ReferencedGoalNotFound]` order, zero
  `tasks` rows, zero `task_audit_log` rows.
- `req_tpv_b_add_task_project_valid_goal_invalid_returns_goal_error` —
  valid Project, invalid Goal → only `ReferencedGoalNotFound`, zero
  writes (proves the Goal check is not simply "always ok when Project
  is ok" - it is genuinely, independently evaluated).
- `req_tpv_c_add_task_project_invalid_goal_valid_returns_only_project_error`
  — invalid Project, valid Goal → only `ReferencedProjectNotFound`, zero
  writes (proves the Goal lookup runs even though Project already
  failed - this is the exact case the original bug would have masked
  differently had the check order been reversed, and is the direct
  regression test for the fix).
- `req_tpv_d_update_task_both_parents_invalid_set_returns_both_errors_in_stable_order`
  — `UpdateTask` with both `project_id`/`goal_id` set to nonexistent
  targets → both errors in stable order; the existing Task (including
  `updated_at`) is byte/logically unchanged; no new audit row.
- `req_tpv_e_update_task_project_valid_goal_invalid_returns_goal_error`
  — `UpdateTask`, valid Project `Set`, invalid Goal `Set` → only
  `ReferencedGoalNotFound`, zero mutation.
- `req_tpv_f_update_task_project_invalid_goal_valid_returns_only_project_error`
  — `UpdateTask`, invalid Project `Set`, valid Goal `Set` → only
  `ReferencedProjectNotFound`, zero mutation.

All six assert the exact `Vec<ValidationError>` (via `assert_eq!`
against a literal `vec![...]`), not just `.contains(...)`, specifically
to pin down the stable ordering requirement - a `.contains()`-only
assertion would not have caught a future regression that returned the
right errors in the wrong order.

---

## 6. Final test count

**200 passed; 0 failed; 0 ignored** (up from 194). All 194 prior tests
remain green, unmodified - this pass added exactly 6 new tests and
changed zero existing test expectations (every previously-passing
single-parent-invalid case still returns exactly the one error it did
before, since pushing one error into an otherwise-empty `Vec` and
returning `ValidationFailed(vec![that_one_error])` is behaviorally
identical to the old direct `return
ActionOutcome::ValidationFailed(vec![that_one_error])` for the
single-failure case).

---

## 7. Files changed

- `src-tauri/src/services/mod.rs` — `add_task`/`update_task`'s parent-
  reference validation blocks rewritten to accumulate into a shared
  `reference_errors: Vec<ValidationError>` and return only after both
  applicable checks have run; doc comments corrected to describe the
  actual (now-fixed) aggregation behavior instead of the original,
  inaccurate short-circuit description.
- `src-tauri/src/services/tests.rs` — 6 new integration tests (`req_tpv_a`
  through `req_tpv_f`), inserted between the existing `req_t12` and
  `req_t13_14_to_21` tests.
- `docs/reports/REPORT_M2_TASK_VERTICAL_SLICE_2026-09-17.md` — a
  "Correction note" addendum added after §10, explaining the inaccuracy
  and pointing to this report; the original §10 text is preserved
  unchanged above the addendum, not silently rewritten.
- `docs/reports/REPORT_M2_TASK_PARENT_VALIDATION_HARDENING_2026-09-17.md`
  — this file.
- `docs/PROJECT_STATE.md` — test count and Task-entity description
  updated to reflect the fix and the new test count.

No domain, migration, action-payload, or `ValidationError`-variant
changes were made - `ReferencedProjectNotFound`/`ReferencedGoalNotFound`
already existed and are reused exactly as before; this is a pure
control-flow correction inside `services::add_task`/`update_task`.

---

## 8. Build results (MEASURED)

- `cargo fmt --check`: clean (after one `cargo fmt` pass fixed
  line-wrapping in the new test code - no logic changed).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **200 passed; 0 failed; 0 ignored**.
- `cargo build --release`: succeeded (`Finished \`release\` profile
  [optimized] target(s) in 2m 20s`).
- `npm run build` (repo root): succeeded — `tsc && vite build`, 27
  modules transformed.
- `npm run tauri build`: **not rerun this pass** - this change touches
  only backend validation control flow, not the database schema or any
  dependency, so a full installer rebuild was not required per this
  run's instructions. No MSI/NSIS claim is made for this pass; the last
  verified installer build remains the one from the M2 Task vertical
  slice (`ace5ab1`).

---

## 9. Commit

`fix: validate both Task parent references` - see the chat reply for
the exact hash. Not pushed (no push authorized this run).

---

## 10. Recommended next step

`UpdateProject` remains the next safest M2 slice - fully frozen (action
shape, patch semantics, `IMPORTANT_CHANGE` floor), implemented-ready,
on an entity that already exists in full. This hardening pass changes
nothing about that recommendation; it was explicitly out of scope for
this run and remains queued. `RecurringRule` remains deliberately
deferred pending its own design/freeze pass.
