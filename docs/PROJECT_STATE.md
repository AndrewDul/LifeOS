# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it
whenever project state actually changes (`CLAUDE.md`, Documentation
policy) — not only as an end-of-run formality.

Last updated: 2026-09-17 (M2 Goal vertical slice — storage hardened for
production, first real domain entity implemented).

---

## 1. Milestone status

| Milestone | Status |
|---|---|
| M0 — Environment ready | **DONE** (§5) |
| M1 — Visual shell | **ACCEPTED BY USER on 2026-09-17** (§3) |
| M2 — LifeOS Core + encrypted storage | Architecture frozen; storage PROVEN; **Goal vertical slice implemented** 2026-09-17 (§4); rest of domain model not started |
| M3-M10 | Not started |

**M1 is closed** — unchanged, not touched this run.

**M2 now has one real, working, tested domain entity: Goal**, end to
end through `domain → actions → services → private services/storage →
encrypted SQLCipher`. The storage foundation was also hardened for
production use this run (spike/production separation, machine-specific
build config removed, OpenSSL runtime dependency measured). Next step is
repeating this same pattern for `Project` — not the rest of the domain
model at once, and still not M3.

---

## 2. What exists right now

A Tauri 2 + React + TypeScript desktop app that launches in a real
window (accepted M1 shell, untouched), plus a working encrypted Goal
store: create and update goals with deterministic validation, confirmation
classification, an audit trail, and a read API - all proven by 21
automated tests. **Not wired into the running app yet** - no Tauri
command layer exists, so the UI cannot create/see real goals yet.

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
    │   ├── main.rs, lib.rs       (run() unchanged; declares the 4 new modules, all private)
    │   ├── domain/mod.rs          Goal, GoalId (UUID v7), GoalStatus, Progress, Priority
    │   ├── actions/mod.rs         Actor, Provenance, ActionMeta, ConfirmationClass,
    │   │                          ActionOutcome<T>, ValidationError, validation logic
    │   ├── security/mod.rs        OS-keyring master-key lifecycle
    │   └── services/
    │       ├── mod.rs              bootstrap_production_db, create_goal, update_goal,
    │       │                       get_active_goals, get_goal (the capability boundary
    │       │                       for this slice - no dedicated capability/ module yet)
    │       ├── tests.rs            12 Goal-slice test functions (16 requirements)
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

### 4b. Goal vertical slice implemented (this run)

The first real domain entity, end to end:

- **Domain** (`domain::Goal`): `id` (UUID v7), `title`, `description`,
  `status` (`ACTIVE`/`PAUSED`/`COMPLETED`/`ABANDONED` -
  `COMPLETED`/`ABANDONED` are terminal, no transition out), `target_date`,
  `priority_user`/`priority_system` (bounded 1-5), `progress` (bounded
  integer 0-100), `domain_tag`, `created_at`/`updated_at` (UTC).
- **Actions**: `CreateGoal` (floor `EXPLICIT_ADD`), `UpdateGoal` (floor
  `IMPORTANT_CHANGE`) - deterministic validation (title trim/non-empty/
  length, description/tag length, priority/progress bounds, status-
  transition legality) runs *before* any write; zero partial writes on
  failure (tested explicitly).
- **Confirmation classification**: `effective_class = max(type_floor,
  provenance_class)`, implemented exactly per the frozen policy - no UI
  modals invented, just typed metadata (`ConfirmationClass`) a future M3
  UI can render against. Every persisted Create/Update writes a
  `goal_audit_log` row (previous-state JSON snapshot for updates) - the
  minimum state to support a future Undo, not a full event-sourcing
  system.
- **Read capability**: `services::get_active_goals()` (the frozen v0.1
  capability API's `get_active_goals()`) and an internal `get_goal(id)` -
  both go through `services`, which *is* the capability boundary for
  this slice (a dedicated `capability` module is deferred until a real
  Tauri command or NeXa transport exists to justify it).
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

### 4c. What's still architecturally frozen but not yet implemented

Personal Knowledge Layer, `GoalRelation`/goal relations, `Project`/
`Milestone`/`Task`/`Challenge`/`Plan`/`SolutionAttempt`/`Outcome`/
`CalendarEvent`/`Reminder`/`Question`/`ProgressUpdate`/
`WellbeingCheckin`, the rest of the frozen action list, `ForgetMemory`'s
corrected (hard-delete) semantics, the `knowledge`/`backup` modules, a
real `capability` module, NeXa transport. Full detail:
`docs/LIFEOS_SPEC.md` §5.1-§5.6.

### Next implementation step (M2)

Repeat the exact same pattern for **`Project`** (it already references
`Goal` via a nullable `goal_id` in the frozen domain model - a good
second entity to prove the pattern handles a cross-entity relationship).
Reuse `domain → actions → services → services::storage::project_repo`
unchanged in shape. Do not implement Milestone in the same pass.

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
  13. (this run) `feat: implement first M2 Goal vertical slice` — see this
      run's report and chat reply for the exact hash.
- Pushed to `origin/main` through commit 7. **Commits 8-13 were not
  pushed** — not authorized in any of those runs' instructions.

### Note on an incident in an earlier run (kept for reference)

`npm create tauri-app@latest -- . --force` briefly deleted `CLAUDE.md`
and all `docs/*.md` files and overwrote `.gitignore` during the original
M1 scaffold. Restored via `git checkout`, no data lost.

---

## 8. What can the user actually do right now?

Launch the real app (`npm run tauri dev`) and use the accepted M1 shell -
unchanged, no real goals visible there yet (no Tauri command wiring).
Separately, `cd src-tauri && cargo test -- --test-threads=1` runs 21
tests proving: the storage foundation (9, regression from the spike) and
a full CreateGoal/UpdateGoal/get_active_goals lifecycle against a real
encrypted, migrated, production-schema database (12) - all passing
against temp databases, never the user's real data.

---

## 9. Next recommended step

Implement the `Project` vertical slice using the exact pattern just
proven for `Goal` (§4b) - `domain::Project` (with a nullable `goal_id`
foreign key), `actions::{CreateProject, AddMilestone, AddTask,
UpdateProgress}` (or at minimum `CreateProject`/`UpdateProject` first,
matching Goal's scope), `services::storage::project_repo`, and a
production migration `V2__create_projects.sql`. Do not implement
Milestone/Task in the same pass - repeat the "one entity, fully proven"
discipline.

---

## 10. Known problems / risks / unresolved questions

- **Neither Goal nor the storage bootstrap is wired into the running
  app** — no Tauri command layer exists yet; `services`' functions are
  only reachable from tests.
- **`ConfirmationRequired`'s `InferredChange` path is implemented but
  untested** — no `Provenance::Inferred` caller exists yet (arrives with
  M5's cloud-LLM integration).
- **`UpdateGoalPayload` cannot explicitly clear a field to NULL** (`None`
  always means "leave unchanged") — documented simplification, revisit
  if a real need appears.
- **`bundled-sqlcipher`'s build-time OpenSSL dependency is real but no
  longer hardcoded** — any machine building this needs
  `scripts/setup-windows-storage-env.ps1` run once (§4a).
- **Clean-machine runtime is inferred from `dumpbin`, not an actual
  execution test** on a machine that never had any dev tooling installed
  — not available in this environment.
- **No dedicated `actions`-level unit tests** — validation/classification
  logic is exercised indirectly through `services`-level integration
  tests; a future slice touching `actions` more heavily should add them.
- **The Personal Knowledge Layer's `Relation.relation_type`** remains an
  intentionally free string — revisit once M6 usage patterns exist.
- **Exact NeXa transport, exact confirmation-class UX (modal/inline/
  toast)** — both deferred, unchanged.
- Everything already listed as pending from M1/M2-freeze (local/cloud
  model integration for M4/M5, Goal Intelligence and Personal Knowledge
  UI for M6, LifeOS's own software license, bilingual detection
  mechanism) is unchanged by this run.
