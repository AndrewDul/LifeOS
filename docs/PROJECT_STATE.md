# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it
whenever project state actually changes (`CLAUDE.md`, Documentation
policy) — not only as an end-of-run formality.

Last updated: 2026-09-17 (M2 storage spike — architecture errata,
storage candidate proven).

---

## 1. Milestone status

| Milestone | Status |
|---|---|
| M0 — Environment ready | **DONE** (§5) |
| M1 — Visual shell | **ACCEPTED BY USER on 2026-09-17** (§3) |
| M2 — LifeOS Core + encrypted storage | Architecture frozen; **storage layer PROVEN** 2026-09-17; schema/action layer **not yet implemented** (§4) |
| M3-M10 | Not started |

**M1 is closed** — see §3. Do not rewrite accepted M1 code except for
genuine bugs or when later real functionality requires touching a
screen it was standing in for.

**M2's encrypted-storage question is now answered**, and the two
architecture-errata items flagged during review are corrected (§4). The
next implementation run's job is the real domain schema and action
layer, starting with one small vertical slice — not the full domain
model at once, and still not M3.

---

## 2. What exists right now

A Tauri 2 + React + TypeScript desktop app that launches in a real
window, with an accepted visual/navigation shell, plus a proven (but
still spike-only, not production) encrypted-storage layer. No real
domain schema, no AI, no chat logic yet.

Repository contents as of this run:

```
LiFeOS/
├── .git/
├── .gitignore
├── .vscode/extensions.json
├── CLAUDE.md
├── README.md
├── docs/
│   ├── LIFEOS_SPEC.md
│   ├── RESEARCH_DECISIONS.md
│   ├── PROJECT_STATE.md   (this file)
│   └── reports/            REPORT_UI_REFINEMENT_..._2026-09-17.md
│                            REPORT_NAVIGATION_CORRECTION_..._2026-09-17.md
│                            REPORT_M2_ARCHITECTURE_FREEZE_2026-09-17.md
│                            REPORT_M2_STORAGE_SPIKE_2026-09-17.md
├── index.html, vite.config.ts, tsconfig*.json, package.json
├── public/                 (default Tauri/Vite icons)
├── src/                    (React frontend - M1, accepted)
│   ├── main.tsx, App.tsx, App.css
│   ├── components/   Galaxy, NexaFace, RadialMenu, EdgeDrawers,
│   │                  CommunicationDisplay, NexaIsland, ScreenShell,
│   │                  ChatScreen, ChatSidebar, PlaceholderScreen
│   └── state/         faceState.ts, navigation.ts
└── src-tauri/               (Rust backend)
    ├── .cargo/config.toml        (M2 spike: OpenSSL env vars, machine-specific - see §4)
    ├── migrations/                V1__init.sql, V2__add_note_column.sql (spike schema, NOT real domain)
    ├── src/
    │   ├── main.rs, main() -> lifeos_lib::run() (unchanged)
    │   ├── lib.rs         run() unchanged; declares `mod services;`
    │   └── services/      M2 storage spike (see §4) - NOT real domain
    │       ├── mod.rs      orchestration + SpikeError, private `mod storage;`
    │       ├── storage/mod.rs   private submodule, pub(super) fns only
    │       └── tests.rs    10-requirement spike test suite (#[cfg(test)])
    ├── Cargo.toml, Cargo.lock, tauri.conf.json, build.rs
    ├── capabilities/default.json
    └── icons/                    (default Tauri icons, not LifeOS-branded yet)
```

`node_modules/`, `dist/`, and `src-tauri/target/` (including the
MSI/NSIS installers produced this run) exist on disk but are gitignored
and untracked, as intended - no build artifacts, generated databases, or
credentials were committed.

---

## 3. M1 — ACCEPTED (unchanged this run)

Accepted by the user on 2026-09-17. Full detail in the two refinement
reports and the navigation-correction report in `docs/reports/`. Summary:
Home (large NeXa face, radial menu, four edge handles opening full-screen
sections), no permanent global nav rail (NeXa Island is the sole
persistent global nav while a section is open), Chat's own dedicated
sidebar, everything visual-only/demo content by design. Not touched this
run.

---

## 4. M2 — LifeOS Core + encrypted storage

### 4a. Architecture errata (corrected this run)

Two issues identified during review of the architecture freeze, fixed
**before** any implementation began:

- **Module-boundary enforcement was wrong as originally written.**
  `storage` was described as a *sibling* of `services`, "restricted" via
  `pub(crate)`/`pub(super)` — that doesn't actually work under Rust's
  visibility rules (`pub(crate)` reaches the whole crate; `pub(super)` on
  a sibling reaches their shared parent, where a future `capability`
  module would also live). **Corrected structure:** `storage` nests
  *inside* `services` as a private submodule (`mod storage;`, no `pub`
  at all), so nothing outside `services` can name it — verified for real
  by deliberately attempting the illegal access and capturing the
  compiler's rejection (`error[E0603]: module 'storage' is private`).
  `docs/LIFEOS_SPEC.md` §5.6 and the architecture-freeze report's §G are
  corrected in place.
- **`ForgetMemory` was wrongly grouped with ordinary soft-deletes.**
  `CancelEvent`-style tombstoning is fine for most deletions, but
  `ForgetMemory` is the user's explicit right-to-be-forgotten action — it
  must permanently remove the targeted `Fact` payload and dependent
  sensitive `Evidence`, keeping only a minimal non-sensitive audit marker
  (id/hash, timestamp, action type), never the original text. No `Undo`
  past confirmation. `docs/LIFEOS_SPEC.md` §5.4/§5.5 and the freeze
  report's §E/§F are corrected in place. Documentation-only — no code
  exists yet for either action layer or memory.

### 4b. Storage candidate: PROVEN — SQLCipher, not sqlite3mc

Full evidence: `docs/reports/REPORT_M2_STORAGE_SPIKE_2026-09-17.md`.

**sqlite3mc could not be tested** — verified directly (not trusting
prior docs) that no mature native-Rust integration exists: `rusqlite`
0.40.2 has no `sqlite3mc` feature, upstream SQLite3MultipleCiphers
documents no Rust bindings, and the only native-Rust crate found
(`walletkit-sqlite`) was 15 days old with 78 downloads. Not rejected on
technical grounds — never run against the checklist, because there was
no mature dependency to run it through.

**SQLCipher (via `rusqlite` 0.37.0's `bundled-sqlcipher` feature, pinned
because `refinery-core` 0.9.0 caps at `rusqlite <=0.37`) passed all 10
required tests plus the Windows Tauri release-build check**, run in an
isolated spike (`src-tauri/src/services/`, a `spike_records` table only
— no real domain schema):

| # | Requirement | Result |
|---|---|---|
| 1 | Encrypted database creation | PASS |
| 2 | Persistence across restart | PASS |
| 3 | Wrong key fails cleanly | PASS (typed `rusqlite::Error`, real SQLCipher HMAC failure) |
| 4 | Encrypted at rest (34-byte plaintext marker, 20,480-byte file) | PASS |
| 5 | Migration V1→V2 (refinery), safe re-run | PASS |
| 6 | Rekey | PASS |
| 7 | Backup (same-installation) | PASS |
| 8 | Restore (row count + content hash) | PASS |
| 9 | Corruption detected safely (copy corrupted, original untouched) | PASS |
| 10 | OS keyring round trip (Windows Credential Manager, real) | PASS |
| 11 | `cargo build --release` | PASS (4m50s, real 4MB binary) |
| 11 | `npm run tauri build` (MSI + NSIS) | PASS (both installers produced, verified on disk) |
| 11 | Code signing | Not configured — correctly out of scope, not a failure |

**Known Windows build dependency, not yet resolved:** `bundled-sqlcipher`
needs an *external* OpenSSL (this machine's Git-for-Windows Perl cannot
build OpenSSL from source — confirmed via a real build failure). Fixed
by installing a prebuilt Windows OpenSSL dev package (`winget install
ShiningLight.OpenSSL.Dev`) and pointing `src-tauri/.cargo/config.toml`
at it — **a machine-specific absolute path**. Any other machine
continuing M2 work needs the same package at the same path, or that
config updated. Revisit `bundled-sqlcipher-vendored-openssl` (removes
this dependency entirely) if a working Perl is ever set up here.

`docs/LIFEOS_SPEC.md` §5 is updated to reflect SQLCipher (not sqlite3mc)
as the proven M2 choice; `RESEARCH_DECISIONS.md` §4 is left as historical
record, not deleted.

### 4c. What's frozen architecturally (unchanged from the freeze, corrected per §4a)

Personal Knowledge Layer (`Entity`/`Fact`/`Relation`/`Episode`/
`Evidence`/`Source`, temporal validity, required epistemic states/
categories); domain model minimum v0.1 (`Goal`, `GoalRelation`,
`Project`, `Milestone`, `Task`, `Challenge`, `Plan`, `SolutionAttempt`,
`Outcome`, `CalendarEvent`, `Reminder`, `Question`, `ProgressUpdate`,
`WellbeingCheckin`); goal relations (11 types); the full typed action
list; confirmation policy (6 classes, `type_floor × provenance`, with
`ForgetMemory` now correctly distinguished per §4a); module boundaries
(one crate, `domain/actions/services/knowledge/security/backup/
migrations/capability`, `storage` nested inside `services` per §4a); the
NeXa capability API function list. Full detail:
`docs/LIFEOS_SPEC.md` §5.1-§5.6.

### Next implementation step (M2, not yet started)

Begin the real domain schema and action layer against the now-proven
storage foundation — **one small vertical slice first** (e.g. a real
`Goal` table through `services`/`storage`/`domain`, reusing the exact
module-privacy pattern proven this run), not the whole domain model at
once. Not started this run, per the stop condition: Goal/Project/Fact
production schemas, the real action layer, M3.

---

## 5. M0 status — Environment ready (RESOLVED, unchanged)

| Prerequisite | Status |
|---|---|
| Rust (`rustup`, `rustc`, `cargo`) | Installed — `rustc 1.98.1`, `cargo 1.98.1` |
| Node.js / npm | `v24.18.0` / `11.1.0` |
| Git | `2.47.1.windows.2` |
| WebView2 Runtime | `153.0.4234.32` |
| Visual Studio "Desktop development with C++" workload | Installed |
| OpenSSL (Windows dev package, for SQLCipher) | **Installed this run** — `winget install ShiningLight.OpenSSL.Dev`, no admin elevation needed (unlike the VS installer in an earlier run) |

**Standing note, still true:** this automated session cannot perform
*admin-elevated* installs (confirmed again N/A this run — `winget` for
this package did not require elevation, unlike the VS Installer
previously). Will still recur for anything that genuinely needs
elevation.

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
- **New this run:** Windows OpenSSL dev package (ShiningLight.OpenSSL.Dev, 4.0.2) at `C:\Program Files\OpenSSL-Win64`
- **Still missing:** a full Perl (Git for Windows' bundled Perl cannot build OpenSSL from source - `Locale::Maketext::Simple` and other CPAN modules absent)

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
  12. (this run) `feat: prove M2 encrypted storage (SQLCipher) and correct architecture errata`
      — see this run's report and chat reply for the exact hash.
- Pushed to `origin/main` through commit 7. **Commits 8-12 were not
  pushed** — not authorized in any of those runs' instructions.

### Note on an incident in an earlier run (kept for reference)

`npm create tauri-app@latest -- . --force` briefly deleted `CLAUDE.md`
and all `docs/*.md` files and overwrote `.gitignore` during the original
M1 scaffold. Restored via `git checkout`, no data lost. Standing lesson
for any future scaffold-into-existing-directory operation.

---

## 8. What can the user actually do right now?

Launch the real app (`npm run tauri dev`) and use the accepted M1 shell -
unchanged from before. Separately, `cd src-tauri && cargo test
services::tests -- --nocapture` runs the M2 storage spike's 9 test
functions proving encrypted storage works end-to-end. Neither is wired
to the other yet — the spike is not integrated into the running app, and
the app still has no real domain functionality, no AI, no persistence
for actual user data.

---

## 9. Next recommended step

Implement the first real M2 vertical slice: one actual domain table
(suggest `Goal`, the simplest frozen entity) through `domain` →
`actions` → `services` → `storage` (reusing the private-submodule
pattern proven this run), with at least the `CreateGoal` and `UpdateGoal`
actions, their deterministic validation, and unit/integration tests -
before attempting the rest of the domain model. Do not implement the
full schema/action set at once.

---

## 10. Known problems / risks / unresolved questions

- **The M2 storage spike is not wired into the running app** — it's
  isolated code (`src-tauri/src/services/`) proving the architecture,
  not yet used by `run()` or any Tauri command.
- **`bundled-sqlcipher`'s external-OpenSSL dependency is machine-specific**
  (§4b) — needs the same setup on any other machine, or a switch to
  `bundled-sqlcipher-vendored-openssl` once a working Perl exists here.
- **The Personal Knowledge Layer's `Relation.relation_type`** remains an
  intentionally free string, not a rigid enum — revisit once M6 usage
  patterns exist.
- **Exact NeXa transport is undecided** — the capability API's function
  boundary is frozen, transport is not.
- **Exact confirmation-class UX (modal vs. inline vs. toast)** is
  deferred to M3+.
- **The one-off dated report file policy question** raised in the prior
  run is resolved — `CLAUDE.md` now explicitly requires them for
  substantial runs (this run produced one).
- Everything already listed as pending from M1/M2-freeze (local/cloud
  model integration for M4/M5, Goal Intelligence and Personal Knowledge
  UI for M6, LifeOS's own software license, bilingual detection
  mechanism) is unchanged by this run.
