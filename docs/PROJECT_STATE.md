# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it at the
end of every substantial run (`CLAUDE.md`, step 6).

Last updated: 2026-09-17 (M1 visual refinement run).

---

## 1. What exists right now

**A Tauri 2 + React + TypeScript desktop app that launches successfully
in a real window on this machine, with a refined visual shell (M1).** No
domain code, no database, no AI, no chat logic — those are later
milestones (`LIFEOS_SPEC.md` §12). M2 has **not** started.

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
│   └── PROJECT_STATE.md   (this file)
├── index.html, vite.config.ts, tsconfig*.json, package.json
├── public/                 (default Tauri/Vite icons)
├── src/                    (React frontend)
│   ├── main.tsx, App.tsx, App.css
│   ├── components/   Galaxy, NexaFace, RadialMenu, CommunicationDisplay,
│   │                  NexaIsland, SideNavigation, ScreenShell,
│   │                  ChatScreen, PlaceholderScreen
│   └── state/         faceState.ts, navigation.ts
└── src-tauri/               (Rust backend)
    ├── src/main.rs, src/lib.rs   (no domain commands yet - run() only)
    ├── Cargo.toml, Cargo.lock, tauri.conf.json, build.rs
    ├── capabilities/default.json
    └── icons/                    (default Tauri icons, not LifeOS-branded yet)
```

`node_modules/`, `dist/`, and `src-tauri/target/` exist on disk but are
gitignored and untracked, as intended.

### M1 status: first launch confirmed, visual refinement pass complete

The MSVC "Desktop development with C++" workload blocker recorded in the
prior version of this document **has been resolved** (by you, manually,
outside this session — this session still cannot perform admin-elevated
installs). `cargo check` now succeeds and `npm run tauri dev` opens a
real window; you manually reviewed the first version of the visual shell.
This run is a **visual refinement pass only**, driven by that review —
no architecture, backend, or milestone progression.

### What the visual shell does now, after this refinement

**Home (default state):**
- Very dark, almost-black galaxy background with clustered (not evenly
  random) star field, ~97% neutral cool-white stars / ~3% sparse warm
  orange, yellow, or cool-blue stars, two very low-opacity nebula haze
  blobs, and near-imperceptible slow drift (stars + haze both animate
  over 180-260s, not noticeable while sitting still).
- A large, centered, dominant NeXa face (two rounded eyes, simple smile,
  soft glow, gentle idle float/breathing/blink/eye-drift) - sized via
  `clamp(320px, 40vmin, 520px)`, several times larger than the original
  M1 pass's fixed 200px.
- Clicking the face toggles a radial ring of 8 dark-glass pills (Today,
  Chat, Calendar, Goals, Projects, Money, Me, Insights) around a shrunk
  face; clicking again (or selecting a screen) closes it.
- A reusable **CommunicationDisplay**: the same face slides to the left
  and an unboxed, editorial-style message appears on the right with a
  single glass "Close" button. Demo-triggered once, ~1.1s after load, so
  it's visually reachable without a prototype trigger button; dismissing
  it returns Home. This is explicitly demo content, not real intelligence
  (`src/App.tsx`, `DEMO_GREETING`).
- **No more persistent edge tabs** - the old bottom "Chat" / top
  "Notifications" / left "Today" prototype buttons and their drawers are
  removed entirely (`EdgeDrawers.tsx` deleted).

**Full-screen sections (Chat, Today, Calendar, Goals, Projects, Money,
Me, Insights, Settings):**
- Shared `ScreenShell` layout: a slim dark-glass `SideNavigation` rail on
  the left (Home + all 9 sections, active-state highlight), a compact
  floating `NexaIsland` (small glass circular tile with the animated face
  inside, Dynamic-Island-style) near the top, and a content area - the
  galaxy stays visible behind all of it.
- Clicking the island opens a small quick menu (Home / Menu); Menu
  expands a compact list of all sections to jump directly, without a
  detour through Home.
- The old standalone "← Home" corner button and the generic "This screen
  is a navigation placeholder" text are gone.
- **Chat** has real (visual-only) shell: demo conversation bubbles
  (NeXa-left / user-right, restrained dark glass, no AI), and a bottom
  input bar ("Message NeXa...", disabled send/attach - visual only, no
  backend).
- Every other section uses a shared `PlaceholderScreen`: a clean heading
  and one honest line naming which milestone brings real content (no
  fake dashboards).

**Still not implemented (unchanged from before):** any backend/database/
AI/voice/memory. `LISTENING`, `LOCAL_PROCESSING`, `WAITING_FOR_CLOUD`,
`SPEAKING` face states have matching CSS but no real trigger yet - only
`IDLE` and a click-triggered `ATTENTION` bounce run for real.

### Visual verification status (IMPORTANT)

**This refinement pass has NOT been visually re-verified by you yet.**
`npm run build` and `cargo check` both pass (§3), which proves the code
compiles and type-checks - it does **not** prove the redesigned galaxy,
face sizing, radial menu, communication display, island, or chat/sidebar
actually look right. Per `CLAUDE.md`, automated build success must not be
reported as a visual PASS. Run `npm run tauri dev` and look at it.

---

## 2. M0 status — Environment ready (RESOLVED)

| Prerequisite | Status |
|---|---|
| Rust (`rustup`, `rustc`, `cargo`) | Installed — `rustc 1.98.1`, `cargo 1.98.1` |
| Node.js / npm | `v24.18.0` / `11.1.0` |
| Git | `2.47.1.windows.2` |
| WebView2 Runtime | `153.0.4234.32` |
| Visual Studio "Desktop development with C++" workload | **Installed (by you, manually) — resolved between the last run and this one** |

The manual unblock step recorded previously in this document is no
longer needed and has been removed. `cargo check --manifest-path
src-tauri/Cargo.toml` now compiles the whole dependency graph and links
successfully (see §3 for this run's exact output).

**Standing note, still true:** this automated session cannot perform
admin-elevated installs on this machine (confirmed in the prior run - two
elevation approaches both failed). This will recur for any future
milestone needing one (code signing, driver-level audio work for voice,
etc.) and will need the same human-in-the-loop step again.

---

## 3. M1 status — Visual shell refinement (BUILDS CLEAN, VISUAL RE-VERIFICATION PENDING)

| Test | Command | Result |
|---|---|---|
| Frontend build | `npm run build` (`tsc && vite build`) | **PASS** — 26 modules transformed, built in 486ms, no TypeScript errors |
| Rust check | `cargo check --manifest-path src-tauri/Cargo.toml` | **PASS** — full dependency graph compiled and linked in 32.58s, no errors |
| Real window appears | (visual) | Previously confirmed by you, before this refinement pass |
| Refined Home/Galaxy/face/radial menu/communication display | (visual) | **NOT YET RE-VERIFIED** — needs your look after this pass |
| Chat screen (sidebar + island + demo conversation) | (visual) | **NOT YET VERIFIED** — new this pass |
| Other sections (placeholder + sidebar + island) | (visual) | **NOT YET VERIFIED** — new this pass |

No LifeOS-authored code changes caused any build/check failure this run.

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
- Visual Studio: Community 2022, 17.13.35931.197, **C++ workload now installed** (see §2)
- WebView2 Runtime: 153.0.4234.32 present

### NeXa local accessibility (unchanged, carried forward)

No live NeXa git repository is accessible from this environment — only
Desktop planning documents (docx/txt). See prior audit detail in git
history; conclusion unchanged: `RESEARCH_DECISIONS.md` §3 treats NeXa as
prior research, not a code dependency.

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
  8. (this run) `feat: refine M1 visual shell and NeXa navigation` — see
     the RUN REPORT for this session for the exact hash.
- Pushed to `origin/main` through commit 7 (prior run). **This run's
  commit was not pushed** unless explicitly authorized in this run's
  instructions — check the RUN REPORT for this session for the exact
  push status.

### Note on an incident in a prior run (kept for reference)

`npm create tauri-app@latest -- . --force` briefly deleted `CLAUDE.md`
and all `docs/*.md` files and overwrote `.gitignore` during the original
M1 scaffold. Both were restored immediately via `git checkout` since
everything was already committed - no data was lost. Not relevant to this
run (no scaffolding tool was used), kept here as a standing lesson for
any future scaffold-into-existing-directory operation.

---

## 6. What can the user actually do right now?

Launch the real app (`npm run tauri dev`) and see the refined visual
shell described in §1 - a large centered NeXa face on a calm dark galaxy,
a radial menu, a demo focused-message display, and full-screen sections
(especially Chat) with a side navigation and floating NeXa island. There
is still no real functionality behind any of it - no data, no AI, no
persistence - by design, this is a visual milestone.

---

## 7. Next recommended step

**You:** run `npm run tauri dev` and visually review this refinement pass
against the brief (galaxy density/color/motion, face size/animation,
radial menu styling, the communication display, the NeXa island and its
quick menu, the Chat sidebar/demo conversation, and the other
placeholders). Report back anything that doesn't match so it can be
corrected before M2 (LifeOS Core + encrypted storage) begins.

---

## 8. Known problems / risks / unresolved questions

- **This visual refinement has not been visually re-verified yet** (§1,
  §3) - only compile/type-check evidence exists so far.
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
  the UI is language-aware, including this run's new Chat demo content
  (English only).
- The side-navigation item order (Today first, Settings last) and the
  quick-menu "Menu" behavior (expanding a compact list rather than a
  second radial layout) were judgment calls where the build-start brief
  was ambiguous or gave only a "suggested" ordering - see this run's RUN
  REPORT for the exact reasoning, in case you want it changed.
