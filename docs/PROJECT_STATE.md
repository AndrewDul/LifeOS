# LifeOS — Project State

Current engineering truth. Read this first, before `LIFEOS_SPEC.md` or
`RESEARCH_DECISIONS.md`, at the start of every session. Update it at the
end of every substantial run (`CLAUDE.md`, step 6).

Last updated: 2026-09-16 (research/architecture-planning run).

---

## 1. What exists right now

**Nothing application-level.** This run was explicitly research and
documentation only — no application code, no scaffolding, no dependencies
installed for the app itself.

Repository contents as of this run:

```
LiFeOS/
├── .git/
├── .gitignore
├── CLAUDE.md
└── docs/
    ├── LIFEOS_SPEC.md
    ├── RESEARCH_DECISIONS.md
    └── PROJECT_STATE.md   (this file)
```

No `src/`, no `src-tauri/`, no `package.json`, no `Cargo.toml`. The next
run that touches code starts from a completely empty application tree.

---

## 2. Measured hardware/software audit (MEASURED RESULT)

Collected 2026-09-16 by direct inspection of this machine (not assumed,
not remembered from a prior session).

### Operating system

- Caption: Microsoft Windows 11 Home
- Version: 10.0.26200 (Build 26200)
- Architecture: 64-bit

### CPU

- 12th Gen Intel(R) Core(TM) i5-12500H
- 12 physical cores / 16 logical processors
- Max clock speed: 3.1 GHz (base; boost not measured)

### RAM

- 63.71 GB total physical memory (2× 32GB Kingston modules, 3200 MT/s)

### GPU

- NVIDIA GeForce RTX 3050 Ti Laptop GPU — **4096 MiB (4GB) VRAM**
  (confirmed via `nvidia-smi`, not the potentially-inflated WMI
  `AdapterRAM` field). Driver 610.62, CUDA UMD version 13.3 (driver-level
  CUDA capability; no CUDA *toolkit* installed, and none is required —
  Ollama bundles its own CUDA runtime).
- Intel(R) Iris(R) Xe Graphics (integrated) — 1GB reported, not relevant
  to CUDA/local-LLM workloads.

### Disks

- `C:` — 475.6 GB total, 166.8 GB free — NVMe Micron_3400_MTFDKBA512TFH
  (512GB class)
- `D:` — 3815.4 GB total, 2769.1 GB free — NVMe Lexar SSD NM790 (4TB
  class)
- Both fixed NVMe SSDs; no spinning disks present. Ample free space for
  local model weights and a local database on `D:`.

### Development tooling installed

| Tool | Version | Status |
|---|---|---|
| Git | 2.47.1.windows.2 | Installed |
| Node.js | v24.18.0 | Installed |
| npm | 11.1.0 | Installed |
| Python | 3.12.7 | Installed |
| Rust (`rustc`/`cargo`) | — | **Not installed** |
| Ollama | — | **Not installed**, no models present |
| Visual Studio | Community 2022, 17.13.35931.197 | Installed, **but without the "Desktop development with C++" workload** (`Microsoft.VisualStudio.Component.VC.Tools.x86.x64` not present per `vswhere` query — confirmed empty result) |
| WebView2 Runtime | 153.0.4234.32 | Installed (satisfies the Tauri prerequisite on Windows) |
| CUDA toolkit (`nvcc`) | — | Not installed (not required for Ollama) |

### Blockers this implies for M0 (Environment ready, `LIFEOS_SPEC.md` §12)

1. **Rust is not installed.** Install via `rustup` before any Tauri/Rust
   work can begin.
2. **The MSVC C++ build workload is missing.** `cargo build` will fail to
   link on Windows without it. Must add "Desktop development with C++" to
   the existing VS 2022 Community install (Visual Studio Installer → Modify
   → check the workload; a multi-GB download).
3. **Ollama is not installed.** Required before the local-helper-model
   benchmark plan (`RESEARCH_DECISIONS.md` §2) can be executed — nothing in
   that plan has been run yet; it is a plan, not a result.

Neither (1) nor (2) is a research/architecture concern — they're concrete,
known setup steps, deliberately not performed in this run because this run
was scoped as research/documentation only, not implementation.

### NeXa local accessibility check (CONFIRMED FACT)

Searched this machine for a NeXa codebase to evaluate for voice-stack
reuse (per the original brief). Found:

- `D:\Dev\NeXa_Rove` — exists but is **empty** (no files).
- `C:\Users\andrz\Desktop\NeXa\` and sibling folders (`NeXa_RoVe`,
  `NeXa_Brain`, `NeXa_CoRe`, `NeXa_SyS`, `NeXa_ToTem`,
  `NeXa_RuNTime_Map_and_Plan`, `continue_NeXa_IkiTGai`, `nexa-plan`) —
  contain **planning documents** (`.docx`, `.txt`, one `.zip`), not a git
  repository and not source code. A grep for `pipecat`/`barge-in`/`echo
  cancellation`/`turn detection` inside the two largest `.txt` files found
  real research content (echo-cancellation RMS measurements, AEC3 module
  references, a `docs/research/m2_6_cloud_realtime_voice/...` path
  reference implying a real NeXa repo exists *somewhere*, just not on this
  machine or not checked into git here).

**Conclusion:** no live NeXa git repository is accessible from this
environment. The voice research in `RESEARCH_DECISIONS.md` §3 treats NeXa
as prior research to be conceptually aware of, not a code dependency —
this is a confirmed fact, not an assumption, and should be re-checked if a
NeXa repo becomes accessible later (e.g. on a different machine or once
pushed somewhere reachable).

---

## 3. Git state

- Canonical repository: `https://github.com/AndrewDul/LifeOS.git` — cloned
  into this directory (was empty on GitHub at the start of this work; no
  re-clone needed for this run, per instruction).
- Branch: `main`.
- Git identity in use: `AndrewDul <dulandrew.dev@gmail.com>` (pre-existing
  machine config, unmodified).
- Nothing has been pushed. `origin/main` does not exist yet — the local
  `main` shows "upstream is gone" because there is nothing on the remote
  to track yet. This is expected and does not need fixing before a first
  push.
- Commits made in this project so far (oldest first):
  1. `chore: protect local user data in gitignore` — initial `.gitignore`.
  2. `chore: harden gitignore for monorepo and Tauri build layout` —
     removed root-anchoring so nested workspace paths stay protected,
     added Tauri/Rust-specific entries.
  3. (this run) documentation commit — see the RUN REPORT for this
     session for the exact hash.

---

## 4. What can the user actually do right now?

Nothing product-facing — there is no application yet. What exists:

- A correctly configured git repository pointed at the canonical GitHub
  remote, with a strong `.gitignore` protecting secrets, credentials, and
  personal data categories before any of that data can exist.
- Four canonical living documents (`CLAUDE.md`,
  `docs/LIFEOS_SPEC.md`, `docs/RESEARCH_DECISIONS.md`,
  `docs/PROJECT_STATE.md`) that fully capture the product definition,
  architecture decisions (with sources), and current state — enough for
  implementation to start from a well-reasoned baseline instead of
  guessing.

---

## 5. Next recommended step

Execute **M0 — Environment ready** (`LIFEOS_SPEC.md` §12): install the VS
2022 "Desktop development with C++" workload, install Rust via `rustup`,
and get a Tauri 2 "hello world" building and running on this machine.
This unblocks everything else (M1's `rusqlite`/sqlite3mc work needs a
working Rust toolchain) and is a small, independently verifiable step
before any domain modeling begins.

---

## 6. Known problems / risks / unresolved questions

(Cross-referenced from `LIFEOS_SPEC.md` §13 and `RESEARCH_DECISIONS.md`
uncertainty flags — kept in sync here as the single "current truth"
summary.)

- **No local helper LLM has been benchmarked or even downloaded.** The
  shortlist and benchmark plan in `RESEARCH_DECISIONS.md` §2 are
  unexecuted. Model choice is not final.
- **No Polish-language LLM quality data exists** for the default cloud
  provider (Claude Sonnet 5) or any alternative evaluated. This is a real
  gap given Polish is an explicit product requirement.
- **Voice is unprototyped.** The cascaded-architecture decision
  (`RESEARCH_DECISIONS.md` §3) is a research conclusion, not a working
  system — the Windows AEC/barge-in path in particular is flagged as a
  genuine unsolved engineering problem.
- **No live NeXa codebase is accessible** from this environment (§2
  above) — any future NeXa-integration work needs that resolved first.
- **Data-sensitivity classification levels are not yet defined** — the
  Privacy Firewall (`LIFEOS_SPEC.md` §4) is an architectural target, not
  an implemented system.
- **LifeOS's own software license has not been decided.** This matters
  because the strongest architectural prior art found (Wealthfolio, Khoj,
  Logseq) is AGPL-3.0 (`RESEARCH_DECISIONS.md` §5) — worth deciding early
  since it's hard to change after code exists.
