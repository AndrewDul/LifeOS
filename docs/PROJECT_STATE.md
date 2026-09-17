# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it at the
end of every substantial run (`CLAUDE.md`, step 6).

Last updated: 2026-09-17 (M1 navigation correction — full-screen edge
transitions, global sidebar removed, NeXa Island is sole global nav).

---

## 1. What exists right now

**A Tauri 2 + React + TypeScript desktop app that launches successfully
in a real window on this machine, with a corrected navigation model
(M1).** No domain code, no database, no AI, no chat logic — those are
later milestones (`LIFEOS_SPEC.md` §12). M2 has **not** started.

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
│   └── reports/            REPORT_UI_REFINEMENT_..._2026-09-17.md,
│                            REPORT_NAVIGATION_CORRECTION_..._2026-09-17.md
├── index.html, vite.config.ts, tsconfig*.json, package.json
├── public/                 (default Tauri/Vite icons)
├── src/                    (React frontend)
│   ├── main.tsx, App.tsx, App.css
│   ├── components/   Galaxy, NexaFace, RadialMenu, EdgeDrawers,
│   │                  CommunicationDisplay, NexaIsland, ScreenShell,
│   │                  ChatScreen, ChatSidebar, PlaceholderScreen
│   │                  (SideNavigation.tsx was deleted this run - see §1a)
│   └── state/         faceState.ts, navigation.ts
└── src-tauri/               (Rust backend)
    ├── src/main.rs, src/lib.rs   (no domain commands yet - run() only)
    ├── Cargo.toml, Cargo.lock, tauri.conf.json, build.rs
    ├── capabilities/default.json
    └── icons/                    (default Tauri icons, not LifeOS-branded yet)
```

`node_modules/`, `dist/`, and `src-tauri/target/` exist on disk but are
gitignored and untracked, as intended.

### M1 status: navigation model corrected (third pass this milestone)

Two prior visual-refinement passes got the *look* closer but the
*navigation model* wrong: edge drawers were small preview panels with an
"Open X" button, and a permanent global sidebar (Home/Today/Chat/.../
Settings) was showing inside every open section. This run corrects both,
per explicit feedback. Full detail and reasoning:
`docs/reports/REPORT_NAVIGATION_CORRECTION_FULLSCREEN_EDGE_TRANSITIONS_2026-09-17.md`
(this run) and
`docs/reports/REPORT_UI_REFINEMENT_HOME_DRAWERS_AND_CHAT_SIDEBAR_2026-09-17.md`
(prior run, background/face/drawer-restoration detail still accurate).
No architecture, backend, or milestone progression - M2 has not started.

### 1a. Current navigation model (canonical as of this run)

**Home** (unchanged in kind): large centered NeXa face, click-to-open
radial menu (8 sections), edge handles at all four screen edges, dark
cosmic galaxy background. Home does not use the NeXa Island - the large
face is Home's own navigation center.

**Edge handles now open the real screen directly** - no more
intermediate preview panel. Clicking a handle immediately opens the full
section, sliding in from that edge and filling the viewport:
- **Top:** Notifications, Settings (slide down)
- **Left:** Dashboard (→ `today`), Goals (slide in from left)
- **Right:** Projects, Me, Insights (slide in from right)
- **Bottom:** Chat (slides up)

**No permanent global sidebar exists anywhere anymore.**
`SideNavigation.tsx` was deleted. Every full-screen section (Chat,
Today, Calendar, Goals, Projects, Money, Me, Insights, Settings,
Notifications) renders through `ScreenShell`, which now contains only
the `NexaIsland` and the section's own content - no app-wide navigation
rail.

**The NeXa Island is the sole persistent global navigation element**
while a section is open: a small dark-glass circular tile (Dynamic-
Island-style) with the animated face inside, centered near the top.
Clicking it opens a small quick menu: **Home** (closes the current
section, animates back out the way it entered, restores the large Home
face) and **Menu** (expands a compact list of every section to jump
directly, without returning Home first). Clicking the island again
closes the quick menu.

**Chat keeps its own dedicated sidebar** (`ChatSidebar` - New Chat,
search placeholder, demo chat list with per-chat transcripts) - this is
Chat's own feature, unaffected by the global-sidebar removal, and sits
directly inside Chat's content area, not as global navigation.

**Transitions:** entering a screen from an edge slides the whole section
in from that edge (CSS `@keyframes`, ~460ms); entering from the radial
menu or the Island's quick menu uses a calm fade+scale instead (no
literal edge to slide from). Returning Home reverses whichever motion
the screen entered with (CSS transition, ~420ms), keyed off the same
`entryDirection` the screen was opened with. See the navigation-
correction report §4-§5 for the exact state model and CSS mechanism.

### Visual verification status (IMPORTANT)

**This navigation correction has NOT been visually re-verified by you
yet.** `npm run build` and `cargo check` both pass (§3), which proves the
code compiles and type-checks - it does **not** prove the four slide
directions, the reverse-close motion, or the Island's quick menu actually
look and feel right. Per `CLAUDE.md`, automated build success must not be
reported as a visual PASS. Run `npm run tauri dev` and look at it.

---

## 2. M0 status — Environment ready (RESOLVED)

| Prerequisite | Status |
|---|---|
| Rust (`rustup`, `rustc`, `cargo`) | Installed — `rustc 1.98.1`, `cargo 1.98.1` |
| Node.js / npm | `v24.18.0` / `11.1.0` |
| Git | `2.47.1.windows.2` |
| WebView2 Runtime | `153.0.4234.32` |
| Visual Studio "Desktop development with C++" workload | Installed (by you, manually) |

**Standing note, still true:** this automated session cannot perform
admin-elevated installs on this machine. This will recur for any future
milestone needing one (code signing, driver-level audio work for voice,
etc.) and will need the same human-in-the-loop step again.

---

## 3. M1 status — Navigation correction (BUILDS CLEAN, VISUAL RE-VERIFICATION PENDING)

| Test | Command | Result |
|---|---|---|
| Frontend build | `npm run build` (`tsc && vite build`) | **PASS** — 27 modules transformed, built in 165-175ms, no TypeScript errors |
| Rust check | `cargo check --manifest-path src-tauri/Cargo.toml` | **PASS** — full dependency graph compiled and linked in 9.49s, no errors |
| Real window appears | (visual) | Previously confirmed by you, before any refinement pass |
| Edge-triggered full-screen slide transitions (all 4 directions) | (visual) | **NOT YET VERIFIED** — new this pass |
| Reverse slide-out on returning Home | (visual) | **NOT YET VERIFIED** — new this pass |
| No global sidebar visible on any open section | (visual) | **NOT YET VERIFIED** — implemented and code-inspected, not eyeballed |
| NeXa Island quick menu (Home / Menu) as sole global nav | (visual) | **NOT YET VERIFIED** — new this pass |
| Chat sidebar still present and distinct | (visual) | **NOT YET VERIFIED** — unchanged code, but not re-looked-at |

No LifeOS-authored code changes caused any build/check failure this run.
Full change detail:
`docs/reports/REPORT_NAVIGATION_CORRECTION_FULLSCREEN_EDGE_TRANSITIONS_2026-09-17.md`.

---

## 4. Measured hardware/software audit (carried forward, unchanged this run)

(GPU/CPU/RAM/disk facts below are unchanged; repeated here for a single
source of truth. Full original detail also available via git history of
this file.)

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

### NeXa local accessibility (unchanged, carried forward)

No live NeXa git repository is accessible from this environment — only
Desktop planning documents (docx/txt). Conclusion unchanged:
`RESEARCH_DECISIONS.md` §3 treats NeXa as prior research, not a code
dependency.

---

## 5. Git state

- Canonical repository: `https://github.com/AndrewDul/LifeOS.git`.
- Branch: `main`.
- Git identity: `AndrewDul <dulandrew.dev@gmail.com>` (unchanged).
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
  10. (this run) `fix: full-screen edge transitions, remove global
      sidebar, NeXa Island as sole global nav` — see this run's report
      and chat reply for the exact hash.
- Pushed to `origin/main` through commit 7. **Commits 8, 9, and 10 were
  not pushed** — not authorized in any of those runs' instructions.

### Note on an incident in an earlier run (kept for reference)

`npm create tauri-app@latest -- . --force` briefly deleted `CLAUDE.md`
and all `docs/*.md` files and overwrote `.gitignore` during the original
M1 scaffold. Both were restored immediately via `git checkout` since
everything was already committed - no data was lost. Kept here as a
standing lesson for any future scaffold-into-existing-directory
operation.

---

## 6. What can the user actually do right now?

Launch the real app (`npm run tauri dev`) and see: a large centered NeXa
face on a calm dark galaxy with a visible constellation; a radial menu
and four edge handles on Home; clicking any of them opens the real
section as a full-screen slide-in from that direction, with only the
compact NeXa Island (not a sidebar) persisting as global navigation;
Chat additionally has its own conversation-list sidebar. There is still
no real functionality behind any of it - no data, no AI, no persistence -
by design, this is a visual/interaction milestone.

---

## 7. Next recommended step

**You:** run `npm run tauri dev` and visually review this correction
specifically: do the four edge-to-fullscreen transitions feel right
(direction, speed, "premium" feel); does returning Home via the Island
correctly reverse the same motion; is the global sidebar really gone
from every section; does the Island's quick menu (Home / Menu) work as
the only way to navigate while a section is open. Report back anything
that doesn't match so it can be corrected before M2 (LifeOS Core +
encrypted storage) begins.

---

## 8. Known problems / risks / unresolved questions

- **This navigation correction has not been visually re-verified yet**
  (§1, §3) - only compile/type-check evidence exists so far.
- **One-off dated report files continue to be created per explicit
  request** (`docs/reports/*.md`, now two of them), still in tension with
  `CLAUDE.md`'s standing "Documentation policy" (no one-off/dated report
  files). Not yet resolved either direction - worth an explicit decision
  soon given this is now a recurring pattern, not a single exception.
- **"Me / Insights" (one item in the original brief) became two separate
  edge handles** ("Me" and "Insights") this run, since a single handle
  can no longer offer a choice once the preview-panel step was removed.
  Flag if a single combined handle was actually wanted instead.
- **Notifications is now a real (placeholder) `ScreenId`** with its own
  `PlaceholderScreen` (milestone: M7), added so its Top edge handle has
  something real to open. Previously it was a drawer-only concept with
  no full screen.
- Screen-to-screen navigation via the Island's "Menu" list does not
  animate an exit of the previously-open screen (only entry of the new
  one) - a deliberate simplification, not a bug.
- The Home-drawer label "Dashboard" still refers to the same screen the
  rest of the app calls "Today" (`today` `ScreenId`) - unchanged
  deliberate naming choice from the prior pass.
- Star cluster and constellation placement in `Galaxy.tsx` is randomized
  per page load (no fixed seed).
- **This automated session still cannot perform admin-elevated installs**
  on this machine - will recur for any future milestone needing one.
- No local helper LLM (Qwen3 4B) has been downloaded, installed, or
  benchmarked — Ollama itself isn't installed yet. Scheduled for M4.
- No cloud LLM (GPT-5.6 Luna) integration exists yet — scheduled for M5.
- Data-sensitivity classification, Goal Intelligence relationships,
  Personal Knowledge/Memory provenance, and the NeXa capability API are
  all design targets in `LIFEOS_SPEC.md`, not implemented — scheduled for
  M6/M2/M6/ongoing respectively.
- LifeOS's own software license is still undecided (relevant given the
  AGPL prior art noted in `RESEARCH_DECISIONS.md` §5).
- Bilingual (PL/EN) detection/normalisation mechanism is a stated design
  target (`LIFEOS_SPEC.md` §1.3) with no implementation yet — nothing in
  the UI is language-aware.
