# R017 — M2 Personal Knowledge Layer Architecture Freeze

**Documentation/architecture only. No Rust or React code changed. No
migration created.** This report freezes the v0.1 design of the
Personal Knowledge Layer — the six-entity family (`Entity`/`Fact`/
`Relation`/`Episode`/`Source`/`Evidence`) already named in R003/
`LIFEOS_SPEC.md` §5.3, but never specified at field level — plus its
supporting join tables and its four actions (`AddMemory`/
`CorrectMemory`/`ConfirmMemory`/`ForgetMemory`). It supersedes and
corrects several under-specified or genuinely contradictory points in
R003's original one-paragraph-per-table sketch, each called out
explicitly below (§2), not silently overwritten. **No implementation
happens in this run** — the next slice (§26/§28) still needs its own
vertical-slice pass, the same way Milestone/Task each got one after
their own freeze.

---

## 1. Purpose and product role

LifeOS is not a planner, task manager, calendar, or chatbot with a
memory bolted on — its differentiator is that it gradually understands
and retains relevant knowledge about the user: facts, preferences,
decisions, experiences, what worked and what didn't, skills, habits,
current state, relationships between things, historical context, and
where each piece of knowledge came from. This layer must eventually let
LifeOS answer questions like "what worked for me last time?", "why did
I make this decision?", "what have I already tried?", "where did this
come from?", "are you sure this is true?", and "forget this
completely" — while remaining **local-first, inspectable, correctable,
deletable, traceable, temporal, extensible**, and reusable by NeXa
later. This report freezes the data model and action contracts that
make those properties structural rather than aspirational.

---

## 2. Existing frozen constraints inherited from R003/LIFEOS_SPEC — and corrections

Read in full before designing (per this run's own instruction): R003
§B (Personal Knowledge Layer), §E (action layer), §F (confirmation
policy); `LIFEOS_SPEC.md` §5.3 (Personal Knowledge/Memory), §5.4
(action layer), §5.5 (confirmation policy), §4.2-4.3 (Privacy
Firewall), §13 (open questions); R005/R006/R008/R009/R010/R015/R016 for
the established M2 engineering conventions (typed IDs, `FieldPatch<T>`,
atomic mutation+audit transactions, `CoreError`, the `services::
storage` privacy boundary, defense-in-depth DB constraints alongside
Core validation, "do not invent product rules mid-implementation").

**Preserved exactly, unchanged, from R003/LIFEOS_SPEC:**

- The six-table minimum family: `Entity`, `Fact`, `Relation`, `Episode`,
  `Evidence`, `Source` — not one generic "memory" table, not a vector
  store, not chat history (§3).
- The 11 memory-type/category values, verbatim (§6).
- The 5 epistemic-state values, verbatim (§7).
- Temporal validity via insert-new-row-and-close-the-old-one, never
  in-place semantic overwrite (§14).
- `ForgetMemory` is true, permanent deletion, not soft-delete/tombstone
  — only a minimal non-sensitive audit marker survives (§19).
- The four-action Memory family: `AddMemory`, `CorrectMemory`,
  `ForgetMemory`, `ConfirmMemory` — no explosion into narrower actions.
- Confirmation floors already frozen in `LIFEOS_SPEC.md` §5.5/R003 §F's
  type-floor table: `AddMemory` = `EXPLICIT_ADD`, `ConfirmMemory` =
  `EXPLICIT_ADD`, `CorrectMemory` = `IMPORTANT_CHANGE`, `ForgetMemory` =
  `DELETE`. **This run does not re-decide these — it confirms and
  documents them** (§16-19).
- `GoalRelation.relation_type` (closed 11-value enum, §5.2) remains
  completely separate from Personal Knowledge `Relation.relation_type`
  (open string) — the two only ever shared a field name, never a type
  or a table (§8 below reaffirms this, unchanged).

**Genuine contradictions found against R003's original sketch, resolved
here (not silently ignored, per this run's explicit instruction):**

1. **`Entity.kind` was a CLOSED enum in R003** (`SELF`/`PERSON`/
   `PLACE`/`ORGANIZATION`/`CONCEPT`/`DOMAIN_REF`/`OTHER`). This run's
   brief explicitly requires an OPEN vocabulary (`entity_type: String`)
   plus a dedicated `is_self: bool` structural flag instead of a
   `kind=SELF` enum value. **Resolution: the open-vocabulary direction
   wins** — a closed entity-type enum cannot represent "vehicle",
   "device", or any future domain LifeOS doesn't have yet without a
   migration, which directly conflicts with this document's own §6
   instruction ("do NOT create a closed enum of every possible entity
   type... LifeOS must support future unknown domains"). Splitting out
   `is_self: bool` also removes a special "one magic enum value means
   the user" case from every future query — a real correction, not
   cosmetic (§6).
2. **`Fact.content` (a single text-plus-optional-JSON blob) in R003
   becomes a structured subject-predicate-object triple** (`subject_
   entity_id`, `predicate`, `value_json` XOR `object_entity_id`) per
   this run's brief. **Resolution: the triple model wins** — it is
   strictly more useful for predicate-based retrieval/querying ("what
   do we know about X's `prefers_work_time`?") and for representing
   entity-to-entity facts ("SELF owns_vehicle MazdaEntity") without a
   second ad hoc table per relationship kind, which a single opaque
   `content` blob cannot do (§9).
3. **`Fact.subject_entity_id` was nullable in R003** ("optionally about
   an Entity"). With a frozen SELF entity now guaranteed to exist,
   every fact this layer will realistically record is about *some*
   entity (the user, another person, a place, a domain object). **Resolution:
   `subject_entity_id` is now `NOT NULL`** — a deliberate tightening,
   not a relaxation; there is no example anywhere in this run's brief
   of a genuinely subject-less fact, and requiring one removes an
   ambiguous NULL-handling branch from every future query (§9).
4. **`Fact.source_id` (a direct FK) in R003 is removed.** This run's
   brief's preferred `Fact` shape has no `source_id` field at all, and
   its §17 explicitly describes the chain as `Source → Evidence →
   Fact/Relation/Episode` — a single provenance path, not two
   (`Fact.source_id` directly, *and* `Fact → Evidence → Source`
   indirectly, which could disagree with each other). **Resolution: a
   Fact's provenance is established exclusively through `fact_evidence`
   → `Evidence.source_id` → `Source`.** Every `Fact` created by
   `AddMemory` must have at least one linked `Evidence` row (Core-
   enforced invariant, §12/§16) (§9, §12).
5. **`Relation` was polymorphic in R003** (`from_type`/`from_id` /
   `to_type`/`to_id`, `ENTITY | FACT`) — an unenforceable-by-FK shape
   this run's own brief elsewhere explicitly warns against ("a
   polymorphic `target_id` with no enforceable FK", §14). This run's
   brief's preferred `Relation` shape is `from_entity_id`/
   `to_entity_id` only. **Resolution: Entity-to-Entity only, real FKs
   both sides.** A knowledge-graph edge that must reference a *Fact*
   directly (rather than the Entity a Fact is about) is not needed by
   any example in this run's brief and is deliberately out of scope for
   v0.1 (§11).
6. **`Evidence.fact_id` was a direct embedded FK in R003** (Evidence →
   exactly one Fact). This run's brief's preferred `Evidence` shape has
   no `fact_id` field, and §14 explicitly asks for normalized link
   tables (`fact_evidence`, plus `relation_evidence`/`episode_evidence`)
   instead. **Resolution: normalized link tables win** — the same
   `Evidence` row (e.g. one journal excerpt) can legitimately support
   more than one `Fact`/`Relation`/`Episode`, which an embedded
   single-parent FK cannot express (§13-14).
7. **`Source.kind` in R003 was a fixed six-value list with no explicit
   open/closed decision.** This run's brief explicitly says "avoid an
   unnecessarily closed enum if future providers/sources require
   growth." **Resolution: `Source.source_type` is frozen OPEN** (plain
   validated `TEXT`, not a `CHECK`-constrained closed set) — R003's six
   values plus this run's brief's additional examples (`CONNECTED_APP`,
   `WEB_RESEARCH`, `DEVICE_OBSERVATION`) become the documented starting
   *canonical* vocabulary, not an enforced ceiling (§13).

No other part of R003/`LIFEOS_SPEC.md`'s Personal Knowledge sketch is
contradicted — everything else in this freeze either preserves it
verbatim or is a genuine field-level elaboration R003 never attempted
(R003 explicitly called its own table listed "table shape, not final
column-level SQL").

---

## 3. Terminology — "Memory" (product) vs. Personal Knowledge (internal model)

**Frozen distinction, product-facing vs. internal:** "Memory" is the
**user-facing product concept** — what a person means when they say
"remember that I prefer working in the morning" or ask "what do you
remember about this project?". Internally, LifeOS never stores a blob
called "memory" — that sentence becomes a structured `Fact` (subject
`SELF`, predicate `prefers_work_time`, value `"morning"`), a `Source`
(this chat message), and `Evidence` (the specific supporting excerpt).
The four Memory *actions* (`AddMemory`/`CorrectMemory`/`ConfirmMemory`/
`ForgetMemory`, §16-19) are named for the product concept a user/UI
reasons about, even though what they manipulate underneath is the
structured Personal Knowledge model.

**Chat history is not the durable Personal Knowledge database.** A
conversation may become a `Source` (with `source_type =
'CHAT_MESSAGE'`) and/or contribute `Evidence`, but the raw transcript
itself is never the authoritative memory model — only what gets
explicitly promoted into a `Fact` (via `AddMemory`, whether user-
triggered or, after confirmation, model-proposed) persists as durable
knowledge. This mirrors the same discipline already established for
domain entities: `Episode` is not chat history either (§10), and a
`CalendarEvent` is not an `Episode` (§22).

---

## 4. Entity schema

```
entities (
    id               TEXT PRIMARY KEY,          -- EntityId, UUIDv7
    entity_type      TEXT NOT NULL,             -- OPEN vocabulary
    canonical_name   TEXT NOT NULL,              -- non-empty, trimmed, bounded length
    is_self          INTEGER NOT NULL DEFAULT 0, -- boolean 0/1
    domain_ref_type  TEXT,                       -- nullable, OPEN vocabulary
    domain_ref_id    TEXT,                       -- nullable, the domain row's id as text
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);
```

| Field | Type | Null? | Meaning | Vocabulary | FK | Temporal |
|---|---|---|---|---|---|---|
| `id` | `TEXT` (UUIDv7) | NOT NULL | `EntityId`, primary key | — | — | — |
| `entity_type` | `TEXT` | NOT NULL | What kind of thing this is (`"person"`, `"place"`, `"organization"`, `"vehicle"`, `"device"`, `"concept"`, …) | **OPEN** — Core validates non-empty/trimmed/bounded length only, no `CHECK` list, so future domains never need a migration to add a type (§6) | — | — |
| `canonical_name` | `TEXT` | NOT NULL | The human-readable name this Entity is known by | free text, Core-validated non-empty/bounded | — | — |
| `is_self` | `INTEGER` (0/1) | NOT NULL | Whether this row is the one canonical Entity representing the current LifeOS user | closed (boolean) | — | — |
| `domain_ref_type` | `TEXT` | nullable | Names which domain module this Entity points at when it is a pointer to an existing structured-domain row (`"Goal"`, `"Project"`, `"Task"`, `"Milestone"`, future domains) | **OPEN**, documented values only | **not FK-enforced** — deliberate, see §21 | — |
| `domain_ref_id` | `TEXT` | nullable | The referenced domain row's id, as text | — | **not FK-enforced**, see §21 | — |
| `created_at` / `updated_at` | `TEXT` (RFC3339 UTC) | NOT NULL | Standard M2 timestamp convention | — | — | `updated_at` may only ever change `canonical_name`/`entity_type`/`domain_ref_*` via a future `UpdateEntity`-shaped action — **no such action is frozen in this run** (entities are created once by `AddMemory`'s entity-resolution needs and otherwise immutable for v0.1; see §25) |

**DB-level invariants (documentation only — not migrated in this run,
§25/§29):**

- `CHECK (length(trim(canonical_name)) > 0)`
- `CHECK (is_self IN (0, 1))`
- `CHECK ((domain_ref_type IS NULL) = (domain_ref_id IS NULL))` — both
  set together or both absent, never one without the other.
- **Exactly one SELF entity, frozen (§6 of this run's brief).** A
  partial unique index: `CREATE UNIQUE INDEX entities_single_self ON
  entities (is_self) WHERE is_self = 1;` — SQLite enforces this at the
  DB level; a second `INSERT ... is_self = 1` fails the constraint. The
  SELF entity is system-created once, during the first Personal
  Knowledge bootstrap (exact mechanism — e.g. lazily on first
  `AddMemory` call vs. eagerly at DB init — is an implementation
  decision for the first slice, §26, not frozen here).
- `UNIQUE (domain_ref_type, domain_ref_id) WHERE domain_ref_type IS NOT
  NULL` — at most one Entity may point at a given domain row, avoiding
  ambiguity about "which Entity represents Project X."

**Aliases/entity resolution: explicitly deferred (per this run's brief
§6).** Nothing in this freeze builds "these two Entity rows actually
refer to the same real-world thing" resolution/merging. A future
`SAME_AS`-style `Relation` (§11) between two Entity rows is one
plausible future direction, not decided now.

---

## 5. Fact schema — the central Personal-Knowledge assertion

```
facts (
    id                      TEXT PRIMARY KEY,      -- FactId, UUIDv7
    subject_entity_id       TEXT NOT NULL REFERENCES entities (id),
    predicate               TEXT NOT NULL,
    value_json              TEXT,                   -- nullable
    object_entity_id        TEXT REFERENCES entities (id),  -- nullable
    memory_type             TEXT NOT NULL,
    epistemic_state         TEXT NOT NULL,
    confidence              REAL,                    -- nullable, 0.0-1.0
    sensitivity             TEXT NOT NULL,
    valid_from               TEXT NOT NULL,
    valid_until              TEXT,                    -- nullable = currently active
    superseded_by_fact_id    TEXT REFERENCES facts (id),  -- nullable
    created_at               TEXT NOT NULL,
    updated_at               TEXT NOT NULL
);
```

| Field | Type | Null? | Meaning | Vocabulary/bounds | FK | Temporal |
|---|---|---|---|---|---|---|
| `id` | `TEXT` (UUIDv7) | NOT NULL | `FactId` | — | — | Immutable |
| `subject_entity_id` | `TEXT` | **NOT NULL** (corrected vs. R003, §2 item 3) | The Entity this Fact is about | — | `entities(id)`, `ON DELETE RESTRICT` | Immutable after creation |
| `predicate` | `TEXT` | NOT NULL | What relationship/attribute this Fact asserts about the subject | **OPEN** — Core validates non-empty/trimmed/bounded length (§8); documented canonical predicates should be reused where known, but new ones need no migration | — | Immutable after creation |
| `value_json` | `TEXT` | nullable, **exactly one of `value_json`/`object_entity_id`** | The scalar or small structured value the predicate resolves to, JSON-encoded | Data only — never executable content (§9) | — | Immutable after creation |
| `object_entity_id` | `TEXT` | nullable, **exactly one of `value_json`/`object_entity_id`** | The Entity the predicate resolves to, when the object is itself a knowable thing rather than a scalar | — | `entities(id)`, `ON DELETE RESTRICT` | Immutable after creation |
| `memory_type` | `TEXT` | NOT NULL | Which of the 11 frozen categories this Fact belongs to (§6) | **CLOSED**, `CHECK IN (11 values)` | — | Correctable via `CorrectMemory` (§17) |
| `epistemic_state` | `TEXT` | NOT NULL | Which of the 5 frozen epistemic states this Fact currently holds (§7) | **CLOSED**, `CHECK IN (5 values)` | — | Mutable in place via `ConfirmMemory` (§18); also set to `STALE` on the predecessor by `CorrectMemory` |
| `confidence` | `REAL` | nullable | Numeric confidence, when meaningful (§15) | `CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0))` | — | Correctable via `CorrectMemory` |
| `sensitivity` | `TEXT` | NOT NULL | Privacy-Firewall classification tier (§14) | **CLOSED**, `CHECK IN (4 values)` | — | Correctable via `CorrectMemory` |
| `valid_from` | `TEXT` (RFC3339 UTC) | NOT NULL | When this Fact's assertion became the active belief | — | — | Set once at creation |
| `valid_until` | `TEXT` (RFC3339 UTC) | nullable | `NULL` = currently active; non-`NULL` = closed/historical (§25) | — | — | Set by `CorrectMemory` on the predecessor |
| `superseded_by_fact_id` | `TEXT` | nullable | Points at the successor Fact that replaced this one | — | `facts(id)`, `ON DELETE RESTRICT`; `CHECK (superseded_by_fact_id IS NULL OR superseded_by_fact_id <> id)` (cannot point to self, §29) | Set by `CorrectMemory` on the predecessor |
| `created_at` / `updated_at` | `TEXT` | NOT NULL | Standard M2 timestamps | — | — | `updated_at` advances on any of the in-place mutations above |

**One-semantic-object invariant (frozen, §7 of this run's brief):**
`CHECK ((value_json IS NOT NULL) <> (object_entity_id IS NOT NULL))` —
exactly one, never both, never neither. This is what lets `SELF --
prefers_work_time --> "morning"` (a scalar) and `SELF --
owns_vehicle --> MazdaEntity` (an entity reference) share one table
without a new SQL table per future life domain.

**`value_json` is the canonical v0.1 scalar/structured representation**
(frozen, §7): a JSON-encoded string (`serde_json`, matching the
`previous_snapshot` convention already established by every other M2
audit row, R014/R015) that may hold a scalar (string/number/bool) or a
small structured object (e.g. `{"value": 91.2, "unit": "kg"}`). It is
**never** executable content (no scripts, no templates, no expressions)
— it is pure data, exactly like every other JSON payload already
persisted in M2. Core enforces a maximum length bound (exact number set
at implementation time, matching the existing convention of bounding
every other free-text/JSON field, e.g. `description`/`note`).

**Fact mutability contract (frozen, new to this document — needed
because §2 items above establish Facts are *mostly* immutable but not
*entirely*):** after creation, a Fact row may ONLY ever have
`epistemic_state`, `confidence`, `sensitivity`, `valid_until`,
`superseded_by_fact_id`, and `updated_at` change — via `ConfirmMemory`
(epistemic_state only) or as the predecessor side-effect of
`CorrectMemory` (§17-18). `subject_entity_id`, `predicate`,
`value_json`, `object_entity_id`, `memory_type`, and `created_at` are
**immutable for the life of the row**. Changing what a Fact asserts
always means creating a new Fact (`CorrectMemory`'s successor, or an
unrelated `AddMemory`) — never an in-place semantic edit.

**No `Fact.source_id`** (§2 item 4) — provenance flows exclusively
through `fact_evidence` → `Evidence.source_id` → `Source` (§12).

---

## 6. Memory types — exact frozen set, with disambiguation

Preserved exactly, unchanged, unrenamed: `FACT`, `PREFERENCE`,
`EXPERIENCE`, `DECISION`, `OBSERVATION`, `HYPOTHESIS`,
`EFFECTIVE_SOLUTION`, `INEFFECTIVE_SOLUTION`, `SKILL`, `HABIT`,
`CURRENT_STATE`. No renaming, no additions this run.

| Value | Meaning |
|---|---|
| `FACT` | A general, stable, typically durable piece of knowledge about the subject, treated as standing truth until explicitly corrected (e.g. "user's employer is Acme Corp"). Not a one-off noticed instance — see `OBSERVATION` below for that. |
| `PREFERENCE` | A subjective like/dislike or preferred way of doing something (e.g. "prefers working in the morning"). Naturally revisited over time via `CorrectMemory`. |
| `EXPERIENCE` | A neutral record that something happened or was tried, **without necessarily asserting an efficacy verdict** (e.g. "tried intermittent fasting for two weeks"). Distinct from `EFFECTIVE_SOLUTION`/`INEFFECTIVE_SOLUTION` below by carrying no judgment about whether it worked. |
| `DECISION` | A record that a discrete choice was made among alternatives at a point in time (e.g. "decided to switch jobs in March 2026"), attributable to the user. Distinct from `PREFERENCE` (a decision is a dated event, a preference is a standing disposition) and from `EXPERIENCE` (a decision is the choice point, not the lived-through aftermath — a `DECISION` and a later `EFFECTIVE_SOLUTION`/`INEFFECTIVE_SOLUTION` about the same matter commonly pair together). |
| `OBSERVATION` | A specific, timestamped, directly-noticed occurrence — more granular/momentary than `FACT`. Distinct from `FACT`: `FACT` is the stable, generalized conclusion; `OBSERVATION` is a single data point that may or may not later be generalized into one. Distinct from `HYPOTHESIS`: an `OBSERVATION` asserts something was directly seen/reported to happen (the noticing itself is not in doubt); a `HYPOTHESIS` is a candidate explanation that has not been confirmed at all. |
| `HYPOTHESIS` | A tentative, unconfirmed candidate belief or pattern (e.g. "may be more productive after exercising"), typically `epistemic_state = INFERRED`. Must never be treated as durable truth in retrieval without appropriate caveats, and must never silently become `USER_CONFIRMED` (§7). |
| `EFFECTIVE_SOLUTION` | An `EXPERIENCE`-family record that specifically carries a **positive** efficacy verdict — this approach was tried and assessed as having worked. |
| `INEFFECTIVE_SOLUTION` | The same, with a **negative** verdict — tried and assessed as not having worked. |
| `SKILL` | A capability/competency the subject possesses or is developing (e.g. "proficient in Rust", "learning Polish"). Distinct from `FACT`/`EXPERIENCE` by being about capability, not a single event or a general fact. |
| `HABIT` | A recurring behavior pattern (e.g. "checks email first thing every morning"). Distinct from `PREFERENCE` (a habit is about actual recurring behavior; a preference is about a liked/wanted state that may or may not be consistently acted on) and from `CURRENT_STATE` (a habit is a standing pattern over time, not a point-in-time snapshot). |
| `CURRENT_STATE` | A point-in-time value the system **expects to change relatively often**, where recency matters more than history for everyday purposes (e.g. "current weight: 91.2kg", "currently working on Project X"). Distinguished from a long-lived `FACT` **only by expected churn rate and framing, not by mechanism** — both use the identical temporal-supersession machinery (§14); a `CURRENT_STATE` fact is simply one whose supersession chain is expected to be read/walked far more often. |

---

## 7. Epistemic states — exact frozen set

Preserved exactly, unchanged: `USER_CONFIRMED`, `OBSERVED`, `INFERRED`,
`DISPUTED`, `STALE`.

| Value | Meaning |
|---|---|
| `USER_CONFIRMED` | The user has explicitly stated or explicitly confirmed this exact semantic value themselves — either via `AddMemory` with an unambiguous, directly-requested provenance, or by later confirming an inferred candidate via `ConfirmMemory`. The **only** state representing direct human authority over the content. |
| `OBSERVED` | Produced by local deterministic code or a directly-witnessed event, without an interpretive/inferential leap (e.g. a system-logged check-in value). Distinct from `INFERRED`: `OBSERVED` is what was directly recorded; `INFERRED` is a conclusion that goes beyond the directly observed data. |
| `INFERRED` | A candidate produced by model reasoning (local Qwen, §23) that has **not** been confirmed by the user. **`INFERRED` is never equivalent to `USER_CONFIRMED` and must never silently become it** — only an explicit `ConfirmMemory` call can make that transition (§18). |
| `DISPUTED` | LifeOS holds conflicting evidence/facts about the same subject/predicate and has not resolved which is correct. A signal to surface uncertainty later, never to be silently treated as settled (§25). |
| `STALE` | Was once current/active but is no longer the live belief — either superseded (a successor Fact is now active) or its `valid_until` passed without renewal. Retained for historical/timeline queries, excluded from "active knowledge" retrieval by default (§25). |

**Epistemic state, confidence, and sensitivity are three separate
dimensions** (frozen, §15/§16 of this run's brief) — a `USER_CONFIRMED`
fact can have `confidence = NULL` (no fabricated percentage, §16); an
`INFERRED` fact can be `sensitivity = PUBLIC`; a `SENSITIVE` fact can
be `DISPUTED`. None of the three is derived from another.

---

## 8. Relation schema — NOT `GoalRelation`

```
relations (
    id               TEXT PRIMARY KEY,       -- RelationId, UUIDv7
    from_entity_id   TEXT NOT NULL REFERENCES entities (id),
    to_entity_id     TEXT NOT NULL REFERENCES entities (id),
    relation_type    TEXT NOT NULL,
    note             TEXT,                    -- nullable
    epistemic_state  TEXT NOT NULL,
    confidence       REAL,                     -- nullable, 0.0-1.0
    sensitivity      TEXT NOT NULL,            -- ADDED beyond this run's illustrative shape, see below
    valid_from       TEXT NOT NULL,
    valid_until      TEXT,                     -- nullable
    created_at       TEXT NOT NULL
);
```

| Field | Type | Null? | Meaning | Vocabulary | FK |
|---|---|---|---|---|---|
| `id` | `TEXT` (UUIDv7) | NOT NULL | `RelationId` | — | — |
| `from_entity_id` / `to_entity_id` | `TEXT` | NOT NULL | The two Entities this edge connects, directional | — | `entities(id)`, `ON DELETE RESTRICT` both sides |
| `relation_type` | `TEXT` | NOT NULL | What kind of relationship this edge asserts | **OPEN** string, Core-validated non-empty/bounded — **deliberately NOT `GoalRelation`'s closed 11-value enum** (§2's inherited, unchanged rule) | — |
| `note` | `TEXT` | nullable | Free-text elaboration | — | — |
| `epistemic_state` | `TEXT` | NOT NULL | Same 5 frozen values as `Fact` (§7) | **CLOSED**, `CHECK IN (5 values)` | — |
| `confidence` | `REAL` | nullable | Same 0.0-1.0 bound as `Fact` | `CHECK` as on `Fact` | — |
| `sensitivity` | `TEXT` | NOT NULL | Same 4 frozen values as `Fact` (§14) | **CLOSED**, `CHECK IN (4 values)` | — |
| `valid_from` / `valid_until` | `TEXT` | NOT NULL / nullable | Temporal validity window | — | — |
| `created_at` | `TEXT` | NOT NULL | Standard timestamp | — | — |

**`sensitivity` was added to `Relation` beyond this run's illustrative
struct**, deliberately: a Relation can trivially carry sensitive
content (e.g. `SELF -- diagnosed_with --> ConditionEntity`), and this
run's own instructions frame sensitivity as a required first-class
dimension of the whole model (§15), not scoped only to `Fact`/
`Episode`/`Evidence`. Flagged here transparently, not silently added.

**`CHECK (from_entity_id <> to_entity_id)`** — self-edges rejected, for
the same structural reason `GoalRelation` rejects them (a relation from
a thing to itself is not a meaningful edge type in this model), though
this is the only rule borrowed from `GoalRelation`'s precedent — the
type vocabulary itself is not.

**No supersession chain, and no action mutates `Relation` in this
freeze.** `valid_from`/`valid_until` exist per this run's brief's
requested shape, but no `CorrectMemory`-equivalent exists for `Relation`
yet — schema is frozen ahead of its own action family, mirroring how
Milestone/Task's schema was frozen before their actions in R010, and
deliberately deferred to slice F (§26).

**Never auto-generated from, and never auto-generates, `GoalRelation`
rows.** The two layers stay fully separate — no code path in this
freeze crosses them (unchanged from R003/§5.2's existing correction).

---

## 9. Episode schema

```
episodes (
    id           TEXT PRIMARY KEY,  -- EpisodeId, UUIDv7
    title        TEXT,               -- nullable
    summary      TEXT NOT NULL,
    started_at   TEXT NOT NULL,
    ended_at     TEXT,                -- nullable
    sensitivity  TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
```

| Field | Type | Null? | Meaning | Vocabulary/bounds |
|---|---|---|---|---|
| `id` | `TEXT` (UUIDv7) | NOT NULL | `EpisodeId` | — |
| `title` | `TEXT` | nullable | Optional short label | free text, bounded |
| `summary` | `TEXT` | NOT NULL | The event/context description | `CHECK (length(trim(summary)) > 0)` |
| `started_at` | `TEXT` | NOT NULL | When the episode began | — |
| `ended_at` | `TEXT` | nullable | When it ended, if bounded | `CHECK (ended_at IS NULL OR ended_at >= started_at)` |
| `sensitivity` | `TEXT` | NOT NULL | Same 4 frozen values | **CLOSED** |
| `created_at` / `updated_at` | `TEXT` | NOT NULL | Standard timestamps | — |

**Episode is NOT chat history, NOT a `Fact`, and NOT a
`CalendarEvent`** (frozen, §12/§22 of this run's brief) — it is a
bounded historical event/context unit an `Evidence` row can point at
(e.g. "user tried a particular solution", "a project incident
happened", "an experiment was performed").

**Entity linkage via a normalized join table, not embedded JSON**
(frozen): `episode_entities(episode_id, entity_id, created_at)`,
composite primary key `(episode_id, entity_id)`, both FKs `ON DELETE
RESTRICT` (§12).

**No mutating action creates/edits `Episode` in this freeze** — schema
only, deferred to slice G (§26), same reasoning as `Relation` above.

---

## 10. Source schema

```
sources (
    id           TEXT PRIMARY KEY,  -- SourceId, UUIDv7
    source_type  TEXT NOT NULL,
    label        TEXT,               -- nullable
    reference    TEXT,               -- nullable
    captured_at  TEXT NOT NULL,
    created_at   TEXT NOT NULL
);
```

| Field | Type | Null? | Meaning | Vocabulary/bounds |
|---|---|---|---|---|
| `id` | `TEXT` (UUIDv7) | NOT NULL | `SourceId` | — |
| `source_type` | `TEXT` | NOT NULL | Where this knowledge originated | **OPEN**, Core-validated non-empty/bounded. Documented starting canonical vocabulary (not enforced): `USER_INPUT`, `CHAT_MESSAGE`, `JOURNAL_ENTRY`, `LOCAL_MODEL_EXTRACTION`, `IMPORTED_DOCUMENT`, `SYSTEM_OBSERVATION`, `CONNECTED_APP`, `WEB_RESEARCH`, `DEVICE_OBSERVATION` (R003's original six plus this run's brief's three additional examples, §2 item 7) |
| `label` | `TEXT` | nullable | Short human-readable descriptor | free text, bounded |
| `reference` | `TEXT` | nullable | A locator/pointer (e.g. a conversation id, a document path, a device identifier) — **metadata only, never raw content** | bounded length, Core-enforced |
| `captured_at` | `TEXT` | NOT NULL | When the source information was captured/observed (distinct from `created_at`, the row-insert time) | — |
| `created_at` | `TEXT` | NOT NULL | Standard timestamp | — |

**No `sensitivity` field on `Source`, deliberately** — `Source` is
designed to structurally never hold sensitive payload; Core validation
enforces `label`/`reference` stay short locator/metadata strings, never
free-form excerpts (§14's privacy split below). This is a structural
guarantee, not a per-row judgment call — the asymmetry with
`Fact`/`Relation`/`Episode`/`Evidence` (all of which have
`sensitivity`) is intentional.

---

## 11. Evidence schema

```
evidence (
    id           TEXT PRIMARY KEY,  -- EvidenceId, UUIDv7
    source_id    TEXT NOT NULL REFERENCES sources (id),
    content      TEXT,               -- nullable
    locator      TEXT,               -- nullable
    sensitivity  TEXT NOT NULL,
    created_at   TEXT NOT NULL
);
```

| Field | Type | Null? | Meaning | Vocabulary/bounds | FK |
|---|---|---|---|---|---|
| `id` | `TEXT` (UUIDv7) | NOT NULL | `EvidenceId` | — | — |
| `source_id` | `TEXT` | NOT NULL | Which `Source` this evidence came from | — | `sources(id)`, `ON DELETE RESTRICT` |
| `content` | `TEXT` | nullable, **at least one of `content`/`locator` required** | The specific supporting payload/snippet/observation (text, or a JSON-encoded structured payload — format is caller-defined, not double-validated at the DB level) | free text/JSON, Core-bounded length | — |
| `locator` | `TEXT` | nullable, see above | A pointer *within* the source (e.g. "line 42", "timestamp 00:03:15") — distinct from `Source.reference`, which points at the source as a whole | bounded | — |
| `sensitivity` | `TEXT` | NOT NULL | Independent per-row classification — **never inherited automatically from the Fact/Relation/Episode it supports** (§7's three-dimensions rule applies per row, not just per Fact) | **CLOSED**, `CHECK IN (4 values)` | — |
| `created_at` | `TEXT` | NOT NULL | Standard timestamp | — | — |

`CHECK ((content IS NOT NULL) OR (locator IS NOT NULL))` — an Evidence
row must carry at least one specific supporting detail, otherwise it is
indistinguishable from bare `Source` metadata.

**Privacy split (frozen, §13-14 of this run's brief):** `Source` stores
metadata only; **personal/sensitive excerpts belong to `Evidence`**, so
`ForgetMemory` (§19) can remove the payload-bearing `Evidence` row
without destroying reusable `Source` metadata unnecessarily. This split
is the whole reason `ForgetMemory`'s deletion algorithm (§19) never
needs to touch `Source` rows at all.

---

## 12. Supporting link tables

```
fact_evidence     (fact_id, evidence_id, created_at)      PK (fact_id, evidence_id)
relation_evidence (relation_id, evidence_id, created_at)  PK (relation_id, evidence_id)
episode_evidence  (episode_id, evidence_id, created_at)   PK (episode_id, evidence_id)
episode_entities  (episode_id, entity_id, created_at)     PK (episode_id, entity_id)
```

All four are pure many-to-many join tables (no independent semantic
payload of their own — deliberately, so evidence can support more than
one Fact/Relation/Episode without duplication, correcting R003's
embedded single-parent `Evidence.fact_id`, §2 item 6).

| Table | FKs | `ON DELETE` |
|---|---|---|
| `fact_evidence` | `fact_id → facts(id)`, `evidence_id → evidence(id)` | `RESTRICT` both sides — deletion of the join row is always an explicit step inside `ForgetMemory`'s transaction (§19), never an implicit cascade (§29's "no accidental cascade" rule) |
| `relation_evidence` | `relation_id → relations(id)`, `evidence_id → evidence(id)` | `RESTRICT` both sides |
| `episode_evidence` | `episode_id → episodes(id)`, `evidence_id → evidence(id)` | `RESTRICT` both sides |
| `episode_entities` | `episode_id → episodes(id)`, `entity_id → entities(id)` | `RESTRICT` both sides |

**Minimum required for v0.1 (per this run's brief's own instruction to
decide this explicitly, §14):** only `fact_evidence` is actually
populated by any action frozen in this run (`AddMemory`/`CorrectMemory`,
§16-17) — `relation_evidence`/`episode_evidence`/`episode_entities`
are schema-frozen now so slices F/G (§26) don't need a follow-up
migration, but remain unpopulated until those slices exist. This
mirrors §8/§9's "schema frozen ahead of its action family" pattern.

**A Fact must have at least one `fact_evidence` row (Core-enforced
invariant, not DB-enforced — SQLite cannot express "at least one child
row exists" as a `CHECK` on the parent).** `AddMemory` (§16) creates the
Fact and its first `fact_evidence` link atomically; there is no path
that creates a Fact with zero evidence in this freeze.

---

## 13. Sensitivity — minimum v0.1 representation

**Frozen, four values** (derived from the example vocabulary
`LIFEOS_SPEC.md` §4.3 already sketched — "public-ok, internal,
sensitive, never-leaves-device" — which this run formally promotes from
illustrative example to the actual frozen v0.1 closed set, resolving
the `LIFEOS_SPEC.md` §13 open question "exact data-sensitivity
classification levels... needs to be defined alongside the actual
schema, not abstractly"):

| Value | Meaning |
|---|---|
| `PUBLIC` | The user has indicated this is not sensitive; safe to reference freely, including potentially in cloud-bound context (still subject to the Privacy Firewall's minimum-necessary-context principle, §4.2 — not inherently restricted, but never exempted from minimization either). |
| `INTERNAL` | Default-tier personal information. Safe for local processing; may be included in minimized cloud context when genuinely necessary for a task, but never sent verbatim/raw and never casually exposed. |
| `SENSITIVE` | Personal information the Privacy Firewall must treat carefully (finances, journal, wellbeing, personal problems, matching `LIFEOS_SPEC.md` §4.2). Eligible for local processing and heavy summarization only; raw content must never cross the cloud boundary — only highly minimized derived signals, only when genuinely necessary. |
| `NEVER_LEAVES_DEVICE` | The strongest tier. Must never appear in any cloud-bound context or output, in any form, under any summarization — for content the user has flagged as never to be shared externally at all. |

Small and deliberately extensible-only-by-a-future-explicit-decision —
not an enormous medical/legal/privacy taxonomy, per this run's brief.
Stored as `TEXT` with a `CHECK IN (4 values)` — **closed**, matching the
representation convention already used for `MemoryType`/
`EpistemicState`/every other closed M2 enum (`LifecycleStatus`,
`TaskStatus`, `MilestoneStatus`, `GoalRelationType`).

---

## 14. Confidence semantics

**Frozen, `Option<f32>`, bounded `0.0`-`1.0`, `NULL` = no meaningful
numeric confidence was assigned** (per this run's brief §16). Explicitly
**not** equated with "the user confirmed saying this" — `epistemic_state
= USER_CONFIRMED` with `confidence = NULL` is the normal, expected shape
for a directly user-stated preference/decision; fabricating a percentage
for something the user just said in plain language would create false
scientific precision this model explicitly avoids. `INFERRED`/
`HYPOTHESIS` knowledge is where a meaningful numeric confidence is most
likely to eventually matter (model-assigned, once the local-model
extraction pipeline exists at M4+) — not required now.

---

## 15. Temporal/supersession model

**Preserved exactly from R003 (§2's "preserved unchanged" list),
scoped explicitly to `Fact` only (§9 of this run's brief — `Relation`
has a validity window but no supersession chain, §8):**

- A semantic change to a Fact never `UPDATE`s the existing row's
  semantic fields in place.
- The **old** Fact: remains present, `valid_until` is closed (set to the
  correction time), `superseded_by_fact_id` points at the successor,
  `epistemic_state` becomes `STALE` where appropriate (§17).
- The **new** Fact: a brand-new `FactId`, the new semantic value,
  `valid_from` = the change/correction time, becomes the active fact
  (`valid_until IS NULL`).
- **Not used for purely technical metadata** (e.g. a hypothetical
  future `last_used` timestamp, §24) — supersession is reserved for
  genuine semantic changes to what is believed true, never for
  bookkeeping fields.

---

## 16. Correction vs. contradiction

**Frozen distinction (§10 of this run's brief):**

- **Correction** — the user explicitly says previous knowledge was
  wrong/outdated and supplies the replacement. → create a successor
  Fact, close the predecessor, predecessor becomes `STALE`, linked via
  `superseded_by_fact_id`. This is exactly `CorrectMemory` (§17).
- **Contradiction** — LifeOS encounters conflicting evidence but the
  truth is unresolved (e.g. two independently-sourced Facts about the
  same subject/predicate disagree, and nothing has told LifeOS which
  one is right). → **do NOT silently replace one with the other; do NOT
  invent a `CorrectMemory` call to resolve it automatically.** Both
  conflicting Facts are preserved; the affected `epistemic_state`(s) may
  become `DISPUTED`; resolving the dispute later is either a future
  `ConfirmMemory` call (picking one, §18) or a future `CorrectMemory`
  call (explicitly correcting one) — but the **detection and marking**
  of a contradiction is not itself one of the four frozen actions in
  this run; no automatic contradiction-detection mechanism is
  implemented or frozen here (candidate for the M6 Personal Intelligence
  layer, §23, operating only as a proposer, never a silent resolver).

---

## 17. Action: `AddMemory`

**Purpose:** create durable Personal Knowledge — a new `Fact`, its
required `Source` (new or an existing one being reused), its required
`Evidence`, and the `fact_evidence` link, all atomically.

**Confirmation floor:** `EXPLICIT_ADD` (frozen already in
`LIFEOS_SPEC.md` §5.5/R003 §F — confirmed, not re-decided, §2).
Directly-requested ("remember that...") executes immediately with
inline acknowledgement + Undo. `Provenance::Inferred` (a model-proposed
candidate) raises the effective class to `INFERRED_CHANGE` via the
existing `effective_class = max(type_floor, provenance_class)` rule
(unchanged, R003 §F) — **always blocks on explicit confirmation, zero
durable writes before confirmation.** An inference must never silently
become durable `USER_CONFIRMED` knowledge — note it also never becomes
durable at all (of any epistemic state) until confirmed; a rejected
inferred candidate leaves no row behind, exactly like every other M2
`INFERRED_CHANGE` action proven end-to-end for `CreateGoal`
(`req_h5`).

**Payload (conceptual shape, not final Rust syntax):**

```
AddMemoryPayload {
    subject_entity_id: Option<EntityId>,   // None -> resolves to the frozen SELF entity
    predicate: String,
    value: FactValue,                       // enum: Scalar(String /* JSON */) | EntityRef(EntityId)
    memory_type: MemoryType,
    epistemic_state: EpistemicState,        // typically USER_CONFIRMED (explicit) or INFERRED (model-proposed)
    confidence: Option<f32>,
    sensitivity: Sensitivity,               // REQUIRED, no silent default (privacy is architectural, CLAUDE.md principle 3)
    source: SourceRef,                      // enum: New(NewSourceData) | Existing(SourceId)
    evidence: NewEvidenceData,               // always newly created, never reused
}
```

`FactValue` is a Rust-level sum type mapped down to the two nullable
`(value_json, object_entity_id)` columns at the storage layer — Core
never needs to separately validate "exactly one was provided" since the
enum makes the other case unrepresentable (an improvement on the raw
SQL shape, same one-semantic-object outcome, §5).

**Validation order (deterministic, before any write, matching every
other M2 action's discipline):** resolve/validate `subject_entity_id`
(exists, defaulting to SELF) → validate `predicate` (non-empty,
bounded) → validate `value` (JSON well-formed if `Scalar`; entity exists
if `EntityRef`) → validate `memory_type`/`epistemic_state`/`sensitivity`
are members of their closed sets → validate `confidence` bounds if
present → resolve `source` (exists, if `Existing`) → validate `evidence`
(at least one of `content`/`locator`). Any failure → typed
`ValidationError`, zero writes.

**Mutation semantics:** one atomic transaction creates, in order: (1)
the `Source` row if `New`, (2) the `Fact` row, (3) the `Evidence` row,
(4) the `fact_evidence` link row, (5) the `fact_audit_log` row
(`action_type = 'AddMemory'`, `previous_snapshot = NULL` — this is a
create, matching the existing `Create*` convention of never routing a
create through `serialize_required_snapshot`, R015 §10). Any failure at
any step rolls back the entire transaction — reusing M2's established
discipline exactly (§20).

---

## 18. Action: `CorrectMemory`

**Purpose:** "the retained semantic knowledge is wrong/outdated;
replace it while preserving history" — never an in-place semantic edit.

**Confirmation floor:** `IMPORTANT_CHANGE` (frozen already, confirmed
not re-decided, §2).

**Payload:**

```
CorrectMemoryPayload {
    predecessor_fact_id: FactId,
    value: FactValue,                        // required - the new value
    memory_type: Option<MemoryType>,          // None -> inherit predecessor's
    confidence: Option<f32>,                  // None -> inherit predecessor's
    sensitivity: Option<Sensitivity>,         // None -> inherit predecessor's
    source: SourceRef,
    evidence: NewEvidenceData,
}
```

`subject_entity_id` and `predicate` are **not** part of the payload —
they are always inherited unchanged from the predecessor. Changing
*what* a Fact asserts (a different subject or predicate entirely) is
definitionally a different assertion — that is `AddMemory` for a new
Fact, not a correction of this one (§5's mutability contract).

**Validation order:** predecessor exists and is currently active
(`valid_until IS NULL`) — correcting an already-superseded/`STALE` Fact
is rejected (there is nothing "current" left to correct; the caller
should `AddMemory` fresh if they mean to re-establish a historical
value) → validate the new `value`/`memory_type`/`confidence`/
`sensitivity` exactly as `AddMemory` does → resolve `source`/`evidence`.

**Mutation semantics (one atomic transaction):** (1) create the new
`Source` if `New`, (2) insert the successor `Fact` row
(`subject_entity_id`/`predicate` copied from predecessor,
`epistemic_state` = the caller's requested starting state — typically
`USER_CONFIRMED` for a user-driven correction, `valid_from` = now), (3)
create the `Evidence` row + `fact_evidence` link for the successor, (4)
update the **predecessor** row: `valid_until` = now (matching the
successor's `valid_from` exactly — no gap, no overlap),
`superseded_by_fact_id` = the successor's id, `epistemic_state` =
`STALE`, `updated_at` = now, (5) insert **one** `fact_audit_log` row
documenting the correction, `previous_snapshot` = the predecessor's
pre-correction serialized state (mandatory, non-`NULL` — reusing
`serialize_required_snapshot`'s guarantee, R015 §10/R016 §10: failure to
serialize means zero mutation, not a silently missing snapshot). Any
failure rolls back the whole transaction — predecessor and successor
both live or neither does.

---

## 19. Action: `ConfirmMemory`

**Purpose:** "the semantic value already stored/candidate is correct
according to the user" — an epistemic-state confirmation, never a
semantic-value change.

**Confirmation floor:** `EXPLICIT_ADD` (frozen already in
`LIFEOS_SPEC.md` §5.5/R003 §F's type-floor table — confirmed, not
re-decided, §2).

**Payload:**

```
ConfirmMemoryPayload {
    fact_id: FactId,
}
```

**Decision (frozen, §21 of this run's brief): updates the same Fact row
in place, does NOT create a new temporal Fact.** Epistemic-state
confirmation is metadata about an existing assertion, not a new
assertion — matching the mutability contract (§5): `epistemic_state`
is one of the fields explicitly allowed to change in place.

**Validation order:** the target Fact exists → is currently active
(`valid_until IS NULL` — confirming a superseded/`STALE` fact is
rejected; "reviving" old history is out of scope for this action, §5) →
its current `epistemic_state` is not already `USER_CONFIRMED`
(confirming an already-confirmed fact is rejected as a no-op, matching
the established "reject a no-op update" convention used throughout M2,
e.g. `req_up11`'s "no fields to update" rejection).

**Mutation semantics:** one atomic transaction: update
`epistemic_state = 'USER_CONFIRMED'`, `updated_at = now` on the target
Fact; insert a `fact_audit_log` row (`action_type = 'ConfirmMemory'`,
`previous_snapshot` = mandatory pre-confirmation serialized state, same
discipline as `CorrectMemory`). Any Fact whose `epistemic_state` was
`INFERRED`, `OBSERVED`, or `DISPUTED` may be confirmed this way —
confirming a `DISPUTED` fact is this action's resolution path for a
contradiction (§16), though it does not automatically touch any
*sibling* conflicting Fact.

---

## 20. Action: `ForgetMemory` — special true deletion

**Purpose:** the user's explicit right-to-be-forgotten instruction for
one targeted piece of knowledge — **not** ordinary soft-delete
(unchanged from R003/`LIFEOS_SPEC.md` §5.4's existing correction, §2).

**Confirmation floor:** `DELETE` (frozen already, confirmed not
re-decided, §2). Always a blocking confirmation; **no Undo is offered
past that confirmation step** — offering Undo would mean secretly
retaining exactly the data the user asked to have removed.

**Payload:**

```
ForgetMemoryPayload {
    fact_id: FactId,
}
```

Targets exactly one Fact. **No broader "erase my entire LifeOS" mode in
this action** (per this run's brief §22) — a bulk/cascading forget
operation, if ever needed, is a separate, future, explicitly-designed
action.

**Deterministic, privacy-safe deletion algorithm (frozen, one atomic
transaction):**

1. Validate the target Fact exists — else `NotFound`, zero mutation.
2. Collect every `evidence_id` currently linked to this `fact_id` via
   `fact_evidence`.
3. Delete the `fact_evidence` link rows for this `fact_id`.
4. For each collected `evidence_id`: if it now has **zero** remaining
   rows across `fact_evidence`, `relation_evidence`, and
   `episode_evidence` (i.e. genuinely orphaned — v0.1 has no
   "explicitly shared/reusable" flag, so any now-unreferenced Evidence
   is deleted), `DELETE FROM evidence WHERE id = evidence_id`.
   Otherwise (still referenced by another Fact/Relation/Episode), leave
   it untouched.
5. **Never** delete the subject/object `Entity` rows — Entities are
   shared nodes that may be referenced by many other Facts/Relations/
   Episodes; forgetting one Fact about an Entity never deletes the
   Entity itself.
6. **Never** delete `Source` rows, regardless of reference count —
   `Source` structurally never holds the forgotten payload (§10/§13's
   privacy split), so retaining it carries no privacy risk even if it
   becomes fully unreferenced. A future dedicated cleanup job MAY prune
   fully-orphaned `Source` rows later; that is explicitly out of scope
   here, not silently assumed.
7. `DELETE FROM facts WHERE id = fact_id` — the actual permanent
   removal.
8. Insert exactly one `fact_audit_log` row: `fact_id` = the now-deleted
   id (the residual, non-sensitive marker — see below), `action_type =
   'ForgetMemory'`, **`previous_snapshot` is ALWAYS `NULL`, with no
   exception** (§21), `actor`/`provenance`/`confirmation_class`/
   `created_at` as usual.
9. Commit. Any failure at any step → full rollback, zero partial
   deletion, typed `StorageError` — never a report of `Success` with a
   partially-forgotten Fact.

**Residual audit marker (frozen, resolving the brief's "id OR
non-reversible hash" either/or):** the marker is the Fact's own
`FactId` — a random UUIDv7 with no derivation from the forgotten
content, so it is already safe/non-sensitive on its own; no additional
hashing is needed. **The original forgotten text/value never persists
anywhere** — not in the audit row, not in a tombstone, not in a
`deleted_at`-style metadata field holding the old text.

**Schema consequence (frozen, new to this document):**
`fact_audit_log.fact_id` is a **plain `TEXT NOT NULL` column, NOT an
enforced foreign key** — deliberately, unlike every other M2 audit
table's parent-id column (e.g. `goal_audit_log.goal_id REFERENCES
goals(id)`, which works because Goals are never hard-deleted). Since
`ForgetMemory` genuinely removes the `facts` row, an FK would either
block the deletion (`RESTRICT`) or destroy the residual marker
(`CASCADE`) — neither is acceptable, so this one column is intentionally
left DB-unconstrained, with correctness guaranteed by Core (the same
transaction that deletes the Fact writes its own former id into this
column) rather than by a database constraint. Flagged explicitly so a
future developer does not "fix" this by adding an FK and accidentally
breaking `ForgetMemory`.

---

## 21. Audit/Undo/privacy behavior

**Normal Personal Knowledge mutations** (`AddMemory` create,
`CorrectMemory`, `ConfirmMemory`) are auditable exactly like every
other M2 action: atomic mutation+audit transaction, mandatory
`previous_snapshot` for update-shaped actions via
`serialize_required_snapshot` (R015/R016 — zero mutation if
serialization itself fails), future Undo can read the audit trail.

**`ForgetMemory` is the one deliberate, explicitly-documented exception
(§20 above), frozen here so it cannot be silently forgotten by a future
implementer:** it must **never** call the ordinary mandatory-snapshot
helper (`serialize_required_snapshot`) for its own audit row —
`previous_snapshot` is hard-coded `NULL` for this action type,
unconditionally. Reusing the normal Update* audit pattern here would
literally retain the supposedly-forgotten content inside the audit log,
defeating the entire purpose of the action. A future implementation
should consider a dedicated, differently-named helper (or an explicit
inline `None`) specifically to make this impossible to get wrong by
copy-paste from `CorrectMemory`'s call site.

---

## 22. Retrieval / Context Pack contract

**Frozen architectural target for a future `get_relevant_memory`
capability — no embeddings, no retrieval code, in this run (§24):**

1. Identify relevant Entities (from the query/context, e.g. the current
   Goal/Project in view, or entities mentioned in a chat turn).
2. Retrieve **active** exact Facts for those Entities (`valid_until IS
   NULL`, and `STALE` excluded unless explicitly requested, §25).
3. Traverse relevant graph `Relation`s, shallow initially (1-2 hops).
4. Include relevant `Episode`s.
5. Keyword/text retrieval where useful.
6. Semantic/vector retrieval **later**, once embeddings exist — **not
   now** (§24).
7. Rank results.
8. Produce a compact **Context Pack**.

**The Context Pack, not raw DB rows, is what ever reaches a model.**
This feeds the existing Privacy Firewall pipeline (`LIFEOS_SPEC.md`
§4.2-4.3, unchanged) → a minimum sanitized Conversation Brief → cloud
Luna. No raw `Fact`/`Evidence` row is ever hand-delivered to a cloud
model; the Context Pack is itself already the minimization step this
retrieval contract exists to produce.

**Active vs. historical vs. disputed (frozen, §25 of this run's
brief):**

- **Active knowledge:** `valid_until IS NULL` and `epistemic_state <>
  'STALE'` (a fact can theoretically be marked `STALE` without a
  formal `valid_until` close in an edge case — the query filters both
  conditions, not just one, to stay correct either way) — the default
  retrieval scope.
- **Historical knowledge:** superseded/closed Facts, retained
  specifically for timeline/history queries — never the default
  retrieval scope, only surfaced when a caller explicitly asks for
  history.
- **`DISPUTED`:** must never be silently treated as settled current
  truth by retrieval — a Context Pack including a `DISPUTED` fact must
  carry that flag through, not present it as plain fact.
- **`ForgetMemory`'d knowledge:** not historical — actually gone, never
  retrievable by any path, by construction (§20).

---

## 23. Personal Knowledge vs. domain authority

**Frozen rule, unchanged from this run's brief, consistent with
everything M2 has done so far:** Personal Knowledge does **not** replace
domain truth. `Project`/`Goal`/`Task`/`Milestone` tables remain
authoritative for their own structured state. Personal Knowledge may
*reference* them through `Entity.domain_ref_type`/`domain_ref_id`
(§4/§24), but:

- A `Fact` must never become a duplicate authoritative `Project` record.
- `Relation` must never become `GoalRelation` (§8, unchanged).
- `Episode` must never become `CalendarEvent`.
- Personal Knowledge code never writes domain SQL directly — any future
  synchronization between the two happens through Core/actions/
  capabilities, exactly like every cross-module interaction already
  established in M2 (e.g. `project_repo::goal_exists` reused by
  `milestone_repo`/`task_repo`'s callers, never duplicated SQL).

No parallel source of truth is created by this freeze.

---

## 24. Entity references to domain objects

**Frozen (§27 of this run's brief):** `Entity.domain_ref_type` (open
string) + `Entity.domain_ref_id` (text) is the chosen shape — optional
typed external-reference metadata, not a duplication of the domain
object's own fields (§4).

**Deliberately NOT an enforced SQL foreign key.** A raw FK is
technically possible at the SQLite level (Rust module privacy and DB
foreign keys are unrelated axes), but is rejected here because:
`domain_ref_type` varies across an open, growing set of domain tables
(`Goal`, `Project`, `Task`, `Milestone`, and future undesigned domains
like Money/Calendar) — a single nullable column cannot `REFERENCES` a
different table depending on a sibling column's value in standard SQL,
and giving up FK enforcement to keep the vocabulary open is the correct
trade-off here, matching §23's "Personal Knowledge does not write
domain SQL directly" principle: this module should not need
compile-time/schema-time knowledge of every domain table that will ever
exist.

**Validity is a Core-level concern instead:** when a caller supplies a
`domain_ref`, Core validates the referenced row currently exists using
the *existing* domain module's own read function (e.g.
`project_repo::project_exists`), exactly the same pattern already used
for `Project.goal_id`/`Milestone.project_id`/`Task.project_id`/
`Task.goal_id` (§8e/§4i/§4j of `PROJECT_STATE.md`) — reused, not
reinvented. **Accepted limitation, documented not hidden:** if a
referenced domain row were ever hard-deleted later, the `domain_ref`
would go stale with no DB-level signal — currently moot, since no M2
domain entity has a hard-delete action at all (§2's inherited
`ON DELETE RESTRICT`-everywhere discipline). No cross-domain wiring is
implemented in this run.

---

## 25. DB integrity rules (for the future implementation)

Frozen expectations, no migration created this run:

- UUIDv7 typed IDs for every table (`EntityId`, `FactId`, `RelationId`,
  `EpisodeId`, `SourceId`, `EvidenceId`), matching every existing M2
  entity.
- `PRAGMA foreign_keys = ON` via the existing single `open_encrypted`
  primitive (R015 §4e's precedent) — no second connection-opening path.
- `ON DELETE` policy explicitly chosen per FK, not defaulted
  thoughtlessly: `RESTRICT` everywhere except the two deliberate,
  explicitly-documented non-FK columns (`entities.domain_ref_id`,
  `fact_audit_log.fact_id`, §24/§20).
- `CHECK` constraints for every **closed** vocabulary
  (`MemoryType` × 11, `EpistemicState` × 5, `Sensitivity` × 4,
  `fact_audit_log.action_type` IN the four Memory action names) — never
  for an **open** one (`entity_type`, `predicate`, `relation_type`,
  `source_type` stay plain `TEXT`, Core-validated only).
- `confidence` bounds (`NULL` or `0.0..=1.0`) on both `facts` and
  `relations`.
- Temporal invariants where practical: `episodes.ended_at >=
  started_at` when both present; `facts.superseded_by_fact_id <> id`.
- Exactly one `is_self = 1` row in `entities` (partial unique index,
  §4).
- Exactly one semantic object per `Fact` (`value_json` XOR
  `object_entity_id`, §5).
- No accidental cascade that destroys unrelated/shared knowledge — every
  join-table FK is `RESTRICT`, and `ForgetMemory`'s deletion is entirely
  explicit, step-by-step, inside Core (§20), never delegated to a DB
  `ON DELETE CASCADE`.

No `V7` migration is created in this run (§37) — the first
implementation slice uses the next available migration number *at that
time* (currently `V7`, but this is not reserved now — reserving
multiple future numbers ahead of need is explicitly against this run's
instruction).

---

## 26. Intentionally deferred features

Per this run's brief §33's explicit classification requirement:

| Feature | Classification | Why |
|---|---|---|
| Locked/pinned memory | **B — deferred** | Never frozen anywhere prior to this run; no usage data exists yet to design it well. Candidate for a future freeze once real usage patterns exist (mirrors how `Relation.relation_type`'s constrainedness was deferred to M6). |
| `last_used` / usage tracking | **B — deferred** | Same reasoning; also explicitly NOT to be modeled via temporal supersession (§15) even once added — it is bookkeeping metadata, not a semantic fact. |
| Memory importance/weight | **B — deferred** | Same reasoning — premature without real retrieval-quality data to calibrate against. |
| Entity alias/resolution ("these are the same real thing") | **B — deferred** | Explicitly named as deferred in this run's brief §6; a future `SAME_AS`-style `Relation` is one plausible direction, not decided. |
| Constrained `relation_type`/`predicate` vocabularies (ontology management) | **B — deferred**, unchanged from R003 | This run's brief explicitly says "do NOT attempt to solve ontology management in R017" (§8). |
| Embeddings/semantic retrieval | **B — deferred** | §22/§24 of this run's brief — a future *index* over the canonical relational data, never the source of truth; must be rebuildable from canonical knowledge, and any `ForgetMemory`'d knowledge's derived embedding must eventually be deleted/rebuilt too. No vector DB chosen or introduced in M2. |
| `CorrectMemory`/supersession-equivalent for `Relation`/`Episode` | **B — deferred** | No action mutates `Relation`/`Episode` at all yet (§8/§9) — their own action family needs its own freeze pass before slices F/G. |
| Bulk/cascading "forget everything about X" | **B — deferred** | Explicitly out of scope for `ForgetMemory` itself (§20/§22 of this run's brief). |
| Backdating `Fact.valid_from` to a historical date at creation time | **B — deferred** | `AddMemory`'s `valid_from` defaults to "now"; recording *when something historically happened* (as opposed to when LifeOS learned it) belongs in `Evidence.content`/a future `Episode`, not a backdated `valid_from`, for v0.1. |
| Memory UI (inspecting value/source/evidence/history) | **B — deferred (explicitly out of scope, §32)** | The data model is built to support it (§22's retrieval contract, §5's supersession chain), but no UI is designed or built in this run. |

Nothing above was silently added to the schema despite being part of
the wider product vision — each is named and classified, not
speculatively scaffolded.

---

## 27. Qwen/Luna boundaries (architectural responsibility only)

**No model integration in this run.** Frozen responsibility split,
consistent with `LIFEOS_SPEC.md` §4.1/§2's existing guarded-action-layer
principle:

- **Local Qwen** (M4+) may later: extract memory candidates, classify
  `memory_type`, suggest entities/predicates, identify possible
  contradictions, summarize Episodes, help retrieval/context selection.
  It **never** directly mutates Personal Knowledge — it proposes typed
  candidates (i.e. it would call `AddMemory`/`CorrectMemory` with
  `Provenance::Inferred`, exactly like any other actor), and
  deterministic Core validates and, per the confirmation policy, blocks
  until the user confirms (§17).
- **Cloud Luna** never gets raw SQL access, never writes Personal
  Knowledge directly, receives only privacy-filtered Context Pack
  output (§22), and may propose typed actions that still go through the
  exact same Core validation path as any other actor — no shortcut.

---

## 28. Risks / open questions

- **`AddMemory`'s SELF-entity bootstrap mechanism** (lazy-on-first-call
  vs. eager-at-DB-init) is not decided here — an implementation detail
  for slice A (§29), not an architectural freeze concern.
- **`Fact.value_json`'s exact max-length bound** is not numerically
  fixed here (§5) — deferred to implementation time, matching how every
  other M2 free-text bound was actually set only once the entity was
  implemented, not invented speculatively during a freeze pass.
- **Cross-Fact contradiction *detection*** (as opposed to the
  `DISPUTED` state's *existence*, which is frozen) is not designed in
  this run — likely an M6 Personal Intelligence concern, proposing
  candidates, never silently auto-resolving (§16/§27).
- **`fact_audit_log`'s non-FK `fact_id` column** (§20) is a deliberate,
  narrow, well-justified exception to M2's audit-log convention — worth
  a one-line comment at the eventual migration site so it doesn't look
  like an oversight to a future reviewer.
- **Whether `Source`/`Evidence` will eventually need their own read
  capabilities** (`get_source`, `get_evidence_for_fact`, etc.) is not
  decided — likely needed once slice A/B (§29) is implemented, following
  the same "read side stays narrow, add only when a real caller needs
  it" discipline already established for every other M2 entity.

---

## 29. Recommended implementation slicing

**Not implemented in this run.** Recommended safest order, evaluated
against dependencies (not a blind copy of this run's brief's suggested
sequence — checked and confirmed correct):

| Slice | Contents | Acceptance criteria |
|---|---|---|
| **A** | `Entity` + `Source` + `Evidence` + `Fact` foundation (tables, the SELF-entity invariant, the one-semantic-object `CHECK`, `fact_evidence`) — no actions yet, just the schema + read-side (`get_fact`, `get_entity`) | Migration applies cleanly on top of V6; SELF entity uniqueness proven with a raw-SQL duplicate-insert-rejected test (matching every other M2 invariant's proof style); one-semantic-object `CHECK` proven both directions |
| **B** | `AddMemory` + its read-side | A full `AddMemory` call atomically creates Fact+Source(new)+Evidence+fact_evidence+audit row; forced-audit-failure rollback test (matching every prior M2 entity's proof, R006/R007/etc.); `Provenance::Inferred` blocks with zero writes, confirmed resubmission succeeds (matching `req_h5`'s established pattern) |
| **C** | `CorrectMemory` + temporal supersession | Predecessor closes correctly (`valid_until`/`superseded_by_fact_id`/`STALE`), successor is independently readable, both survive a forced-audit-failure rollback together (neither half-applies) |
| **D** | `ConfirmMemory` | In-place `epistemic_state` transition proven for `INFERRED`/`OBSERVED`/`DISPUTED` → `USER_CONFIRMED`; rejecting an already-confirmed or non-active target fact proven |
| **E** | `ForgetMemory` true-deletion semantics | The full §20 algorithm proven: Fact gone, orphaned Evidence gone, shared Evidence retained, Entity/Source never touched, `previous_snapshot` provably `NULL`, residual marker present and non-sensitive (a dedicated marker-content test, matching R016's `req_snap1` marker-based proof pattern) |
| **F** | `Relation` (schema already frozen here, §8) — needs its own dedicated action-freeze pass first (no `LinkKnowledge`-equivalent action exists yet) | Out of this run's scope to define further |
| **G** | `Episode` (schema already frozen here, §9) — same, needs its own action-freeze pass | Out of this run's scope to define further |
| **H** | Retrieval/Context Pack foundation (§22) | Out of this run's scope to define further |

**Recommended exact next implementation slice: A, immediately followed
by B** (Entity+Source+Evidence+Fact foundation, then `AddMemory`) —
these two are the minimum needed before *any* Personal Knowledge data
can exist at all, and `AddMemory` is the only Memory action with no
dependency on another Memory action already having run (`CorrectMemory`/
`ConfirmMemory`/`ForgetMemory` all require an existing Fact to target).
This mirrors the exact dependency reasoning that made Goal → Project →
GoalRelation → Milestone → Task the correct real order in M2 so far —
each slice needs the previous one to already exist, never the reverse.

---

## Addendum (R018, 2026-09-18) — architecture consistency corrections

A dedicated post-freeze review
(`docs/reports/R018_M2_PERSONAL_KNOWLEDGE_ARCHITECTURE_CONSISTENCY_HARDENING_2026-09-18.md`)
found eleven consistency/privacy gaps in this report before any `V7`
migration or implementation slice was written, all fixed in R018:

- The SELF partial unique index (§4 above) guarantees *at most one*
  row, never existence — R018 freezes a Core-level `ensure_self_entity`
  bootstrap to guarantee *at least one*, together giving *exactly one*.
- `AddMemory` (§17 above) had no path to create a *new* Entity — every
  subject/object reference required one to already exist. R018 freezes
  a typed `EntityInputRef` (`SelfEntity`/`Existing`/`New`) usable
  inside `AddMemory` (and `CorrectMemory`'s value), with new-Entity
  creation folded into the same atomic transaction.
- `CorrectMemory`'s payload (§18 above) had no `epistemic_state`
  field, but its mutation prose said the successor's epistemic state
  was "the caller's requested starting state" — a direct
  contradiction. R018 freezes that a `CorrectMemory` successor is
  *always* `USER_CONFIRMED`, with a Personal-Knowledge-specific
  confirmation overlay ensuring an inferred correction still blocks
  before writing (the generic `effective_class` formula alone would
  not have forced that block for this specific action's type-floor).
- `CorrectMemoryPayload.confidence` (§18 above) used `Option<f32>`,
  which cannot express "clear the old confidence back to `NULL`."
  R018 freezes `FieldPatch<f32>` instead, matching M2's established
  nullable-field convention.
- `CorrectMemory`/`ConfirmMemory`'s "is this Fact active" checks (§18-19
  above) validated only `valid_until IS NULL`, not the full two-part
  active-Fact definition this report's own §22 already used elsewhere.
  R018 freezes one canonical `is_active_fact` predicate, reused
  everywhere.
- `ConfirmMemory` confirming a `DISPUTED` Fact (§19 above) was
  described as "a resolution path for a contradiction" — overstated.
  R018 clarifies it resolves only the one target assertion; sibling
  disputes remain, and retrieval must keep surfacing them.
- **Critical:** `ForgetMemory` (§20-21 above) only guaranteed its own
  *new* audit row had no payload — it never accounted for pre-existing
  `CorrectMemory`/`ConfirmMemory` audit rows for the same Fact, which
  this report's own §18-19 require to carry mandatory, non-`NULL`,
  payload-bearing snapshots. R018 freezes a mandatory scrub step
  (`UPDATE fact_audit_log SET previous_snapshot = NULL WHERE fact_id =
  ... AND previous_snapshot IS NOT NULL`) before the Fact is deleted.
- **Critical:** `facts.superseded_by_fact_id ... REFERENCES facts(id)`
  (§5 above) has no `ON DELETE` clause, defaulting to `RESTRICT` —
  meaning `ForgetMemory` on any Fact that was ever someone's successor
  would be rejected outright by the database. R018 freezes an explicit
  step clearing every inbound reference before deletion, deliberately
  *not* rewiring predecessors to point at the forgotten Fact's own
  successor.
- `ForgetMemory`'s Evidence handling (§20 above) retained Evidence
  still linked elsewhere — but that Evidence could itself contain the
  forgotten payload. R018 freezes unconditional deletion of all
  Evidence directly linked to the forgotten Fact, accepting that this
  may remove supporting evidence from other, still-retained knowledge.
- `Source` (§10, §20 above) was frozen with no `sensitivity` field and
  described as carrying "no privacy risk" and never deleted. R018
  corrects this: `Source` gains a `sensitivity` column (metadata can
  still be sensitive), and `ForgetMemory` now deletes orphaned Source
  rows while retaining genuinely shared ones.
- This report's field-level freeze (§5, §28 above) still left several
  fields as merely "bounded... exact number set at implementation
  time." R018 freezes exact numeric maximum lengths for every bounded
  Personal Knowledge field now.

See R018 for the full corrected design, the rewritten `ForgetMemory`
transaction algorithm, and updated Slice A/B acceptance criteria.
R018 is authoritative wherever it corrects this report.
