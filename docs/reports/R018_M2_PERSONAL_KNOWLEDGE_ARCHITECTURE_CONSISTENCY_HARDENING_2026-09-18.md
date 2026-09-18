# R018 — M2 Personal Knowledge Architecture Consistency Hardening

**Documentation/architecture only. No Rust or React code changed. No
migration created.** A focused post-freeze review of R017 found eleven
consistency/privacy gaps — some cosmetic (a misdescribed invariant),
several genuinely load-bearing (an inbound foreign key that would have
blocked `ForgetMemory` outright; old audit snapshots that could have
retained the exact text `ForgetMemory` exists to erase). This report
corrects all of them before any `V7` migration or implementation slice
is written, exactly as R017 itself told a future implementer to expect
("§28 Risks/open questions" already flagged several of these as
unresolved — this pass resolves them). **R017's original text is not
rewritten** — a clearly labelled addendum is appended to it (§17 below);
this report is authoritative wherever the two disagree.

---

## 1. Pre-run state

- R017 (M2 Personal Knowledge Layer Architecture Freeze) completed.
  Local checkpoint `31a1f1e` — `docs: freeze M2 Personal Knowledge
  Layer architecture`. No Personal Knowledge implementation exists.
  262/262 existing implementation tests remain the current baseline
  (unaffected — nothing here touches implemented code). Working tree
  clean. R001-R017 exist; this is **R018**, the next allocation.

---

## 2. Review findings

Verified against R017's actual current text (not a paraphrase) before
writing any correction — quoting or citing the exact line/section each
time:

1. R017 §4 literally says "**Exactly one SELF entity, frozen**... A
   partial unique index... SQLite enforces this at the DB level" — but
   a partial unique index only rejects a *second* `is_self = 1` row; it
   says nothing about whether a *first* one was ever created. R017 also
   explicitly left the bootstrap mechanism "not frozen here" (§4, §28).
   Genuinely under-specified, not merely cosmetic: without a frozen
   bootstrap guarantee, "exactly one" is not actually true of a fresh
   database with zero rows.
2. R017 §17's `AddMemoryPayload` only accepts `subject_entity_id:
   Option<EntityId>` and `FactValue::EntityRef(EntityId)` — both
   require an Entity that **already exists**. There is no path in R017
   to record the *first* fact about a brand-new person/place/vehicle/
   device/concept at all, which contradicts §6's own worked examples
   ("SELF owns_vehicle MazdaEntity" presupposes a `MazdaEntity` already
   exists — R017 never explains how it got there).
3. R017 §18's `CorrectMemoryPayload` has no `epistemic_state` field,
   but its mutation-semantics prose (§18, step 2) says the successor's
   `epistemic_state` is "**the caller's requested starting state**" —
   a field that does not exist in the payload cannot be caller-
   requested. A direct contradiction between the frozen payload shape
   and the frozen mutation description.
4. R017 §17 freezes "zero durable writes before confirmation" for an
   inferred `AddMemory` candidate, but never states precisely what
   `epistemic_state` gets persisted *after* the user confirms — leaving
   room to (wrongly) assume the model's originally-proposed
   `epistemic_state` (typically `INFERRED`) survives verbatim into a
   durable row.
5. R017 §18's `confidence: Option<f32>` ("`None` -> inherit
   predecessor's") cannot express "replace the Fact and deliberately
   *clear* the old confidence back to `NULL`" — `None` is already
   claimed to mean "inherit," so there is no way to mean "clear." This
   is exactly the `Option<T>`-vs-`FieldPatch<T>` distinction R006/R017
   itself already established for every other nullable field in M2.
6. R017 §22 defines active knowledge as `valid_until IS NULL AND
   epistemic_state <> 'STALE'` (both conditions), but §18/§19's
   `CorrectMemory`/`ConfirmMemory` validation-order prose each only
   says "predecessor/target... is currently active (`valid_until IS
   NULL`" — one half of the two-part predicate, not the whole thing.
7. R017 §19 says confirming a `DISPUTED` Fact "is this action's
   resolution path for a contradiction," immediately followed by "it
   does not automatically touch any sibling conflicting Fact" — true,
   but stated as if it were a small caveat rather than the actual
   headline: confirming one Fact does not resolve a *contradiction* at
   all, only that one assertion. Under-stated, not fully resolved.
8. **Critical.** R017 §20/§21 freezes that `ForgetMemory`'s **own** new
   audit row has `previous_snapshot = NULL`, but says nothing about
   **pre-existing** `fact_audit_log` rows for the same Fact — and
   R017 §18/§19 themselves establish that `CorrectMemory`/
   `ConfirmMemory` write mandatory, non-`NULL`, payload-bearing
   `previous_snapshot` values. A Fact that was ever corrected or
   confirmed before being forgotten would leave its exact prior
   semantic content sitting in the audit log after `ForgetMemory` ran —
   directly violating R017 §2's own preserved-unchanged rule "the
   original forgotten text/value must never persist anywhere,"
   inherited from `LIFEOS_SPEC.md` §5.4.
9. **Critical.** R017 §5 freezes `facts.superseded_by_fact_id TEXT
   REFERENCES facts (id)` with no `ON DELETE` clause — SQLite's default
   is the equivalent of `RESTRICT`. If Fact A was superseded by Fact B
   (`A.superseded_by_fact_id = B.id`) and the user later forgets B,
   `DELETE FROM facts WHERE id = B.id` (R017 §20 step 7) is **rejected
   by the database itself**, unconditionally, for every Fact that was
   ever anyone's successor. R017's own §20 algorithm never accounts for
   this inbound reference at all — `ForgetMemory` as originally frozen
   cannot actually forget a large class of real Facts.
10. R017 §20 step 4 retains Evidence "if it now has zero remaining
    rows... otherwise... leave it untouched" — but an Evidence row
    that is *also* still linked to another Fact/Relation/Episode may
    itself contain the exact forgotten text (Evidence is where
    personal/sensitive excerpts live, per R017 §11's own privacy
    split) — retention-because-still-referenced cannot guarantee the
    forgotten payload is actually gone.
11. R017 §10/§20 states Source is "**never** delete[d]... regardless of
    reference count... retaining it **carries no privacy risk**" and
    "structurally never holds sensitive payload" (§10) — too strong.
    `Source.label`/`Source.reference` are still user-supplied/model-
    supplied free text (e.g. a document path naming a medical
    condition, a label like `"oncology results"`) and can themselves be
    privacy-sensitive even while staying metadata-only in the narrow
    sense (no raw excerpt). R017 gave `Source` no `sensitivity` field
    at all, which this finding shows is a gap, not a deliberate
    asymmetry.

Two further, narrower findings, found only by cross-checking R017's
design against the *actual*, already-implemented, unchanged
`ConfirmationClass`/`effective_class` code (`actions/mod.rs`) rather
than against R017's prose alone — **not named in this run's brief, but
directly required to correctly freeze item 3/4 above, so surfaced
explicitly rather than silently assumed away:**

12. `effective_class = max(type_floor, provenance_class)` compares
    `ConfirmationClass::severity()`: `SafeRead=0 < ExplicitAdd=1 <
    InferredChange=2 < ImportantChange=3 < Delete=4` (`actions/mod.rs`,
    verified directly). `AddMemory`'s floor (`ExplicitAdd=1`) is below
    `InferredChange=2`, so an inferred `AddMemory` naturally computes
    to `InferredChange` and correctly blocks — no gap. **`ForgetMemory`**'s
    floor (`Delete=4`) already dominates `InferredChange=2` regardless
    of provenance, and `DELETE`'s own default behavior is *already*
    "always a blocking confirmation dialog" (R003 §F) — no gap either.
    But **`CorrectMemory`**'s floor is `ImportantChange=3`, which
    *also* already dominates `InferredChange=2` — so the *generic*
    formula alone does **not** force a pre-write block for an inferred
    `CorrectMemory`; `IMPORTANT_CHANGE`'s own default behavior is
    "execute immediately... requires an explicit acknowledgement step"
    (R003 §F), not a pre-write block. This is a real gap between what
    R017's brief required ("`CorrectMemory` inferred: → block → zero
    writes") and what the existing, unmodified confirmation-class
    machinery alone would actually produce. Resolved in §6 below with a
    narrow, explicitly-documented Personal-Knowledge-specific overlay —
    **no change to `ConfirmationClass`/`effective_class`/any other M2
    action's behavior is made or proposed.**
13. `ConfirmMemory`'s floor (`ExplicitAdd=1`) is below `InferredChange=2`
    — an inferred `ConfirmMemory` proposal *does* naturally block via
    the existing generic formula, with no override needed. Confirmed,
    not a finding requiring correction — recorded here so the
    reasoning in §6/§16 is complete and auditable, not merely asserted.

---

## 3. Corrected SELF invariant / bootstrap

**Frozen as two cooperating layers, not one:**

- **Database:** the partial unique index (`CREATE UNIQUE INDEX
  entities_single_self ON entities (is_self) WHERE is_self = 1;`,
  unchanged from R017 §4) guarantees **at most one** row with
  `is_self = 1`. It proves uniqueness, never existence.
- **Core bootstrap:** a new, frozen `ensure_self_entity(...)` call,
  invoked from the **normal, trusted Core/service bootstrap path** —
  i.e. eagerly, as part of opening the Personal Knowledge portion of
  the database, not lazily on first `AddMemory` call (R017 §4/§28 had
  left lazy-vs-eager explicitly undecided; this resolves it to eager).
  Guarantees **at least one** canonical SELF Entity exists before any
  Personal Knowledge action or read-side function becomes reachable.

**Together: exactly one SELF Entity in every normally-bootstrapped
LifeOS database.**

**`ensure_self_entity` behavior, frozen:**

- **0 SELF rows** → create one, with the canonical initial values
  below.
- **1 SELF row** → reuse it (idempotent — safe to call on every
  bootstrap, matching the existing `bootstrap_db_at`/
  `resolve_master_key` idempotent-on-every-launch convention, R015
  §4).
- **>1 SELF rows** → an integrity error. Should be structurally
  impossible given the DB-level unique index above, but is detected
  and reported anyway (a typed `CoreErrorKind::Internal` error, never
  silently "pick the first one and continue") — defense-in-depth
  matching M2's established pattern of never trusting a single layer
  alone (e.g. R008's FK-enforcement-plus-application-check precedent).

**Canonical initial SELF row, frozen:**

```
entity_type      = "person"
canonical_name   = "SELF"
is_self          = true
domain_ref_type  = NULL
domain_ref_id    = NULL
```

`"SELF"` is an **internal canonical label**, not a display string — a
future Memory UI may show "Me" or the user's actual name without that
being an identity change (the row's `id` and `is_self = true` are what
make it *the* SELF entity; `canonical_name` is cosmetic and, like every
other Entity field outside the frozen-immutable set, could in principle
be edited later by a not-yet-designed action without disturbing
identity).

**Updated Slice A acceptance (supersedes R017 §29's Slice A row):**

- A fresh `V7`+ bootstrap creates **exactly one** SELF Entity (proven
  as the *combined* Core+DB invariant — not just the DB constraint in
  isolation).
- A second, independent bootstrap call against the same already-
  initialized database (a close+reopen) reuses the **same** SELF
  `EntityId` — proven by comparing ids across two calls, not merely
  proving "some SELF row exists both times."
- A raw-SQL second `INSERT ... is_self = 1` is genuinely rejected by
  the partial unique index (proving the DB layer independently, the
  same "attempt the illegal thing, capture the real rejection" style
  already used throughout M2, e.g. R015's `req_kl3`).
- `ensure_self_entity` is proven idempotent across 3+ repeated calls
  (matching R016's `req_dkl9` "repeated reopen" proof style).

---

## 4. `EntityInputRef` — New/Existing/Self, inside `AddMemory` only

**No new public action is introduced** (the four Memory actions stay
exactly `AddMemory`/`CorrectMemory`/`ConfirmMemory`/`ForgetMemory`, no
`CreateEntity`). Instead, `AddMemory` (and, by extension, `CorrectMemory`'s
`value` field, §6) accepts a **typed entity reference** wherever R017
previously required an already-existing `EntityId`:

```
EntityInputRef {
    SelfEntity,
    Existing(EntityId),
    New(NewEntityData),
}

NewEntityData {
    entity_type: String,
    canonical_name: String,
    domain_ref: Option<DomainRef>,
}

DomainRef {
    domain_ref_type: String,
    domain_ref_id: String,
}
```

`AddMemoryPayload.subject_entity_id: Option<EntityId>` (R017 §17) is
replaced by `subject: EntityInputRef` (`None`'s old "defaults to SELF"
behavior is now `EntityInputRef::SelfEntity`, made explicit rather than
implicit). `FactValue::EntityRef(EntityId)` (R017 §17) is replaced by
`FactValueInput::Entity(EntityInputRef)` — the object side gains the
exact same New/Existing/Self power as the subject side:

```
FactValueInput {
    ScalarJson(String),
    Entity(EntityInputRef),
}
```

(`FactValueInput` replaces R017's `FactValue` everywhere it appeared —
including inside `CorrectMemoryPayload.value`, §6 — one type, reused,
not two parallel ones.)

**Resolution rules, frozen:**

- `SelfEntity` → resolves to the canonical SELF Entity (guaranteed to
  already exist by the bootstrap invariant, §3 — a simple lookup, no
  creation path needed here).
- `Existing(id)` → the Entity must already exist; else a typed
  `ReferencedEntityNotFound` validation error (new name, matching the
  existing `ReferencedGoalNotFound`/`ReferencedProjectNotFound` naming
  convention), zero writes.
- `New(data)` → `entity_type`/`canonical_name` validated (non-empty,
  trimmed, within the bounds frozen in §14) → if `domain_ref` is
  present, both its fields are validated and Core checks **no existing
  Entity already represents that exact `(domain_ref_type,
  domain_ref_id)` pair** (a pre-check query, matching every other M2
  "friendly typed error before the raw DB constraint" pattern, e.g.
  `CreateProject`'s `ReferencedGoalNotFound` check ahead of the real
  FK) — if one already does, a new, structured
  `DuplicateDomainReference` validation error is returned, **zero
  writes, no silent merge/reuse of the pre-existing Entity**. Otherwise
  the new Entity row is created **inside the same `AddMemory`
  transaction** as everything else.

**Atomicity, frozen (extends R017 §17's mutation semantics):** a single
`AddMemory` call may therefore atomically create, in order: (1) a new
subject `Entity` if `subject = New(...)`, (2) a new object `Entity` if
`value = Entity(New(...))`, (3) the `Source` row if `New`, (4) the
`Fact` row, (5) the `Evidence` row, (6) the `fact_evidence` link row,
(7) the `fact_audit_log` row. **Any failure at any step rolls back
everything, including any newly-created Entity rows** — a forced-
failure test proving the Entity rows disappear too (not just Fact/
Evidence) is required Slice B acceptance evidence (§18), since this is
new behavior R017's original Slice B acceptance (§29) never anticipated.

**No entity aliasing/merging is implemented or implied here** (unchanged
from R017 §4/§6's explicit deferral) — `DuplicateDomainReference`
rejects the ambiguous case outright rather than silently picking a side.

A rare same-transaction edge case is worth naming, not hidden: if a
single `AddMemory` call requests **both** a new subject Entity **and**
a new object Entity that happen to share the identical `domain_ref`,
the pre-check for each runs against already-committed state and cannot
see the other's still-uncommitted insert — the real
`UNIQUE(domain_ref_type, domain_ref_id)` DB constraint (R017 §4) is
what actually catches this specific pathological case, surfacing as a
typed `StorageError` rather than a silently-accepted duplicate. This is
the same "friendly pre-check first, real DB constraint as defense-in-
depth behind it" layering used everywhere else in M2 — not a gap, an
expected division of labor.

---

## 5. Corrected `AddMemory` semantics (inferred confirmation)

**Frozen precisely (resolving Finding 4):** the persisted
`epistemic_state` after an inferred `AddMemory` candidate is confirmed
is **unconditionally `USER_CONFIRMED`** — the model's originally-
proposed `epistemic_state` (typically `INFERRED`, describing where the
*candidate* came from) is **not** carried into the durable row verbatim.
Approving the exact proposed claim **is** the semantic-confirmation act,
not merely permission to store an unconfirmed inference: the user isn't
saying "you may remember that you have a hunch about this," they're
saying "yes, that's true."

**This override applies only when the action actually blocked and was
then explicitly approved** (i.e. `Provenance::Inferred` produced a real
confirmation step). A **directly** `Provenance::Explicit` `AddMemory`
call (never blocked at all) persists **exactly** whatever
`epistemic_state` its payload specifies — this is the one and only path
by which a deliberately tentative memory can be recorded, per R017's
brief itself: the user explicitly, directly (not via confirming a
model's proposal) calls `AddMemory` with `memory_type = HYPOTHESIS`,
`epistemic_state = INFERRED`, requesting exactly that. **Unconfirmed
inference never silently becomes `USER_CONFIRMED`; only an actual user
confirmation act permits that specific transition — and when it
happens, it is unconditional, not a suggestion the row might still
carry the model's original tag.**

`AddMemoryPayload.epistemic_state` (R017 §17) is therefore reframed,
not removed: it is the value used **only** for a directly `Explicit`
call; for an `Inferred` call that later gets confirmed, Core overrides
it to `USER_CONFIRMED` regardless of what the payload originally
carried, immediately before the write.

---

## 6. Corrected `CorrectMemory` payload / epistemic-state behavior

**Resolves Finding 3 (the payload/prose contradiction) and Finding 12
(the confirmation-class gap) together, since they are the same
underlying issue: what epistemic state can a successor Fact ever have,
and under what conditions.**

**Frozen: `CorrectMemory` does not accept an arbitrary successor
`epistemic_state` — it never did in R017's actual payload shape (§18);
only the prose was wrong, and is corrected here, not the shape.** A
`CorrectMemory` call that actually executes **always** produces a
successor with `epistemic_state = USER_CONFIRMED` — hardcoded by Core,
never caller-suppliable, in every case:

- **Directly `Provenance::Explicit`** (a user-driven correction,
  "actually, that's wrong, it's really X") → executes with the
  `IMPORTANT_CHANGE` acknowledgement step (unchanged, R003 §F's
  existing default behavior for that class) → successor is
  `USER_CONFIRMED`.
- **`Provenance::Inferred`** (a model-proposed correction) →
  **Personal-Knowledge-specific overlay, frozen here, new to this
  document:** regardless of what the generic `effective_class =
  max(type_floor, provenance_class)` formula alone would compute
  (Finding 12: `ImportantChange(3)` already dominates
  `InferredChange(2)`, so the generic ladder alone would *not* force a
  pre-write block), **every one of the four Memory actions, when
  `Provenance::Inferred`, unconditionally blocks with zero durable
  writes before an explicit user confirmation — regardless of the
  action's own type-floor.** This is a narrow, explicitly-documented
  safety overlay specific to Personal Knowledge (analogous in shape to
  the existing orthogonal `SENSITIVE` overlay, which also "only ever
  raises... never lowers" a requirement, R003 §F) — **it changes
  nothing about `ConfirmationClass`, `effective_class`, or any other
  M2 action's behavior**, which stay exactly as already implemented;
  it is a documented interpretation the future Personal Knowledge
  action-layer code must apply explicitly (an `if provenance ==
  Inferred { block unconditionally }` check ahead of/alongside the
  generic `effective_class` call), not a change to shared confirmation
  infrastructure. Once the user approves the **exact** proposed
  correction, the successor is written as `USER_CONFIRMED` — same
  outcome as the explicit path, same reasoning as §5's `AddMemory`
  override.

**There is no v0.1 path — direct or inferred — where `CorrectMemory`
produces a durable successor with any `epistemic_state` other than
`USER_CONFIRMED`.** A tentative model hypothesis that should be
retained *without* replacing current knowledge is explicitly **not**
`CorrectMemory`'s job (R017's brief itself: "use `AddMemory` as
`HYPOTHESIS`/`INFERRED` — do NOT `CorrectMemory`") — `CorrectMemory`
is reserved for genuine, user-affirmed corrections only.

---

## 7. `confidence` — `FieldPatch<f32>`, not `Option<f32>`

**Resolves Finding 5.** `CorrectMemoryPayload.confidence` (R017 §18)
changes from `Option<f32>` to `FieldPatch<f32>` — reusing the exact
`FieldPatch<T>` type already frozen for every other genuinely-nullable
M2 field (R006):

- `Unchanged` → successor inherits the predecessor's `confidence`
  verbatim.
- `Set(value)` → successor gets `Some(value)` (still bounds-checked
  `0.0..=1.0`, unchanged).
- `Clear` → successor gets `NULL` — the one case `Option<f32>` could
  never express (Finding 5).

**`memory_type`/`sensitivity` correctly stay `Option<MemoryType>`/
`Option<Sensitivity>`, unchanged from R017 — no `Clear` invented for
either.** Both are **non-nullable** columns (R017 §5, `memory_type TEXT
NOT NULL`/`sensitivity TEXT NOT NULL`) — there is no `NULL` state to
clear *to*, so `None` = inherit / `Some(x)` = set is already the
complete, correct semantics, exactly matching the existing rule
"`FieldPatch<T>` only for genuinely nullable fields; plain `Option<T>`
for non-nullable ones" (R011 §2, reused verbatim here, not reinvented).

**Corrected `CorrectMemoryPayload`:**

```
CorrectMemoryPayload {
    predecessor_fact_id: FactId,
    value: FactValueInput,               // required (§4's type, reused)
    memory_type: Option<MemoryType>,     // None -> inherit, Some -> set
    confidence: FieldPatch<f32>,         // Unchanged -> inherit, Set -> replace, Clear -> NULL
    sensitivity: Option<Sensitivity>,    // None -> inherit, Some -> set
    source: SourceRef,
    evidence: NewEvidenceData,
}
```

(No `epistemic_state` field — §6 above.)

---

## 8. Canonical active-Fact predicate

**Resolves Finding 6.** One canonical predicate, named once, reused
everywhere active-Fact status is checked — never re-derived slightly
differently at different call sites:

```
is_active_fact(fact) :=
    fact.valid_until IS NULL
    AND fact.epistemic_state <> 'STALE'
```

- `CorrectMemory`'s predecessor-must-be-active check (R017 §18) uses
  this exact predicate, not merely `valid_until IS NULL`.
- `ConfirmMemory`'s target-must-be-active check (R017 §19) uses this
  exact predicate, not merely `valid_until IS NULL`.
- The retrieval contract's "active knowledge" scope (R017 §22, which
  already stated the two-part form correctly) is the **same** predicate,
  now explicitly named as such rather than a coincidentally-identical
  separate definition three sections away.

**Why both halves matter (unchanged reasoning from R017 §22, now
applied consistently):** a `STALE` row with an inconsistent `NULL`
`valid_until` (an edge case that should not normally occur, but which
Core must not silently trust) must not be treated as active merely
because one half of the invariant happens to match. Implemented once
(a single Core function or a single reused SQL fragment
`WHERE valid_until IS NULL AND epistemic_state <> 'STALE'`), never
duplicated with drift risk.

---

## 9. `ConfirmMemory` and `DISPUTED` Facts — corrected scope

**Resolves Finding 7.** `ConfirmMemory(DISPUTED target)`:

- The **target** Fact's `epistemic_state` becomes `USER_CONFIRMED`.
- **Sibling conflicting Facts are untouched** — their own
  `epistemic_state` (commonly still `DISPUTED`) does not change as a
  side effect.
- **The overall contradiction may still exist** — confirming one side
  of a disagreement does not retroactively declare the matter settled
  system-wide.
- **Retrieval must continue to surface any remaining `DISPUTED`
  siblings** (R017 §22's existing rule, unchanged) — a Context Pack
  must not silently drop a still-`DISPUTED` sibling just because a
  *different* Fact about the same subject/predicate was just confirmed.

**`ConfirmMemory` confirms exactly one assertion. It is not a
conflict-resolution workflow.** A future orchestration that resolves an
entire disputed set (e.g. confirming the winner and separately
`ForgetMemory`-ing or `CorrectMemory`-ing the loser(s)) would compose
**multiple** existing actions — **no new public action is frozen here**
for that orchestration, matching this run's explicit constraint.

---

## 10. `ForgetMemory` — scrubbing pre-existing audit snapshots

**Resolves Finding 8 — critical.** Before deleting the target Fact,
`ForgetMemory`'s transaction must first neutralize any payload-bearing
`previous_snapshot` values already written by **earlier**
`CorrectMemory`/`ConfirmMemory` calls against the same Fact:

```sql
UPDATE fact_audit_log
SET previous_snapshot = NULL
WHERE fact_id = :target_fact_id
  AND previous_snapshot IS NOT NULL;
```

**Everything else in those older rows is left untouched** — `id`,
`fact_id`, `action_type`, `actor`, `provenance`, `confirmation_class`,
`created_at` are all pure structural metadata; `previous_snapshot` is
the **only** `fact_audit_log` column that can ever carry serialized
semantic content (R017 §5/§18-19's `serialize_required_snapshot`
convention), so scrubbing that one column is sufficient — no other
column needs review.

The **new** `ForgetMemory` audit row is still inserted exactly as R017
§20 already froze — `previous_snapshot = NULL`, unconditionally, no
exception. The scrub above and the new row's own `NULL` value are two
separate, both-necessary steps (§14's rewritten algorithm, step 4 vs.
step 12).

---

## 11. Inbound supersession-link handling before deletion

**Resolves Finding 9 — critical.** Before deleting target Fact `T`,
find every Fact `P` where `P.superseded_by_fact_id = T.id` (i.e. every
Fact `T` itself superseded) and, for each:

- Set `P.superseded_by_fact_id = NULL`.
- Leave `P.valid_until` **unchanged**.
- Leave `P.epistemic_state` **unchanged** (normally already `STALE`).
- Update `P.updated_at`.

This is not a new exception to the Fact mutability contract (R017
§5) — `superseded_by_fact_id`/`updated_at` were already in the
explicitly-mutable set; this simply adds `ForgetMemory` as a second
legitimate caller of that existing mutation (previously only
`CorrectMemory` ever set this field, to point *at* a successor; now
`ForgetMemory` may also null it out when that successor is the one
being forgotten).

**Deliberately not rewired to skip to `T`'s own successor.** If `T`
itself had been superseded by some Fact `S` (`T.superseded_by_fact_id
= S.id`), `P` is **not** silently repointed at `S`. `ForgetMemory`
intentionally creates a privacy-driven hole in historical navigation —
pretending `P` was directly superseded by `S` would misrepresent
history (it wasn't; `T` was in between, and `T` is now gone by the
user's explicit request). `T`'s own outgoing `superseded_by_fact_id`
(pointing at `S`) causes no deletion problem on its own, since `T`'s
row is simply removed along with it.

**Consequence for the FK:** `facts.superseded_by_fact_id ... ON DELETE
RESTRICT` (unchanged, R017 §5) no longer blocks deletion, because by
the time `DELETE FROM facts WHERE id = T.id` runs, nothing still points
at `T` — the inbound reference was explicitly cleared first, in-
transaction, by Core, never by relying on the database to cascade or
reject.

---

## 12. Evidence — privacy-first true deletion, not conditional retention

**Resolves Finding 10.** R017 §20 step 4's "retain if still referenced
elsewhere" rule is replaced with an unconditional rule:

**All `Evidence` directly linked to the forgotten Fact (via
`fact_evidence`) is deleted, full stop — regardless of whether it is
also currently linked to some `Relation`/`Episode` via
`relation_evidence`/`episode_evidence`.** Concretely: every join row in
`fact_evidence`/`relation_evidence`/`episode_evidence` that references
one of the collected `evidence_id`s is deleted first (satisfying the FK
`RESTRICT` on `evidence_id`, mirroring §11's reasoning for
`superseded_by_fact_id`), then the `Evidence` rows themselves are
deleted.

**Rationale (privacy beats evidence retention):** without content-level
evidence splitting/redaction — not designed in v0.1 — retaining a
shared `Evidence` row cannot guarantee the forgotten payload is
actually gone, since that row may itself contain the exact forgotten
text (R017 §11's own privacy split puts personal/sensitive excerpts in
`Evidence`, not `Source`). This may incidentally remove supporting
evidence from a different, still-retained `Relation`/`Episode` — that
is an accepted, explicitly-named side effect; **those other knowledge
objects are never themselves deleted**, only one piece of their
supporting evidence.

**Practical scope note, not a loophole:** for v0.1's actually-frozen
action set, `AddMemory`/`CorrectMemory` **already** always create a
fresh `Evidence` row rather than reusing one (R017 §17-18,
`NewEvidenceData`) — so in every currently-reachable state, Evidence
tied to a Fact is claim-local anyway, and this stricter rule is a
robustness/future-proofing guarantee (once `relation_evidence`/
`episode_evidence` are ever actually populated by future slices F/G)
more than something expected to bite in v0.1's own reachable states.
`ForgetMemory` must never attempt semantic/AI analysis to *guess*
whether a shared Evidence row contains the forgotten value — the
unconditional deletion rule exists precisely so that judgment call is
never needed.

---

## 13. `Source` — sensitivity and lifecycle, corrected

**Resolves Finding 11.** `Source` remains **metadata-only** — this part
of R017 §10/§13-14 is unchanged; `Evidence` remains the only place a
payload/snippet/excerpt may live. What changes: **metadata itself can
still be privacy-sensitive** (a document path, a label, a conversation
reference can each reveal sensitive context even with no excerpt
attached) — the R017 claim that Source retention "carries no privacy
risk" is corrected, not merely softened (§16).

**New field, frozen:**

```
sources (
    ...,
    sensitivity  TEXT NOT NULL,   -- same 4-value closed set as Fact/Relation/Episode/Evidence
    ...
);
-- CHECK sensitivity IN ('PUBLIC', 'INTERNAL', 'SENSITIVE', 'NEVER_LEAVES_DEVICE')
```

`Entity` remains the one table with **no** `sensitivity` field, and
stays that way in v0.1 — it is structural identity metadata (an id, a
type, a name), not itself a knowledge assertion; nothing in this
correction pass requires or justifies adding one. (Not silently added
elsewhere either — `sensitivity` now applies to exactly `Fact`/
`Relation`/`Episode`/`Source`/`Evidence`, five tables, corrected from
R017's four.)

**`ForgetMemory`'s Source cleanup, new, frozen (as part of §14's
rewritten algorithm):** after the collected Evidence rows are deleted,
for each `source_id` collected earlier, check whether that `Source` now
has **zero** remaining `Evidence` rows referencing it
(`SELECT COUNT(*) FROM evidence WHERE source_id = ?`, within the same
transaction). If zero → delete the `Source`. If still referenced by
Evidence belonging to other, retained knowledge → leave it untouched.

**Net effect:** orphan Source metadata created *only* for the forgotten
knowledge is removed; shared Source metadata that other, retained
knowledge still depends on survives. "Never delete Source" is removed
from the freeze entirely.

---

## 14. `ForgetMemory` — final corrected atomic transaction

**Supersedes R017 §20's 9-step algorithm in full — this is the
authoritative sequence:**

1. Validate target Fact `T` exists — else `NotFound`, zero mutation.
2. Collect every `evidence_id` linked to `T` via `fact_evidence`.
3. Collect the `source_id` of each collected Evidence row.
4. **Scrub** all payload-bearing `previous_snapshot` values from
   existing `fact_audit_log` rows for `T` (§10) — before anything else
   is deleted.
5. **Clear inbound supersession references**: for every Fact `P` with
   `P.superseded_by_fact_id = T.id`, set it `NULL` and touch
   `updated_at` (§11).
6. Delete every `fact_evidence`/`relation_evidence`/`episode_evidence`
   row referencing any of the collected `evidence_id`s (§12).
7. Delete the collected `Evidence` rows themselves (§12).
8. Delete any of the collected `Source` rows that now have zero
   remaining `Evidence` references (§13).
9. Delete any remaining `fact_evidence` rows for `T` defensively (a
   redundant, idempotent safety net — step 6 should already have
   removed them via the evidence ids collected in step 2, but this
   guards against any edge case where that link was not otherwise
   captured).
10. `DELETE FROM facts WHERE id = T.id` — the actual permanent removal.
    (Legal at this point: step 5 cleared every inbound
    `superseded_by_fact_id` reference; steps 6-9 cleared every
    `fact_evidence` reference; `T`'s own outgoing
    `superseded_by_fact_id`, if any, causes no problem, since `T`'s row
    is simply gone along with it.)
11. **Never** delete subject/object `Entity` rows (unchanged, R017
    §20).
12. Insert the minimal `ForgetMemory` audit marker: `fact_id = T.id`,
    `action_type = 'ForgetMemory'`, `previous_snapshot = NULL`
    (unconditional, no exception), `actor`/`provenance`/
    `confirmation_class`/`created_at` as usual — no semantic payload
    anywhere in this row.
13. Commit.

**Any failure at any step → full rollback of the entire transaction** —
never a partially-forgotten Fact, never a `Success` result alongside a
half-applied deletion. **No Undo after a confirmed `ForgetMemory`
executes** (unchanged, R017 §20/§21). **No tombstone containing the
payload anywhere** (unchanged). Any future embeddings/derived indexes
for `T` or its deleted Evidence must also be invalidated once those
systems exist (unchanged scope note, R017 §26).

---

## 15. Exact field bounds — frozen v0.1 constants

R017 left `Fact.value_json` and several other fields as merely
"bounded... exact number set at implementation time" (R017 §5/§28) —
this would have forced the eventual `V7`/Core implementation to invent
schema policy mid-implementation, exactly what an architecture freeze
exists to prevent. Existing M2 conventions, verified directly against
`domain/mod.rs`, provide three established tiers:
`TITLE_MAX_LEN = 200`, `DESCRIPTION_MAX_LEN = 4000`,
`DOMAIN_TAG_MAX_LEN = 60` (all currently Core-validation-only — M2 has
no precedent for a DB-level `CHECK(length(x) <= N)`; only non-emptiness,
enum membership, and numeric bounds are DB-`CHECK`-enforced, e.g.
`CHECK (length(trim(title)) > 0)` in `V1__create_goals.sql`. Personal
Knowledge's bounds follow the identical convention — Core-validation-
only, no new DB-level length `CHECK` pattern introduced).

| Constant | Field | Max chars | Tier / rationale |
|---|---|---|---|
| `ENTITY_TYPE_MAX_LEN` | `Entity.entity_type` | 60 | Short open-vocabulary word — matches `DOMAIN_TAG_MAX_LEN` |
| `CANONICAL_NAME_MAX_LEN` | `Entity.canonical_name` | 200 | A name/label — matches `TITLE_MAX_LEN` |
| `DOMAIN_REF_TYPE_MAX_LEN` | `Entity.domain_ref_type` | 60 | Short module/type name — matches `DOMAIN_TAG_MAX_LEN` |
| `DOMAIN_REF_ID_MAX_LEN` | `Entity.domain_ref_id` | 200 | An id string (UUIDv7 is 36 chars; generous headroom) — matches `TITLE_MAX_LEN` |
| `PREDICATE_MAX_LEN` | `Fact.predicate` | 100 | Short identifier-shaped open vocabulary, slightly more room than a bare tag (e.g. `prefers_communication_channel`) — new intermediate tier |
| `VALUE_JSON_MAX_LEN` | `Fact.value_json` | 2000 | A deliberately **small** scalar/structured JSON value (R017 §5's own framing) — new tier, well short of description-length, to structurally discourage using `Fact` as a document store |
| `RELATION_TYPE_MAX_LEN` | `Relation.relation_type` | 100 | Same reasoning/tier as `predicate` |
| `RELATION_NOTE_MAX_LEN` | `Relation.note` | 4000 | Free-text elaboration — matches `DESCRIPTION_MAX_LEN` (the same bound `GoalRelation.note` already reuses, `actions/mod.rs` line ~594) |
| `EPISODE_TITLE_MAX_LEN` | `Episode.title` | 200 | Matches `TITLE_MAX_LEN` |
| `EPISODE_SUMMARY_MAX_LEN` | `Episode.summary` | 4000 | Matches `DESCRIPTION_MAX_LEN` |
| `SOURCE_TYPE_MAX_LEN` | `Source.source_type` | 60 | Short open-vocabulary word — matches `DOMAIN_TAG_MAX_LEN` |
| `SOURCE_LABEL_MAX_LEN` | `Source.label` | 200 | Matches `TITLE_MAX_LEN` |
| `SOURCE_REFERENCE_MAX_LEN` | `Source.reference` | 500 | A locator/pointer (path, URL, conversation id) — longer than a title, deliberately far short of description-length, staying metadata-only per §13 |
| `EVIDENCE_CONTENT_MAX_LEN` | `Evidence.content` | 4000 | The supporting excerpt/payload — matches `DESCRIPTION_MAX_LEN` |
| `EVIDENCE_LOCATOR_MAX_LEN` | `Evidence.locator` | 200 | A short pointer within a source — matches `TITLE_MAX_LEN` |

**The future `V7`/Core implementation must copy these exact values
rather than choosing new ones ad hoc** — that is the entire point of
freezing them now rather than leaving them for implementation time.

---

## 16. Corrected privacy wording — Source/Evidence claims

Every place R017 asserted or implied that `Source` retention "carries
no privacy risk," or that `Source` "structurally never holds sensitive
payload" in a way that implied metadata itself is inherently safe
(R017 §10, §20 step 6), is corrected to:

> `Source` remains metadata-only — it never holds a raw excerpt/
> payload, which is what `Evidence` is for. But metadata itself can
> still be privacy-sensitive (a label, a reference/locator can reveal
> sensitive context on their own). `Source.sensitivity` (§13) exists so
> the future Privacy Firewall can act on that. Orphan Source metadata
> (zero remaining Evidence referencing it) is removed during
> `ForgetMemory` (§13-14); Source still genuinely referenced by other,
> retained knowledge is left in place.

No claim in this document treats metadata as inherently non-sensitive.

---

## 17. R017 addendum — do not silently rewrite history

Appended verbatim to the end of R017 (§18 below describes the actual
edit) — the original R017 text above the addendum is **not** modified:

> ## Addendum (R018, 2026-09-18) — architecture consistency corrections
>
> A dedicated post-freeze review
> (`docs/reports/R018_M2_PERSONAL_KNOWLEDGE_ARCHITECTURE_CONSISTENCY_HARDENING_2026-09-18.md`)
> found eleven consistency/privacy gaps in this report before any `V7`
> migration or implementation slice was written, all fixed in R018:
>
> - The SELF partial unique index (§4 above) guarantees *at most one*
>   row, never existence — R018 freezes a Core-level `ensure_self_entity`
>   bootstrap to guarantee *at least one*, together giving *exactly one*.
> - `AddMemory` (§17 above) had no path to create a *new* Entity — every
>   subject/object reference required one to already exist. R018 freezes
>   a typed `EntityInputRef` (`SelfEntity`/`Existing`/`New`) usable
>   inside `AddMemory` (and `CorrectMemory`'s value), with new-Entity
>   creation folded into the same atomic transaction.
> - `CorrectMemory`'s payload (§18 above) had no `epistemic_state`
>   field, but its mutation prose said the successor's epistemic state
>   was "the caller's requested starting state" — a direct
>   contradiction. R018 freezes that a `CorrectMemory` successor is
>   *always* `USER_CONFIRMED`, with a Personal-Knowledge-specific
>   confirmation overlay ensuring an inferred correction still blocks
>   before writing (the generic `effective_class` formula alone would
>   not have forced that block for this specific action's type-floor).
> - `CorrectMemoryPayload.confidence` (§18 above) used `Option<f32>`,
>   which cannot express "clear the old confidence back to `NULL`."
>   R018 freezes `FieldPatch<f32>` instead, matching M2's established
>   nullable-field convention.
> - `CorrectMemory`/`ConfirmMemory`'s "is this Fact active" checks (§18-19
>   above) validated only `valid_until IS NULL`, not the full two-part
>   active-Fact definition this report's own §22 already used elsewhere.
>   R018 freezes one canonical `is_active_fact` predicate, reused
>   everywhere.
> - `ConfirmMemory` confirming a `DISPUTED` Fact (§19 above) was
>   described as "a resolution path for a contradiction" — overstated.
>   R018 clarifies it resolves only the one target assertion; sibling
>   disputes remain, and retrieval must keep surfacing them.
> - **Critical:** `ForgetMemory` (§20-21 above) only guaranteed its own
>   *new* audit row had no payload — it never accounted for pre-existing
>   `CorrectMemory`/`ConfirmMemory` audit rows for the same Fact, which
>   this report's own §18-19 require to carry mandatory, non-`NULL`,
>   payload-bearing snapshots. R018 freezes a mandatory scrub step
>   (`UPDATE fact_audit_log SET previous_snapshot = NULL WHERE fact_id =
>   ... AND previous_snapshot IS NOT NULL`) before the Fact is deleted.
> - **Critical:** `facts.superseded_by_fact_id ... REFERENCES facts(id)`
>   (§5 above) has no `ON DELETE` clause, defaulting to `RESTRICT` —
>   meaning `ForgetMemory` on any Fact that was ever someone's successor
>   would be rejected outright by the database. R018 freezes an explicit
>   step clearing every inbound reference before deletion, deliberately
>   *not* rewiring predecessors to point at the forgotten Fact's own
>   successor.
> - `ForgetMemory`'s Evidence handling (§20 above) retained Evidence
>   still linked elsewhere — but that Evidence could itself contain the
>   forgotten payload. R018 freezes unconditional deletion of all
>   Evidence directly linked to the forgotten Fact, accepting that this
>   may remove supporting evidence from other, still-retained knowledge.
> - `Source` (§10, §20 above) was frozen with no `sensitivity` field and
>   described as carrying "no privacy risk" and never deleted. R018
>   corrects this: `Source` gains a `sensitivity` column (metadata can
>   still be sensitive), and `ForgetMemory` now deletes orphaned Source
>   rows while retaining genuinely shared ones.
> - This report's field-level freeze (§5, §28 above) still left several
>   fields as merely "bounded... exact number set at implementation
>   time." R018 freezes exact numeric maximum lengths for every bounded
>   Personal Knowledge field now.
>
> See R018 for the full corrected design, the rewritten `ForgetMemory`
> transaction algorithm, and updated Slice A/B acceptance criteria.
> R018 is authoritative wherever it corrects this report.

---

## 18. Updated Slice A/B acceptance criteria

**Corrected Slice A** (supersedes R017 §29's Slice A row):

- `V7` Personal Knowledge foundation: `Entity`, `Source` (including
  `sensitivity`), `Evidence`, `Fact`, `fact_evidence`, `fact_audit_log`
  — all exact `CHECK`s/bounds from this report — plus the
  `ensure_self_entity` bootstrap step and narrow reads (`get_fact`,
  `get_entity`).
- **Acceptance:**
  - Fresh bootstrap → exactly one SELF Entity, proven as the combined
    Core+DB invariant (§3).
  - Reopen → same SELF `EntityId` (§3).
  - Raw-SQL duplicate `is_self = 1` insert rejected (§3).
  - `ensure_self_entity` proven idempotent across repeated calls (§3).
  - A Fact with **neither** `value_json` nor `object_entity_id` set is
    rejected by the one-semantic-object `CHECK` (R017 §5).
  - A Fact with **both** set is rejected by the same `CHECK`.
  - `confidence` out-of-bounds rejected, on both `facts` (already
    frozen) and, once Slice F exists, `relations`.
  - `sensitivity` invalid-value rejected on `facts`, and now also on
    `sources` (§13).
  - Every exact length bound in §15's table is proven Core-side
    (reject-at-the-limit-plus-one style, matching the existing
    `req_up12_13`-style overlong-field tests) — no DB-level length
    `CHECK` exists to separately verify, per §15's explicit convention
    note.
  - `V6`→`V7` migration proven non-destructive (a populated `V6`
    database upgrades cleanly, matching every prior migration-
    compatibility test in M2).
  - `services::storage` privacy boundary re-verified (the established
    illegal-access-probe proof, repeated against the new module shape).

**Corrected Slice B** (supersedes R017 §29's Slice B row):

- `AddMemory`, using `EntityInputRef` (`SelfEntity`/`Existing`/`New`)
  for both subject and object; new-Entity creation (subject and/or
  object) folded into the same atomic transaction as `Source`(new-or-
  existing)/`Fact`/`Evidence`/`fact_evidence`/audit.
- **Acceptance:** everything R017 §29's original Slice B already
  required (full atomic creation proof; forced-audit-failure rollback;
  `Provenance::Inferred` blocks with zero writes, confirmed
  resubmission succeeds, persisting `USER_CONFIRMED` per §5) **plus**:
  a forced-failure test proving a **newly-created Entity row** (subject
  and/or object) is rolled back along with everything else when a
  later step in the same transaction fails — not merely that Fact/
  Evidence disappear (§4).

---

## 19. Remaining genuinely deferred questions

Everything R017 §26/§28 already deferred remains deferred, unchanged by
this pass (locked/pinned memory, `last_used` tracking, memory
importance, entity alias/resolution, ontology management, embeddings,
`Relation`/`Episode`'s own action families, bulk forget, backdating
`valid_from`, the Memory UI). Additionally, genuinely open after this
pass:

- **Whether a future dedicated cleanup job should periodically prune
  Source rows that become orphaned through some path *other* than
  `ForgetMemory`** (e.g. if a future action ever hard-deletes a
  Relation/Episode independently) — out of scope; no such action exists
  in the frozen v0.1 set.
- **Whether `Entity` should eventually gain its own `sensitivity`
  field** once a real use case demonstrates identity metadata itself
  needs privacy classification beyond what `domain_ref`/`canonical_name`
  bounds already limit — explicitly not added speculatively here (§13).
- **The exact Core error taxonomy for the new validation errors**
  (`ReferencedEntityNotFound`, `DuplicateDomainReference`,
  `FactNotActive`, `FactAlreadyConfirmed`) — named and scoped here,
  but their exact `ValidationError` enum variant shape is an
  implementation detail for Slice A/B, not frozen further in this docs-
  only pass.

---

## 20. Exact recommended next implementation slice

**Unchanged from R017 §29's conclusion, now on a corrected foundation:**
Slice A (`Entity`+`Source`+`Evidence`+`Fact` foundation, this report's
corrected schema/bootstrap/bounds) immediately followed by Slice B
(`AddMemory`, this report's corrected `EntityInputRef`/atomicity/
confirmation semantics). **Not started in this run** — still
documentation only, per this run's explicit constraint.

---

## Addendum (R019, 2026-09-18) — final privacy/foundation corrections

A final pre-`V7` consistency pass
(`docs/reports/R019_M2_PERSONAL_KNOWLEDGE_PRIVACY_AND_FOUNDATION_FINALIZATION_2026-09-18.md`)
found and corrected six further gaps before any migration or
implementation existed:

- **`Entity` needed a `sensitivity` field too**, by this report's own
  reasoning for `Source` (§13 above) — metadata like
  `canonical_name`/`domain_ref` can itself be sensitive. R019 freezes
  it, using the same four-value set, structurally independent of
  every other table's own `sensitivity`.
- **`ForgetMemory`'s "never delete subject/object Entity rows" rule
  (§20 above) was too broad** once `AddMemory` could create brand-new
  Entities (§4 above) — a forgotten Fact's only mention of a
  newly-created, sensitive Entity could otherwise survive forever.
  R019 freezes a deterministic orphan-Entity cleanup step: delete a
  captured non-SELF Entity only if no surviving Fact/Relation/
  `episode_entities` reference remains.
- **The same-payload duplicate-`domain_ref` case** (two `New()`
  Entity inputs in one `AddMemory` call requesting the identical
  domain reference) was left to the real DB constraint alone. R019
  freezes an explicit Core preflight check across the whole payload,
  ahead of the DB constraint, matching every other duplicate-
  detection case in this document.
- **R017's and R018's stated `V7` table scope disagreed** — R017 froze
  all eleven tables (six entities + four join tables) as schema-ready;
  R018's corrected Slice A acceptance listed only six. R019 resolves
  this explicitly: `V7` creates all eleven tables from day one, with
  `Relation`/`Episode`'s own action families still deferred - table
  existence is not mutation permission.
- **`NewSourceData`/`NewEvidenceData`'s exact shapes**, left unfrozen
  after `Source.sensitivity` was added, are now specified in full.
- **"True/permanent deletion" was never explicitly separated into a
  logical guarantee (what LifeOS's own code/queries provide) versus a
  forensic-storage guarantee (what the underlying encrypted SQLite
  file's actual, inspected configuration provides).** Direct
  inspection found this project currently sets neither
  `journal_mode` nor `secure_delete` anywhere — R019 freezes the
  logical guarantee (fully upheld) as the correct thing to tell a
  user, explicitly declines to claim an unverified forensic guarantee,
  and freezes a concrete, actionable future hardening step
  (`PRAGMA secure_delete = ON`, set-and-verified, matching the
  existing `enable_foreign_keys` precedent) rather than promising
  something physical-media-level that no application code can
  actually guarantee.

See R019 for the full corrected `ForgetMemory` transaction (now 14
steps), the resolved `V7` table list, and the complete logical-vs-
forensic deletion analysis. R019 is authoritative wherever it corrects
R017 or R018.
