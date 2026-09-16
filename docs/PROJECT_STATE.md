# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it at the
end of every substantial run (`CLAUDE.md`, step 6).

Last updated: 2026-09-16 (M0 + M1 build-start run).

---

## 1. What exists right now

**A scaffolded Tauri 2 + React + TypeScript desktop app with a working
visual shell (M1), sitting on a Rust toolchain that cannot yet link on
this machine (M0 partially blocked).** No domain code, no database, no
AI, no chat — those are later milestones (`LIFEOS_SPEC.md` §12).

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
│   ├── components/          Galaxy, NexaFace, RadialMenu, EdgeDrawers, Screen
│   └── state/                faceState.ts, navigation.ts
└── src-tauri/               (Rust backend)
    ├── src/main.rs, src/lib.rs   (no domain commands yet - run() only)
    ├── Cargo.toml, Cargo.lock, tauri.conf.json, build.rs
    ├── capabilities/default.json
    └── icons/                    (default Tauri icons, not LifeOS-branded yet)
```

`node_modules/`, `dist/`, and `src-tauri/target/` exist on disk (from
`npm install` / build attempts) but are gitignored and untracked, as
intended.

### What the visual shell actually does (M1)

- Full-screen dark galaxy background (CSS gradients + a starfield of ~140
  twinkling stars, two soft nebula blobs) — implemented, not yet visually
  confirmed in a real window (see §2, blocked launch).
- An animated NeXa face (SVG: two rounded "bean" eyes, a smile-arc mouth)
  centered on screen, with idle breathing/blink animation.
- Face-state infrastructure (`IDLE`/`LISTENING`/`LOCAL_PROCESSING`/
  `WAITING_FOR_CLOUD`/`SPEAKING`/`ATTENTION`) wired as CSS state classes
  driven by a `FaceState` type - only `IDLE` and a click-triggered brief
  `ATTENTION` bounce are actually exercised right now; the other four
  states have matching CSS (pulsing ring, spin-pulse ring, mouth
  open/close loop) but nothing yet calls `setFaceState` for them, since
  voice and cloud calls don't exist yet.
- Clicking the face shrinks it slightly and reveals a radial ring of 8
  glass-effect tiles (Today, Chat, Calendar, Goals, Projects, Money, Me,
  Insights), positioned by trigonometry around the face.
- Clicking a tile opens a generic placeholder `Screen` (title + "← Home"
  back control) - no domain data, navigation shell only, as scoped.
- Left/top/bottom edge tab buttons open placeholder drawer panels
  (Today / Notifications / Chat) - implemented as fixed-position buttons
  per the brief ("can initially be desktop buttons/hit areas"), not
  touch gestures.
- **This has not been visually confirmed in a running window** — the
  frontend compiles and builds cleanly in isolation (§3), but the Tauri
  window that would actually render it cannot launch yet (§2).

---

## 2. M0 status — Environment ready (PARTIALLY BLOCKED)

| Prerequisite | Status |
|---|---|
| Rust (`rustup`, `rustc`, `cargo`) | **Installed this run** — `rustc 1.98.1`, `cargo 1.98.1` (measured via `rustc --version`/`cargo --version` after install) |
| Node.js / npm | Already present — `v24.18.0` / `11.1.0` (reconfirmed) |
| Git | Already present — `2.47.1.windows.2` (reconfirmed) |
| WebView2 Runtime | Already present — `153.0.4234.32` (from prior audit, unchanged) |
| Visual Studio "Desktop development with C++" workload | **Still missing — blocked, not installed this run** |

### The blocker, in detail (CONFIRMED FACT + MEASURED RESULT)

`vs_installer.exe modify --add Microsoft.VisualStudio.Workload.NativeDesktop
--quiet --norestart` requires the calling process to already be running
elevated (admin) — quiet/passive VS installer operations refuse to
self-elevate via UAC. This automated session is not running as
administrator (`[Security.Principal.WindowsPrincipal]::IsInRole(Administrator)`
returned `False`), and does not have an interactive desktop session
available to approve a UAC consent prompt: both a plain launch (exit code
`5007`, VS installer log: *"Commands with --quiet or --passive should be
run elevated from the beginning"*) and a `-Verb RunAs` elevation request
(exit code `1`, no prompt observed/approvable) failed. **This is an
environment constraint of this automated session, not a Tauri/Rust
problem** — it will recur for any future run that needs an admin-elevated
install, and is worth remembering for later milestones (e.g. code-signing
tools, driver installs) rather than re-discovering each time.

**Consequence, demonstrated with real command output:** `cargo check` and
`npm run tauri dev` both fail at the same step — `link.exe` fails to link
Rust build scripts (`serde_core`, `thiserror`, `icu_normalizer_data`,
etc.), with rustc's own diagnostic: *"in the Visual Studio installer,
ensure the 'C++ build tools' workload is selected."* The specific link
error text ("link: extra operand ... Try 'link --help'") indicates the
linker actually being invoked is not MSVC's `link.exe` at all but a
different `link` utility earlier on `PATH` (a known Windows/Rust/
git-bash interaction) — but the root cause either way is the same missing
workload; this is not a second, unrelated problem to chase.

### Exact unblock step (requires you, not this session)

Open an **elevated** PowerShell or Command Prompt (right-click → "Run as
administrator") and run:

```
"C:\Program Files (x86)\Microsoft Visual Studio\Installer\vs_installer.exe" modify --installPath "C:\Program Files\Microsoft Visual Studio\2022\Community" --add Microsoft.VisualStudio.Workload.NativeDesktop --quiet --norestart
```

or open the "Visual Studio Installer" app from the Start menu → Modify on
the Community 2022 install → check "Desktop development with C++" →
Modify. Either way, after it finishes, `cargo check` (from a fresh shell,
so it picks up any PATH changes) should succeed, and `npm run tauri dev`
should launch a real window.

---

## 3. M1 status — Visual shell (CODE COMPLETE, LAUNCH UNVERIFIED)

| Test | Command | Result |
|---|---|---|
| Frontend build | `npm run build` (`tsc && vite build`) | **PASS** — 22 modules transformed, built in 412ms, no TypeScript errors |
| Rust type-check | `cargo check` (in `src-tauri/`) | **FAIL** — linker error, see §2 |
| Full dev launch | `npm run tauri dev` | **FAIL** — same linker error, no window opened |
| Real window appears | (visual) | **NOT VERIFIED** — blocked by the above |
| Galaxy/face/radial-menu/navigation render correctly | (visual) | **NOT VERIFIED** — blocked by the above |

**FIRST APP LAUNCH: FAIL.** Root cause: the M0 blocker in §2. No other
issues were found before hitting it — the frontend is proven to compile
correctly in isolation, and the Rust side fails at the *linking* stage
(after successfully parsing/type-checking `serde`/`thiserror`/etc.'s own
build scripts), not at a LifeOS code problem. There is no evidence of any
bug in the LifeOS-authored code (`src/`, `src-tauri/src/`) — the failure
is entirely in the toolchain prerequisite.

---

## 4. Measured hardware/software audit (carried forward, unchanged this run)

(GPU/CPU/RAM/disk facts below are unchanged from the prior research run;
repeated here for a single source of truth. Full original detail also
available via git history of this file.)

- OS: Microsoft Windows 11 Home, 10.0.26200, 64-bit
- CPU: 12th Gen Intel Core i5-12500H, 12 physical / 16 logical cores, 3.1GHz base
- RAM: 63.71 GB (2× 32GB Kingston, 3200 MT/s)
- GPU: NVIDIA GeForce RTX 3050 Ti Laptop — 4096 MiB (4GB) VRAM (`nvidia-smi`), driver 610.62; Intel Iris Xe integrated (not CUDA-relevant)
- Disks: `C:` 475.6GB (166.8GB free), `D:` 3815.4GB (2769.1GB free), both NVMe SSD
- Git 2.47.1.windows.2, Node v24.18.0, npm 11.1.0, Python 3.12.7
- **Rust: now installed — rustc 1.98.1, cargo 1.98.1** (was: not installed)
- Ollama: still not installed (needed starting M4)
- Visual Studio: Community 2022, 17.13.35931.197, **C++ workload still missing** (see §2)
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
  7. (this run) `docs: record M0/M1 build-start results` — see the RUN
     REPORT for this session for the exact hash.
- First push to `origin main` was authorized and performed this run (see
  the RUN REPORT for exact confirmation) — this is the first time this
  repository's history left the local machine.

### Note on an incident during this run

`npm create tauri-app@latest -- . --force` (used to scaffold into the
non-empty repo root) **deleted `CLAUDE.md` and all three `docs/*.md`
files** as part of its "force" overwrite of the target directory, and
separately overwrote `.gitignore` with its own generic template. Both
were restored immediately via `git checkout -- <path>` since everything
was already committed — no data was actually lost, but this is worth
remembering: **scaffolding tools with a `--force`/overwrite flag can
delete pre-existing files in the target directory, not just add new
ones.** Future scaffolding-into-non-empty-directory operations should
`git status` immediately afterward and diff against HEAD before doing
anything else, exactly as happened here.

---

## 6. What can the user actually do right now?

Not yet launch the app — `npm run tauri dev` fails before a window opens
(§2, §3). What exists once the one manual unblock step (§2) is done:

- A real Tauri 2 + React + TypeScript project (`npm install` already run)
  whose frontend is proven to build cleanly.
- A written, reviewable visual shell (galaxy background, animated face
  with state infrastructure, radial menu, 8 placeholder screens, edge
  drawers) that has not yet been visually confirmed in a live window.
- Four up-to-date canonical docs describing exactly what v0.1 will be.

---

## 7. Next recommended step

**You:** install the VS "Desktop development with C++" workload using
the exact elevated command in §2 (this session cannot do it — no
elevation available). **Then:** re-run `npm run tauri dev` from a fresh
shell and confirm a real window opens showing the galaxy/face/radial
menu; report back what you see (or paste any new error) so the M1 visual
verification can actually complete. This is the single step blocking
everything else in `LIFEOS_SPEC.md` §12's milestone sequence.

---

## 8. Known problems / risks / unresolved questions

- **M1 is code-complete but visually unverified.** The launch blocker in
  §2 means nobody — human or agent — has actually seen the galaxy/face/
  radial menu render in a window yet. Treat it as "should work" evidence
  (clean TS compile, no logic errors found on inspection), not "does
  work" evidence, until the blocker is cleared and it's actually seen
  running.
- **This automated session cannot perform admin-elevated installs.**
  Confirmed this run (§2). Any future milestone needing one (code
  signing, driver-level audio work for voice, etc.) will hit the same
  wall and need the same human-in-the-loop step.
- No local helper LLM (Qwen3 4B) has been downloaded, installed, or
  benchmarked — Ollama itself isn't installed yet. Unchanged from before,
  scheduled for M4.
- No cloud LLM (GPT-5.6 Luna) integration exists yet — scheduled for M5.
- Data-sensitivity classification, Goal Intelligence relationships,
  Personal Knowledge/Memory provenance, and the NeXa capability API are
  all design targets in `LIFEOS_SPEC.md`, not implemented — scheduled for
  M6/M2/M6/ongoing respectively.
- LifeOS's own software license is still undecided (relevant given the
  AGPL prior art noted in `RESEARCH_DECISIONS.md` §5).
- Bilingual (PL/EN) detection/normalisation mechanism is a stated design
  target (`LIFEOS_SPEC.md` §1.3) with no implementation yet — nothing in
  the M1 UI is language-aware.
