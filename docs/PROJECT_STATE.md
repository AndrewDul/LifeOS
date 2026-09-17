# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it
whenever project state actually changes (`CLAUDE.md`, Documentation
policy) — not only as an end-of-run formality.

Last updated: 2026-09-17 (M2 Goal hardening — atomic mutation+audit,
typed Core error model, tri-state field-patch semantics, removed an
unapproved terminal-status rule, direct action-layer unit tests, and
documentation corrections, on top of the same day's Goal vertical
slice/storage hardening).

---

## 1. Milestone status

| Milestone | Status |
|---|---|
| M0 — Environment ready | **DONE** (§5) |
| M1 — Visual shell | **ACCEPTED BY USER on 2026-09-17** (§3) |
| M2 — LifeOS Core + encrypted storage | Architecture frozen; storage PROVEN + hardened; **Goal vertical slice implemented AND hardened** 2026-09-17 (§4); rest of domain model not started |
| M3-M10 | Not started |

**M1 is closed** — unchanged, not touched this run.

**M2 now has one real, working, hardened, tested domain entity: Goal**,
end to end through `domain → actions → services → private
services/storage → encrypted SQLCipher`. Two runs on 2026-09-17 built
this: first the vertical slice itself (storage foundation hardening +
Goal implemented), then a **dedicated hardening pass** after
architectural review found issues that had to be corrected before the
Goal pattern could safely be copied to `Project` — see §4b-§4c. Next
step is repeating the now-hardened pattern for `Project` — not the rest
of the domain model at once, and still not M3.

---

## 2. What exists right now

A Tauri 2 + React + TypeScript desktop app that launches in a real
window (accepted M1 shell, untouched), plus a working, hardened
encrypted Goal store: create and update goals with deterministic
validation, atomic mutation+audit transactions, tri-state patch
semantics (Set/Unchanged/Clear), typed storage errors, confirmation
classification, and a read API - all proven by 42 automated tests (9
storage-spike regression, 17 Goal-slice/hardening integration against a
real encrypted DB, 16 direct action-layer unit tests). **Not wired into
the running app yet** - no Tauri command layer exists, so the UI cannot
create/see real goals yet.

Repository contents as of this run:

```
LiFeOS/
├── .git/, .gitignore, .vscode/extensions.json, CLAUDE.md, README.md
├── scripts/
│   └── setup-windows-storage-env.ps1   (new - see §4b)
├── docs/
│   ├── LIFEOS_SPEC.md, RESEARCH_DECISIONS.md, PROJECT_STATE.md
│   └── reports/    REPORT_UI_REFINEMENT_..._2026-09-17.md
│                    REPORT_NAVIGATION_CORRECTION_..._2026-09-17.md
│                    REPORT_M2_ARCHITECTURE_FREEZE_2026-09-17.md
│                    REPORT_M2_STORAGE_SPIKE_2026-09-17.md
│                    REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md
├── index.html, vite.config.ts, tsconfig*.json, package.json
├── public/, src/    (React frontend - M1, accepted, untouched)
└── src-tauri/
    ├── migrations/V1__create_goals.sql          (PRODUCTION schema - Goal only)
    ├── src/
    │   ├── main.rs, lib.rs       (run() unchanged; declares the 5 new modules, all private)
    │   ├── core_error.rs          CoreError/CoreErrorKind - typed Core-boundary error model
    │   │                          (Storage/Migration/Security/Internal), added in hardening
    │   ├── domain/mod.rs          Goal, GoalId (UUID v7), GoalStatus, Progress, Priority
    │   │                          (no terminal-status rule - removed in hardening)
    │   ├── actions/
    │   │   ├── mod.rs              Actor, Provenance, ActionMeta, ConfirmationClass,
    │   │   │                       ActionOutcome<T> (StorageError now typed CoreError),
    │   │   │                       ValidationError, FieldPatch<T> (hardening), validation
    │   │   └── tests.rs             16 direct action-layer unit tests (hardening, new)
    │   ├── security/mod.rs        OS-keyring master-key lifecycle
    │   └── services/
    │       ├── mod.rs              bootstrap_production_db, create_goal/update_goal
    │       │                       (now atomic: &mut Connection + rusqlite transaction),
    │       │                       get_active_goals, get_goal (capability API contract's
    │       │                       names - no dedicated capability/ module yet, see
    │       │                       LIFEOS_SPEC.md §5.6/§10.1's hardening-run correction)
    │       ├── tests.rs            17 Goal-slice/hardening integration tests (incl. 2
    │       │                       forced-audit-failure transaction-rollback tests)
    │       └── storage/            PRIVATE (mod storage; - no pub at all)
    │           ├── mod.rs           generic encrypted-SQLite primitives + prod migrations
    │           ├── goal_repo.rs     Goal SQL access, pub(in crate::services)
    │           └── spike/           #[cfg(test)] only - old storage-spike suite, moved
    │               ├── mod.rs, tests.rs, migrations/V1__init.sql, V2__add_note_column.sql
    ├── Cargo.toml, Cargo.lock, tauri.conf.json, build.rs
    ├── capabilities/default.json
    └── icons/
```

`node_modules/`, `dist/`, `src-tauri/target/` (including this run's
MSI/NSIS installers) are gitignored and untracked. **No generated user
database, no real credential-store secret, and no build artifact was
committed** - verified before committing (§7).

**`src-tauri/.cargo/config.toml` was deleted this run** (was a
committed, machine-specific hardcoded OpenSSL path) - see §4b.

---

## 3. M1 — ACCEPTED (unchanged this run)

Not touched. See prior entries in this file's git history and the two
refinement reports + navigation-correction report for full detail.

---

## 4. M2 — LifeOS Core + encrypted storage

### 4a. Storage hardened for production use (this run)

- **Spike/production separation:** `src-tauri/migrations/` now contains
  **only** the real Goal schema (`V1__create_goals.sql`). The old
  `spike_records`-creating migrations were moved (`git mv`, history
  preserved) to `src-tauri/src/services/storage/spike/migrations/`,
  reachable only from `#[cfg(test)]`-only code, never a release build.
  Proven by a real test (`services::tests::req3_production_db_has_no_spike_records`),
  not just asserted.
- **Module-boundary re-verified after restructuring:** the tree grew a
  level deeper (`services::storage::goal_repo`); the same "attempt the
  illegal access, capture the real compiler error" proof was repeated
  against the new shape - `error[E0603]: module 'storage' is private`,
  confirmed again, not assumed to still hold.
- **OpenSSL runtime dependency measured, not assumed:** `dumpbin
  /dependents` on the release binary shows **no `libcrypto`/`libssl`
  DLL** in its import table - OpenSSL is statically linked, so the
  *shipped* app has no runtime dependency on it (build-time only). MSI
  and NSIS installers rebuilt successfully with the larger dependency
  graph (+ `uuid`, `chrono`, `dirs`) and confirmed on disk.
- **Machine-specific config removed:** `src-tauri/.cargo/config.toml`
  (hardcoded absolute OpenSSL path) deleted. Replaced by
  `scripts/setup-windows-storage-env.ps1` - checks the standard install
  location, fails clearly if missing (with the exact `winget` command to
  fix it), and sets `OPENSSL_DIR`/`OPENSSL_LIB_DIR`/`OPENSSL_INCLUDE_DIR`
  as **persistent user environment variables** (no admin needed) if
  found. Run once per dev machine; a new terminal picks it up
  automatically.

Full evidence: `docs/reports/REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`
Parts A1-A3.

### 4b. Goal vertical slice implemented (2026-09-17, later hardened - §4c)

The first real domain entity, end to end:

- **Domain** (`domain::Goal`): `id` (UUID v7), `title`, `description`,
  `status` (`ACTIVE`/`PAUSED`/`COMPLETED`/`ABANDONED` - all four are
  peer states with no terminal-state restriction, corrected in the
  hardening pass, §4c), `target_date`, `priority_user`/`priority_system`
  (bounded 1-5), `progress` (bounded integer 0-100), `domain_tag`,
  `created_at`/`updated_at` (UTC).
- **Actions**: `CreateGoal` (floor `EXPLICIT_ADD`), `UpdateGoal` (floor
  `IMPORTANT_CHANGE`) - deterministic validation (title trim/non-empty/
  length, description/tag length, priority/progress bounds) runs
  *before* any write; zero partial writes on failure (tested
  explicitly, and since the hardening pass this extends to the goal
  write + its audit row being one atomic transaction, §4c).
- **Confirmation classification**: `effective_class = max(type_floor,
  provenance_class)`, implemented exactly per the frozen policy - no UI
  modals invented, just typed metadata (`ConfirmationClass`) a future M3
  UI can render against. Every persisted Create/Update writes a
  `goal_audit_log` row (previous-state JSON snapshot for updates) - the
  minimum state to support a future Undo, not a full event-sourcing
  system.
- **Read capability**: `services::get_active_goals()` (matching the
  frozen v0.1 capability API contract's `get_active_goals()` name) and
  an internal `get_goal(id)` - both go through `services` directly today
  only because `services::tests` is their only caller; a dedicated
  `capability` module is deferred until a real Tauri command or NeXa
  transport exists to front (see `LIFEOS_SPEC.md` §5.6/§10.1's
  hardening-run wording correction - the capability *contract* is
  frozen, the module implementing it is not built).
- **Production DB lifecycle**: `services::production_db_path()`
  (`%APPDATA%\com.lifeos.app\lifeos.db` via `dirs::data_dir()`, the same
  mechanism Tauri's own path API uses) + `security::
  get_or_create_production_master_key()` (service `LifeOS`, account
  `database-master-key-v1`, random 256-bit, OS keyring, idempotent,
  never logged) + `services::bootstrap_production_db()` (create dir,
  open, verify key, migrate). **Not called from anywhere yet** - no
  Tauri command/`run()` wiring this run.

Full evidence, exact SQL, and every test result:
`docs/reports/REPORT_M2_GOAL_VERTICAL_SLICE_2026-09-17.md`.

### 4c. Goal hardening (2026-09-17, dedicated follow-up run)

Architectural review of the vertical slice above found several issues
that had to be fixed before the Goal pattern could safely be copied to
`Project` or any other entity. All were fixed in this repo, not
deferred:

1. **Atomic mutation + audit.** `CreateGoal`/`UpdateGoal` previously
   wrote the goal row, then called
   `let _ = storage::goal_repo::insert_audit_entry(...)` - silently
   discarding a failed audit write while still returning
   `ActionOutcome::Success`. Both now run inside one `rusqlite`
   transaction (`conn.transaction()`); either write failing means the
   transaction is never committed and `Transaction::drop` rolls it back
   automatically. `create_goal`/`update_goal` now take `&mut Connection`
   (transactions need unique access), not `&Connection`.
2. **Proven with a real forced-failure test, not just code inspection.**
   A `BEFORE INSERT ... RAISE(FAIL, ...)` SQLite trigger on
   `goal_audit_log`, installed only inside the test, forces the audit
   write to fail after the goal write already succeeded within the same
   transaction. Two tests
   (`req_h1_create_goal_audit_failure_rolls_back_goal_insert`,
   `req_h2_update_goal_audit_failure_rolls_back_goal_update`) prove zero
   goal rows/no goal mutation survive, the connection remains usable
   afterward, and the action returns a typed `StorageError`, never
   `Success`.
3. **Typed Core error model.** `ActionOutcome::StorageError(String)` is
   now `StorageError(CoreError)` (new `core_error` module) -
   `CoreError` carries a closed `CoreErrorKind` (`Storage`/`Migration`/
   `Security`/`Internal`) as its public, matchable identity; the
   underlying driver error text is kept only as an internal
   `diagnostic()` string, never part of `Display`, never the
   authoritative API.
4. **`FieldPatch<T>` tri-state patch semantics.** `UpdateGoalPayload`'s
   `description`/`target_date`/`priority_user`/`domain_tag` (the
   nullable `Goal` fields; `domain_tag` is new to the payload this
   pass) now use `FieldPatch<T> { Unchanged, Set(T), Clear }` instead of
   a plain `Option<T>` that could not distinguish "leave alone" from
   "clear to NULL". `title`/`status`/`progress` stay `Option<T>` since
   those `Goal` fields are non-nullable. Proven against a real DB
   (`req_h4_update_goal_field_patch_set_clear_unchanged`) and with
   direct validation unit tests.
5. **Removed the unapproved terminal-status rule.** The vertical slice
   had invented "`COMPLETED`/`ABANDONED` cannot transition further" -
   never part of the frozen Goal status definition. Removed
   (`GoalStatus::can_transition_to` and
   `ValidationError::InvalidStatusTransition` deleted); every
   `GoalStatus -> GoalStatus` transition an explicit action requests is
   now allowed and still audit-logged
   (`req_h3_completed_goal_can_be_reopened_and_transition_is_audited`).
6. **Direct action-layer unit tests added.** 16 new tests in
   `actions/tests.rs` exercise `validate_create_goal`/
   `validate_update_goal`/`effective_class` directly (no DB, no I/O) -
   the vertical-slice report's flagged gap ("actions only tested
   indirectly") is closed.
7. **`Provenance::Inferred` path proven end-to-end, not deferred to
   M5.** `req_h5_inferred_create_goal_requires_confirmation_and_does_not_write`
   proves `CreateGoal` with `Provenance::Inferred` returns
   `ConfirmationRequired` and writes zero rows, then that a confirmed
   resubmission persists.
8. **Documentation corrected** (`LIFEOS_SPEC.md`): top-level status
   banner, the `GoalRelation` "from the start" overclaim (§5.2 - the V1
   migration implements `Goal` only), and the capability-module wording
   (§5.6/§10.1 - the capability API *contract* is frozen, a `capability/`
   module implementing it is not built yet).

Preserved, not regressed: SQLCipher encryption, keyring master-key
lifecycle, production/spike migration separation, the private
`services::storage` compile-time boundary (re-verified again via the
illegal-access probe), UUID v7 Goal IDs, UTC timestamps, progress 0-100,
priority 1-5, the release/package build, `setup-windows-storage-env.ps1`,
and all 9 storage-spike regression tests.

Full evidence: `docs/reports/REPORT_M2_GOAL_HARDENING_2026-09-17.md`.

**Goal is now considered safe to use as the template for `Project`** -
see that report's §13 for the exact reasoning.

### 4d. What's still architecturally frozen but not yet implemented

Personal Knowledge Layer, `GoalRelation`/goal relations, `Project`/
`Milestone`/`Task`/`Challenge`/`Plan`/`SolutionAttempt`/`Outcome`/
`CalendarEvent`/`Reminder`/`Question`/`ProgressUpdate`/
`WellbeingCheckin`, the rest of the frozen action list, `ForgetMemory`'s
corrected (hard-delete) semantics, the `knowledge`/`backup` modules, a
real `capability` module, NeXa transport. Full detail:
`docs/LIFEOS_SPEC.md` §5.1-§5.6.

### Next implementation step (M2)

Repeat the now-hardened pattern for **`Project`** (it already references
`Goal` via a nullable `goal_id` in the frozen domain model - a good
second entity to prove the pattern handles a cross-entity relationship).
Reuse `domain → actions → services → services::storage::project_repo`,
applying the same atomicity/typed-error/`FieldPatch` discipline from the
start rather than retrofitting it afterward. Do not implement Milestone
in the same pass, and do not invent product rules (like the removed
terminal-status one) inside the implementation - flag them as open
questions instead.

---

## 5. M0 status — Environment ready (RESOLVED, unchanged)

| Prerequisite | Status |
|---|---|
| Rust (`rustup`, `rustc`, `cargo`) | Installed — `rustc 1.98.1`, `cargo 1.98.1` |
| Node.js / npm | `v24.18.0` / `11.1.0` |
| Git | `2.47.1.windows.2` |
| WebView2 Runtime | `153.0.4234.32` |
| Visual Studio "Desktop development with C++" workload | Installed |
| OpenSSL (Windows dev package, for SQLCipher build) | Installed (`ShiningLight.OpenSSL.Dev`); env vars now set via `scripts/setup-windows-storage-env.ps1`, not committed config |

**Standing note, still true:** this automated session cannot perform
*admin-elevated* installs. `winget` package installs used so far have
not required elevation.

---

## 6. Measured hardware/software audit (carried forward, unchanged this run)

- OS: Microsoft Windows 11 Home, 10.0.26200, 64-bit
- CPU: 12th Gen Intel Core i5-12500H, 12 physical / 16 logical cores, 3.1GHz base
- RAM: 63.71 GB (2× 32GB Kingston, 3200 MT/s)
- GPU: NVIDIA GeForce RTX 3050 Ti Laptop — 4096 MiB (4GB) VRAM, driver 610.62
- Disks: `C:` 475.6GB (166.8GB free), `D:` 3815.4GB (2769.1GB free), both NVMe SSD
- Git 2.47.1.windows.2, Node v24.18.0, npm 11.1.0, Python 3.12.7, Rust 1.98.1
- Ollama: still not installed (needed starting M4)
- Visual Studio: Community 2022, 17.13.35931.197, C++ workload installed
- WebView2 Runtime: 153.0.4234.32 present
- Windows OpenSSL dev package (ShiningLight.OpenSSL.Dev, 4.0.2) at `C:\Program Files\OpenSSL-Win64` - build-time only, confirmed via `dumpbin` that `lifeos.exe` has no runtime dependency on it (§4a)
- Still missing: a full Perl (Git for Windows' bundled Perl cannot build OpenSSL from source)
- MSVC toolchain confirmed usable for binary inspection too: `dumpbin.exe` (VS 2022's `VC\Tools\MSVC\14.43.34808\bin\Hostx64\x64\`) used this run

No live NeXa git repository accessible — unchanged, `RESEARCH_DECISIONS.md` §3.

---

## 7. Git state

- Canonical repository: `https://github.com/AndrewDul/LifeOS.git`.
- Branch: `main`. Git identity: `AndrewDul <dulandrew.dev@gmail.com>`.
- Commits made in this project so far (oldest first):
  1. `chore: protect local user data in gitignore`
  2. `chore: harden gitignore for monorepo and Tauri build layout`
  3. `chore: ignore local Claude Code session state`
  4. `docs: establish LifeOS architecture baseline`
  5. `docs: lock v0.1 product direction (bilingual, GPT-5.6 Luna, Qwen3 4B, visual language, M0-M10 plan)`
  6. `feat: scaffold Tauri 2 + React + TS app and build the M1 visual shell`
  7. `docs: record M0/M1 build-start results`
  8. `feat: refine M1 visual shell and NeXa navigation`
  9. `feat: restore home edge drawers, refine face/background, add chat sidebar`
  10. `fix: full-screen edge transitions, remove global sidebar, NeXa Island as sole global nav`
  11. `docs: close M1, freeze M2 architecture, fix reporting policy`
  12. `feat: prove M2 encrypted storage (SQLCipher) and correct architecture errata`
  13. `feat: implement first M2 Goal vertical slice`
  14. (this run) `fix: harden M2 Goal action atomicity and patch semantics`
      — see this run's report and chat reply for the exact hash.
- Pushed to `origin/main` through commit 7. **Commits 8-14 were not
  pushed** — not authorized in any of those runs' instructions.

### Note on an incident in an earlier run (kept for reference)

`npm create tauri-app@latest -- . --force` briefly deleted `CLAUDE.md`
and all `docs/*.md` files and overwrote `.gitignore` during the original
M1 scaffold. Restored via `git checkout`, no data lost.

---

## 8. What can the user actually do right now?

Launch the real app (`npm run tauri dev`) and use the accepted M1 shell -
unchanged, no real goals visible there yet (no Tauri command wiring).
Separately, `cd src-tauri && cargo test -- --test-threads=1` runs 42
tests proving: the storage foundation (9, regression from the spike), a
full CreateGoal/UpdateGoal/get_active_goals lifecycle - now including
atomic-rollback-under-forced-failure and tri-state patch semantics -
against a real encrypted, migrated, production-schema database (17), and
direct action-layer validation/classification logic with no DB involved
(16) - all passing against temp databases, never the user's real data.

---

## 9. Next recommended step

Implement the `Project` vertical slice using the now-hardened pattern
proven for `Goal` (§4b-§4c) - `domain::Project` (with a nullable
`goal_id` foreign key), `actions::{CreateProject, AddMilestone, AddTask,
UpdateProgress}` (or at minimum `CreateProject`/`UpdateProject` first,
matching Goal's scope) using atomic transactions + typed `CoreError` +
`FieldPatch` from the start, `services::storage::project_repo`, and a
production migration `V2__create_projects.sql`. Do not implement
Milestone/Task in the same pass - repeat the "one entity, fully proven"
discipline, and do not invent unfrozen product rules inside the
implementation (§4c's lesson from the removed terminal-status rule).

---

## 10. Known problems / risks / unresolved questions

- **Neither Goal nor the storage bootstrap is wired into the running
  app** — no Tauri command layer exists yet; `services`' functions are
  only reachable from tests.
- **`bundled-sqlcipher`'s build-time OpenSSL dependency is real but no
  longer hardcoded** — any machine building this needs
  `scripts/setup-windows-storage-env.ps1` run once (§4a).
- **Clean-machine runtime is inferred from `dumpbin`, not an actual
  execution test** on a machine that never had any dev tooling installed
  — not available in this environment.
- **The Personal Knowledge Layer's `Relation.relation_type`** remains an
  intentionally free string — revisit once M6 usage patterns exist.
- **`target_date` still has no content validation beyond being a
  well-formed `DateTime<Utc>`** (e.g. nothing rejects a date far in the
  past) — pre-existing gap, not introduced or expanded this run, and
  intentionally left out of the hardening scope (not one of the 7 issues
  architectural review flagged); revisit if it turns out to matter in
  practice.
- **`CoreError` derives `Debug`, which does include the internal
  `diagnostic` string** (only `Display` is deliberately generic/safe) -
  fine for test assertions and future internal logging, but a future
  capability layer must format `CoreError` for any external caller via
  `Display`/`kind()`, never `{:?}`, or the "no arbitrary database string
  as the authoritative error" guarantee leaks back in through the side
  door.
- **`GoalRelation` is still schema-frozen only, not implemented** - the
  hardening run corrected `LIFEOS_SPEC.md` §5.2's overclaim that the V1
  Goal migration already contains it; it needs its own vertical slice
  before M2 is complete (§4d).
- **Exact NeXa transport, exact confirmation-class UX (modal/inline/
  toast)** — both deferred, unchanged.
- Everything already listed as pending from M1/M2-freeze (local/cloud
  model integration for M4/M5, Goal Intelligence and Personal Knowledge
  UI for M6, LifeOS's own software license, bilingual detection
  mechanism) is unchanged by this run.
