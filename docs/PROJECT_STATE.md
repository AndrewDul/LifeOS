# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it
whenever project state actually changes (`CLAUDE.md`, Documentation
policy) — not only as an end-of-run formality.

Last updated: 2026-09-17 (M1 closed/accepted; M2 architecture freeze).

---

## 1. Milestone status

| Milestone | Status |
|---|---|
| M0 — Environment ready | **DONE** (§2) |
| M1 — Visual shell | **ACCEPTED BY USER on 2026-09-17** (§3) |
| M2 — LifeOS Core + encrypted storage | **ARCHITECTURE FROZEN, not yet implemented** (§4) |
| M3-M10 | Not started |

**M1 is closed.** The user manually reviewed the running app and accepts
the current UI/navigation shell. Placeholder content, exact labels, menu
composition, and other visual details may still change later, but none
of that blocks architecture or backend work anymore. **Do not rewrite
accepted M1 code** except for genuine bugs or when a later milestone's
real functionality requires touching a screen it was standing in for.

**M2 is now the active milestone.** Its architecture is frozen (this
run) but no storage code has been written yet — see §4 and
`docs/reports/REPORT_M2_ARCHITECTURE_FREEZE_2026-09-17.md` for the full
design. The next implementation run's first job is the **storage spike**
(§4, "Next implementation step"), not the domain schema.

---

## 2. What exists right now

A Tauri 2 + React + TypeScript desktop app that launches successfully in
a real window, with an accepted visual/navigation shell. No domain code,
no database, no AI, no chat logic yet.

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
├── index.html, vite.config.ts, tsconfig*.json, package.json
├── public/                 (default Tauri/Vite icons)
├── src/                    (React frontend - M1, accepted)
│   ├── main.tsx, App.tsx, App.css
│   ├── components/   Galaxy, NexaFace, RadialMenu, EdgeDrawers,
│   │                  CommunicationDisplay, NexaIsland, ScreenShell,
│   │                  ChatScreen, ChatSidebar, PlaceholderScreen
│   └── state/         faceState.ts, navigation.ts
└── src-tauri/               (Rust backend)
    ├── src/main.rs, src/lib.rs   (no domain commands yet - run() only)
    ├── Cargo.toml, Cargo.lock, tauri.conf.json, build.rs
    ├── capabilities/default.json
    └── icons/                    (default Tauri icons, not LifeOS-branded yet)
```

`node_modules/`, `dist/`, and `src-tauri/target/` exist on disk but are
gitignored and untracked, as intended.

---

## 3. M1 — ACCEPTED

Accepted by the user on 2026-09-17 after manually reviewing the running
app. Summary of what M1 delivered (full detail across the two
refinement reports and the navigation-correction report in
`docs/reports/`):

- Home: large centered NeXa face (click opens a radial menu of 8
  sections), four edge handles that open full-screen sections directly,
  a dark cosmic galaxy background with a constellation formation, and a
  demo `CommunicationDisplay` (face slides left, unboxed message right).
- Every full-screen section (Chat, Today, Calendar, Goals, Projects,
  Money, Me, Insights, Settings, Notifications) opens as a real
  full-viewport slide-in from the direction it was triggered from, with
  **no permanent global navigation rail** - the NeXa Island (a small
  floating glass tile with the animated face) is the sole persistent
  global navigation element while a section is open, via its Home/Menu
  quick menu.
- Chat has its own dedicated sidebar (New Chat, search, demo chat list)
  distinct from global navigation.
- Everything is visual-only: demo/placeholder content, no backend, no
  AI, no persistence, by design.

**What "accepted" means going forward:** the UI/navigation is no longer
a blocker for M2+ work. Placeholder labels, menu composition, and minor
visual details are expected to keep evolving as real functionality lands
in later milestones (M3 onward) - that evolution does not require
reopening or re-litigating M1 itself, and M1's existing components
should be extended/wired to real data then, not rewritten from scratch.

| Test | Result |
|---|---|
| Frontend build (`npm run build`) | PASS (multiple runs, most recently 27 modules, ~170ms, zero TS errors) |
| Rust check (`cargo check`) | PASS (most recently 9.49s, zero errors) |
| Real window launches | Confirmed by user |
| Visual/interaction review | **Confirmed by user, 2026-09-17 — ACCEPTED** |

---

## 4. M2 — LifeOS Core + encrypted storage (ARCHITECTURE FROZEN)

Full design, reasoning, and acceptance criteria:
`docs/reports/REPORT_M2_ARCHITECTURE_FREEZE_2026-09-17.md`. The frozen,
durable parts of the architecture (domain model, action layer,
confirmation policy, module boundaries, capability API) are also written
into `docs/LIFEOS_SPEC.md` §5 as the canonical reference going forward -
the report carries the full reasoning and tradeoffs, the spec carries the
decisions.

### What's frozen (summary — see the report/spec for full detail)

- **Ownership boundary:** LifeOS Core (Rust) is the only thing that
  touches SQL. UI, the cloud LLM, and future NeXa all go through a typed
  action/service layer - enforced at compile time via Rust module
  privacy (storage functions are not `pub` outside `services`), not just
  convention.
- **Personal Knowledge Layer:** minimum relational model of `Entity`,
  `Fact`, `Relation`, `Episode`, `Evidence`, `Source`, with temporal
  validity (facts are superseded, never overwritten) and the required
  epistemic states (`USER_CONFIRMED`/`OBSERVED`/`INFERRED`/`DISPUTED`/
  `STALE`) and categories (`FACT`/`PREFERENCE`/`EXPERIENCE`/`DECISION`/
  `OBSERVATION`/`HYPOTHESIS`/`EFFECTIVE_SOLUTION`/`INEFFECTIVE_SOLUTION`/
  `SKILL`/`HABIT`/`CURRENT_STATE`).
- **Domain model:** minimum v0.1 structures frozen for `Goal`,
  `GoalRelation`, `Project`, `Milestone`, `Task`, `Challenge`, `Plan`,
  `SolutionAttempt`, `Outcome`, `CalendarEvent`, `Reminder`, `Question`,
  `ProgressUpdate`, plus a minimal `WellbeingCheckin` (added because the
  frozen capability API requires `log_wellbeing()` - flagged as a
  deliberate small addition, see the report §C).
- **Goal relations:** `SUPPORTS`/`CONFLICTS_WITH`/`DEPENDS_ON`/`BLOCKS`/
  `CONTRIBUTES_TO`/`REQUIRES`/`PARENT_OF`/`CHILD_OF`/`DUPLICATES`/
  `SUPERSEDES`/`COMPETES_FOR_RESOURCE`.
- **Action layer:** the full typed action list from the build brief
  (Goal/Project/Calendar/Challenge/Memory/Question/Plan actions),
  validated deterministically in Core before persistence - never by an
  LLM.
- **Confirmation policy:** six classes (`SAFE_READ`/`EXPLICIT_ADD`/
  `INFERRED_CHANGE`/`IMPORTANT_CHANGE`/`DELETE`/`SENSITIVE`), with a
  `type_floor × provenance` rule rather than a fixed per-action table -
  see the report §F for the full mapping.
- **Module boundaries (one crate, not several):** `domain/`, `actions/`,
  `services/`, `storage/`, `knowledge/`, `security/`, `backup/`,
  `migrations/`, `capability/` as Rust modules inside the existing
  `lifeos` crate - a deliberate refinement of the earlier
  crate-per-domain research note (`RESEARCH_DECISIONS.md` §5): start as
  modules, split into crates later only if a real plugin boundary
  demands it.
- **Encryption/key architecture target:** random DB master key in the OS
  credential store (`keyring` crate) by default; a separate,
  password/recovery-key-wrapped portable backup is a future capability
  the backup format must not preclude; no plaintext secrets in logs.
  **`sqlite3mc` is NOT declared final** - it remains the leading
  candidate from `RESEARCH_DECISIONS.md` §4b, unproven until the storage
  spike (below) passes.
- **NeXa capability API:** the frozen function list
  (`get_current_situation`, `get_active_goals`, `get_today_plan`,
  `get_relevant_memory`, `create_goal`, `update_goal`, `create_challenge`,
  `record_solution_attempt`, `record_outcome`, `create_calendar_event`,
  `change_priority`, `log_wellbeing`, `get_progress`) as Rust functions in
  `services`/`capability` - transport to NeXa itself is explicitly not
  decided yet.
- **M2 acceptance criteria:** ten concrete tests (persistence across
  restart, encrypted-at-rest, wrong-key rejection, migration, backup/
  restore, deterministic validation rejecting bad input, temporal fact
  history, relation integrity, no direct UI/LLM SQL path, unit/
  integration coverage) - see the report §J for exact detail.

### Next implementation step (M2, not yet started)

**The storage spike.** Before any domain schema or action code is
written, prove `sqlite3mc` (or its fallback, `SQLCipher`) against the
full checklist in the report §H: create an encrypted DB, reopen after
restart, wrong key fails, migration works, rekey works, backup works,
restore works, corrupt DB fails safely, and - critically, not yet
proven - it actually builds/packages correctly in a Windows Tauri
*release* build, not just `cargo check` in dev mode. This is a small
standalone proof, not the real schema.

---

## 5. M0 status — Environment ready (RESOLVED, unchanged)

| Prerequisite | Status |
|---|---|
| Rust (`rustup`, `rustc`, `cargo`) | Installed — `rustc 1.98.1`, `cargo 1.98.1` |
| Node.js / npm | `v24.18.0` / `11.1.0` |
| Git | `2.47.1.windows.2` |
| WebView2 Runtime | `153.0.4234.32` |
| Visual Studio "Desktop development with C++" workload | Installed (by you, manually) |

**Standing note, still true:** this automated session cannot perform
admin-elevated installs on this machine. Will recur for any future
milestone needing one.

---

## 6. Measured hardware/software audit (carried forward, unchanged this run)

- OS: Microsoft Windows 11 Home, 10.0.26200, 64-bit
- CPU: 12th Gen Intel Core i5-12500H, 12 physical / 16 logical cores, 3.1GHz base
- RAM: 63.71 GB (2× 32GB Kingston, 3200 MT/s)
- GPU: NVIDIA GeForce RTX 3050 Ti Laptop — 4096 MiB (4GB) VRAM (`nvidia-smi`), driver 610.62; Intel Iris Xe integrated (not CUDA-relevant)
- Disks: `C:` 475.6GB (166.8GB free), `D:` 3815.4GB (2769.1GB free), both NVMe SSD
- Git 2.47.1.windows.2, Node v24.18.0, npm 11.1.0, Python 3.12.7
- Rust: rustc 1.98.1, cargo 1.98.1
- Ollama: still not installed (needed starting M4)
- Visual Studio: Community 2022, 17.13.35931.197, C++ workload installed
- WebView2 Runtime: 153.0.4234.32 present

No live NeXa git repository is accessible from this environment (only
Desktop planning documents) - `RESEARCH_DECISIONS.md` §3 treats NeXa as
prior research, not a code dependency. Unchanged.

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
  11. (this run) `docs: close M1, freeze M2 architecture, fix reporting policy`
      — see this run's report and chat reply for the exact hash.
- Pushed to `origin/main` through commit 7. **Commits 8-11 were not
  pushed** — not authorized in any of those runs' instructions.

### Note on an incident in an earlier run (kept for reference)

`npm create tauri-app@latest -- . --force` briefly deleted `CLAUDE.md`
and all `docs/*.md` files and overwrote `.gitignore` during the original
M1 scaffold. Restored immediately via `git checkout`, no data lost. Kept
as a standing lesson for any future scaffold-into-existing-directory
operation.

---

## 8. What can the user actually do right now?

Launch the real app (`npm run tauri dev`) and use the accepted M1 shell -
navigate Home, the radial menu, all four edge-triggered sections, and
Chat's own sidebar. There is still no real functionality behind any of
it - no data, no AI, no persistence - M2 (which will change that,
starting with storage) has not been implemented yet, only designed.

---

## 9. Next recommended step

Execute the **M2 storage spike** (§4): a small standalone proof (not the
real schema, not wired into the app yet) that `sqlite3mc` satisfies every
item in the report's §H checklist, including a real Windows Tauri release
build - not just `cargo check`. Only after that passes (or a fallback
candidate is chosen instead) should the actual domain schema and action
layer be implemented.

---

## 10. Known problems / risks / unresolved questions

- **M2's storage spike has not been run.** `sqlite3mc` is a research
  recommendation, not a proven implementation choice, until it is.
- **The Personal Knowledge Layer's `Relation` type is intentionally
  generic** (loosely-typed `relation_type` string rather than a rigid
  enum) to avoid over-specifying now - the report flags this as worth
  revisiting once real usage patterns exist.
- **`log_wellbeing()` required adding a minimal `WellbeingCheckin`
  domain type** not originally in the frozen domain-model list - a
  deliberate small addition, not scope creep (see §4 above).
- **Exact NeXa transport is undecided** - the capability API's function
  boundary is frozen, but whether NeXa reaches it via a local socket,
  embedded HTTP, or something else is deferred.
- **Exact confirmation-class UX (modal vs. inline vs. toast) is
  deferred to M3+** - only the architectural classification is frozen
  now.
- Everything already listed as pending from M1 (local/cloud model
  integration for M4/M5, Goal Intelligence and Personal Knowledge UI for
  M6, LifeOS's own software license, bilingual detection mechanism) is
  unchanged by this architecture-only run.
