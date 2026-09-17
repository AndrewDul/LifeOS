# REPORT — M2 Project / Storage Integrity Hardening

**Date:** 2026-09-17
**Scope:** A focused hardening pass on the storage/type layer shared by
`Goal` and `Project`, before a third domain entity or `GoalRelation`
begins. Explicitly **not** a new domain entity. Explicitly **not** in
scope: `GoalRelation`, `Milestone`, `Task`, `Calendar`, `Challenge`,
Personal Knowledge, a `capability/` transport, M3 UI, Qwen, GPT/Luna.

---

## 1. Pre-run risks

Four issues, identified by architectural review of the Project vertical
slice, needed addressing before a third entity reuses the pattern:

1. `projects.goal_id REFERENCES goals(id)` was declared in
   `V2__create_projects.sql` but SQLite's foreign-key enforcement was
   never actually turned on (`PRAGMA foreign_keys` was never set
   anywhere) - the constraint was documentation, and the *only* real
   integrity guarantee was `CreateProject`'s own application-level
   `goal_exists` check. A direct SQL statement bypassing `CreateProject`
   entirely (a bug elsewhere in Core, a future migration script, manual
   debugging) could silently write a dangling `goal_id`.
2. `Project.progress` was added during the Project vertical slice as a
   flagged, open interpretation - not a literal item in the
   architecture-freeze report's Project field table - and left
   unresolved in `LIFEOS_SPEC.md`.
3. `domain::GoalStatus` was reused, correctly in behavior, for
   `Project.status` - but the type's name still implied it was
   Goal-specific, which is misleading now that two entities share it.
4. The Project vertical slice's own implementation surfaced a real gap
   in the frozen M2 action set: nothing lets a caller edit an existing
   Project's `title`/`description`/`status`/`priority`/`goal_id`. This
   run's brief was explicit that this must be *documented*, not
   silently resolved by inventing an `UpdateProject` action.

---

## 2. Why SQLite FK enforcement was previously incomplete

SQLite disables foreign-key constraint checking by **default** for
backward compatibility with pre-3.6.19 schemas - `PRAGMA foreign_keys =
ON` must be set on every single connection; it is not a database-file
property and is not inherited from a previous session. Declaring
`REFERENCES goals(id)` in a `CREATE TABLE` statement is necessary but
not sufficient - without the pragma, SQLite parses and stores the
constraint metadata but never checks it on `INSERT`/`UPDATE`/`DELETE`.
Nothing in this codebase ever issued that pragma before this run, so
the V2 migration's own comment (accurately, at the time) said
enforcement happened only in application code.

---

## 3. Where PRAGMA foreign_keys is now enabled

`services::storage::open_encrypted` (`storage/mod.rs`) - the single
connection-opening primitive - now calls a new `enable_foreign_keys`
helper immediately after the SQLCipher `key` pragma:

```rust
pub(super) fn open_encrypted(path: &Path, key_hex: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "key", format!("x'{key_hex}'"))?;
    enable_foreign_keys(&conn)?;
    Ok(conn)
}

fn enable_foreign_keys(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let enabled: i64 = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    if enabled != 1 {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISUSE),
            Some("PRAGMA foreign_keys = ON did not take effect on this connection".to_string()),
        ));
    }
    Ok(())
}
```

This is the common connection-opening/bootstrap path, not a per-
repository fix, exactly as the brief required - `enable_foreign_keys`
does not appear in `goal_repo.rs`/`project_repo.rs` at all. Every
caller of `open_encrypted` gets it automatically and identically:

- **Production**: `services::bootstrap_db_at`/`bootstrap_production_db`.
- **Version-targeted migration tests**: `services::bootstrap_db_at_version`
  (used by the V1→V2 upgrade-compatibility test).
- **The storage spike**: its own `create_and_seed`/`reopen_and_read`/
  etc. wrappers all call `super::open_encrypted` - so spike connections
  cannot silently diverge from production FK behavior, satisfying the
  brief's explicit requirement.

The pragma is read back and verified immediately (not assumed to have
"just worked") - `enable_foreign_keys` returns a typed `rusqlite::Error`
if the read-back value isn't `1`, which propagates through
`BootstrapError::Storage`/`CoreError::from` exactly like any other
storage failure. No silent failure path exists.

**Ordering note (verified, not assumed):** `PRAGMA foreign_keys` is a
per-connection backend flag, not a page/schema read - it does not
require the database to already be correctly keyed, so placing it
immediately after the `key` pragma and before `verify_key`'s first real
page read is safe; a wrong-key database still fails at `verify_key` as
before, unaffected by this change (confirmed by `req3_wrong_key_fails_cleanly`
and `req13_wrong_key_rejected_for_production_db` both still passing, §11).

---

## 4. DB-level FK rejection evidence

Two new integration tests deliberately bypass `CreateProject`'s
validation to prove the database itself, not just Core, rejects invalid
state - raw SQL kept local to the test file, never exposed as a
repository/production function:

**`req_i3_invalid_direct_project_goal_id_rejected_by_sqlite_fk`** -
a direct `conn.execute("INSERT INTO projects (...) VALUES (...)", ...)`
naming a `goal_id` that does not exist in `goals`. Result:

```
Err(rusqlite::Error::SqliteFailure(Error { code: ConstraintViolation, .. }, _))
```

asserted via `err.code == rusqlite::ErrorCode::ConstraintViolation`, and
`SELECT count(*) FROM projects` confirmed `0` afterward - the row was
never written.

**`req_i4_valid_direct_project_goal_id_accepted`** - the identical raw
`INSERT` path, but referencing a real, just-created `Goal`'s id.
Succeeds, and `project_row_count` becomes `1` - proving the constraint
discriminates valid from invalid references rather than rejecting
everything indiscriminately.

**`req_i1_foreign_keys_enabled_on_fresh_production_connection`** /
**`req_i2_foreign_keys_enabled_after_reopen`** - `PRAGMA foreign_keys`
read back as `1` on a fresh `bootstrap_db_at` connection, and again
after a full connection drop + reopen (proving the per-connection
pragma is reliably re-applied, not something that only happened to work
once).

---

## 5. Why application-level reference validation is still retained

`CreateProject`'s `storage::project_repo::goal_exists` pre-check
(§7 of the Project vertical-slice report) is **not** removed now that
the database itself enforces the constraint. Both layers serve
different purposes and are kept deliberately:

- **Application-level check** (`ValidationError::ReferencedGoalNotFound`
  via `ActionOutcome::ValidationFailed`): a friendly, typed, expected
  outcome through the exact same deterministic channel every other
  structural validation failure uses - a caller building UI/tool-call
  error messages never needs to special-case a raw SQL error string.
- **Database-level constraint** (`PRAGMA foreign_keys = ON`): genuine
  defense-in-depth against any *other* code path that might one day
  write to `projects` without going through `create_project` - a
  migration script, a future admin/debug tool, a bug. It protects data
  integrity even if the application-level check is ever bypassed,
  removed, or has a bug of its own.

This matches ordinary defense-in-depth practice: a friendly check close
to the user, a strict constraint at the data layer, neither one a
substitute for the other.

---

## 6. Canonical Project.progress decision

Resolved in `LIFEOS_SPEC.md` §5.1 (and cross-referenced in §13's open
questions list), verbatim intent:

- `Project.progress` = the **current** normalized Project progress
  snapshot = integer `0..=100` = the same shared `Progress` type
  `Goal.progress` already uses.
- The separate, still-unimplemented `ProgressUpdate` entity (frozen in
  the architecture-freeze report: `subject_type`/`subject_id`/`value`/
  `note`/`recorded_at`) is a temporal *history* of progress
  observations/changes over time - **not** a replacement for
  `Project.progress`, and does not remove the need for a current
  snapshot living directly on `Project` (the same way `Goal.progress`
  already works).

No code changed for this - `domain::Project.progress` was already
implemented correctly; only its documentation status moved from "open
interpretation" to "frozen."

---

## 7. Progress snapshot vs. future ProgressUpdate distinction

Restated plainly, as the brief specified:

```
Project.progress
    current state: 65%

ProgressUpdate (still unimplemented)
    historical record:
      2026-09-10 -> 40%
      2026-09-15 -> 55%
      2026-09-17 -> 65%
```

`ProgressUpdate` itself is **not** implemented this run - this section
resolves only what `Project.progress` *means*, matching the brief's
explicit "documentation/schema clarification only because the code
already implements Project.progress correctly."

---

## 8. GoalStatus → LifecycleStatus refactor

A behavior-preserving Rust identifier rename, applied via a scoped
whole-word find/replace across the seven files that referenced
`GoalStatus`, followed by manual doc-comment correction (not a blind
mechanical rename left unreviewed):

- `domain/mod.rs` - the enum definition itself, `Goal.status` and
  `Project.status` field types, and every doc comment explaining why
  Project reuses this type.
- `actions/mod.rs`, `actions/tests.rs` - `UpdateGoalPayload.status`,
  validation logic, and all Goal-status test fixtures.
- `services/mod.rs`, `services/tests.rs` - `create_goal`/`create_project`
  default-status construction, and all Goal/Project test assertions.
- `services/storage/goal_repo.rs`, `services/storage/project_repo.rs` -
  `LifecycleStatus::parse`/`as_str` calls in row-mapping code.

Variant names (`Active`/`Paused`/`Completed`/`Abandoned`) and their
`as_str()`/`parse()` string mappings (`"ACTIVE"`/`"PAUSED"`/
`"COMPLETED"`/`"ABANDONED"`) are byte-for-byte unchanged - only the enum
*type name* changed. No terminal-state rule was reintroduced (the
`can_transition_to` removal from the Goal hardening pass stays removed;
`domain/mod.rs`'s doc comment explaining that removal was updated to
note it now applies to both entities that share this type, not
reintroduced).

**Neither `V1__create_goals.sql` nor `V2__create_projects.sql` was
edited** - a Rust type-name rename touches no schema and needs no
migration; SQLite has no concept of a Rust type name.

---

## 9. Proof database values/schema did not change

Three lines of evidence, not one:

1. **Direct unit proof** (`domain::tests`, new this run):
   `lifecycle_status_as_str_matches_frozen_db_text_values` asserts all
   four `as_str()` outputs literally; `lifecycle_status_parse_round_trips_through_as_str`
   round-trips every variant through `parse(as_str())`;
   `lifecycle_status_parse_rejects_unknown_text` confirms unrecognized/
   wrong-case text still returns `None`.
2. **Full regression, unmodified logic**: every pre-existing Goal test
   (`services::tests::req1` through `req_h5`, `actions::tests::*`) and
   every pre-existing Project test (`req_p1` through `req_p16`) still
   passes, unchanged in behavior - only the token `GoalStatus` in their
   source became `LifecycleStatus` (§16). These tests write and read
   real rows through the real `goals`/`projects` tables, so any
   behavioral drift from the rename would have surfaced as a test
   failure.
3. **Migration files themselves inspected, confirmed untouched** - `git
   diff` against `V1__create_goals.sql`/`V2__create_projects.sql` is
   empty for this run (§16/§17).

---

## 10. Project action-set gap

Documented, not resolved, in `LIFEOS_SPEC.md` §5.1 (full text) and §13
(a one-line cross-reference), plus `PROJECT_STATE.md` §4e point 6 and
§10's known-problems list. Summary: `Project` persists `title`/
`description`/`status`/`priority`/`goal_id` alongside `progress`, but
the frozen M2 action set for Project is only `CreateProject`/
`AddMilestone`/`AddTask`/`UpdateProgress` - there is no frozen action to
edit any of those five fields after creation. Resolving this (one
`UpdateProject` mirroring `UpdateGoal`'s shape, or several narrower
actions) is explicitly deferred to a future, conscious architecture
decision - not invented inside this run, echoing the lesson from the
Goal vertical slice's silently-invented terminal-status rule (which had
to be removed in the Goal hardening pass).

---

## 11. Exact tests/build results

All commands run from `D:\Dev\LiFeOS\src-tauri` unless noted, PowerShell,
`cargo 1.98.1`.

| Command | Result |
|---|---|
| `cargo fmt --check` | **PASS** - no diff (the `GoalStatus` → `LifecycleStatus` rename was a pure `sed` token substitution that did not disturb line lengths/formatting; new code was written pre-formatted) |
| `cargo check --tests` | **PASS** - `Finished \`dev\` profile [unoptimized + debuginfo] target(s)`, zero warnings |
| `cargo test -- --test-threads=1` | **PASS** - `running 73 tests` / `test result: ok. 73 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`. **All 66 previous tests remained green** (confirmed by name in the run output - every `req*`/`actions::tests::*` test from the Goal and Project reports is present and passing). 7 new tests: `req_i1`-`req_i4` (FK enforcement, `services::tests`) + 3 `domain::tests::lifecycle_status_*` (rename proof). New total: **73**. |
| `cargo build --release` | **PASS** - `Finished \`release\` profile [optimized] target(s) in 2m 32s` |
| `npm.cmd run build` (repo root) | **PASS** - `tsc && vite build` succeeded, `✓ 27 modules transformed`, `✓ built in 119ms`. No frontend files touched this run. |
| `npm.cmd run tauri build` | **PASS** - run because the common DB-open path changed and the shipped backend must still package successfully. Run in the background, verified genuinely by reading the captured log file directly (not the background wrapper's reported exit code, per this repo's standing lesson): the log's own `$LASTEXITCODE` line reads `EXIT_CODE=0`. `Finished \`release\` profile [optimized] target(s) in 2m 33s`, `Finished 2 bundles at: ...msi\LifeOS_0.1.0_x64_en-US.msi` and `...nsis\LifeOS_0.1.0_x64-setup.exe`. Both files independently confirmed present on disk: `LifeOS_0.1.0_x64_en-US.msi` (2,973,696 bytes) and `LifeOS_0.1.0_x64-setup.exe` (1,370,818 bytes), timestamps matching the build run. |
| Module-boundary probe | Re-run against the current state (no `services::storage` visibility was touched this run, but re-verified anyway per the brief's explicit instruction): temporarily added `fn illegal_boundary_probe() { let _ = services::storage::project_repo::insert_project; }` to `lib.rs`, `cargo check` → `error[E0603]: module 'storage' is private`, confirmed, probe removed, `cargo check --tests` re-run clean. |

---

## 12. Files changed

**New:**
- `src-tauri/src/domain/tests.rs` - 3 `LifecycleStatus` round-trip unit
  tests.
- `docs/reports/REPORT_M2_PROJECT_STORAGE_HARDENING_2026-09-17.md` -
  this report.

**Modified:**
- `src-tauri/src/domain/mod.rs` - `GoalStatus` renamed to
  `LifecycleStatus` throughout; doc comments rewritten (rename
  rationale, both-entities sharing note, terminal-state-removal note
  updated); `Project.progress`'s doc comment rewritten to state the now-
  canonical decision instead of an open interpretation; added
  `#[cfg(test)] mod tests;`.
- `src-tauri/src/services/storage/mod.rs` - added `enable_foreign_keys`,
  called from `open_encrypted`; `GoalStatus` references (none directly,
  only via re-exported helpers - N/A) unaffected.
- `src-tauri/src/services/storage/goal_repo.rs`,
  `src-tauri/src/services/storage/project_repo.rs` - `GoalStatus` →
  `LifecycleStatus`; `project_repo.rs`'s `goal_exists` doc comment
  rewritten to describe both enforcement layers now being real.
- `src-tauri/src/services/mod.rs` - `GoalStatus` → `LifecycleStatus`;
  `create_project`'s doc comment rewritten (FK now genuinely enforced,
  not just declared).
- `src-tauri/src/actions/mod.rs`, `src-tauri/src/actions/tests.rs` -
  `GoalStatus` → `LifecycleStatus`.
- `src-tauri/src/services/tests.rs` - `GoalStatus` → `LifecycleStatus`;
  4 new FK-enforcement integration tests (`req_i1`-`req_i4`) and their
  `foreign_keys_pragma_value` helper.
- `src-tauri/src/lib.rs` - illegal-boundary probe added and removed
  (§11), net diff is none.
- `docs/LIFEOS_SPEC.md` - §5.1: `Project.progress` canonical decision
  added; OPEN M2 ACTION-SET GAP added; §13: one-line cross-reference to
  the action-set gap added.
- `docs/PROJECT_STATE.md` - new §4e (this hardening pass), renumbered
  §4f; §1/§2/§7/§8/§9/§10 updated for the new test count, commit list,
  file tree, and known-problems changes.

**Explicitly NOT changed:**
`src-tauri/migrations/V1__create_goals.sql`,
`src-tauri/migrations/V2__create_projects.sql` (§9) - no schema/data
change, per the brief's explicit instruction not to edit either
migration merely for the rename, and no other reason required a schema
change this run. `Cargo.toml`/`Cargo.lock` - no dependency changes.
`services/storage/spike/*` - untouched, still passing as regression
(and now also implicitly covered by FK enforcement via the shared
`open_encrypted` primitive, with no behavior change since spike's schema
has no FK relationships). Any file under `src/` (React frontend) -
untouched. `CLAUDE.md` - no operating-policy change this run.

---

## 13. Remaining risks

- **FK enforcement's guarantee is scoped to `open_encrypted`'s
  callers.** True and complete today (production, migration tests, and
  spike all share this one primitive) - but a future connection-opening
  path added elsewhere would not automatically inherit it. Flagged in
  `PROJECT_STATE.md` §10 as a standing review note, not a current gap.
- **The Project action-set gap is real and now visible, not fixed.** A
  user cannot yet rename a project, change its priority, or relink it
  to a different goal through any frozen action - by design, this run
  documents rather than resolves it.
- **Neither Goal nor Project is wired into the running app** -
  unchanged, no Tauri command layer exists yet.
- **Everything already listed as a risk in the Goal hardening and
  Project vertical-slice reports** (`target_date` content validation,
  `CoreError`'s `Debug` exposing `diagnostic`, clean-machine runtime
  inferred from `dumpbin`, exact NeXa transport/confirmation UX) is
  unchanged by this run.

---

## 14. Whether GoalRelation is now safe to start

**Yes, for the storage/type-integrity concerns this run addressed.**
The database now genuinely enforces referential integrity
(`GoalRelation` will itself need `from_goal_id`/`to_goal_id` foreign
keys to `goals(id)`, and can now rely on the same `open_encrypted`
primitive for real enforcement from its first migration, not a second
retrofit); the shared status-type naming confusion that would have
compounded with a third/fourth consumer is resolved before it spread
further; and the discipline of documenting open questions rather than
resolving them silently in code has now been exercised twice (Goal's
terminal-status removal, this run's action-set-gap documentation) and
held both times.

**One live structural note specific to `GoalRelation`:** unlike
`Project.goal_id` (a single nullable FK), `GoalRelation` is an edge
table with **two** foreign keys (`from_goal_id`, `to_goal_id`) into the
*same* `goals` table - a genuinely new shape this codebase has not
built yet, worth attention when it's implemented, but not itself an
open question.

**Correction (2026-09-17, M2 GoalRelation vertical-slice report §2):**
this section originally claimed here that `GoalRelation.relation_type`
was an `UNRESOLVED QUESTION` per the architecture freeze's "no rigid
taxonomy yet" note - that was a factual mistake, conflating three
distinct things:

1. `GoalRelation.relation_type` - a **frozen, closed 11-value enum**
   (`SUPPORTS`/`CONFLICTS_WITH`/`DEPENDS_ON`/`BLOCKS`/`CONTRIBUTES_TO`/
   `REQUIRES`/`PARENT_OF`/`CHILD_OF`/`DUPLICATES`/`SUPERSEDES`/
   `COMPETES_FOR_RESOURCE`), not open at all.
2. The Personal Knowledge Layer's separate `Relation.relation_type`
   (`LIFEOS_SPEC.md` §5.3) - genuinely an intentionally free string,
   deferred until M6 usage patterns exist. Unrelated entity, unrelated
   field, same name only by coincidence.
3. `Goal.domain_tag` - the field the architecture freeze's own table
   actually flags "(free string - `UNRESOLVED QUESTION`: no rigid
   taxonomy yet)." That note was never about any `relation_type` at
   all.

`GoalRelation` was always safe to start on this specific point - see
`docs/reports/REPORT_M2_GOAL_RELATION_VERTICAL_SLICE_2026-09-17.md` for
the vertical slice that implements it against the frozen enum exactly
as listed above.
