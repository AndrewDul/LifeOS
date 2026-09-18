# R025 — M2 Personal Knowledge Relation Action-Family Architecture Freeze

**Type:** Documentation / architecture only. No production code, schema, or
tests changed in this run.

**Status:** Architecture CLOSED for v0.1 implementation. Implementation is
blocked on one small, additive V8 migration — `relation_audit_log` only,
exactly (see §6/§22, and the post-freeze correction addendum at the end
of this report, which removed a remaining `relations.updated_at`
ambiguity found immediately after the original freeze).

---

## 1. Pre-run verified state

- CONFIRMED FACT: clean working tree at commit `5db53d0` ("fix: harden
  ForgetMemory transaction isolation") before this run began.
- CONFIRMED FACT: 400 tests exist as of `5db53d0` (per R024's hardening
  addendum). This run adds no tests.
- CONFIRMED FACT: the Memory action family (`AddMemory`, `CorrectMemory`,
  `ConfirmMemory`, `ForgetMemory`) is fully implemented, hardened, and
  committed (R020–R024 plus two hardening passes). Relation has zero
  production write path; only its V7 schema and typed `RelationId`
  newtype exist.
- Read in full this run: current `CLAUDE.md`; the Relation-related
  sections of R017–R019 (from prior-session context, re-verified against
  the live schema rather than re-trusted blindly); the current
  `migrations/V1__create_goals.sql` through `V7__create_personal_knowledge.sql`;
  `src/actions/mod.rs` (GoalRelation / LinkGoals contrast, current
  `ValidationError` set); `src/services/mod.rs` (`link_goals`, absence of
  `unlink_goals`, `memory_action_requires_confirmation`,
  `serialize_required_snapshot`, the full `forget_memory` transaction
  shape); `src/services/storage/personal_knowledge_repo.rs`
  (`delete_entity_if_orphan_non_self`); `Cargo.toml`
  (`rusqlite = "=0.37.0"`, confirming `TransactionBehavior::Immediate`
  availability).

## 2. Current V7 Relation schema (verified against the live migration file)

```sql
CREATE TABLE relations (
    id               TEXT PRIMARY KEY,
    from_entity_id   TEXT NOT NULL REFERENCES entities (id) ON DELETE RESTRICT,
    to_entity_id     TEXT NOT NULL REFERENCES entities (id) ON DELETE RESTRICT,
    relation_type    TEXT NOT NULL CHECK (length(trim(relation_type)) > 0),
    note             TEXT,
    epistemic_state  TEXT NOT NULL CHECK (epistemic_state IN
                        ('USER_CONFIRMED','OBSERVED','INFERRED','DISPUTED','STALE')),
    confidence       REAL CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0)),
    sensitivity      TEXT NOT NULL CHECK (sensitivity IN
                        ('PUBLIC','INTERNAL','SENSITIVE','NEVER_LEAVES_DEVICE')),
    valid_from       TEXT NOT NULL,
    valid_until      TEXT,
    created_at       TEXT NOT NULL,
    CHECK (from_entity_id <> to_entity_id)
);

CREATE TABLE relation_evidence (
    relation_id  TEXT NOT NULL REFERENCES relations(id) ON DELETE RESTRICT,
    evidence_id  TEXT NOT NULL REFERENCES evidence(id) ON DELETE RESTRICT,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (relation_id, evidence_id)
);
```

Verified against the actual file (not assumed): directional (separate
`from`/`to`, no symmetric flag); `relation_type` open-vocabulary, same
convention as `entity_type`/`predicate`/`source_type` (no closed list);
self-edge rejected at the DB layer; both endpoints `ON DELETE RESTRICT`
(an Entity cannot be silently hard-deleted while referenced — matches
`facts.subject_entity_id`/`object_entity_id`); same 5-value
`epistemic_state` and 4-value `sensitivity` closed sets as `Fact`;
nullable `confidence` bounded `[0.0, 1.0]`; `valid_from`/`valid_until`
present (temporal validity, matching `Fact`'s shape); `relation_evidence`
is a standard M:N link table, structurally identical to `fact_evidence`.

**Two confirmed gaps relative to `facts`:**
- No `updated_at` column.
- No `superseded_by_relation_id`-equivalent column.

There is **no `relation_audit_log` table anywhere in the schema.**

## 3. Relation vs GoalRelation (contrast, verified)

`GoalRelation` (V3, `goal_relation_audit_log`) is a closed-vocabulary,
create-only join between two `Goal`s:
`LinkGoalsPayload{from_goal_id, to_goal_id, relation_type: GoalRelationType (11-value closed enum), note}`.
`grep -n "fn link_goals\|unlink_goals\|UnlinkGoals" src/services/mod.rs`
confirms **no `unlink_goals` exists** — `link_goals` is the only production
function. GoalRelation has zero lifecycle: no epistemic state, no
confidence, no temporal validity, no correction, no deletion path.

Personal Knowledge `Relation` is architecturally unrelated. It already has
the full Fact-grade shape (`epistemic_state`, `confidence`, `sensitivity`,
`valid_from`/`valid_until`) baked into V7 from the start, and its
`relation_type` is deliberately open-vocabulary (the migration's own
comment states this was intentional, "a completely separate layer from
GoalRelation — the two only ever shared a field name"). This freeze
reaffirms that: **Personal Knowledge Relation is not GoalRelation, must
never be merged with it, and must never adopt its closed-enum or
create-only shape.**

## 4. Relation vs Fact boundary

A **Fact** is an attribute of one Entity: `(subject_entity_id, predicate,
value)`, where `value` may be a scalar or an `EntityRef`. A **Relation** is
a directional, first-class, traversable edge between two Entities:
`(from_entity_id, relation_type, to_entity_id)`.

The deciding question is not "does this involve another Entity?" (a
`FactValue::EntityRef` already allows that) but: **is the edge itself the
thing that matters — will something need to traverse or query FROM the
far side, or independently of the near side?** If yes, it's a Relation.
If the assertion is really a single attribute that happens to be
Entity-valued (no onward traversal need), it's a Fact.

Examples (validated against real LifeOS use cases, not product fluff):

| From | relation_type | To | Why Relation, not Fact |
|---|---|---|---|
| SELF | `married_to` | Person Entity | Graph traversal matters ("who is my spouse's sibling") |
| SELF | `works_on` | Project Entity | Connectivity query ("what am I working on") |
| Task Entity A | `depends_on` | Task Entity B | Classic edge between two first-class nodes, not an attribute of either |
| SELF | `allergic_to` | Food Entity | Cross-domain traversal ("does this Recipe contain a Food I'm allergic to") |
| SELF | `owns` | Vehicle Entity | Ownership is inherently an edge; Vehicle may later carry its own `domain_ref` |
| SELF | `studies_at` | Institution Entity | Edge to an external, independently-referenceable node |
| SELF | `employed_by` | Company Entity | Edge; Company may itself later gain Relations (subsidiaries, etc.) |
| Device Entity | `paired_with` | Device Entity (phone) | Device-to-device edge, not an attribute of either device |
| SELF | `practices_habit` | Habit Entity | Habit may later connect to other tracked concepts (only when traversal is genuinely needed — otherwise prefer a scalar Fact) |

Deliberate Fact-side contrast examples (same "another Entity is involved"
shape, but correctly Fact, not Relation):

| Subject | predicate | value | Why Fact, not Relation |
|---|---|---|---|
| SELF | `preferred_name` | `"Andrew"` (scalar) | No second Entity at all |
| SELF | `favorite_color` | `"blue"` (scalar) | No second Entity at all |
| SELF | `favorite_restaurant` | Restaurant Entity (`EntityRef`) | Entity-valued, but no traversal need from the restaurant's side — a single attribute of SELF, not a graph edge |

This last pair is the sharpest boundary case and is the one that most
concretely operationalizes the rule: `FactValue::EntityRef` exists
precisely so a single Entity-valued attribute does NOT have to become a
Relation. Relation is reserved for edges that are themselves
semantically/traversally significant.

## 5. Discovered schema gaps and their severity

Two additional pieces of evidence, gathered by direct inspection rather
than assumption, materially change how severe each gap is:

- **`updated_at` is not a strictly universal M2 convention.**
  `grep -n "updated_at" migrations/V1__create_goals.sql
  migrations/V2__create_projects.sql migrations/V4__create_milestones.sql
  migrations/V5__create_tasks.sql` shows it present on `goals`,
  `projects`, `tasks` — but **absent from `milestones`** (V4). Milestone
  already has its own mutable lifecycle (`UpdateMilestone` mutates
  `title`/`target_date`/`status`/`order_index` in place) and functions
  correctly with no row-level "last changed" timestamp; its audit log's
  `occurred_at` is what actually carries history. This is a real,
  already-proven precedent that a mutable M2 table does not strictly
  require `updated_at` to support an in-place update action. Conclusion:
  `relations` lacking `updated_at` is a real but **non-blocking** gap.
- **Every M2 entity family has its own dedicated audit table — no
  exception, no generic table, ever.** `grep -n "CREATE TABLE.*audit_log"
  migrations/*.sql` returns exactly six tables: `goal_audit_log`,
  `project_audit_log`, `goal_relation_audit_log`, `milestone_audit_log`,
  `task_audit_log`, `fact_audit_log`. There is zero precedent anywhere in
  this codebase's history for a shared/generic audit table. `relations`
  has none. This is the one gap that is unambiguously **blocking**: every
  M2 write path gets its own dedicated audit row, and Relation has
  nowhere to put one.
- **No `superseded_by_relation_id`-equivalent exists**, and — as resolved
  in §16 below — this turns out **not to be required**, because
  `CorrectRelation` is frozen as an in-place metadata correction, not a
  Fact-style successor chain. This gap is real but is resolved by design,
  not by schema change.

## 6. V7-sufficiency verdict

**Answer to the CRITICAL FIRST QUESTION:** (C) **V7 is not sufficient as
it stands, but the gap is narrow, additive, and low-risk.** A minimal V8
is required before any Relation action can be implemented — not because
`relations`/`relation_evidence` themselves need to change, but because
**no audit table exists for Relation writes**, and every M2 mutating
action in this codebase's unbroken history writes an audit row. This is
not "V7 is broadly wrong" — the row-storage schema is confirmed correct
and sufficient for the entire frozen action family below. It is
specifically: audit infrastructure is missing.

**Required V8 scope (frozen conceptually, NOT created this run):**

```sql
CREATE TABLE relation_audit_log (
    id                  TEXT PRIMARY KEY,
    relation_id         TEXT NOT NULL,   -- deliberately not an FK; must
                                          -- survive ForgetRelation hard-
                                          -- deleting the row it describes,
                                          -- exactly matching fact_audit_log.fact_id
    action_type         TEXT NOT NULL CHECK (action_type IN
                           ('AddRelation','CorrectRelation','ConfirmRelation','ForgetRelation')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI','CloudLLM','NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit','Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,            -- NULL for AddRelation/ForgetRelation,
                                          -- required for CorrectRelation/ConfirmRelation
    occurred_at         TEXT NOT NULL
);
```

Structurally identical to `fact_audit_log`, for the same reason every
other family's audit table mirrors the same shape: minimal risk,
maximum consistency, zero new conceptual pattern introduced.

**EXCLUDED from v0.1: `relations.updated_at`.** *(Corrected by the
post-freeze consistency pass below — originally left as "recommended,
not required," which was itself a contradiction of a CLOSED verdict; see
the addendum at the end of this report for the full correction.)* No
row-level "last modified" column is added to `relations` in v0.1.
`CorrectRelation`'s and `ConfirmRelation`'s mutation timestamps are fully
represented by `relation_audit_log.occurred_at` — `milestones` already
proves a mutable M2 table does not need its own `updated_at` to support
correct in-place updates (§5), and there is no frozen sync/change-feed
requirement today that consumes a Relation-level modification time. If
one emerges later, it is a new, explicit decision for that subsystem, not
something to guess at here.

**Not required:** `superseded_by_relation_id` — see §16.

**Final v0.1 V8 scope, in full: `relation_audit_log` — and nothing
else.** No `ALTER TABLE relations` of any kind.

This V8 is purely additive: no existing table is altered at all, no FK is
weakened, V1–V7 are untouched, and (since `relations` currently has zero
production rows — schema-only, no write path has ever existed) there is
no real-data migration risk.

## 7. Semantic definition of a Personal Knowledge Relation

A Personal Knowledge Relation is a directional assertion connecting two
existing or newly-created Personal Knowledge Entities: `from --relation_type--> to`.

Core explicitly does **not**, in v0.1:
- infer an inverse edge (asserting `A works_on B` never creates `B has_worker A`)
- compute transitive closure
- create symmetric counterparts automatically
- merge Relation with GoalRelation in any way
- treat two Entities with the same canonical name as the same Entity
- perform any ontology/reasoning over `relation_type` strings

## 8. Final public Relation action family

`AddRelation`, `CorrectRelation`, `ConfirmRelation`, `ForgetRelation` —
same four-verb shape as Memory, no `UpdateRelation`.

**Explicit decision on generic `UpdateRelation`: NO.** A blanket
arbitrary-field-mutation action was evaluated and rejected for the same
reason it was rejected for Fact: semantic correction should have
explicit, scoped semantics rather than an open-ended field bag.
`CorrectRelation` below is the intentionally narrow answer to "some
fields legitimately need to change."

### AddRelation
- **Why it exists / why public:** the only way to assert a new Relation.
- **Mutates:** inserts one `relations` row, one `relation_evidence` link,
  optionally new `entities`/`sources`/`evidence` rows.
- **Does NOT:** infer inverses, deduplicate against existing identical
  edges, backdate `valid_from`.
- **Confirmation floor:** `ExplicitAdd`.
- **Inferred behavior:** unconfirmed inferred call → zero writes (generic
  ladder blocks; `InferredChange` dominates `ExplicitAdd`, no overlay
  needed). Confirmed-inferred call → persisted `epistemic_state` forced
  to `USER_CONFIRMED` (mirrors `AddMemory`'s exact rule). Direct
  `Explicit` call persists whatever `epistemic_state` the caller
  specified.
- **Audit:** one `relation_audit_log` row, `previous_snapshot = NULL`
  (Create-shaped, matches `AddMemory`).
- **Evidence:** always fresh (never reused), linked atomically.
- **New vs same row:** always a new row.
- **History:** N/A (nothing precedes it).
- **Undo:** none.

### CorrectRelation
- **Why it exists / why public:** the only way to adjust `note`,
  `confidence`, or `sensitivity` on an already-asserted Relation without
  changing what edge it represents.
- **Mutates:** `note`, `confidence`, `sensitivity` on the SAME row, via
  `FieldPatch<T>` tri-state patches (reusing the existing pattern).
  **Does not touch** `from_entity_id`, `to_entity_id`, `relation_type`,
  or `epistemic_state`.
- **Does NOT:** create a successor row, touch identity fields, touch
  epistemic state (that is `ConfirmRelation`'s exclusive job).
- **Confirmation floor:** `ImportantChange`, with the exact same
  Personal-Knowledge inferred-blocking overlay `CorrectMemory` needed
  (`ImportantChange` severity 3 is not dominated by `InferredChange`
  severity 2 in the generic ladder). `memory_action_requires_confirmation`
  is already fully generic (`fn(meta: &ActionMeta, class:
  ConfirmationClass) -> bool`, no Fact-specific logic inside it) and is
  reused verbatim, unmodified.
- **Audit:** one `relation_audit_log` row, mandatory
  `previous_snapshot` via `serialize_required_snapshot` (already generic
  over any `Serialize` type, reused unmodified).
- **Evidence:** untouched (no Evidence lifecycle in a metadata-only
  correction).
- **New vs same row:** same row, in place — this is the deliberate
  divergence from `CorrectMemory`.
- **History:** preserved only via the audit trail's `previous_snapshot`,
  not via a live successor chain — see §16 for the full reasoning.
- **Undo:** none.

### ConfirmRelation
- **Why it exists / why public:** pure epistemic confirmation in place,
  separate from `CorrectRelation` for the same single-responsibility
  reason `ConfirmMemory` is separate from `CorrectMemory`.
- **Mutates:** `epistemic_state` only (`INFERRED`/`OBSERVED`/`DISPUTED` →
  `USER_CONFIRMED`). Already-`USER_CONFIRMED` → no-op rejection (same
  shape as `ConfirmMemory`).
- **Confirmation floor:** `ExplicitAdd`, no overlay needed (dominated by
  `InferredChange` in the generic ladder, same reasoning as
  `ConfirmMemory`).
- **Audit:** mandatory `previous_snapshot`.
- **Evidence:** untouched.
- **New vs same row:** same row.
- **History:** N/A — same identity, epistemic state only.
- **Undo:** none.

### ForgetRelation
- **Why it exists / why public:** the only hard-delete path; privacy-safe
  removal of a Relation and everything exclusively linked to it.
- **Mutates:** hard-deletes the target `relations` row; deletes
  `relation_evidence` links for it; deletes linked `evidence` rows
  (privacy-first, even if shared with another retained Fact/Relation/
  Episode — same rule `ForgetMemory` uses); deletes each linked `source`
  only if now orphaned; scrubs `relation_audit_log.previous_snapshot` for
  the target; deletes each candidate `from`/`to` Entity only if now
  orphaned (SELF never deleted); inserts one minimal, payload-free
  `ForgetRelation` audit marker.
- **Confirmation floor:** `Delete`, blocks every unconfirmed call
  regardless of provenance, no overlay needed (severity 4, never
  dominated).
- **Result type:** `ActionOutcome<RelationId>`, **never**
  `ActionOutcome<Relation>` — a success result must never hand back the
  forgotten semantic object (identical rule to `ForgetMemory`).
- **Transaction:** MUST use `TransactionBehavior::Immediate`, with the
  entire deletion-set determination (target lookup, candidate Entity
  capture, Evidence collection, Source collection) executed inside that
  transaction — exactly the fix R024's hardening pass applied to
  `ForgetMemory`, applied here from the start rather than as a follow-up.
- **Undo:** none — explicitly, there is no Undo concept anywhere in
  `ActionOutcome` (confirmed via source inspection), so this is a true
  statement, not a caveat.

## 9. Exact payload/result types

```rust
pub(crate) struct AddRelationPayload {
    pub from: EntityInputRef,
    pub to: EntityInputRef,
    pub relation_type: String,
    pub note: Option<String>,
    pub epistemic_state: EpistemicState,
    pub confidence: Option<f32>,
    pub sensitivity: Sensitivity,
    pub source: SourceRef,
    pub evidence: NewEvidenceData,
}
// -> ActionOutcome<Relation>

pub(crate) struct CorrectRelationPayload {
    pub relation_id: RelationId,
    pub note: FieldPatch<String>,
    pub confidence: FieldPatch<f32>,
    pub sensitivity: FieldPatch<Sensitivity>,
}
// -> ActionOutcome<Relation>

pub(crate) struct ConfirmRelationPayload {
    pub relation_id: RelationId,
}
// -> ActionOutcome<Relation>

pub(crate) struct ForgetRelationPayload {
    pub relation_id: RelationId,
}
// -> ActionOutcome<RelationId>
```

All four reuse `EntityInputRef`, `SourceRef`, `NewSourceData`,
`NewEvidenceData`, `FieldPatch<T>` verbatim from the Memory family — no
Relation-specific parallel types are introduced, per the brief's explicit
instruction to reuse rather than invent.

`valid_from` is Core-generated from `meta.requested_at` on `AddRelation`
(no backdating in v0.1, matching `AddMemory`). `valid_until` always
starts `NULL` and is never set by any v0.1 action — see the deferred
"soft closure" gap in §30.

## 10. Endpoint (`EntityInputRef`) rules

- Both `from` and `to` may be `New(NewEntityData)` simultaneously — reuses
  `AddMemory`'s existing same-payload duplicate-`domain_ref` preflight
  check, generalized from comparing subject/value to comparing from/to.
- Either endpoint may be `SelfEntity` — unrestricted.
- `from == to` is rejected. Resolution: both endpoints are resolved to
  concrete `EntityId`s via the same `resolve_entity_input_ref` logic
  `AddMemory` already has (covering `SelfEntity`/`Existing`/`New`
  uniformly), then the two resulting IDs are compared once, after
  resolution — this is correct regardless of which `EntityInputRef`
  variant combination produced the collision, and matches the live DB
  `CHECK (from_entity_id <> to_entity_id)`, which remains correct and
  unchanged. A pure-structural fast-path check (identical `Existing(id)`
  or both `SelfEntity`) may additionally reject before touching the DB,
  matching `LinkGoals`'s `SelfGoalRelation` precedent, but the
  post-resolution check is the authoritative one.
- Same-payload and persisted duplicate `domain_ref` behavior, authoritative
  `domain_ref` validation, existing-Entity immutability, new-Entity
  rollback-on-failure, and candidate-orphan-Entity cleanup on a later
  `ForgetRelation` all reuse `AddMemory`/`ForgetMemory`'s exact mechanisms
  unmodified — including `delete_entity_if_orphan_non_self`, which
  **already** checks `relations.from_entity_id`/`to_entity_id` (confirmed
  by direct read this run), so no Relation-specific exception is needed
  anywhere in Entity orphan handling.

## 11. `relation_type` policy

Open vocabulary, reusing the existing `RELATION_TYPE_MAX_LEN = 100`
constant (already frozen in `domain/mod.rs`, confirmed via grep). Core
validates only trim/non-empty/max-length — no enum, no normalization, no
synonym rewriting, no inverse inference. Examples given anywhere in this
report are illustrative only, never canonical.

## 12. `note` policy

Optional free-text elaboration of the Relation itself — not Evidence, not
Source content. Governed by `Relation.sensitivity` (may contain sensitive
content). Bounded by the existing `RELATION_NOTE_MAX_LEN = 4000` constant.

**No normalization**: consistent with every other optional free-text
field in this codebase (`label`/`reference`/`content`/`locator`), `note`
is bound-checked only — no trim, no empty-to-`None` collapsing, no
rejection of whitespace-only values. This is the same convention already
applied elsewhere, not a special case invented for Relation.

Mutable only through `CorrectRelation`.

## 13. Source/Evidence semantics

`AddRelation` reuses `SourceRef`/`NewSourceData`/`NewEvidenceData`
verbatim: Existing Source unchanged on reuse; New Source created
atomically; Evidence always fresh (never shared/reused at creation);
`relation_evidence` link created atomically in the same transaction; full
rollback on any failure. No Relation-specific Source/Evidence types are
justified or created.

## 14. Duplicate-edge policy

**No deduplication.** Core does not prevent multiple active Relations
with identical `(from, relation_type, to)`. This is the consistent
extension of Fact's own already-frozen "no implicit dedup" rule (two
independent `AddMemory` calls may create two Facts that look alike) —
Relation gets the same treatment rather than a new, Relation-specific
rule. `domain_ref` dedup remains a distinct, narrower concern (avoiding
two Entities silently claiming the same authoritative external identity)
that has no analogue for Relation-vs-Relation duplication. Retrieval-time
deduplication/ranking, if ever needed, is explicitly a future
retrieval/Context Pack concern, not Core's write-time job — no DB
`UNIQUE` constraint is added.

## 15. Contradiction semantics

No automatic contradiction detection by `relation_type` string matching.
`DISPUTED` is available epistemically (already in the closed 5-value
set). Higher-level orchestration MAY propose a Relation as `DISPUTED`.
There is no global conflict resolver in v0.1 — matches Fact's own
already-frozen position exactly.

## 16. Active Relation predicate / correction / history model

```rust
pub(crate) fn is_active_relation(relation: &Relation) -> bool {
    relation.valid_until.is_none() && relation.epistemic_state != EpistemicState::Stale
}
```

Structurally identical in shape to `domain::is_active_fact`, following
the existing frozen convention rather than inventing a second one.
`valid_until` is treated as a pure open/closed marker, never compared
against wall-clock "now" — `Some(timestamp)` unconditionally means
closed, regardless of current time, exactly matching Fact's established
behavior (confirmed via the R022/R023 implementation, which never
performs a wall-clock comparison either).

**Correction/history model — the central design resolution of this
report:** Relation's identity is the triple `(from_entity_id,
relation_type, to_entity_id)`. Unlike Fact — whose identity is
`(subject_entity_id, predicate)` with a separately-correctable `value` —
Relation has no analogous "value" field separate from its identity; the
edge itself IS the entire semantic content. Consequently:

- Changing `from`, `to`, or `relation_type` is never a "correction" — it
  is asserting a *different* edge. The correct operation is
  `ForgetRelation` (old edge) + `AddRelation` (new edge), exactly
  mirroring how changing a Fact's subject/predicate is `AddMemory` for a
  different assertion, never `CorrectMemory`.
- What can legitimately change without altering identity is metadata:
  `note`, `confidence`, `sensitivity`.
- Because only metadata is correctable, `CorrectRelation` does not need
  a successor-row/supersession-chain mechanism at all — it is an
  in-place update with a mandatory audit snapshot, structurally identical
  to `UpdateMilestone`/`UpdateProject`'s already-proven pattern, not to
  `CorrectMemory`'s successor-Fact pattern.
- This is precisely why `superseded_by_relation_id` turns out **not** to
  be required (§5/§6): there is no successor row to point to. The
  "history" of a correction lives entirely in
  `relation_audit_log.previous_snapshot`, exactly as it already does for
  `UpdateMilestone`/`UpdateProject`.

This is a deliberate divergence from copying Fact's rules mechanically —
Relation's identity model genuinely differs, and the action family
reflects that rather than forcing a structural parallel that wouldn't
carry real meaning.

## 17. Confirmation-floor mapping (final)

| Action | Floor | Overlay needed? |
|---|---|---|
| `AddRelation` | `ExplicitAdd` | No (dominated by `InferredChange` in generic ladder) |
| `CorrectRelation` | `ImportantChange` | Yes — reuses `memory_action_requires_confirmation` unmodified |
| `ConfirmRelation` | `ExplicitAdd` | No |
| `ForgetRelation` | `Delete` | No (severity 4, never dominated) |

The generic confirmation ladder (`SafeRead=0 < ExplicitAdd=1 <
InferredChange=2 < ImportantChange=3 < Delete=4`) is unchanged.

## 18. Provenance/epistemic rules

- `AddRelation`, unconfirmed + `Inferred`: zero writes.
- `AddRelation`, confirmed + `Inferred`: persisted `epistemic_state`
  forced to `USER_CONFIRMED` (mirrors `AddMemory`'s exact rule, verified
  against its actual implementation this run).
- `AddRelation`, `Explicit`: persists exactly the caller-specified
  `epistemic_state`.
- `CorrectRelation`: never touches `epistemic_state`, regardless of
  provenance.
- `ConfirmRelation`: `INFERRED`/`OBSERVED`/`DISPUTED` → `USER_CONFIRMED`;
  already-`USER_CONFIRMED` → no-op rejection. Identical shape to
  `ConfirmMemory`.

## 19. Privacy Firewall composition (reaffirmed, not implemented)

Outbound classification for any bundle including a Relation = max
sensitivity among all actually-included components: from-Entity,
to-Entity, Relation itself, Source, Evidence. No implementation this run.

## 20. Self-edge and directionality

Self-edge prohibition is retained: no compelling LifeOS example
(reviewed in §4/§29) requires `from == to`; the existing DB `CHECK`
remains correct and unchanged.

Relations are always stored directionally, with **no automatic inverse**.
For symmetric real-world relationships (e.g. `married_to`), v0.1's rule
is: the caller's single directional claim is stored as-is; if a symmetric
pair is desired, the calling orchestration explicitly issues two
`AddRelation` calls. No speculative `symmetric` column is added.

## 21. Minimum future read API

`get_relation(RelationId) -> Option<Relation>` is the only strictly
required read for the action family itself (`CorrectRelation`/
`ConfirmRelation`/`ForgetRelation` all need to look up the target).
`get_relations_for_entity` / `get_active_relations_for_entity` are
plausible near-future additions but are explicitly **not** frozen here —
broader retrieval/Context Pack design is out of scope for this report.

## 22. Gap-analysis table

| Requirement | V7 support? | Action needed | Future migration? |
|---|---|---|---|
| Relation row storage (all fields) | Yes | None | No |
| Relation read struct / `get_relation` | Schema yes; Rust struct/fn not written | Write in future slice | No |
| `updated_at` on `relations` | No | **Deliberately excluded from v0.1** — `relation_audit_log.occurred_at` is the authoritative mutation timestamp | No |
| Supersession/history pointer | Not needed (see §16) | None | No |
| `relation_audit_log` | No | Must be created | **Required** |
| Evidence links (`relation_evidence`) | Yes | None | No |
| Hard-delete path | Schema supports (RESTRICT everywhere, correct deletion order) | Write in future slice | No |
| Active predicate | Schema supports | Write in future slice | No |
| Entity orphan cleanup | Already implemented, already includes `relations.from/to` | None | No |
| Confirmation state transition | Schema supports | Write in future slice | No |
| `previous_snapshot` support | Requires `relation_audit_log` | Blocked on same | **Required** |

**Verdict: V8 required, scope EXACTLY `relation_audit_log` — one new,
independent table, nothing else. `relations.updated_at` is not part of
v0.1; no `ALTER TABLE relations` occurs in V8.**

## 23. Migration-compatibility rules for the future V8

Additive only; no destructive rewrite of V1–V7; `relations` has zero
production rows today (schema-only, no write path has ever existed), so
there is no real-data backfill risk; existing raw-SQL Relation test
fixtures (used by earlier Fact-family tests) remain valid since nothing
about `relations` itself changes; existing encrypted DBs migrate
deterministically via the same refinery mechanism already used for
V1–V7; no FK weakening; all 400 existing tests remain valid (a new,
independent, additive table cannot break any existing constraint or
query).

## 24. Final Rust `Relation` domain shape (future — not created this run)

```rust
pub(crate) struct Relation {
    pub id: RelationId,
    pub from_entity_id: EntityId,
    pub to_entity_id: EntityId,
    pub relation_type: String,
    pub note: Option<String>,
    pub epistemic_state: EpistemicState,
    pub confidence: Option<f32>,
    pub sensitivity: Sensitivity,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}
```

This is the complete, final v0.1 shape — no `updated_at` field, commented
or otherwise. No fields are assumed to exist beyond what §2 confirms is
in the live schema. `relations.updated_at` is not part of v0.1 (see the
post-freeze correction addendum); mutation timestamps for
`CorrectRelation`/`ConfirmRelation` live exclusively in
`relation_audit_log.occurred_at`. If a future need for a row-level
modification timestamp emerges (e.g. a sync/change-feed subsystem), that
is a new, explicit decision for that subsystem — not a reopening of this
freeze.

## 25. Implementation slicing recommendation

Recommended (not binding — to be reconsidered at the start of the first
implementation slice, per the brief's instruction not to assume the
split blindly):

- **R026** — Relation foundation + `AddRelation` combined: the V8
  migration (`relation_audit_log` only), `Relation` domain struct,
  `get_relation`/`is_active_relation`, and `AddRelation` itself. Combined
  because the foundation this time is proportionally small (one new
  table, no new domain concepts beyond what Fact already established) —
  unlike R020's much larger original Slice A (11 tables, four new closed
  enums, Entity/Source/Evidence all at once).
- **R027** — `CorrectRelation`.
- **R028** — `ConfirmRelation`.
- **R029** — `ForgetRelation`, built with `TransactionBehavior::Immediate`
  from the start (no separate hardening pass should be needed this time,
  since the exact fix is already known from R024's hardening).

Four slices, one fewer than Fact's five-report sequence (R020–R024),
reflecting how much machinery (Entity/Source/Evidence handling, the
confirmation ladder, `FieldPatch`, `serialize_required_snapshot`,
`memory_action_requires_confirmation`) Relation reuses unmodified.
Delete/correction paths are kept in their own dedicated slices, not
combined with anything else, per the brief's explicit instruction not to
merge dangerous paths merely to save report numbers.

## 26. Future test-acceptance categories (no tests written this run)

Schema migration/reopen (V8); Relation serialization/read; self-edge
reject; open `relation_type` validation; Entity endpoint existence
(Existing/New/SELF, both endpoints New); New-endpoint atomicity/rollback;
Source/Evidence creation and rollback; inferred-confirmation gates
(zero-write proof); confidence finite-bounds; note bounds; active/
inactive predicate; `CorrectRelation` in-place metadata correction
(inherit/override per field via `FieldPatch`); `ConfirmRelation`
state-transition matrix + no-op rejection; `ForgetRelation` deletion +
privacy (shared Evidence, orphan/shared Source, orphan/shared Entity
including SELF-never-deleted, audit-snapshot scrub); rollback proofs for
every action; concurrent deletion-set safety (`Immediate` transaction
lock-contention proof, mirroring R024's hardening test); no regression
in any of the 400 existing Fact-family tests; encrypted storage
regression.

## 27. Open questions — classification

| Question | Status |
|---|---|
| Is V7 sufficient? | FROZEN NOW — no, narrow V8 required (§6) |
| Audit architecture | FROZEN NOW — dedicated `relation_audit_log`, mirrors `fact_audit_log` (§6) |
| Action family names/shapes | FROZEN NOW (§8/§9) |
| `CorrectRelation` successor vs in-place | FROZEN NOW — in-place, metadata-only (§16) |
| `superseded_by_relation_id` need | FROZEN NOW — not needed (§16) |
| `updated_at` on `relations` | FROZEN NOW — deliberately excluded from v0.1; `relation_audit_log.occurred_at` is the authoritative mutation timestamp (§5/§6, corrected in the post-freeze addendum below) |
| Duplicate-edge policy | FROZEN NOW — no dedup, no UNIQUE constraint (§14) |
| Self-edge / directionality / symmetry | FROZEN NOW — retain prohibition, no auto-inverse (§20) |
| Confirmation floors / overlay need | FROZEN NOW (§17) |
| Provenance/epistemic rules | FROZEN NOW (§18) |
| Minimum future read API | FROZEN NOW, narrowly (§21) — broader retrieval explicitly DEFERRED |
| Soft temporal closure of a Relation (`valid_until` without full deletion) | **DEFERRED** — no v0.1 action sets `valid_until`; only hard `ForgetRelation` exists. Flagged, not solved, this run. |
| Whether Relation identity should ever be relaxed (e.g. multiple notes per edge) | DEFERRED — no evidence of need yet |
| Broad retrieval/Context Pack interaction with Relation | DEFERRED — explicitly out of scope |

**No open question required stopping the implementation recommendation.**
Every question either resolved cleanly against existing precedent or was
deliberately, explicitly deferred as a genuine v0.1 scope limit (not
hidden).

## 28. Deferred limitation, stated plainly

v0.1's Relation family has no way to softly end a Relation's temporal
validity while preserving its history — only hard deletion
(`ForgetRelation`, which erases it) or leaving it active indefinitely.
`valid_until` remains in the schema for future use but is never set by
any v0.1 action. If a genuine need for soft closure emerges later, it
would need its own design pass (a narrow `EndRelation`-style action
setting `valid_until` in place is fully supported by the existing V7
schema with no further migration; a full supersession-chain redesign
would not be, and is not recommended without concrete evidence of need).

## 29. Final verdict

**Architecture is CLOSED for v0.1 implementation.** Implementation may
begin once one narrow, additive V8 — `relation_audit_log`, and exactly
that, nothing else — is created. That migration is the first step of the
recommended R026 slice, not an open blocker to this freeze: its scope is
now fully decided, not left for an implementer to choose. There are zero
implementation-time schema decisions remaining for R026.

**Next report: R026.**

## 30. Validation

- Docs-only run: `git diff --check` reports nothing (no non-doc files
  touched). `git status`/`git diff --stat` show exactly this report file
  plus the `PROJECT_STATE.md`/`LIFEOS_SPEC.md` updates described below.
  No cargo/npm/Tauri build was run, consistent with the brief's
  instruction that none is required when zero production code or schema
  changed.

---

## Post-freeze schema-scope consistency correction (2026-09-18)

A review of R025 immediately after its own commit found a genuine
internal contradiction: §29 declared the architecture **"CLOSED for
design,"** while §6/§22 simultaneously left `relations.updated_at` as
**"recommended, not required... a future implementer may decide either
way."** Leaving any schema question open for the implementer to decide is
exactly the kind of implementation-time architectural choice this whole
freeze exists to prevent — R017–R019 established that discipline for
Fact before any Fact-action code was written, and R025 was required to
hold Relation to the same standard. A "closed" freeze cannot contain an
unresolved "either way."

**Final v0.1 decision: `relations.updated_at` is excluded entirely.** It
is not part of the V8 migration, not part of the frozen `Relation`
domain struct, and not part of any future implementation-time choice.

Reasoning:
1. No Relation action's correctness depends on it — `AddRelation`,
   `ConfirmRelation`, and `ForgetRelation` never read or need it, and
   `CorrectRelation`'s in-place metadata mutation is fully described by
   its mandatory `relation_audit_log.previous_snapshot` write.
2. `CorrectRelation`'s correction history is authoritative in
   `relation_audit_log.previous_snapshot` + `relation_audit_log.occurred_at`
   — nothing is lost by not also stamping the row itself.
3. `ConfirmRelation`'s confirmation time is likewise fully represented by
   its own `relation_audit_log.occurred_at` row.
4. `milestones` (V4) already proves, as a real precedent in this exact
   codebase, that an M2 table with its own mutable in-place-update
   lifecycle (`UpdateMilestone`) operates correctly with no row-level
   `updated_at` at all.
5. There is no frozen sync/change-feed requirement today that consumes a
   Relation-level modification timestamp — adding the column now would be
   purely speculative, which CLAUDE.md's "no speculative scaffolding"
   principle already forbids.
6. If a genuine future need for row-level modification time emerges (a
   sync subsystem, a change-feed), that is a new, explicit design
   decision for that subsystem when it is actually built — not something
   to guess at inside this freeze.

**Consequently, the exact, final v0.1 V8 scope is:**

```sql
CREATE TABLE relation_audit_log (
    id                  TEXT PRIMARY KEY,
    relation_id         TEXT NOT NULL,   -- deliberately not an FK
    action_type         TEXT NOT NULL CHECK (action_type IN
                           ('AddRelation','CorrectRelation','ConfirmRelation','ForgetRelation')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI','CloudLLM','NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit','Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);
```

— and nothing else. No `ALTER TABLE relations`. No
`superseded_by_relation_id` (unchanged from the original freeze, §16 —
that conclusion was never in question). No generic Personal-Knowledge
audit-table redesign.

**Unchanged by this correction:** `CorrectRelation` and `ConfirmRelation`
semantics are exactly as originally frozen in §8/§16/§18 — in-place
metadata-only correction (`note`/`confidence`/`sensitivity` via
`FieldPatch`, never identity fields, never `epistemic_state`) and
in-place epistemic confirmation, respectively. Neither action's payload,
result type, confirmation floor, or provenance rules changed. This
correction touches only the row-level `updated_at` question and the
now-narrower V8 scope it implied.

**Confirmed:** no production code, schema, or tests changed by this
correction — it is a documentation-only fix to the R025 report and the
two living docs that summarized it. `V7` is unchanged; no `V8` migration
file was created. Episode actions and retrieval/Context Pack remain
entirely unstarted. The next substantial report remains **R026**.

With this correction applied, R025's architecture is now genuinely
**CLOSED for v0.1 implementation** — no schema question is left for an
implementer to decide.
