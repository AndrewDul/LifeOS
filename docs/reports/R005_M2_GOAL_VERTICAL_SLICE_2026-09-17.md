# R005 — M2 Goal Vertical Slice: Storage Hardening + First Real Domain

Date: 2026-09-17
Scope: Part A (storage foundation cleanup for production use) + Part B
(Goal vertical slice: domain → actions → services → private
services/storage → encrypted SQLCipher). No Project/Task/Calendar/
Personal Knowledge/Fact. No Goal Intelligence reasoning. No Qwen/GPT/
Luna. No M3 UI wiring. M1 UI untouched.

---

## 1. Pre-run state

M1 accepted and closed. M2 architecture frozen
(`docs/reports/R003_M2_ARCHITECTURE_FREEZE_2026-09-17.md`, corrected
same day per its own errata). SQLCipher via `rusqlite`'s
`bundled-sqlcipher` feature proven as the M2 storage candidate
(`docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md`), but only as an
isolated `spike_records` proof - no production schema, no domain code,
no action layer existed. `src-tauri/.cargo/config.toml` hardcoded this
machine's OpenSSL path as committed repository config.

---

## 2. Storage cleanup performed (Part A)

### A1 — Spike artifact separation

- `src-tauri/migrations/` (the path `refinery::embed_migrations!`
  resolves for PRODUCTION use) now contains **only**
  `V1__create_goals.sql` - the real Goal schema. The old
  `V1__init.sql`/`V2__add_note_column.sql` (which create
  `spike_records`) were moved (`git mv`, preserving history) to
  `src-tauri/src/services/storage/spike/migrations/` - a directory
  `refinery::embed_migrations!` only reaches from
  `services::storage::spike`, which is compiled `#[cfg(test)]`-only and
  never part of a release build.
- Proven with a real test, not just asserted:
  `services::tests::req3_production_db_has_no_spike_records` bootstraps
  a fresh production database and queries `sqlite_master` for a
  `spike_records` table - asserts it does not exist. **PASS.**
- The spike's own 10-requirement regression suite
  (`docs/reports/R004_M2_STORAGE_SPIKE_2026-09-17.md`) was moved,
  unmodified in behavior, into
  `services::storage::spike::tests` (`#[cfg(test)]`-only) - see §14.
  All 9 still pass.

### A2 — OpenSSL / Windows runtime dependency: measured, not assumed

Inspected the actual release binary's import table with
`dumpbin /dependents` (from the VS 2022 MSVC toolchain already installed
this machine):

```
& "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.43.34808\bin\Hostx64\x64\dumpbin.exe" /dependents "D:\Dev\LiFeOS\src-tauri\target\release\lifeos.exe"

  Image has the following dependencies:
    bcryptprimitives.dll
    ntdll.dll
    kernel32.dll
    user32.dll
    api-ms-win-core-synch-l1-2-0.dll
    comctl32.dll
    oleaut32.dll
    gdi32.dll
    dwmapi.dll
    ole32.dll
    shell32.dll
    shlwapi.dll
    advapi32.dll
    api-ms-win-crt-math-l1-1-0.dll
    api-ms-win-crt-string-l1-1-0.dll
    api-ms-win-crt-convert-l1-1-0.dll
    api-ms-win-crt-runtime-l1-1-0.dll
    api-ms-win-crt-stdio-l1-1-0.dll
    api-ms-win-crt-locale-l1-1-0.dll
    api-ms-win-crt-heap-l1-1-0.dll
```

**No `libcrypto*.dll` or `libssl*.dll` anywhere in the list.** Every
listed DLL is a standard Windows OS/Universal-CRT component present on
any Windows 10/11 install - nothing LifeOS-specific needs bundling.
OpenSSL is **statically linked** into `lifeos.exe` (the `.lib` files
`bundled-sqlcipher` links against at `OPENSSL_LIB_DIR` resolved to
static archives, not DLL import libraries, in this configuration). Also
confirmed no stray OpenSSL DLL was copied alongside the binary by any
build step (`target/release/` contains only `lifeos_lib.dll`, our own
crate's `cdylib` output, not an external dependency).

**Result: PASS.** OpenSSL is a *build-time-only* dependency (needs the
dev package installed on the machine that compiles LifeOS) - the
*shipped* application has no runtime dependency on it, and the
installer therefore does not need to bundle anything extra for this.
This is stronger evidence than "the installer was produced" (explicitly
not equated with clean-machine-runtime proof, per the brief) - it is a
direct inspection of what the binary actually needs to load, though it
still stops short of an actual execution test on a machine that has
never had Rust/Tauri/OpenSSL installed (not available in this
environment).

### A3 — Machine-specific build config removed

`src-tauri/.cargo/config.toml` (the hardcoded
`C:\Program Files\OpenSSL-Win64\...` paths) **deleted**. Replaced with
`scripts/setup-windows-storage-env.ps1`:
- Checks the standard OpenSSL dev-package install location.
- Fails with a clear, actionable error (including the exact `winget`
  install command) if not found.
- If found, sets `OPENSSL_DIR`/`OPENSSL_LIB_DIR`/`OPENSSL_INCLUDE_DIR`
  as **persistent user environment variables**
  (`[Environment]::SetEnvironmentVariable(..., "User")`) - no admin
  rights required, survives across terminal sessions/reboots, and any
  later `cargo build` (in a *new* terminal) picks it up automatically
  without a committed config file.
- Contains no secrets - only a public install path.

Run once for real on this machine (`pwsh -File
scripts/setup-windows-storage-env.ps1`) - output confirmed it detected
the install and set the three variables. Note: because this session's
own shell processes do not re-read Windows environment variables set by
a separate process mid-session (confirmed pattern from earlier in M2 -
the same reason `rustup`'s PATH update needed manual export), this run's
own remaining build/test commands still exported the three variables
directly per-command; a fresh terminal for a human developer would not
need to.

---

## 3. Production DB path / key lifecycle (Part B)

`services::production_db_path()` resolves the real path via
`dirs::data_dir()` - the same underlying mechanism Tauri's own
`app.path().app_data_dir()` uses, called directly since no `AppHandle`
exists yet (nothing wires this into `run()` this run). Windows:
`%APPDATA%\com.lifeos.app\lifeos.db` (matches `tauri.conf.json`'s
`identifier`).

`security::get_or_create_production_master_key()`:
- **Credential identity:** service `LifeOS`, account
  `database-master-key-v1` - versioned account name so a future
  rotation scheme (`-v2`, etc.) can coexist unambiguously.
- Random 256-bit key, generated once via `rand`, hex-encoded.
- Stored/retrieved via the `keyring` crate (Windows Credential Manager).
- Idempotent - safe to call on every app startup; generates only if no
  entry exists yet.
- Never logged - see §14's req15 (static scan) and code inspection: the
  key only ever exists as a local `String` passed directly between
  `security::`/`services::`/`storage::` functions, never stored in a
  struct with a `Debug`/`Display` impl.

`services::bootstrap_db_at(path, key_hex)` is the reusable core
(create parent dir → open encrypted → verify key → run migrations),
parameterized so tests exercise it against a temp path/key, never the
real production path or keyring entry.
`services::bootstrap_production_db()` wraps it with the real path/key
for future use (not called from anywhere yet this run).

---

## 4. Production migration (Part C2)

`src-tauri/migrations/V1__create_goals.sql` - `goals` and
`goal_audit_log` tables only, no Project/Task/Fact:

```sql
CREATE TABLE goals (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL CHECK (length(trim(title)) > 0),
    description     TEXT,
    status          TEXT NOT NULL CHECK (status IN ('ACTIVE','PAUSED','COMPLETED','ABANDONED')),
    target_date     TEXT,
    priority_user   INTEGER CHECK (priority_user IS NULL OR (priority_user BETWEEN 1 AND 5)),
    priority_system INTEGER CHECK (priority_system IS NULL OR (priority_system BETWEEN 1 AND 5)),
    progress        INTEGER NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
    domain_tag      TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX idx_goals_status ON goals (status);
CREATE INDEX idx_goals_domain_tag ON goals (domain_tag);
CREATE INDEX idx_goals_target_date ON goals (target_date);

CREATE TABLE goal_audit_log (
    id                  TEXT PRIMARY KEY,
    goal_id             TEXT NOT NULL REFERENCES goals (id),
    action_type         TEXT NOT NULL CHECK (action_type IN ('CreateGoal','UpdateGoal')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI','CloudLLM','NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit','Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);
CREATE INDEX idx_goal_audit_log_goal_id ON goal_audit_log (goal_id);
```

CHECK constraints enforce non-empty title, valid status enum, bounded
priority (1-5) and progress (0-100) at the database layer too - defense
in depth alongside the action-layer validation (§8), not a substitute
for it. `goal_audit_log.goal_id` is a foreign key to `goals.id`.
Indexes on the columns the frozen v0.1 UI/queries obviously need
(status filter for "active goals", domain_tag, target_date).

---

## 5. Goal domain model (Part C1)

`src-tauri/src/domain/mod.rs` - pure data, no I/O:

- **`GoalId`**: wraps `uuid::Uuid`, generated via `Uuid::now_v7()` -
  **UUID v7** as directed, time-ordered and assignable offline, suitable
  for future multi-device/NeXa sync without a central ID authority.
- **`GoalStatus`**: `Active`/`Paused`/`Completed`/`Abandoned`.
  `can_transition_to` encodes the one transition rule this slice
  enforces: **`Completed` and `Abandoned` are terminal** - no
  transition out of either; every other transition (including a same-
  status no-op) is allowed. Documented as the single source of truth
  `actions::validate_update_goal` calls.
- **Progress**: `u8`, bounded `0..=100` - chosen explicitly over a float
  or open scale specifically so the representation is unambiguous, per
  the build brief.
- **Priority** (`priority_user`/`priority_system`): `i32`, bounded
  `1..=5` (1 = lowest, 5 = highest) - a small, documented scale chosen
  since none was specified; `priority_system` is a placeholder field for
  future M6 Goal Intelligence (`LIFEOS_SPEC.md` §5.2) - nothing computes
  it in M2.
- Field size limits: title ≤200 chars, description ≤4000, domain_tag
  ≤60 - deliberately generous but finite, enforced in `actions`, not SQL
  alone.
- Timestamps: `chrono::DateTime<Utc>` throughout (`created_at`,
  `updated_at`, `target_date`), stored as RFC3339 text in SQLite (which
  has no native datetime type - the standard idiomatic approach,
  sortable lexicographically too).

---

## 6. CreateGoal implementation (Part C3/C4/C5)

`services::create_goal(conn, meta, payload)`:
1. `actions::validate_create_goal` - trims and validates title (non-
   empty, ≤200 chars), description/domain_tag length, priority bounds.
   Returns **all** errors found, not just the first.
2. Confirmation class: `effective_class(ExplicitAdd, meta.provenance)` -
   `ExplicitAdd` for a UI-explicit request (this slice's tests), would
   compute `InferredChange` for an AI-proposed one (untested this slice
   - no CloudLLM/NeXa caller exists yet, but the logic is real and unit-
   tested via `actions::effective_class` directly... *(no separate
   `actions`-level unit test file was added this run - classification is
   exercised indirectly through `services::tests`' assertions on
   `confirmation_class`; a dedicated `actions` unit-test module is a
   reasonable next-slice addition, noted in §17)*.
3. If the class blocks (`InferredChange`/`Delete` - neither reachable
   with this slice's only tested provenance, `Explicit`) and the caller
   hasn't pre-confirmed, returns `ActionOutcome::ConfirmationRequired`
   **without writing anything**.
4. Otherwise: builds the `Goal` (new `GoalId`, `status = Active`,
   `progress = 0`), inserts it, and **always** writes a
   `goal_audit_log` entry (`previous_snapshot = NULL` for a create) -
   the frozen policy only *requires* an audit entry at `ImportantChange`
   and above; logging `ExplicitAdd` too is a documented superset, not a
   deviation, giving full Goal history from day one.

No partial write is possible: validation happens strictly before any
`INSERT` (verified by `req7`/`req8` asserting `SELECT count(*) FROM
goals` is 0 after a rejected call).

---

## 7. UpdateGoal implementation (Part C3/C4/C5)

`services::update_goal(conn, meta, payload)`:
1. Fetches the existing goal - `ActionOutcome::NotFound` if absent
   (structured, not an error string).
2. `actions::validate_update_goal(&existing, &payload)` - rejects an
   update touching no fields at all, empty/overlong title, progress/
   priority out of bounds, and (using the *existing* goal's current
   status) invalid status transitions.
3. Confirmation class: `effective_class(ImportantChange, provenance)` -
   `ImportantChange` for the tested UI-explicit path, per the build
   brief's explicit instruction (`UpdateGoal: type floor =
   IMPORTANT_CHANGE`).
4. Snapshots the existing goal as JSON (`serde_json`) *before* mutating
   - this is the "minimum appropriate encrypted audit/action
   information" the brief asked for to support a future Undo, stored in
   `goal_audit_log.previous_snapshot`. Deliberately not a full event-
   sourcing system: one row per mutation, one JSON snapshot, nothing
   more.
5. Applies only the `Some(...)` fields from the payload (`None` = leave
   unchanged - this slice does not support explicitly clearing an
   optional field back to `NULL`, a documented simplification, see §17),
   updates `updated_at`, persists, writes the audit entry.

Zero partial writes: validation (including the NotFound check) happens
before any `UPDATE` statement runs - proven by `req11` asserting the
goal's `status`/`updated_at` are byte-for-byte unchanged after a
rejected transition attempt.

---

## 8. Confirmation classification (Part C5)

`actions::effective_class(type_floor, provenance) = max` by severity,
exactly as frozen (`LIFEOS_SPEC.md` §5.5):
`SafeRead(0) < ExplicitAdd(1) < InferredChange(2) < ImportantChange(3) <
Delete(4)`, with `Sensitive` kept as an orthogonal, unused-this-slice
overlay (no Goal field is Privacy-Firewall-tagged sensitive).
`provenance_class` = `ExplicitAdd` for `Provenance::Explicit`,
`InferredChange` for `Provenance::Inferred` - orthogonal to *which*
`Actor` (UI/CloudLLM/NeXa) is asking, matching the freeze's own framing.

**No UI modals invented.** `blocks_until_confirmed()` (true only for
`InferredChange`/`Delete`) is the only behavioral consequence
implemented - a caller gets `ConfirmationRequired` with the computed
class as typed metadata; rendering that into an actual confirmation
UX is explicitly an M3 concern, not built here.

---

## 9. get_active_goals capability (Part D)

`services::get_active_goals(conn)` - `SAFE_READ`, no validation needed,
a thin wrapper over `storage::goal_repo::get_active_goals` (which
filters `WHERE status = 'ACTIVE'` at the SQL layer, verified by `req12`
alongside a `Paused` goal to prove the filter actually excludes it).
`services::get_goal(conn, id)` also added - a narrow internal read used
by tests and future `UpdateGoal` UX (e.g. "show me what I'm about to
change"), not growing the public capability surface beyond the frozen
list otherwise.

**No `capability` module added this run.** `services`'s `pub(crate)`
functions *are* the capability boundary for this slice - a dedicated
`capability` module would be an empty pass-through with no real Tauri
command or NeXa transport behind it yet ("No React wiring yet"),
deferred until one exists to justify it.

---

## 10. Exact module-boundary enforcement (re-verified after restructuring)

Same principle as the storage-spike report, **re-verified for the new,
deeper module tree** (`services` → `services::storage` →
`services::storage::goal_repo`), not assumed to still hold after the
restructuring:

```
src-tauri/src/
├── lib.rs          mod actions; mod domain; mod security; mod services;  (all private)
├── domain/          pure types, no I/O
├── actions/          pure validation/classification, no I/O
├── security/         keyring only
└── services/
    mod.rs             mod storage;  (private - no pub at all)
    storage/
      mod.rs            pub(super) primitives (open_encrypted, verify_key, rekey, integrity_check)
      goal_repo.rs       pub(in crate::services) - one level deeper than storage/mod.rs,
                         so pub(super) alone would only reach storage/mod.rs itself;
                         pub(in crate::services) is the correct visibility for a
                         grandchild module that still must stay inside `services`
      spike/             #[cfg(test)] only - never in a release build
```

**Proof, not just a claim** (repeated deliberately, since the module
tree changed structurally from the spike): a throwaway function was
added to `lib.rs` attempting `services::storage::goal_repo::insert_goal`
directly, then removed immediately after capturing the result:

```
$ cargo check
error[E0603]: module `storage` is private
  --> src\lib.rs:22:23
   |
22 |     let _ = services::storage::goal_repo::insert_goal;
   |                       ^^^^^^^             ----------- function `insert_goal` is not publicly re-exported
   |                       |
   |                       private module
```

`cargo check --tests` re-run clean immediately after removing the probe
(§14).

---

## 11. Tests and exact results (Part E)

Command: `cd src-tauri && cargo test -- --test-threads=1`

```
running 21 tests
test services::storage::spike::tests::req10_keyring_roundtrip ... ok
test services::storage::spike::tests::req1_encrypted_database_creation_and_read ... ok
test services::storage::spike::tests::req2_persistence_across_reopen ... ok
test services::storage::spike::tests::req3_wrong_key_fails_cleanly ... ok
test services::storage::spike::tests::req4_encrypted_at_rest_no_plaintext ... ok
test services::storage::spike::tests::req5_migration_v1_to_v2 ... ok
test services::storage::spike::tests::req6_rekey ... ok
test services::storage::spike::tests::req7_8_backup_and_restore ... ok
test services::storage::spike::tests::req9_corruption_detected_safely ... ok
test services::tests::req10_update_nonexistent_goal_returns_not_found ... ok
test services::tests::req11_invalid_update_makes_zero_changes ... ok
test services::tests::req12_get_active_goals_filters_correctly ... ok
test services::tests::req13_wrong_key_rejected_for_production_db ... ok
test services::tests::req15_no_key_material_in_source_prints ... ok
test services::tests::req1_2_fresh_production_db_encrypted_with_goal_schema ... ok
test services::tests::req3_production_db_has_no_spike_records ... ok
test services::tests::req4_master_key_survives_reopen_via_keyring ... ok
test services::tests::req5_6_create_goal_succeeds_and_persists_across_reopen ... ok
test services::tests::req7_empty_and_whitespace_title_rejected ... ok
test services::tests::req8_overlong_fields_rejected ... ok
test services::tests::req9_valid_update_goal_succeeds ... ok

test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.35s
```

(SQLCipher's own HMAC-failure log lines print for the wrong-key/
corruption tests, same as the storage spike - real cipher behavior, not
test noise, omitted above for brevity.)

Mapping to the build brief's 16 required checks:

| # | Requirement | Test(s) | Result |
|---|---|---|---|
| 1 | Fresh production DB initializes encrypted | `req1_2_fresh_production_db_encrypted_with_goal_schema` | PASS |
| 2 | Production DB contains Goal schema | `req1_2_...` (goals + goal_audit_log tables) | PASS |
| 3 | Production DB does NOT contain spike_records | `req3_production_db_has_no_spike_records` | PASS |
| 4 | Master key survives reopen via keyring | `req4_master_key_survives_reopen_via_keyring` | PASS (real OS credential store, test-specific account) |
| 5 | Valid CreateGoal succeeds | `req5_6_create_goal_succeeds_and_persists_across_reopen` | PASS |
| 6 | Created Goal persists after close/reopen | `req5_6_...` (second bootstrap_db_at call) | PASS |
| 7 | Empty/whitespace title rejected before write | `req7_empty_and_whitespace_title_rejected` | PASS (0 rows written) |
| 8 | Overlong/invalid fields rejected deterministically | `req8_overlong_fields_rejected` | PASS (both errors reported, 0 rows written) |
| 9 | Valid UpdateGoal succeeds | `req9_valid_update_goal_succeeds` | PASS |
| 10 | UpdateGoal on nonexistent ID → structured NotFound | `req10_update_nonexistent_goal_returns_not_found` | PASS |
| 11 | Invalid UpdateGoal makes ZERO DB changes | `req11_invalid_update_makes_zero_changes` | PASS (status/updated_at byte-identical) |
| 12 | get_active_goals returns only ACTIVE | `req12_get_active_goals_filters_correctly` | PASS |
| 13 | SQLCipher wrong-key protection intact | `req13_wrong_key_rejected_for_production_db` | PASS |
| 14 | services/storage privacy boundary unbypassable | manual compile-fail proof (§10) | PASS |
| 15 | No key material in logs/test output | `req15_no_key_material_in_source_prints` | PASS (static source scan - see caveat in test doc comment: no logging framework is wired up yet, so this is a heuristic safety net, not a runtime capture; documented rather than overclaimed) |
| 16 | Existing spike regression tests still pass after relocation | 9 tests under `services::storage::spike::tests` | PASS |

---

## 12. Build/release/package results (Part F)

All commands actually run, exact results, no assumed passes:

| Command | Result |
|---|---|
| `cargo fmt --check` (before fixing) | FAIL - style-only diffs (line-wrapping), no logic changes |
| `cargo fmt` (auto-fix) then `cargo fmt --check` | PASS (exit 0) |
| `cargo check --tests` | PASS - 0 warnings after adding module-level `#[allow(dead_code)]` (same justification as the storage-spike run: nothing is wired into `run()` yet) |
| `cargo test -- --test-threads=1` | PASS - 21/21 (§11) |
| `cargo build --release` | PASS - `Finished release profile [optimized] target(s) in 3m 32s`; real `lifeos.exe` (4,016,640 bytes) confirmed on disk |
| `npm run build` (frontend) | PASS - unaffected by this run's changes, 27 modules, 144ms |
| `npm.cmd run tauri build` | PASS - `Finished release profile [optimized] target(s) in 2m 57s`; both `LifeOS_0.1.0_x64_en-US.msi` (2,973,696 bytes) and `LifeOS_0.1.0_x64-setup.exe` (1,370,844 bytes) produced and confirmed on disk. Code signing not configured - correctly out of scope, not a failure (unchanged from the storage-spike run's finding). |

Rerun specifically because the Goal slice added new dependencies
(`uuid`, `chrono`, `dirs`) to the same crate `bundled-sqlcipher`/
`rusqlite` lives in - confirms the packaging pipeline still succeeds
with the larger dependency graph, not just that the spike's smaller one
did.

---

## 13. Files changed

**New:**
- `scripts/setup-windows-storage-env.ps1`
- `src-tauri/migrations/V1__create_goals.sql` (production)
- `src-tauri/src/domain/mod.rs`
- `src-tauri/src/actions/mod.rs`
- `src-tauri/src/security/mod.rs`
- `src-tauri/src/services/storage/goal_repo.rs`
- `src-tauri/src/services/storage/spike/mod.rs` (moved orchestration + errors)
- `src-tauri/src/services/storage/spike/tests.rs` (moved, unmodified behavior)
- `src-tauri/src/services/storage/spike/migrations/V1__init.sql`, `V2__add_note_column.sql` (moved via `git mv`)
- `src-tauri/src/services/tests.rs` (new Goal-slice tests; old spike tests.rs content relocated to spike/tests.rs)

**Modified:**
- `src-tauri/Cargo.toml` (+`uuid`, `chrono`, `dirs`; comments updated)
- `src-tauri/Cargo.lock`
- `src-tauri/src/lib.rs` (+`mod actions; mod domain; mod security;`)
- `src-tauri/src/services/mod.rs` (full rewrite: production bootstrap + Goal actions)
- `src-tauri/src/services/storage/mod.rs` (generic primitives kept, spike moved out, goal_repo/migrations added)
- `docs/LIFEOS_SPEC.md`, `docs/PROJECT_STATE.md` (see below)

**Deleted:**
- `src-tauri/.cargo/config.toml` (machine-specific, replaced by the setup script)
- `src-tauri/migrations/V1__init.sql`, `V2__add_note_column.sql` (moved, not deleted - see above)

---

## 14. Known risks

- **`cargo fmt` reformatted 8 files** as part of this run (whitespace/
  line-wrap only, `cargo fmt --check` confirms 0 remaining diffs, and
  the full test suite was re-run after and still passes) - worth noting
  since it touched files beyond the newly-authored ones.
- **`bundled-sqlcipher`'s build-time OpenSSL dependency is still real**,
  just no longer hardcoded into committed config - any machine building
  this needs `scripts/setup-windows-storage-env.ps1` run once (or the
  package installed + env vars set some other way) before `cargo build`
  works. Not a regression from the storage-spike run, just re-stated
  since the mechanism changed.
- **`ActionOutcome::ConfirmationRequired`'s `InferredChange` path is
  implemented but not exercised by any test this run** - no
  `Provenance::Inferred` caller exists yet (would come with M5's cloud-
  LLM integration). The logic is straightforward enough (a single `if`
  gated on `effective_class`) that this is a reasonable, explicitly-
  flagged gap rather than a silent one.
- **`UpdateGoalPayload` cannot explicitly clear an optional field to
  NULL** (`None` always means "leave unchanged", never "clear") - a
  deliberate, documented simplification for this slice; revisit if a
  real need to null out e.g. `description` or `target_date` appears.
- **No dedicated `actions`-level unit tests** were added separately from
  `services`-level integration tests - `effective_class`/
  `validate_create_goal`/`validate_update_goal` are exercised indirectly
  through `services::tests`' assertions, not via focused pure-function
  unit tests. Works today, but a future slice touching `actions` more
  heavily should add them directly.
- **Clean-machine runtime is inferred from `dumpbin` output, not an
  actual execution test** on a machine that has never had any Rust/
  Tauri/OpenSSL tooling installed - not available in this environment
  (see §2, A2).

---

## 15. Exact recommended next M2 slice

**Project**, as the next domain entity - it already has a real
relationship to `Goal` (`Project.goal_id`, nullable) in the frozen
v0.1 domain model (`LIFEOS_SPEC.md` §5.1), so implementing it next
proves the module structure handles a *second* entity and a foreign-key
relationship between entities cleanly, before attempting `Task`/
`Challenge`/the Personal Knowledge Layer (each meaningfully larger).
Reuse the exact same domain → actions → services →
`services::storage::project_repo` pattern proven here - no new
architectural decisions should be needed, only repetition of a now-
validated shape. Do not implement Project's *sub*-entities (Milestone)
in the same pass; keep each slice this small.
