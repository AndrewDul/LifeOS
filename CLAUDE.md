# CLAUDE.md — LifeOS Operating Rules

Permanent operating rules for Claude (or any agent) working in this repository.
This file does not change often. Product/architecture decisions live in
`docs/LIFEOS_SPEC.md`, research/alternatives live in
`docs/RESEARCH_DECISIONS.md`, and current engineering truth lives in
`docs/PROJECT_STATE.md`. Read `docs/PROJECT_STATE.md` at the start of every
session before doing anything else.

## What LifeOS is

A local-first personal operating system for managing and understanding a
user's life (goals, projects, money, calendar, wellbeing, journal, problems/
solutions, long-term memory, AI chat), built first for the creator's own
laptop but designed as a real, modular, extensible product. Full product
definition: `docs/LIFEOS_SPEC.md`.

## Non-negotiable engineering principles

1. **Research first, reuse first, build last.** Before writing a significant
   subsystem, look for a maintained library, framework, or existing
   implementation. Do not rebuild mature systems (recurrence/RRULE math,
   vector search, CRDT sync, voice orchestration, etc.) unless research shows
   nothing reusable exists.
2. **One domain logic layer, three interfaces.** UI/forms, text chat, and
   voice must all call the exact same domain capability/action functions
   (e.g. `CreateGoal`, `AddTransaction`, `LogWellbeingCheckin`). Never
   implement business logic inside a UI component, a chat handler, or a
   voice handler. The cloud LLM's tools are thin wrappers around the same
   action layer the forms call.
3. **Privacy is architectural, not promised.** User data (finances, journal,
   wellbeing, problems, memories, preferences, private events) is local
   first and stays local unless the user explicitly opts in to sending it.
   The cloud LLM never receives the raw private database — only the minimum
   necessary context, produced locally. See the Privacy Firewall in
   `docs/LIFEOS_SPEC.md`. Do not weaken this to make a feature easier to
   build.
4. **Provider-agnostic cloud LLM.** Never hard-code LifeOS to one cloud LLM
   vendor. The provider abstraction is a first-class interface; model/vendor
   is configuration, not code.
5. **Local-first, cross-platform by design.** Windows is the initial target;
   Linux and macOS are future targets. Do not couple the architecture to
   Windows-only APIs when a cross-platform crate/plugin exists (this was
   verified as feasible across the whole stack in `RESEARCH_DECISIONS.md`).
6. **Extensibility is a design input, not an afterthought.** New life
   domains (Health, Learning, Relationships, Career, Travel, Documents,
   Vehicles, Home, NeXa integration) must be addable later without
   rewriting the core. Prefer domain capabilities, command/action
   abstractions, and stable service boundaries over a giant monolith.
7. **Bilingual by design, one system.** LifeOS supports Polish, English,
   and mixed PL/EN input as a single system that detects and responds in
   the user's language — never two parallel language modes. Do not
   unnecessarily translate the user's private memory database; preserve
   original wording where it matters, keep structured meaning
   language-independent.
8. **Cloud LLM never owns state.** The cloud conversational model has no
   direct database access and cannot directly change goals, money,
   calendar, memory, or any other LifeOS state. LifeOS Core (local) owns
   state and actions; the cloud model only requests actions through the
   guarded action layer and only receives minimum-necessary context
   through the Privacy Firewall.

## Documentation policy

Maintain these living documents, kept current rather than rewritten from
scratch:

- `CLAUDE.md` — this file, permanent operating rules.
- `docs/LIFEOS_SPEC.md` — canonical product, UX, architecture, requirements.
- `docs/RESEARCH_DECISIONS.md` — research, alternatives, sources, decisions.
- `docs/PROJECT_STATE.md` — current engineering truth. Update it whenever
  project state actually changes (a milestone closes, a decision is made,
  a blocker appears or clears) — not only as a formality at the end of a
  run.

`README.md` may remain minimal (a short pointer to these docs), if it
exists at all.

**Every substantial run also produces a dedicated report** under
`docs/reports/`, named `REPORT_<SUBJECT>_<YYYY-MM-DD>.md` (e.g.
`REPORT_M2_ARCHITECTURE_FREEZE_2026-09-17.md`). A run is "substantial" if
it does real architecture, implementation, or debugging work — a trivial
edit (a typo fix, a one-line config change, a single doc correction) does
not need one. When genuinely in doubt, write the report: a stray report
costs little, but a missing one loses reasoning that is expensive to
reconstruct later.

The report is where the full reasoning lives — what was done, why, files
touched, decisions and tradeoffs, validation results, and what's still
open. `PROJECT_STATE.md` and `LIFEOS_SPEC.md` stay short and current —
they should point to the relevant report(s) for detail rather than
duplicate it. The chat/terminal response at the end of a run is a summary
for the user in the moment, never the only record — if that conversation
is later lost, `PROJECT_STATE.md` plus the reports must still tell the
full story on their own. Git history (commits and diffs) remains the
record of *what changed in the code*; reports are the record of *why*,
which git history alone does not capture.

## Git policy

- Canonical repository: `https://github.com/AndrewDul/LifeOS.git`. It is
  already cloned into the working directory. Never re-clone, never
  `git init`, never create a nested LifeOS directory.
- Use the existing configured git identity (`AndrewDul
  <dulandrew.dev@gmail.com>`) as-is. Do not change `user.name`/`user.email`
  unless the user explicitly asks.
- Do **not** add AI/Claude/Anthropic attribution — no
  `Co-Authored-By: Claude`, no "Generated by Claude", no tool signatures —
  to commit messages, commit trailers, source files, or documentation,
  unless the user explicitly requests it for a given run. Commit messages
  describe the engineering change only.
- Local commits are allowed for coherent, completed, tested units of work.
  Do not create one giant meaningless commit; do not create noisy
  one-line-per-file commits either.
- Before every commit: `git status`, `git diff`, `git diff --cached`, and
  explicitly inspect staged files for secrets or private data.
- Never commit: the personal database, financial records, journals, memory
  files, API keys, credentials, `.env` files, private exports, personal
  logs, or sensitive backups. The `.gitignore` is the first line of
  defense — keep it current as new data categories are introduced.
- Never push without explicit authorization in the current run's
  instructions. Never force-push. Never rewrite published history without
  explicit instruction. Never skip hooks (`--no-verify`) unless explicitly
  asked.

## After every substantial prompt/run

1. Inspect current state before changing things (`git status`, read the
   relevant living docs — especially `PROJECT_STATE.md`).
2. Research before building when relevant (prefer mature libraries; record
   findings in `RESEARCH_DECISIONS.md`).
3. Implement only what is in scope for the current run — no speculative
   scaffolding, no unrequested features.
4. Test what was built (exact commands, exact results — no unverified
   "should work" claims).
5. Inspect `git diff` before committing; verify no secrets/private data
   staged.
6. Update `docs/PROJECT_STATE.md` to reflect the new engineering truth.
7. Update `CLAUDE.md`, `docs/LIFEOS_SPEC.md`, or `docs/RESEARCH_DECISIONS.md`
   if decisions changed — these are living documents, not one-time writes.
8. For substantial runs, write the dedicated report under `docs/reports/`
   (Documentation policy, above).
9. Provide the full mandatory RUN REPORT (below) in the chat response.

## Never silently hide

- Failed tests
- Partial implementations
- Fallbacks or workarounds introduced under pressure
- Unresolved TODOs
- Architectural compromises
- Security problems
- Privacy problems
- Uncertainty ("I assumed X" must be stated as an assumption, not fact)

Never say "working" without evidence (command output, test result,
screenshot description). Distinguish explicitly between:

- **CONFIRMED FACT** — verified by direct inspection of this repo/system.
- **MEASURED RESULT** — obtained by actually running something and
  recording the output.
- **RESEARCH FINDING** — obtained from external sources; cite them.
- **ENGINEERING RECOMMENDATION** — a judgment call, not yet validated.
- **UNRESOLVED QUESTION** — genuinely open, needs a decision or more data.

## Mandatory RUN REPORT

Every completed LifeOS run must end with a report containing all of:

1. Objective
2. What I inspected
3. Research performed (with current sources where relevant)
4. Reuse opportunities
5. Measured results
6. Decisions (accepted/rejected and why)
7. What changed
8. How it was implemented
9. Why it was done that way
10. Dependencies added/removed
11. Tests (exact commands/results — PASS/FAIL/PARTIAL)
12. Security impact
13. Privacy impact
14. Known problems
15. Risks
16. Unresolved questions
17. Files changed
18. Git state (branch, HEAD, status, diff summary)
19. Commits created (hash, message, purpose)
20. Current user-visible product state — what can the user actually do
    right now?
21. Next recommended step — ONE concrete next engineering step.

## Scope discipline

This repository moves in stages: research → architecture/spec → domain
model → implementation, one deliberate layer at a time. Do not scaffold or
implement production application code during a research/planning run unless
a run's instructions explicitly say to. When in doubt about whether
something is in scope, it is safer to flag it as an `UNRESOLVED QUESTION` in
the run report than to build it speculatively.
