# LifeOS — Product, UX, and Architecture Specification

Canonical, living specification. This document is authoritative for what
LifeOS is and how it is built. Research backing these decisions (with
sources, alternatives, and REUSE/ADAPT/REJECT verdicts) lives in
`RESEARCH_DECISIONS.md`. Current engineering status lives in
`PROJECT_STATE.md`. Operating rules for working in this repo live in
`../CLAUDE.md`.

Status: implementation started (M0/M1). See `PROJECT_STATE.md` for
exactly what exists; most of this document still describes design targets
for later milestones, not built functionality.

---

## 1. Product definition

LifeOS is a local-first personal operating system for managing and
understanding a user's life. Built first for the creator's own Windows
laptop and daily use, but designed as a real product that could later be
used by other people and integrate with the separate NeXa project.

Platform targets: Windows first; Linux and macOS are future targets. The
architecture must not be unnecessarily coupled to Windows.

LifeOS should help the user:

- know what to do now
- understand whether they are progressing in the right direction
- manage goals, projects, money, work/income
- manage an internal calendar, deadlines, reminders, recurring plans
- track wellbeing, mood, energy, stress
- journal
- track problems, record attempted solutions, record which solutions
  worked, remember what helps and what harms the user
- understand recurring patterns
- manage personal development, record decisions, manage ideas
- maintain long-term memory
- receive useful AI recommendations
- perform weekly/monthly reviews and monitor progress over time

The system must remain modular and extensible: new life domains (Health,
Learning, Relationships, Career, Travel, Documents, Vehicles, Home, NeXa
integration) must be addable later without rewriting the core.

### 1.1 Not merely a single-purpose app

LifeOS is not merely a mental-health app, a debt tracker, a TODO app, a
calendar, or a habit tracker. It provides **general support in life** —
it should eventually understand what the user is currently dealing with,
whatever domain that belongs to (projects, learning, skills, remembering
something, changing habits, sleep, body/fitness, diet, money, career,
work, business, software projects, personal organisation, decisions,
recurring blockers, personal development, and future domains not yet
known), and increasingly understand what works for *this* user.

### 1.2 Canonical lifecycle

The general shape LifeOS applies across any domain, when the user brings
it a problem, challenge, or goal:

```
PROBLEM / CHALLENGE / GOAL
        -> UNDERSTAND
        -> RETRIEVE RELEVANT USER CONTEXT
        -> RESEARCH IF REQUIRED
        -> OPTIONS
        -> DISCUSS WITH USER
        -> PLAN
        -> EXECUTE
        -> MONITOR
        -> REVIEW RESULT
        -> LEARN WHAT WORKED
        -> UPDATE PERSONAL KNOWLEDGE
```

This lifecycle is a design lens for every domain (§5.2, §5.3), not a
feature to build in one run — later milestones implement pieces of it.

### 1.3 Bilingual (Polish/English) by design

LifeOS is a bilingual Polish/English personal life operating system. It
must support Polish, English, and mixed Polish/English utterances as
**one system**, not two separate language modes — the system detects the
user's language naturally and responds in the same language unless asked
otherwise. Personal knowledge preserves the user's original wording/
language where useful, while structured semantic information (§5.3) is
stored independently of language. Local translation/normalisation may
happen when required for extraction/classification, but LifeOS must not
unnecessarily translate the user's entire private memory database.

---

## 2. Interaction model

LifeOS supports three interfaces to the **same underlying domain logic**:
UI/forms, text chat, voice conversation. These must not implement separate
business logic — all three call the same domain capability/action layer.

```
form  -> CreateGoal(...)
chat  -> CreateGoal(...)
voice -> CreateGoal(...)
```

Example natural-language → action mappings the chat/voice layer must
resolve to calls on the same action layer the forms use:

| Utterance | Resolved action |
|---|---|
| "I spent £42 on fuel" | `AddTransaction` |
| "On Friday at 2 PM I am going to view a van" | `CreateCalendarEvent` |
| "I feel terrible today, energy 2 out of 10" | `LogWellbeingCheckin` |
| "Van is now my highest priority" | `ChangePriority` |
| "My new goal is to get my first paying DeVDul client by December" | `CreateGoal` |
| "Move tomorrow's task to Friday" | `UpdateTask` |
| "That solution fixed the problem" | `ResolveProblem` + record the successful solution |
| "I want to focus less on NeXa this week" | propose a priority/time-allocation change (confirmed with the user, not silently applied) |

Conversation is a first-class operating interface for LifeOS, not a chatbot
bolted onto the application.

### Guarded-action-layer principle

Adapted from prior-art research (see `RESEARCH_DECISIONS.md` §5): the AI
(cloud LLM) must call through the **exact same typed, permissioned action
layer** the UI uses — never a privileged backdoor into raw data. This is
both an architecture pattern and the mechanical enforcement point for the
Privacy Firewall (§4): if the AI can only invoke named actions/functions
with typed arguments, there is no path for it to receive more than what
those functions return.

### AI-optional resilience

Core capture, search, and CRUD operations (create a task, log an event,
write a journal entry, view the calendar) must work with **zero AI or
network dependency**. AI features (chat reasoning, recommendations, later
voice) are additive and must degrade gracefully when the cloud LLM is
unreachable — the user should never be blocked from operating their own
data because a network call failed.

---

## 3. Cloud LLM (user-facing conversation)

The user-facing conversation (final text/voice output, reasoning,
planning, advice, decision support, structured tool/action generation,
research, later voice) is generated by a **cloud LLM**. LifeOS does not
rebuild a local conversational LLM system the way the sibling NeXa project
does.

- **v0.1 final decision: OpenAI GPT-5.6 Luna.** This is a product decision
  for v0.1, superseding the research-stage default of Anthropic Claude
  Sonnet 5 recorded in `RESEARCH_DECISIONS.md` §1 (kept there as the
  research evidence/reasoning, not deleted). Luna is the budget/fast tier
  of the GPT-5.6 family — deliberately **not** the deep-reasoning tier;
  LifeOS does not use expensive/deep reasoning by default for everyday
  conversation.
- **Purpose:** natural user-facing conversation in Polish and English,
  explaining LifeOS Core's local results, discussing options, helping the
  user reason through plans/problems, and (later) optional web research.
- **Never hard-coded, regardless of the current default.** The provider is
  accessed through a replaceable abstraction. Model ID, system prompt, and
  tool definitions are configuration, not code, so switching provider
  requires no architectural change — only an adapter for that vendor's
  wire format (OpenAI's is the reference shape; Anthropic's Messages API
  is not OpenAI-wire-compatible, so the abstraction must not assume any
  one shape is "the" shape).
- **Multilingual requirement:** the assistant must work well in Polish,
  English, and mixed PL/EN input (§1.3) — detecting and responding in the
  user's language without a mode switch. No provider evaluated has a
  public Polish-specific benchmark; this remains an item to validate with
  a small internal eval once chat is implemented (`RESEARCH_DECISIONS.md`
  §1, uncertainty flags).

### 3.1 Hard constraint: the cloud model does not own state

**GPT-5.6 Luna (or whichever cloud model is configured) must NOT have
direct access to the SQLite database, must NOT be the canonical memory
system, and must NOT directly change goals, money, calendar, memory, or
any other LifeOS state.** LifeOS Core (local, deterministic + the local
helper model, §4) owns state and actions. The cloud model only ever
requests an action through the guarded action layer (§2) and only ever
receives the minimum required context assembled by the future Privacy
Firewall (§4.3) — never raw database access. This is `CLAUDE.md`
principle 8, restated here as the binding product requirement for M5
(Conversation, §12).

---

## 4. Local intelligence and the Privacy Firewall

### 4.1 Division of responsibility

- **Cloud LLM**: user-facing conversation, reasoning, planning, structured
  tool calls, research, later natural voice.
- **Local helper model — v0.1 final decision: Qwen3 4B** (Ollama-hosted in
  development, NOT user-facing). This is a product decision superseding
  the research-stage shortlist in `RESEARCH_DECISIONS.md` §2 (kept there
  as evidence; the benchmark plan in that section is still the right way
  to *validate* Qwen3 4B once M4 is reached — a product decision to build
  against it is not the same as it being benchmark-proven yet). Later
  responsibilities: intent extraction, structured extraction, entity
  extraction, candidate memory extraction, goal detection, classification,
  context selection, local summarisation, privacy preprocessing, PL/EN
  normalisation where necessary.
  - **Runtime abstraction requirement:** development runtime is Ollama,
    but the local-model runtime interface must stay abstract enough that
    production LifeOS can later bundle `llama.cpp` as a sidecar instead —
    end users must not be required to install Ollama manually. Treat
    "local model runtime" as a swappable interface from the start (same
    principle as the cloud-provider abstraction, §3), even though only
    the Ollama backend is implemented now.
- **Local deterministic code** (no LLM involved): financial calculations,
  dates, scheduling, rule evaluation, database operations, permissions,
  recurring-task expansion, progress calculations, notifications, and the
  domain command/action layer itself. Anything that does not require
  judgment must not go through an LLM.

### 4.2 Privacy is architectural

**User data belongs to the user.** Finances, journal, wellbeing, personal
problems, memories, preferences, history, goals, private events, and
personal observations are local-first and remain locally stored unless the
user explicitly chooses otherwise.

**The cloud LLM must never automatically receive the complete private
database.** It receives only the minimum necessary context, computed
locally. Example:

Instead of sending: name, address, employer, exact bank data, full
transaction history, private journal —

LifeOS generates locally and sends only what's needed to solve the
specific request, e.g.: `country = UK`, `available project budget ≈ £X`,
`mandatory expenses ≈ £Y`, `deadline = 6 weeks`.

The cloud model solves the reasoning problem using only the minimized
context. This is not a policy promise, it is a mechanical property of the
action layer (§2, guarded-action-layer principle): the cloud LLM only ever
sees what a called tool/function explicitly returns to it.

### 4.3 Privacy Firewall components (design targets, not yet implemented)

- **Data sensitivity classification** — every stored field/entity tagged
  with a sensitivity level (e.g. public-ok, internal, sensitive,
  never-leaves-device) at the schema level, not bolted on later.
- **Context minimisation** — local summarization/aggregation functions
  that turn raw records into the smallest sufficient statistic for a given
  cloud request (the £X/£Y/deadline example above).
- **PII detection & redaction** — a dedicated lightweight local model
  (not the general local helper LLM) purpose-built for this: research
  flagged GLiNER (zero-shot NER/span-tagging, CPU-friendly, 40+ entity
  types) as a better fit than prompting a general-purpose LLM for this
  narrow, high-stakes job. See `RESEARCH_DECISIONS.md` §2.
- **Anonymisation** — replacing identifying values with placeholders/
  categories before anything leaves the device, where redaction alone
  isn't sufficient.
- **Outbound request audit trail** — every payload sent to a cloud
  provider is logged locally (what was sent, to which provider, for what
  purpose, when) so the user can inspect it after the fact.
- **User approval for especially sensitive data** — a confirmation gate
  before any data tagged "sensitive" or above leaves the device, even if
  minimized.
- **Future UI**: a "Data used locally / Data sent to cloud / Provider /
  Purpose" inspector, so the user can audit what left the device and why,
  at any time — not just trust a settings toggle.

This firewall must not rely purely on provider privacy promises (retention
windows, training opt-outs). Those are a second layer of defense, not the
primary one.

---

## 5. Local database

The canonical user database is local SQLite, with an encryption layer and
OS-native key storage. Full evaluation and reasoning: 
`RESEARCH_DECISIONS.md` §4.

- **Encryption**: `sqlite3mc` (SQLite3 Multiple Ciphers, ChaCha20-Poly1305)
  rather than SQLCipher. This is a deliberate deviation from the initially
  proposed SQLCipher — sqlite3mc is actively maintained, format-compatible
  with SQLCipher (low switching risk), uses a faster AEAD cipher, and has
  no external OpenSSL/libcrypto build dependency (a real advantage before
  the Rust/MSVC toolchain is even installed on this machine).
- **Encryption key storage**: the OS-native secure credential store via the
  `keyring` Rust crate (Windows Credential Manager/DPAPI today; macOS
  Keychain and Linux Secret Service later, same code). **Not** Tauri's
  Stronghold plugin — Tauri's own maintainers have confirmed Stronghold is
  deprecated and slated for removal in v3 (see `RESEARCH_DECISIONS.md`
  §4), so it must not be adopted as new infrastructure now.
- **Driver/migrations**: `rusqlite` (with `sqlite3mc` bundled) +
  `refinery` for versioned schema migrations. Not the official
  `tauri-plugin-sql` (sqlx-based) — it lacks first-class
  SQLCipher/sqlite3mc support as of this research.
- **OS disk encryption** (BitLocker/FileVault/LUKS) is a complementary
  layer, not a substitute — Windows 11 Home's "Device Encryption" requires
  a Microsoft account and is off by default on local accounts (this dev
  machine's actual state), and disk encryption alone doesn't protect a
  copied-out database file (e.g. a backup on a USB stick).
- **Future semantic/vector search**: `sqlite-vec` — a maintained,
  dependency-free SQLite extension, successor to the now-unmaintained
  `sqlite-vss` — reserved for later long-term-memory retrieval features,
  not needed for the initial structured domains.

### 5.1 Preliminary domain concepts (not a frozen schema)

The following domain concepts are very likely, based on the product
definition, but domain boundaries should be finalized during actual schema
design, not frozen here:

`UserProfile`, `Goal`, `Project`, `Task`, `CalendarEvent`, `Reminder`,
`RecurringRule`, `Transaction`, `IncomeSource`, `Budget`,
`WellbeingCheckin`, `JournalEntry`, `Problem`, `SolutionAttempt`,
`Decision`, `Idea`, `Review`, `Memory`, `Observation`, `Recommendation`,
`Notification`, `ConversationAction`.

Prior-art research (`RESEARCH_DECISIONS.md` §5) suggests two schema
patterns worth carrying forward when the schema is actually designed:

- **Rust crate separation by domain** (adapted from Wealthfolio's
  architecture): split the backend into independent crates per domain
  (core, storage-sqlite, money, calendar, memory, ...) rather than one
  monolith, so each domain is independently testable and the future
  plugin/extensibility boundary is already in place.
- **Scored memory mirrored to a human-readable form** (adapted from
  OpenHuman's "Memory Tree" pattern): keep the authoritative long-term
  memory store in SQLite, but mirror it to a readable/editable form so the
  user can audit and correct what the system "remembers" about them.

### 5.2 Goal Intelligence (design target — not implemented this run)

Goals must not be isolated TODO records. LifeOS will eventually understand
relationships between goals such as: `SUPPORTS`, `CONFLICTS_WITH`,
`DEPENDS_ON`, `BLOCKS`, `CONTRIBUTES_TO`, `REQUIRES`, `PARENT_OF`,
`CHILD_OF`, `DUPLICATES`, `SUPERSEDES`, `COMPETES_FOR_RESOURCE`.

LifeOS may propose goal priorities based on: user-declared importance,
deadlines, cost of delay, dependencies, goals unlocked, resource
conflicts, time, money, current capacity, wellbeing, current situation,
risk, and user motivation.

**LifeOS does not make the final decision.** Priority recommendations are
discussable with the user; the user has final authority. This is a hard
product constraint, not a UI nicety — any future automatic priority
change must be proposed and confirmed, never silently applied (consistent
with the "I want to focus less on NeXa this week" example in §2).

Implemented starting at M6 (Personal Intelligence, §12) — recorded here
now so the initial `Goal` schema (M2+) leaves room for relationship edges
instead of needing a breaking migration later.

### 5.3 Personal Knowledge / Memory (design target — not implemented this run)

Future memory must distinguish at minimum these kinds: `FACT`,
`PREFERENCE`, `EXPERIENCE`, `DECISION`, `OBSERVATION`, `HYPOTHESIS`,
`EFFECTIVE_SOLUTION`, `INEFFECTIVE_SOLUTION`, `SKILL`, `HABIT`,
`CURRENT_STATE`. Do **not** flatten LifeOS memory into one text-memory
table — the design must support relationships between memories/entities
and temporal change, not just a growing list of strings.

Important knowledge requires provenance. Conceptually every memory record
carries: `source`, `source_reference`, `created_at`, `valid_from`,
`valid_until`, `confidence`, a confirmation/evidence state, `sensitivity`,
and related entities. Confirmation/evidence states distinguish at least:
`USER_CONFIRMED`, `OBSERVED`, `INFERRED`, `DISPUTED`, `STALE`.

This directly extends the "scored memory mirrored to a human-readable
form" pattern in §5.1 — the mirror should be legible enough that a user
can see *why* LifeOS believes something about them, not just *what*.

Implemented starting at M6 (Personal Intelligence, §12); the local helper
model (§4) is the mechanism that produces memory candidates from
conversation/journal input, which the user (or confidence thresholds)
then moves toward `USER_CONFIRMED` or `OBSERVED`.

---

## 6. Calendar

LifeOS has its own internal calendar as the canonical calendar — Google
Calendar/Outlook integration is not required initially. Locally stored:
events, deadlines, recurring events, reminders, planned work blocks,
project milestones.

Recurrence math (RFC5545) is not hand-rolled: `rrule.js` in the React
frontend (calendar rendering/editing) and the Rust `rrule` crate in the
backend (computing next-fire times for reminders/notifications that must
work even when the UI isn't open). Both are spec-driven implementations of
the same RFC, kept independent rather than sharing a native binding, per
`RESEARCH_DECISIONS.md` §5.

External behaviour initially consists primarily of native OS
notifications, not third-party calendar sync.

---

## 7. Notifications

LifeOS sends native OS notifications for: upcoming events, deadline risk,
payment reminders, weekly review, missed priority, planned task reminders,
important goal warnings. Notifications must continue to fire when the main
window is not foregrounded (tray/background execution), and must avoid
spam (batching/priority rules, not yet designed in detail).

Mechanism: official Tauri plugins — `tauri-plugin-notification`,
`tauri-plugin-autostart`, the core tray APIs, `tauri-plugin-updater`, and
`tauri-plugin-single-instance` (so the app doesn't launch twice when
tray-resident). All are genuinely cross-platform; the one platform-specific
note is that Linux tray icons require `libayatana-appindicator` (or
`libappindicator3`) to be present, which becomes a packaging dependency to
declare when Linux support is built. Details: `RESEARCH_DECISIONS.md` §4.

---

## 8. Voice (future — not built yet)

Voice is another input/output interface to the same action/capability
system as chat — it must not gain a second, different "brain." Full
research and comparison: `RESEARCH_DECISIONS.md` §3.

**Planned direction: a cascaded pipeline** (STT → the same cloud LLM
already used for text chat, with the same tool/domain-action bindings →
TTS), not a provider-native speech-to-speech model. Native S2S APIs
(OpenAI Realtime, Gemini Live, Amazon Nova Sonic) are architecturally a
*second brain* — a different model with its own tool-calling schema and
persona behavior that would have to be kept in sync with the text-chat
brain forever. Cascade avoids that by construction: voice is STT-in →
same-LLM-same-tools → TTS-out.

- **Orchestration**: prototype with Pipecat's non-WebRTC transport mode,
  not LiveKit Agents — LiveKit's core value (WebRTC SFU, multi-party/
  telephony transport) solves a network-transport problem LifeOS doesn't
  have (a single local device talking to its own mic/speaker). Revisit
  LiveKit only if a browser/mobile-remote or phone-call channel is ever
  added.
- **NeXa reuse**: no live NeXa git repository is accessible in this
  environment — only Desktop planning documents (docx/txt, not code)
  referencing prior NeXa research into AEC3 echo cancellation, self-echo
  suppression, and barge-in handling. That research area is real and
  reappears here (see the AEC3/echo-cancellation prototyping item below),
  but there is no NeXa source code to pull from; it must be re-solved
  for LifeOS's own Rust/Tauri context.
- **Engineering gaps to prototype before committing**: end-to-end cascade
  latency under real tool-calling load (LLM time-to-first-token will
  dominate, not STT/TTS); the speaker-to-mic self-echo/AEC path on Windows
  (`cpal` + WASAPI loopback + the `webrtc-audio-processing` Rust crate for
  AEC3 — a genuine gap neither Pipecat nor LiveKit solves for a
  non-browser-mediated local loopback); whether to bundle Pipecat as a
  Python sidecar or hand-roll the (comparatively small) STT/LLM/TTS
  orchestration loop directly in Rust; Tauri webview audio-capture
  reliability on Linux (WebKitGTK), which argues for capturing audio
  natively via `cpal` rather than through the webview at all.
- **Privacy**: audio leaves the device in every cloud-brain option
  evaluated (cascade or native S2S) — there is no fully-local path to a
  cloud-quality voice assistant. This must be disclosed to the user, and a
  text-only fallback must remain available for privacy-sensitive use.

---

## 9. UI/UX

No production UI is implemented yet (M1 begins the visual shell only —
navigation and layout, no domain data). This section is the visual
language, information architecture, and text wireframes to build against.

### 9.1 Visual language — Home, face, radial menu

LifeOS shares NeXa's visual language.

**Home:** full-screen dark galaxy background; an animated NeXa face
centered on screen — two white rounded bean-like eyes, a white mouth/
smile, clean and minimal; the galaxy remains visible behind the UI at all
times (the face and menus float over it, nothing fully occludes it).

**Face animation states** (infrastructure required now, M1; real
state-driving logic comes later with each capability): `IDLE`,
`LISTENING`, `LOCAL_PROCESSING`, `WAITING_FOR_CLOUD`, `SPEAKING`,
`ATTENTION`.

**Speaking state:** do not merely swap between smile/straight mouth
shapes — the mouth must visibly open/close with smooth animation. Later,
mouth openness may be driven approximately by output-audio amplitude.
Perfect phoneme lip-sync is explicitly not required.

**Radial menu:** clicking the face smoothly shrinks it slightly, keeps it
centered, and reveals glass-effect tiles around it like numbers around a
clock face. Primary ring (do not overload it):

```
Today | Chat | Calendar | Goals | Projects | Money | Me | Insights
```

Secondary/configuration access (later, not the first ring): Memory, Quick
Forms, Model/AI, Settings.

**Home gestures / edge drawers** (can start as desktop buttons/hit areas —
perfect touch gestures are not required immediately, but the architecture
must allow this interaction model later):

- **Bottom edge** — chat access and chat-history controls (New Chat,
  recent chats, continue chat, delete/archive chat — later).
- **Left edge** — Today/Dashboard drawer.
- **Top edge** — notification/question/decision drawer.

### 9.2 Information architecture

```
Today | Chat | Calendar | Goals | Projects | Money | Me | Insights | Settings
```

LifeOS should feel like a personal command centre, not enterprise
accounting software. Avoid dashboard overload everywhere below — each
screen should answer a small number of specific questions well, not
surface every possible metric.

### 9.3 Today

Answers immediately: What matters today? What should I do next? Am I on
track? What requires attention? What is upcoming?

```
+-------------------------------------------------------------+
| TODAY                                              [date]   |
+-------------------------------------------------------------+
| Current direction: <one-line summary of top goal/focus>     |
+-------------------------------------------------------------+
| TOP 3 TODAY                                                  |
|  1. [ ] ...                                                  |
|  2. [ ] ...                                                  |
|  3. [ ] ...                                                  |
+-------------------------------------------------------------+
| GOALS PROGRESS        | PROJECTS PROGRESS                    |
|  Goal A  [====  ] 62% |  Project X — on track, next: ...     |
|  Goal B  [==    ] 30% |  Project Y — blocked: ...             |
+-------------------------------------------------------------+
| MONEY SNAPSHOT         | UPCOMING                            |
|  Safe-to-spend: £N     |  Fri 14:00  View the van             |
|  Next bill: ... in Nd  |  Mon        Project X milestone       |
+-------------------------------------------------------------+
| WELLBEING SNAPSHOT     | WARNINGS                            |
|  Energy 6/10, mood OK  |  ⚠ Deadline risk: ...                |
+-------------------------------------------------------------+
| INSIGHTS (1-2 surfaced, not a feed)                          |
|  "You tend to skip journaling on high-stress days."          |
+-------------------------------------------------------------+
```

### 9.4 Chat

Full-height conversation view. Natural operation of the system per §2 —
every action the chat performs is visibly the same action a form would
trigger (e.g. a chat-created task appears identically to a form-created
task, with an inline confirmation of what was just done, not just prose).

### 9.5 Calendar

The local LifeOS calendar (§6): month/week/day views, recurring events,
deadlines, reminders, planned work blocks, project milestones layered on
one timeline.

### 9.6 Goals

Per goal: target, deadline, progress, current status, next actions.

```
GOAL: First paying DeVDul client
  Deadline: Dec 2026        Status: On track
  Progress: [======    ] 60%
  Next actions:
   - Follow up with lead #3
   - Publish portfolio update
```

### 9.7 Projects

Active projects; priority; progress; current milestone; blockers.

```
PROJECT: Van purchase
  Priority: High            Progress: [===       ] 30%
  Current milestone: Viewing on Friday
  Blockers: Financing approval pending
```

### 9.8 Money

Eventually: cash, income, expenses, budgets, recurring costs (fuel, food,
work income), savings, protected buffer, safe-to-spend, forecast. Not
built in this phase — domain model only (§5.1).

### 9.9 Me

Eventually: wellbeing, preferences, habits, journal, recurring problems,
successful solutions, personal development, relevant memory.

### 9.10 Insights

Eventually: learned patterns and AI recommendations, surfaced sparingly —
this is the screen most at risk of becoming noise; default to showing few,
high-confidence insights rather than a feed.

### 9.11 Settings

Privacy (the Privacy Firewall inspector, §4.3), AI providers (cloud LLM
selection, §3), local model (§4.1), notifications (§7), data/export,
backups.

---

## 10. Extensibility

LifeOS will evolve while being used; new modules and product ideas are
expected to be discovered during development, not just at design time.

Avoid: a giant monolithic service; hard-coded domain knowledge inside the
chat UI; cloud-model-specific business logic; duplicated logic between
forms/chat/voice; database structures that can't evolve.

Prefer: domain capabilities as the unit of extension; commands/actions as
the interface between UI/chat/voice and domain logic; clear service
boundaries (reinforced by the Rust-crate-per-domain pattern, §5.1); stable
interfaces; versioned migrations; a swappable cloud-LLM-provider interface
(§3). This should allow future capabilities (Health, Learning,
Relationships, Career, Travel, Documents, Vehicles, Home, NeXa
integration) without rebuilding the core.

### 10.1 Future NeXa integration (architectural from day one)

LifeOS is currently a standalone application, but it is intentionally
built so that NeXa can later receive LifeOS as a capability, without the
desktop UI ever being the business-logic owner:

```
NeXa Core -> LifeOS Capability/API -> LifeOS Core
```

NeXa should later be able to call capability operations such as
`get_current_situation`, `get_active_goals`, `get_today_plan`,
`get_relevant_memory`, `create_goal`, `update_goal`, `create_challenge`,
`record_solution_attempt`, `record_outcome`, `create_calendar_event`,
`change_priority`, `log_wellbeing`, `get_progress` — **without directly
manipulating LifeOS database tables.** This is the same guarded action
layer (§2) the desktop UI and the cloud LLM already use; a NeXa
capability boundary is a third caller of that one layer, not a new
integration point into the database. No NeXa-specific code is written in
this build phase — this subsection records the constraint so the action
layer isn't accidentally designed as desktop-UI-only.

---

## 11. Technology stack

Full evaluation, alternatives considered, and sources:
`RESEARCH_DECISIONS.md` §4–§5.

| Layer | Choice | Deviation from initial candidate stack? |
|---|---|---|
| Desktop shell | Tauri 2 | No — confirmed as the right choice |
| Frontend | React + TypeScript | No |
| Backend | Rust | No |
| Local DB | SQLite | No |
| DB encryption | `sqlite3mc` (ChaCha20-Poly1305) | **Yes** — was SQLCipher |
| Encryption key storage | `keyring` crate (OS credential store) | **Yes** — Stronghold considered and rejected (deprecated upstream) |
| DB driver/migrations | `rusqlite` + `refinery` | Clarification — not the sqlx-based official plugin |
| Cloud LLM | Provider-agnostic; **v0.1 final: OpenAI GPT-5.6 Luna** | Product decision, §3 — supersedes the research-stage Sonnet-5 default |
| Local helper LLM | **v0.1 final: Qwen3 4B**, Ollama-hosted in dev, runtime kept abstract for a future llama.cpp sidecar | Product decision, §4 — benchmark plan in `RESEARCH_DECISIONS.md` §2 still validates it at M4 |
| Calendar/recurrence | `rrule.js` (frontend) + `rrule` Rust crate (backend) | No — confirms "don't hand-roll RFC5545" |
| Notifications/background | Tauri official plugins (notification, autostart, tray, updater, single-instance) | No |
| Forms/validation | React Hook Form + Zod | No |
| Charts | Recharts | No |
| Calendar UI component | react-big-calendar | Clarification — chosen over FullCalendar to avoid its paid Premium tier |
| Future vector/semantic search | `sqlite-vec` | No — successor to the now-unmaintained `sqlite-vss` |
| Future voice orchestration | Pipecat (non-WebRTC mode) | Clarification — LiveKit Agents considered and deferred |

---

## 12. Development milestones (current build plan)

Vertical milestones — the app is tested continuously, not built to a big
bang. Each milestone should end with a working, tested increment — no
"60% done, half-implemented" states left behind (per `CLAUDE.md`). This
list supersedes the earlier M0–M7 sketch (kept in spirit, expanded and
reordered against `PROJECT_STATE.md`'s current reality).

- **M0 — Environment ready.** Install/verify Windows C++/MSVC
  prerequisites for Tauri; install Rust via `rustup`; verify cargo/rustc;
  verify Node/npm; verify WebView2; verify Tauri prerequisites; document
  exact versions in `PROJECT_STATE.md`.
- **M1 — Visual shell.** Scaffold Tauri 2 + React + TypeScript; launch the
  real LifeOS desktop window; galaxy Home; initial animated NeXa face;
  face-state infrastructure (§9.1); face click interaction; radial glass
  menu; basic screen/navigation shell; left/bottom/top drawer
  architecture. No fake backend functionality. **End of M1: the user must
  be able to launch and see the real app.**
- **M2 — LifeOS Core + encrypted storage.** Local encrypted SQLite
  (`sqlite3mc`), secure key handling (`keyring`), migrations (`refinery`),
  core action architecture (§2), initial profile/settings. No AI
  dependency for CRUD (AI-optional resilience, §2).
- **M3 — Core life UI.** Today, Goals, Projects, Calendar; Quick Add /
  schema-driven forms; PL/EN UI architecture (§1.3); backend recurrence
  authority (the Rust `rrule` crate, §6).
- **M4 — Local intelligence.** Ollama development runtime; Qwen3 4B (§4);
  benchmark on the actual laptop (`RESEARCH_DECISIONS.md` §2's plan);
  PL/EN extraction; structured-output reliability; local intent/entity
  extraction; memory candidates; privacy preprocessing.
- **M5 — Conversation.** GPT-5.6 Luna provider (§3); provider abstraction;
  Chat UI; Conversation Brief/context builder; Privacy Firewall (§4.3); no
  direct DB access from cloud (§3.1); actions mediated locally.
- **M6 — Personal intelligence.** Goal Intelligence (§5.2); Challenges;
  Problems/Solutions; Personal Knowledge Layer (§5.3); memory provenance;
  questions from LifeOS; insights;
  confirmed/observed/inferred distinction.
- **M7 — Proactive LifeOS.** Native notifications; reminders; follow-ups;
  check-ins; Today supervision; weekly review; adaptive plan suggestions;
  anti-spam/quiet-hour rules.
- **M8 — Money + Me.** Finances, budgets, safe-to-spend, forecasts; sleep,
  mood, energy, stress; habits, skills; user-configurable tracked
  metrics.
- **M9 — Voice.** `microphone -> local STT -> same LifeOS input pipeline
  -> Qwen/Core/Privacy -> GPT-5.6 Luna -> TTS -> speaker`. Text and voice
  use the same LifeOS domain/action system (§2, §8) — no second,
  independent conversational brain.
- **M10 — Distribution.** `LifeOS-Setup.exe`; clean-machine installation
  test; hardware detection; local-model download/setup; backup; restore;
  diagnostics; update path.

### 12.1 Usable-Alpha definition

LifeOS becomes a **Usable Alpha** when the creator can realistically use
it for at least seven consecutive days for real life management. At
minimum the user must be able to: launch LifeOS normally; use the NeXa
Home interface; see what matters today; create/update goals; manage
projects; use the LifeOS Calendar; chat naturally in Polish and English;
enter changes through conversation; retain relevant local long-term
personal knowledge; record a Challenge/problem; record attempted
solutions; record whether a solution worked; receive useful LifeOS
questions/follow-ups; see progress; receive native reminders/
notifications; inspect important AI-proposed changes; undo/reject
inappropriate changes; keep private data local; see that cloud
conversation receives only approved/minimized context.

Success is **not** "all planned features exist." Success is: LifeOS
genuinely helps the user understand what to do, make plans, remember
important context, adapt when reality changes, and learn what works for
that user. "First run" (a real desktop window launches, M1) and "Usable
Alpha" (the above) are different bars — do not conflate them in a RUN
REPORT.

---

## 13. Open questions

These are genuinely unresolved and should not be treated as decided:

- Qwen3 4B and GPT-5.6 Luna are now the chosen v0.1 models (§3, §4) — but
  chosen is not the same as *validated*. The benchmark plan in
  `RESEARCH_DECISIONS.md` §2 still needs to run against Qwen3 4B at M4,
  and a small internal Polish eval still needs to run against GPT-5.6
  Luna once chat exists (M5).
- Exact data-sensitivity classification levels and which specific fields
  fall into each tier (§4.3) — needs to be defined alongside the actual
  schema, not abstractly.
- Multi-device sync strategy — CRDT-based (AFFiNE's y-octo/OctoBase
  pattern) vs. a sidecar tool like Syncthing vs. no sync in v1. Deferred;
  not needed for a single-device initial release.
- Exact notification batching/anti-spam rules (M7).
- Whether/how NeXa integration will actually work in practice, given no
  NeXa codebase is currently accessible from this environment — the
  capability-API shape (§10.1) is a design constraint, not yet a tested
  integration.
- Exact bilingual detection/normalisation mechanism (which layer decides
  the response language, how mixed-language utterances are handled) —
  design target stated in §1.3, mechanism not yet designed.
