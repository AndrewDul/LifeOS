# R030 — M2 Personal Knowledge Episode Action-Family Architecture Freeze

**Type:** Documentation / architecture only. No production code, schema,
or tests changed in this run.

**Status:** Architecture CLOSED for v0.1 implementation. Implementation
is blocked on one small, additive V9 migration — `episode_audit_log`
only, exactly (mirroring R025's precedent for Relation/V8).

---

## 1. Pre-run verified state

CONFIRMED FACT: clean working tree at commit `7ed16e3` ("test: harden
ForgetRelation deletion proofs") before this run began. 543 tests
passing. Read in full: `CLAUDE.md`, `docs/PROJECT_STATE.md`,
`docs/LIFEOS_SPEC.md`, R017–R029 (including every post-run hardening
addendum). Inspected live code: `V7__create_personal_knowledge.sql`'s
`episodes`/`episode_evidence`/`episode_entities` tables directly (not
from memory), `EpisodeId`, `EPISODE_TITLE_MAX_LEN`/
`EPISODE_SUMMARY_MAX_LEN`, `Sensitivity`, `EntityInputRef`/`SourceRef`/
`NewEntityData`/`NewSourceData`/`NewEvidenceData`, `Evidence`/`Entity`/
`Source`, `Fact`/`Relation`'s full action patterns, `FieldPatch<T>`, the
confirmation-class machinery, `memory_action_requires_confirmation`, the
now-unbroken seven-dedicated-audit-table precedent, and
`delete_evidence_links_for_evidence`/`delete_source_if_orphan`/
`delete_entity_if_orphan_non_self` (all three already fully generic,
confirmed reusable unchanged).

## 2. Exact current V7 Episode schema (verified against the live migration)

```sql
CREATE TABLE episodes (
    id           TEXT PRIMARY KEY,
    title        TEXT,
    summary      TEXT NOT NULL CHECK (length(trim(summary)) > 0),
    started_at   TEXT NOT NULL,
    ended_at     TEXT,
    sensitivity  TEXT NOT NULL CHECK (sensitivity IN ('PUBLIC', 'INTERNAL', 'SENSITIVE', 'NEVER_LEAVES_DEVICE')),
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);

CREATE TABLE episode_evidence (
    episode_id   TEXT NOT NULL REFERENCES episodes (id) ON DELETE RESTRICT,
    evidence_id  TEXT NOT NULL REFERENCES evidence (id) ON DELETE RESTRICT,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (episode_id, evidence_id)
);

CREATE TABLE episode_entities (
    episode_id   TEXT NOT NULL REFERENCES episodes (id) ON DELETE RESTRICT,
    entity_id    TEXT NOT NULL REFERENCES entities (id) ON DELETE RESTRICT,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (episode_id, entity_id)
);
```

Verified directly, not assumed: `title` nullable, no length CHECK at the
DB layer (Core-side bound only); `summary` `NOT NULL` with a non-blank
`CHECK`; `ended_at` nullable with `ended_at >= started_at` when present
(no upper/lower temporal bound versus `created_at`/`updated_at` at the
DB layer — that is Core's job, frozen below); `sensitivity` the same
frozen 4-value closed set every Personal Knowledge table uses;
**`episodes` already has `updated_at`** (unlike `relations`, which
notably lacks one — R025's `updated_at` gap does not exist for
Episode); no `epistemic_state`, no `confidence`, no supersession
pointer of any kind. Both join tables are plain M:N with composite
primary keys and `ON DELETE RESTRICT` on both foreign keys — identical
shape to `fact_evidence`/`relation_evidence`. `episode_entities` stores
membership only — no role/type/position column. `EpisodeId` (typed
UUIDv7 newtype) and `EPISODE_TITLE_MAX_LEN = 200`/
`EPISODE_SUMMARY_MAX_LEN = 4000` already exist in `domain/mod.rs`. No
production write path reaches any of the three tables — schema only
since R019/R020, confirmed by the migration's own comment ("Episode's
own action family is deferred to a future dedicated freeze pass, slice
G" — this report is that pass).

## 3. Episode semantic definition and boundaries

**An Episode is a bounded real-world historical (or currently-ongoing)
event or context unit in the user's Personal Knowledge** — something
that actually happened or is happening, recorded with a start (and
optionally an end), a narrative summary, optional participating
Entities, and supporting Evidence.

An Episode is explicitly **NOT**:

- **Raw chat history / a chat message.** A message is conversational
  input, not a canonical knowledge record. It may later be *distilled*
  into a Fact/Relation/Episode by a future extraction step (M6,
  Qwen/Luna proposing typed `Provenance::Inferred` candidates through
  the normal Core action layer — never writing state directly, per
  CLAUDE.md's "cloud LLM never owns state"), but the raw message itself
  is never stored as one of these four canonical types.
- **A Fact.** A Fact is a single structured attribute
  (`subject_entity_id`/`predicate`/`value`) — a proposition about one
  Entity, not a bounded, narratively-described occurrence.
- **A Relation.** A Relation is a directional edge between two Entities
  — a standing connection, not a time-bounded event.
- **A CalendarEvent.** CalendarEvent (LIFEOS_SPEC.md §6/§9.5, still
  undesigned/unbuilt) is for **future-scheduled** items — "on Friday at
  2 PM I am going to view a van" is explicitly a `CreateCalendarEvent`
  example already in the spec. Episode is retrospective/ongoing-real
  knowledge, never a scheduling instrument.
- **An audit event.** `*_audit_log` rows are Core-internal action
  history, invisible to the user as knowledge objects; an Episode is a
  first-class Personal Knowledge object the user can be shown, corrected,
  and forgotten.
- **A free-form diary blob.** Episode is structured (bounded time,
  optional entities, Evidence-backed), not an unstructured prose dump —
  `summary` carries the narrative content, but the surrounding structure
  (time bounds, entity links, Evidence) is what makes it a *canonical
  knowledge record* rather than a diary entry.

Concrete examples: a user performed an experiment; a project incident
occurred; a meeting actually happened; a trip occurred; a solution was
tried (as a rich, timed, multi-entity event — see the Fact-vs-Episode
boundary example in §45); an important real-world interaction happened.

**Frozen: an Episode may represent BOTH ongoing (`ended_at = None`) and
purely historical completed (`ended_at = Some(...)`) events.** Nothing
in the schema or the frozen action family forces a choice between the
two — `ended_at` being nullable already expresses "ongoing/unknown-end"
naturally, and `UpdateEpisode` is exactly the mechanism by which an
ongoing Episode's `ended_at` gets set once the real-world event
concludes (§12).

## 4. Temporal semantics

Five distinct timestamps, never conflated:

- **`started_at`** — real-world event start. **Caller-supplied**, not
  Core-generated (a deliberate, important divergence from `Fact.valid_from`/
  `Relation.valid_from`, both of which are always `meta.requested_at`
  with no caller input/no backdating allowed — see below).
- **`ended_at`** — real-world event end, nullable = ongoing or unknown.
  Also caller-supplied when present.
- **`created_at`** — when the canonical Episode row was first stored.
  Always `meta.requested_at`, never caller-supplied.
- **`updated_at`** — when the Episode row was last successfully
  mutated. Always `meta.requested_at` on a successful `UpdateEpisode`,
  never caller-supplied.
- **`relation_audit_log`-style `occurred_at`** (future
  `episode_audit_log.occurred_at`) — when the specific Core action
  itself ran. Always `meta.requested_at`.

**Why `started_at`/`ended_at` are caller-supplied, unlike Fact/Relation's
`valid_from`:** Episode's entire purpose is recording *when something
happened in the real world*, which Core cannot infer or generate — the
caller (user, or an AI proposing a candidate) must state it. This is the
single most important temporal-model difference between Episode and its
Fact/Relation siblings, and is deliberately NOT "fixed" to match them.

**Frozen: `started_at` may NOT be in the future relative to
`meta.requested_at`, and if `ended_at` is `Some(...)`, it may not be in
the future either.** This is the explicit type-system-level boundary
between Episode (historical/ongoing) and CalendarEvent (future,
undesigned/unbuilt) — an Episode can never describe something that
hasn't happened yet. Backdating is explicitly **allowed and expected as
the normal case** (a user telling LifeOS today about a trip from last
month sets `started_at` deep in the past) — this is the opposite of
Fact/Relation's "no backdating" rule, and is correct precisely because
Episode's temporal fields mean something structurally different.

For `UpdateEpisode`, the *resulting* (post-patch) `started_at`/
`ended_at` pair is what gets validated against both the future-date rule
and the `ended_at >= started_at` rule — never each patch validated in
isolation (§28).

## 5. `ended_at` is not row-lifecycle validity

**Frozen: no `is_active_episode` predicate exists or is needed.**
`ended_at` describes event time, not database-record lifecycle. It does
not mean inactive/stale/ineligible-for-correction/ineligible-for-
deletion. No action in the frozen v0.1 family needs such a predicate:
`UpdateEpisode` only requires the target to *exist* (§25);
`ForgetEpisode` is a privacy right independent of any lifecycle state
(§39). Inventing `is_active_episode` merely for symmetry with
`is_active_fact`/`is_active_relation` would be exactly the "mechanical
copying" this report was explicitly instructed to avoid.

## 6. Schema sufficiency verdict

| Item | V7 support? | Verdict |
|---|---|---|
| A. No `episode_audit_log` | Missing | **Requires V9** |
| B. Episode already has `updated_at` | Present | Sufficient as-is |
| C. No `epistemic_state`/`confidence`/supersession | Absent | Sufficient as-is — deliberately excluded (§7/§8) |
| D. `episode_entities` has no role column | Absent | Sufficient as-is — deliberately deferred (§9) |
| E. `episode_evidence` is M:N | Present | Sufficient as-is |

**Verdict: V7 is sufficient for the row-storage shape of a complete v0.1
Episode lifecycle. The only required schema addition is audit
infrastructure — one new, independent table.**

## 7. Epistemic-state decision

**Frozen: Episode does NOT get an `epistemic_state` field in v0.1.**

Reasoning: a Fact/Relation is a specific *proposition* ("X is true of
Y") whose truth-value can be disputed, confirmed, or become stale as
beliefs change — `epistemic_state` exists to track exactly that. An
Episode is a *record that something happened*, not a proposition with
the same kind of standing belief-state. Its reliability is already fully
carried by two existing mechanisms that apply uniformly to every Core
action regardless of target type: (a) the Evidence/Source chain
(exactly what backs "why do we believe this happened"), and (b) the
confirmation-gate/provenance mechanism, which already prevents an
unconfirmed `Inferred` Episode proposal from ever being persisted
without explicit approval — the *same* protection Fact/Relation get,
without needing a stored per-row marker.

Adding `epistemic_state` would also require inventing Episode-specific
semantics for a `DISPUTED` state (disputed relative to what — the
Evidence itself, or the interpretation of it?) and an `INFERRED →
USER_CONFIRMED` transition with no clear behavioral payoff: nothing
about *what happened* changes when a human confirms an Episode: they
either accept the record (do nothing further) or actively correct/
delete it. This directly resolves §13 too — see below.

## 8. Confidence decision

**Frozen: Episode does NOT get a `confidence` field in v0.1.** Same
reasoning as §7: confidence is a scalar attached to a *proposition*'s
certainty. An Episode is grounded in Evidence, not a probability
estimate over a claim. If Evidence support is weak, that is visible in
the Evidence content itself, not a separate numeric axis. Not adding
this avoids schema growth driven by symmetry rather than genuine
retrieval/action need — no concrete v0.1 action or retrieval requirement
was identified that confidence would serve and Evidence/Source cannot.

## 9. Entity-link semantics

**Frozen: `episode_entities` means "this Entity is relevant to /
participates in / is associated with this Episode" — generic
membership, no role.** Current schema structurally cannot represent
roles (actor/location/subject/participant/project) — a role column
would require a schema change. **Classified as DEFERRED, not required
now**: generic association is sufficient for v0.1 (matches the brief's
explicit preference for minimal v0.1 where generic association
suffices); no role is silently encoded into `entity_type` or the
summary text as a workaround.

## 10. Evidence-link semantics

**Frozen:** `episode_evidence` is support/provenance for the Episode,
identical in spirit to `fact_evidence`/`relation_evidence`.

- **`AddEpisode` MUST always create at least one fresh Evidence row** —
  matches `AddMemory`/`AddRelation`'s mandatory-Evidence precedent
  exactly. An Episode with zero Evidence behind it undermines the same
  privacy/provenance model every other Personal Knowledge write already
  upholds. Zero-Evidence Episodes are **not** allowed through production
  Core.
- Evidence is **always fresh, never reused** — matches `AddMemory`/
  `AddRelation` exactly; no arbitrary existing-Evidence-reuse API is
  invented.
- Exactly **one** Evidence link at creation (payload shape:
  `evidence: NewEvidenceData`, singular) — multiple-Evidence-at-once is
  **DEFERRED**, matching Fact/Relation's own identical limitation (their
  `AddMemory`/`AddRelation` are singular too).
- **Post-creation evidence attachment is DEFERRED** — no
  `AddEpisodeEvidence`/`AttachEpisodeEvidence` action in v0.1 (§11/§30).

## 11. Exact action family

| Candidate | Verdict |
|---|---|
| `AddEpisode` | **FROZEN IN v0.1** |
| `UpdateEpisode` | **FROZEN IN v0.1** (never `CorrectEpisode` — §12) |
| `ForgetEpisode` | **FROZEN IN v0.1** |
| `ConfirmEpisode` | **REJECTED AS SEMANTICALLY WRONG** — no `epistemic_state` exists to transition (§7/§13) |
| `LinkEpisodeEntity` / `UnlinkEpisodeEntity` | **DEFERRED** (§30) |
| `AddEpisodeEvidence` / `AttachEpisodeEvidence` | **DEFERRED** (§10) |

**Final v0.1 family: exactly three actions — `AddEpisode`,
`UpdateEpisode`, `ForgetEpisode`.**

## 12. Update vs Correct — explicit decision

**Frozen: `UpdateEpisode`, never `CorrectEpisode`.**

`CorrectMemory`/`CorrectRelation` exist because the *previously recorded
belief was wrong* and needs replacing. Episode mutation is different in
kind: setting `ended_at` when an ongoing event concludes is the *normal
continuation* of the same event's lifecycle, not a correction of an
error — the event genuinely was ongoing, and now it has ended. Likewise,
refining `summary`/`title` as more detail becomes known ("adding more
detail to what happened") is knowledge *refinement*, not "this was
wrong." Calling this action `CorrectEpisode` would misrepresent the
common case (event completion) as an error-correction, which it
structurally is not. `UpdateEpisode` also matches the naming already
used for exactly this kind of in-place lifecycle mutation elsewhere in
M2 (`UpdateProject`/`UpdateMilestone`/`UpdateTask`), rather than forcing
the `Correct*` vocabulary onto an action that doesn't share
`CorrectMemory`/`CorrectRelation`'s "replace a wrong belief" premise.

## 13. `ConfirmEpisode` decision

**Explicitly excluded — no canonical Episode field changes state on
confirmation, so a `ConfirmEpisode` action would be a true no-op with
nothing to persist.** Inferred Episode proposals remain safe through the
exact same three-part mechanism every other Personal Knowledge action
already relies on: (1) the confirmation gate blocking unconfirmed
`Inferred` persistence before any write; (2) audit `provenance` recording
`Inferred` permanently, even after a confirmed resubmission (never
rewritten to `Explicit`); (3) Evidence/Source carrying the actual
support for the claim. Nothing about "what happened" changes when a
human approves an already-confirmed-at-creation-time Episode — there is
no further state to move.

## 14. Add/Create naming

**Frozen: `AddEpisode`.** Episode is a Personal Knowledge object in the
same family as `Fact`/`Relation` — same module, same Evidence/Source
infrastructure, same `Add*` naming convention (`AddMemory`,
`AddRelation`) as its two siblings. The competing `Create*` convention
(`CreateGoal`, `CreateProject`) belongs to a different family (plain
domain CRUD objects with no Evidence/provenance model) and is
deliberately not applied here — internal consistency within Personal
Knowledge wins over consistency with unrelated domains. No alias.

## 15. Exact future Rust Episode shape

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Episode {
    pub id: EpisodeId,
    pub title: Option<String>,
    pub summary: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub sensitivity: Sensitivity,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Matches the live V7 schema field-for-field. No `epistemic_state`, no
`confidence`, no speculative field. Not implemented this run.

## 16. Exact `AddEpisode` payload

```rust
pub(crate) struct AddEpisodePayload {
    pub title: Option<String>,
    pub summary: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub sensitivity: Sensitivity,
    pub entities: Vec<EntityInputRef>,
    pub source: SourceRef,
    pub evidence: NewEvidenceData,
}
// -> ActionOutcome<Episode>
```

No caller-supplied `EpisodeId`/`created_at`/`updated_at`/audit
timestamp. `started_at`/`ended_at` ARE caller-supplied (§4 — the
deliberate divergence from Fact/Relation). `entities` reuses
`EntityInputRef` verbatim, no new type. `source`/`evidence` reuse
`SourceRef`/`NewEvidenceData` verbatim (§10/§23).

## 17. Summary validation rule

**Frozen:** required field — reuses the existing `validate_pk_required_field`
helper verbatim (same one `predicate`/`canonical_name`/`entity_type`/
`relation_type` already use): trimmed of outer whitespace, rejected if
empty after trim, bounded by `EPISODE_SUMMARY_MAX_LEN = 4000`, and the
**trimmed** value is what gets stored (matching every other required
Personal Knowledge field's precedent — outer whitespace is not
semantically meaningful for an identifier-like/required field the way it
is for free text). This is not a new rule invented for Episode; it is
the existing required-field convention applied here.

## 18. Title validation rule

**Frozen:** optional field — reuses the existing `validate_pk_optional_field`
helper verbatim: no trim, no emptiness requirement, bounded by
`EPISODE_TITLE_MAX_LEN = 200`. `None`, `Some("")`, and `Some("   ")` are
all valid and preserved **exactly as supplied** — matching every other
optional free-text Personal Knowledge field (`note`, `content`, `label`).
Core never generates a title from the summary or any other content.

## 19. Start/end validation (creation)

Frozen exactly:

- `ended_at == None` → valid.
- `ended_at == started_at` → valid (a zero-duration/instant event; the
  DB `CHECK (ended_at >= started_at)` already permits equality).
- `ended_at > started_at` → valid (the normal case).
- `ended_at < started_at` → **`ValidationError::EpisodeEndedBeforeStarted`**
  (new variant, frozen conceptually here, not created this run) —
  rejected as a typed Core error before ever reaching the DB `CHECK`,
  matching the "Core validates structurally, DB CHECK is defense-in-
  depth" convention used throughout this project.
- `started_at > meta.requested_at` → **`ValidationError::EpisodeTimeInFuture
  { field: "started_at" }`** (new, field-parameterized variant, matching
  `PersonalKnowledgeFieldTooLong`'s existing style) — rejected (§4's
  CalendarEvent boundary).
- `ended_at` is `Some(...)` and `> meta.requested_at` →
  `ValidationError::EpisodeTimeInFuture { field: "ended_at" }` — rejected
  for the same reason.

## 20. Initial entity-association semantics

- **May the collection be empty?** **Yes.** Unlike Fact (mandatory
  single subject) or Relation (mandatory two endpoints), Episode's core
  content is its narrative/summary — entity participation is optional
  context. A weather event or a purely external occurrence may have zero
  Entity associations.
- **Maximum count?** **No arbitrary cap** — an open collection, matching
  the non-restrictive philosophy applied everywhere else in Personal
  Knowledge; no artificial limit is invented.
- **Exact-ID duplicates** (`Existing(X)` + `Existing(X)`, or multiple
  `SelfEntity` entries, or `SelfEntity` + `Existing(actual SELF id)`):
  **Core silently deduplicates by resolved `EntityId`** before creating
  `episode_entities` links, rather than rejecting. Reasoning: this is a
  *set* of relevant Entities, not a structurally-constrained pair the
  way Relation's `from`/`to` are (there is no "self-edge"-style identity
  conflict here — a duplicate mention is simply redundant input, not an
  error) — and the composite primary key `(episode_id, entity_id)` would
  reject a literal duplicate insert anyway, so silent pre-insert dedup is
  the minimal-friction, safe choice (an LLM proposing a candidate across
  several reasoning steps may naturally repeat a mention).
- **Two `New` entities with the same `domain_ref`** (same-payload): **rejected**
  — reuses the existing `DuplicateDomainReference` check verbatim
  (unlike exact-ID duplicates, two `New` candidates can't be silently
  merged without arbitrarily picking one).
- **`New` vs already-persisted duplicate `domain_ref`:** **rejected** —
  reuses the existing second-layer persisted-duplicate check verbatim.
- **Equal `canonical_name` with no `domain_ref`:** **allowed** — two
  distinct `New` Entities with the same display name but no `domain_ref`
  are legitimately different Entities, matching `AddMemory`/`AddRelation`'s
  existing "no canonical-name-based dedup" rule.
- **Ordering:** **not significant** — `episode_entities` has no order
  column; payload list order carries no semantic meaning.

## 21. SELF implicit-inclusion decision

**Frozen: SELF is NEVER implicit.** The caller must explicitly include
`EntityInputRef::SelfEntity` in `entities` if SELF's participation is
meaningful to that Episode. Unlike Fact (subject is *always* someone/
something, frequently SELF by default in casual phrasing) and Relation
(an edge always has two concrete parties), many genuine Episodes will
not center on SELF at all (e.g. "a project incident occurred" may
involve only Project-linked Entities). Forcing implicit SELF inclusion
would also complicate the "empty collection is valid" rule (§20) for no
benefit. This matches the existing discipline that SELF is always an
explicit choice, never auto-inserted, exactly as it already is for
`AddMemory`/`AddRelation`.

## 22. `domain_ref` validation

Reuses the exact existing authoritative validation (`Goal`/`Project`/
`Milestone`/`Task`, `validate_domain_ref_authoritative`, unmodified) and
the exact existing same-payload/persisted-duplicate detection layers,
verbatim. **No new supported `domain_ref_type` is added in R030.**
Rollback expectations match `AddMemory`/`AddRelation` exactly: any
failure anywhere in the transaction rolls back every newly-created
Entity/Source/Evidence/Episode row; pre-existing `Existing` rows are
never mutated regardless.

## 23. Source/Evidence creation

Reuses `SourceRef`/`NewSourceData`/`NewEvidenceData` verbatim. `Existing`
Source reused unchanged; `New` Source created atomically; Evidence
always fresh (§10); Evidence sensitivity remains fully independent, no
automatic inheritance from Episode/Source/Entity. No Episode-specific
Source/Evidence type is created.

## 24. Episode duplicate policy

**Frozen: no Core-enforced uniqueness.** Two Episodes with identical
`summary`/`started_at`/`ended_at`/entity-links are **allowed**, with
distinct `EpisodeId`s — a direct, consistent extension of the
already-frozen Fact/Relation "no implicit dedup" philosophy. No fuzzy or
embedding-based duplicate detection exists or is planned for v0.1; any
future deduplication is explicitly a retrieval-time concern, never a
write-time constraint.

## 25. `UpdateEpisode` — exact payload

```rust
pub(crate) struct UpdateEpisodePayload {
    pub episode_id: EpisodeId,
    pub title: FieldPatch<String>,
    pub summary: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub ended_at: FieldPatch<DateTime<Utc>>,
    pub sensitivity: Option<Sensitivity>,
}
// -> ActionOutcome<Episode>
```

Nullability mapping follows the already-established rule exactly:
nullable DB field (`title`, `ended_at`) → `FieldPatch<T>`; non-nullable
DB field (`summary`, `started_at`, `sensitivity`) → `Option<T>`. No
`id`/`created_at` mutation possible through this payload. **Entity/
Evidence links are explicitly NOT part of this action** (§30 — deferred).

**Identity model:** `EpisodeId` alone identifies the historical event
record. Unlike `Relation` (where `from`/`to`/`relation_type` form an
immutable identity triple and only `note`/`confidence`/`sensitivity` are
mutable), **Episode has no identity/metadata split** — every field
except `id`/`created_at` is mutable, because there is no separate
"identity" concept beyond the `EpisodeId` itself; `title`/`summary`/
`started_at`/`ended_at`/`sensitivity` are all just evolving descriptions
of the *same* identified event. Editing any of them updates the SAME
row, never creates a successor. If the user wants to describe a
genuinely different event, that is `AddEpisode` for the new one and
(optionally) `ForgetEpisode` for the old — a caller-level decision, never
inferred by Core.

**No active-state precondition** (§5) — `UpdateEpisode` requires only
that the target *exist*; there is no analogous "must be active" gate the
way `CorrectMemory`/`CorrectRelation` require.

## 26. `updated_at` semantics

- `AddEpisode`: `created_at = updated_at = meta.requested_at`.
- Successful `UpdateEpisode`: `updated_at = meta.requested_at`
  unconditionally on success; `created_at` never changes.
- `ForgetEpisode`: the row disappears entirely; the deletion's timing is
  recorded only in `episode_audit_log.occurred_at`.
- No link-mutation actions exist in v0.1 (§30), so there is no question
  of whether they would touch `Episode.updated_at` — moot.

## 27. Structural no-op update rule

**Frozen:** a payload where `title == Unchanged` AND `summary == None`
AND `started_at == None` AND `ended_at == Unchanged` AND `sensitivity ==
None` is rejected with the existing, generic
`ValidationError::NoFieldsToUpdate` — no new Episode-specific variant.
Purely structural (was *any* field patch requested?), runs before
confirmation and before any DB read. **Same-value explicit resupply is
accepted, never coerced into a no-op** — matching `CorrectRelation`'s
identical frozen precedent (§33 of R027/R028's own established rule): a
caller explicitly resupplying the current value for any one field still
counts as a requested update.

## 28. Resulting temporal state validation

**Frozen:** the resulting (post-patch) `started_at`/`ended_at` pair is
computed in memory from the pre-state (read through the `Immediate`
transaction) and the requested patches, THEN validated as one combined
check — `ended_at IS NULL OR ended_at >= started_at`, plus the same
no-future-date rule from §19 applied to the resulting values — never
validating each field's patch in isolation (which could accidentally
permit an invalid combined state, e.g. moving `started_at` past an
unchanged `ended_at`). **This validation requires the transaction-read
target** — it cannot run purely, before the target is fetched, precisely
because resolving `Unchanged`/`None` needs the current stored values.

## 29. Transaction boundaries

- **`AddEpisode`:** a normal `conn.transaction()` (`Deferred`) after the
  confirmation gate and DB-dependent resolution — matching `AddMemory`/
  `AddRelation` exactly. No pre-existing target row is read-then-
  mutated, so there is no TOCTOU window to close.
- **`UpdateEpisode`:** `TransactionBehavior::Immediate`, opened
  immediately after the confirmation gate, BEFORE the target is ever
  read — applying the R027 (`CorrectRelation`) lesson **from the start**,
  never repeating `CorrectMemory`/`CorrectRelation`'s original mistake a
  third time.
- **`ForgetEpisode`:** `TransactionBehavior::Immediate`, opened
  immediately after the confirmation gate, BEFORE target/deletion-set
  reads — applying the R024/R029 (`ForgetMemory`/`ForgetRelation`)
  lesson from the start.

## 30. Evidence/Entity link mutation after creation

**Frozen: Option A — associations are fixed at `AddEpisode` creation
time; dedicated link/unlink/attach actions are explicitly DEFERRED, not
v0.1.** Neither `Fact` nor `Relation` has a post-creation Evidence-
attachment action either — inventing one for Episode alone would add an
entirely new capability class this codebase has never needed, for no
identified concrete requirement. **What is absent in v0.1, stated
plainly:** a caller cannot add or remove an Episode's Entity
associations, or attach additional Evidence to an existing Episode,
without forgetting and recreating it. A future dedicated `LinkEpisodeEntity`/
`UnlinkEpisodeEntity` pass may add this later if a concrete need emerges
— not guessed at here.

## 31. Confirmation floors

| Action | Floor | Overlay needed? |
|---|---|---|
| `AddEpisode` | `ExplicitAdd` | No (dominated by `InferredChange` in the generic ladder) |
| `UpdateEpisode` | `ImportantChange` | **Yes** — reuses `memory_action_requires_confirmation` unmodified |
| `ForgetEpisode` | `Delete` | No (severity 4, never dominated) |

`UpdateEpisode`'s `ImportantChange` floor is not dominated by
`InferredChange` in the generic `effective_class` ladder (severity 3 >
2) — exactly the same gap `CorrectMemory`/`CorrectRelation` had — so the
already-generic, already-proven `memory_action_requires_confirmation`
overlay (no Fact/Relation-specific logic inside it) is reused verbatim.
The global confirmation ladder itself is unchanged.

## 32. Inferred provenance behavior

- `AddEpisode`, unconfirmed `Inferred`: blocks with zero writes (generic
  ladder alone suffices). Confirmed `Inferred`: the Episode is simply
  created — there is no `epistemic_state` field to force to any value
  (unlike `AddMemory`/`AddRelation`, which force `USER_CONFIRMED`);
  audit `provenance` remains `Inferred` permanently.
- `UpdateEpisode`, unconfirmed `Inferred`: blocked by the PK overlay
  (§31).
- `ForgetEpisode`: blocks unconditionally regardless of provenance, no
  overlay needed.

## 33. Can an unconfirmed Episode ever be persisted?

**No — never, under any provenance, without `meta.confirmed == true`**
at the action's own floor (the same universal rule every other Personal
Knowledge action already has). A confirmed-`Inferred` `AddEpisode`
persists the Episode exactly as an `Explicit` one would; the *only*
record that it originated as an AI proposal lives in
`episode_audit_log.provenance = "Inferred"`, never in any field on the
`Episode` row itself (since none exists to carry that distinction —
§7/§8's direct consequence).

## 34. Audit architecture decision

**Required — matches the now-unbroken eight-table-after-this precedent**
(`goal_audit_log`/`project_audit_log`/`goal_relation_audit_log`/
`milestone_audit_log`/`task_audit_log`/`fact_audit_log`/
`relation_audit_log`, Episode makes the eighth). Exact conceptual shape,
structurally identical to `relation_audit_log`:

```sql
CREATE TABLE episode_audit_log (
    id                  TEXT PRIMARY KEY,
    episode_id          TEXT NOT NULL,
    action_type         TEXT NOT NULL CHECK (action_type IN ('AddEpisode', 'UpdateEpisode', 'ForgetEpisode')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI', 'CloudLLM', 'NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit', 'Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);
```

`episode_id` is deliberately **NOT** a foreign key — same reasoning as
`fact_audit_log.fact_id`/`relation_audit_log.relation_id`: a future
`ForgetEpisode` must hard-delete the Episode row while this table's own
(payload-free, once scrubbed) structural audit marker survives.
`action_type`'s CHECK enumerates exactly the three frozen v0.1 actions —
no `ConfirmEpisode` value, since that action does not exist (§13).

## 35. Exact V9 scope

**`episode_audit_log` only — nothing else.** `episodes`/
`episode_evidence`/`episode_entities` are **not** altered: `episodes`
already has `updated_at` (§2/§6); no `epistemic_state`/`confidence`/role
column is added (§7/§8/§9 — permanently excluded, not merely deferred
for schema purposes); no backfill is needed (`episodes` has zero
production rows today — schema-only, no write path has ever existed).
Purely additive, same minimal-risk pattern as V8. **No V9 file is
created this run.**

## 36. Audit snapshot semantics

- `AddEpisode`: `previous_snapshot = NULL` (create-shaped, matches
  `AddMemory`/`AddRelation`).
- `UpdateEpisode`: mandatory exact PRE-Episode snapshot
  (`serialize_required_snapshot`, reused unmodified — requires `Episode`
  to derive `Serialize`, which it does, §15).
- `ForgetEpisode`: the new marker's `previous_snapshot` is
  unconditionally `NULL`; every prior payload-bearing
  `episode_audit_log` row for the target is scrubbed to `NULL` before
  the target itself is deleted (predicate-based —
  `WHERE episode_id = ? AND previous_snapshot IS NOT NULL`, never
  `WHERE action_type IN (...)`, covering any future payload-bearing
  action automatically, matching `scrub_relation_audit_snapshots`'s
  exact pattern).
- No link-mutation actions exist in v0.1 (§30), so there is no question
  of what a link-mutation snapshot would represent — moot.

## 37. `ForgetEpisode` — target eligibility

**Frozen: ANY existing Episode is forgettable, regardless of `ended_at`
being `None` or `Some(...)`.** `ended_at` is never misused as deletion
eligibility (§5). No active predicate is invented for this purpose —
forgetting is a privacy right, identical in kind to `ForgetMemory`/
`ForgetRelation`'s already-frozen precedent.

## 38. `ForgetEpisode` — privacy-safe deletion algorithm

One `Immediate` transaction, after the confirmation gate:

1. Fetch target Episode through `&tx`. Missing → `NotFound`, transaction
   drops, zero writes.
2. Capture candidate `EntityId`s from `episode_entities` for the target,
   deduplicated.
3. Collect every `EvidenceId` linked via `episode_evidence`.
4. Collect each linked Evidence's `SourceId`, deduplicated.
5. Scrub `episode_audit_log.previous_snapshot` for the target
   (predicate-based, §36).
6. For each collected Evidence: delete ALL its join rows from
   `fact_evidence`/`relation_evidence`/`episode_evidence` (privacy-
   first) — reuses `delete_evidence_links_for_evidence` **unchanged**.
7. Delete each collected Evidence row — reuses `delete_evidence`
   **unchanged**.
8. Delete each collected Source iff now orphaned — reuses
   `delete_source_if_orphan` **unchanged**.
9. Defensive sweep: `DELETE FROM episode_evidence WHERE episode_id = target.id`.
10. Delete the target's `episode_entities` links — required **before**
    the target hard-delete, since `episode_entities.episode_id` is
    `ON DELETE RESTRICT` (same FK-ordering reason `ForgetRelation`'s own
    defensive `relation_evidence` sweep precedes its target delete).
11. Hard-delete the target Episode (`changed == 1` enforced in every
    build profile, never `debug_assert!`).
12. Candidate Entity orphan cleanup — reuses
    `delete_entity_if_orphan_non_self` **unchanged** (already checks
    `episode_entities.entity_id` alongside `facts`/`relations`, so no
    Episode-specific exception is needed — confirmed by direct code
    inspection this run).
13. Insert exactly one minimal, payload-free `ForgetEpisode` audit
    marker.
14. `COMMIT`.

No DB cascades anywhere in this sequence; every deletion is an explicit,
narrow, single-purpose statement, matching `ForgetMemory`/`ForgetRelation`'s
established discipline exactly.

## 39. Shared-Evidence privacy

**Frozen: identical privacy-first rule.** Evidence linked to the target
Episode is deleted even if also linked to a retained Fact, Relation, or
another Episode — the retained object survives, merely losing that
Evidence's support link. No Episode-specific exception; this is a
direct, consistent third application of the same rule `ForgetMemory`/
`ForgetRelation` already established.

## 40. Entity orphan cleanup

Reuses `delete_entity_if_orphan_non_self` **completely unchanged** — its
existing guard already spans `facts.subject_entity_id`/
`object_entity_id`, `relations.from_entity_id`/`to_entity_id`, and
`episode_entities.entity_id`. SELF is never deleted (structural guard
inside the function itself); `domain_ref` alone never retains an orphan.
No second predicate is invented.

## 41. No Episode cascade

Forgetting one Episode never deletes another Episode with a similar
time/summary, never deletes related Facts/Relations, and never deletes
linked Entities that remain referenced elsewhere. Only Evidence support
links and genuinely orphaned metadata may disappear, strictly under the
privacy-first rules above.

## 42. Outbound privacy composition (reaffirmed, not implemented)

For a future Episode-shaped Context item: outbound sensitivity
classification = max sensitivity among the Episode itself, its linked
Entities, its Evidence, and that Evidence's Source — the same
already-frozen Personal Knowledge composition principle, reaffirmed
here without implementation.

## 43. Fact vs Relation vs Episode vs CalendarEvent — classification examples

| Example | Classification | Why |
|---|---|---|
| "User prefers X" | **Fact** | A single scalar attribute of SELF, no event, no second Entity needed. |
| "User worked on Project A yesterday for 3 hours" | **Episode** | A bounded real-world event with a duration, Project A as a participating Entity. |
| "Project A depends on Task B" | **Relation** | A standing edge between two Entities, no temporal event boundary. |
| "Meeting scheduled tomorrow" | **CalendarEvent** | Future-scheduled — outside Episode's historical/ongoing scope entirely (§4). |
| "Meeting happened yesterday" | **Episode** | A bounded real-world event that already occurred. |
| "User tried solution X and it failed" | **Ambiguous — Fact OR Episode, deciding factor stated below** | As a scalar belief ("this solution didn't work"), it is a **Fact** with `memory_type = IneffectiveSolution` (already frozen, R017 §6) — the already-correct answer for a simple attempt/outcome record. As a *richer, timed, multi-entity event* ("on March 3rd I spent 2 hours trying X on Project Y, evidenced by an error log"), it is an **Episode**. The deciding factor: is this a scalar belief/preference-style attribute, or a bounded event with its own time window and narrative? |
| "User owns car Y" | **Relation** | An ownership edge, an ongoing state, not a bounded occurrence. |
| "A deployment incident happened" | **Episode** | A bounded real-world event, typically Project/service-linked, evidenced by an incident report/logs. |
| "Raw chat message" | **None of the four** | Not Personal Knowledge itself — may later be distilled into a Fact/Relation/Episode by a future extraction step, never stored as-is (CLAUDE.md: the cloud LLM never owns state). |
| "User currently feels tired" | **Fact** | A `CURRENT_STATE`-typed attribute of SELF at a point in time, not a bounded event. |
| "Trip happened last month" | **Episode** | A clean, bounded, backdated real-world event, likely entity-linked to a place/companion. |
| "Reminder to call someone" | **CalendarEvent / Reminder** | Future action-oriented, not a historical record (matches the spec's own `AddReminder` concept). |

## 44. Retrieval future-readiness check (not implemented)

The frozen Episode model already carries everything a future Context
Pack would need to answer: *what happened* (`summary`/`title`), *when*
(`started_at`/`ended_at`), *which entities were involved*
(`episode_entities`), *what evidence supports it*
(`episode_evidence` → `Evidence` → `Source`), and *how sensitive it is*
(`sensitivity`, composed per §42). No essential gap was identified for
this purpose. Retrieval itself is not designed or implemented here.

## 45. Schema gap table

| Question | Current V7 support? | v0.1 decision | Needs V9? | Deferred? |
|---|---|---|---|---|
| Episode audit | No | Required | **Yes** | No |
| `epistemic_state` | No | Not needed — permanently excluded | No | No |
| `confidence` | No | Not needed — permanently excluded | No | No |
| Entity roles | No | Generic membership only | No | **Yes** |
| Multiple Evidence at creation | Schema allows (M:N); action limits to one | Exactly one at creation | No | Multiple-at-once deferred |
| Post-create entity-link mutation | No production path | Not in v0.1 | No | **Yes** |
| Post-create evidence-link mutation | No production path | Not in v0.1 | No | **Yes** |
| Event-time validation | DB checks `ended_at >= started_at` only | Core adds full validation (future-date rule, combined resulting-state check) | No | No |
| `updated_at` | Already present | Reused as-is | No | No |
| `ForgetEpisode` hard delete | Schema supports (RESTRICT everywhere, correct order) | Implement per §38 algorithm | No | No |
| Privacy-first Evidence deletion | Schema supports | Reuse existing helpers unchanged | No | No |

## 46. Open-question classification

Every question raised in this freeze resolved to exactly one of:

- **FROZEN NOW:** Episode definition/boundaries; temporal semantics;
  no active predicate; schema sufficiency (row-storage); action family
  (Add/Update/Forget only); Update-not-Correct naming; `ConfirmEpisode`
  exclusion; Add-naming; Rust shape; both action payloads; summary/title
  validation; start/end validation; entity-association semantics
  (empty-allowed, no cap, dedup rule, domain_ref reuse); SELF-not-
  implicit; Source/Evidence policy; duplicate-Episode policy; structural
  no-op rule; resulting-temporal-state validation; identity model;
  `updated_at` semantics; confirmation floors; inferred behavior; audit
  architecture; snapshot rules; transaction boundaries; `ForgetEpisode`
  eligibility/algorithm; shared-Evidence privacy; Entity orphan reuse;
  no-cascade guarantee; privacy composition reaffirmation.
- **DEFERRED (deliberately, not implementation-blocking):** Entity
  roles on `episode_entities`; post-creation entity-link mutation
  (`LinkEpisodeEntity`/`UnlinkEpisodeEntity`); post-creation Evidence
  attachment; multiple-Evidence-at-creation.
- **REQUIRES V9:** `episode_audit_log` only, exact scope frozen in §35.

**No question was left as "implementation can decide later."**

## 47. Implementation slice plan

- **R031** — Episode foundation + V9 (`episode_audit_log` only) +
  `Episode` domain struct + `get_episode` + narrow internal reads
  (`list_episode_entity_ids`/`list_episode_evidence_ids`, needed by
  `ForgetEpisode`'s deletion-set collection, not speculative) +
  `AddEpisode`. Combined, matching R026's precedent — this foundation is
  proportionally small (one new table, no new domain concepts beyond
  what Fact/Relation already established).
- **R032** — `UpdateEpisode`, built with `TransactionBehavior::Immediate`
  from the start.
- **R033** — `ForgetEpisode`, built with `TransactionBehavior::Immediate`
  from the start.

Three slices — one fewer than Relation's four (R026–R029), since
`ConfirmEpisode` does not exist and link/evidence-mutation actions are
deferred.

## 48. Final verdict

**Architecture is CLOSED for v0.1 implementation.** Implementation may
begin once one narrow, additive V9 (`episode_audit_log`, and exactly
that) is created — the first step of the recommended R031 slice, not an
open blocker to this freeze. Zero implementation-time schema or
semantic decisions remain for Episode's action family.

**Next report: R031.**

## 49. Validation

Docs-only run: `git diff --check` reports nothing (no non-doc files
touched). `git status`/`git diff --stat` show exactly this report file
plus the `PROJECT_STATE.md`/`LIFEOS_SPEC.md` updates described below. No
cargo/npm/Tauri build was run, consistent with zero production code or
schema changes this run.
