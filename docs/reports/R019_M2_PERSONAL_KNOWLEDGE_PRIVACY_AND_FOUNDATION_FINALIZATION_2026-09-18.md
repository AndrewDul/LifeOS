# R019 — M2 Personal Knowledge Privacy and Foundation Finalization

**Documentation/architecture only. No Rust or React code changed. No
migration created.** The final consistency pass on the Personal
Knowledge Layer before any `V7` migration or implementation slice is
written. Closes a remaining privacy gap R018 itself left open (Entity
metadata can be sensitive too, not just Source metadata), specifies an
orphan-Entity cleanup step `ForgetMemory` was missing entirely,
resolves a genuine ambiguity between R017's and R018's stated `V7`
table scope, freezes the exact payload shapes for `Source`/`Evidence`
creation now that both carry `sensitivity`, and — the most consequential
correction — separates **what "true deletion" actually guarantees**
into two explicitly distinct claims: a logical/product privacy
guarantee (required, and now fully specified) and a forensic-storage
guarantee (honestly evaluated against this project's actual, inspected
configuration, not assumed). **R017's and R018's original text is not
rewritten** — a one-line forward pointer is added to R017's existing
addendum, and a full, clearly labelled addendum is appended to R018
(§18 below); this report is authoritative wherever any of the three
disagree.

---

## 1. Pre-run state

- R017 (Personal Knowledge Layer Architecture Freeze) and R018
  (Personal Knowledge Architecture Consistency Hardening) both
  completed. Local checkpoint `bf90368` — `docs: harden Personal
  Knowledge architecture consistency`. No Personal Knowledge
  implementation or migration exists. 262/262 existing implementation
  tests remain the current baseline (unaffected). Working tree clean.
  R001-R018 exist; this is **R019**, the next allocation.

---

## 2. Final review findings

Verified against R017/R018's actual current text, and against the
actual current storage code (`src/services/storage/mod.rs`,
`Cargo.toml`), by direct inspection — not modified in this pass:

1. R018 §13 corrected `Source` to carry `sensitivity` because "metadata
   itself can still be privacy-sensitive," then, in the very same
   report, froze "`Entity` remains the one table without a
   `sensitivity` field... structural identity metadata, not itself a
   knowledge assertion" (R018 §13). The same reasoning R018 applied to
   `Source.label`/`reference` applies just as directly to
   `Entity.canonical_name`/`domain_ref_type`/`domain_ref_id` — a name
   like `"Dr. [Name], Oncologist"` or a `domain_ref` pointing at a
   private `Project` is exactly the kind of metadata-that-is-itself-
   sensitive R018 had just finished arguing `Source` needed protecting
   against. R018's own stated reasoning contradicts its own stated
   conclusion for `Entity` specifically.
2. R018 §20's corrected `ForgetMemory` algorithm (13 steps) froze
   "**Never** delete subject/object `Entity` rows" unconditionally,
   inherited unchanged from R017 §20. But `AddMemory`'s corrected
   design (R018 §4) can now create a **brand-new** Entity solely to
   serve as one Fact's subject or object. If that Fact is later
   forgotten, the Entity R018's own algorithm refuses to ever delete
   can be the **only** place the forgotten semantic content (a
   sensitive `canonical_name`) still lives — `ForgetMemory` would
   delete the Fact, its Evidence, its orphaned Source, and scrub its
   audit snapshots (R018 §10-14), yet leave the actual forgotten name
   sitting in a live `entities` row. A genuine, unaddressed gap in
   R018's own privacy guarantee.
3. R018 §4 froze the same-transaction `DuplicateDomainReference` case
   as something "naturally caught by the real DB constraint as
   defense-in-depth" rather than a Core-level pre-check — correct that
   the DB constraint *does* catch it, but leaving a deterministic,
   fully-predictable-in-advance input conflict (two `New()` entity
   inputs in the *same* payload requesting the *same* `domain_ref`) to
   surface only as a raw `StorageError` is inconsistent with every
   other duplicate-detection case in this same document, which all get
   a friendly pre-check first (R017 §24, R018 §4's own "Existing"-vs-
   "existing-in-DB" case).
4. R017 froze the full six-table-plus-four-join-table schema (`Entity`/
   `Fact`/`Relation`/`Episode`/`Source`/`Evidence` +
   `fact_evidence`/`relation_evidence`/`episode_evidence`/
   `episode_entities`) as "supporting schemas... frozen ahead of their
   own action family" (R017 §8/§9/§12). R018's corrected Slice A
   acceptance (§18) then listed only six of those eleven tables
   (`entities`/`sources`/`evidence`/`facts`/`fact_evidence`/
   `fact_audit_log`) — while R018's own rewritten `ForgetMemory`
   algorithm (§14) already references `relation_evidence`/
   `episode_evidence` by name in its join-row-deletion step. R017 and
   R018 do not actually agree on what `V7` creates. A genuine
   ambiguity, not resolved by either report alone.
5. Neither R017 nor R018 froze the exact payload shape for
   `NewSourceData`/finalized `NewEvidenceData` *after* `sensitivity`
   was added to `Source` (R018 §13) — R017 §17's `SourceRef`/
   `NewEvidenceData` sketches predate that field entirely.
6. **The most significant finding.** R017 §19-21 and R018 §10/§14 both
   use language like "true, permanent deletion" and "the original
   forgotten text/value never persists anywhere" without ever
   distinguishing *what LifeOS's own code/queries can guarantee*
   (a logical/product claim) from *what the underlying encrypted SQLite
   file's actual on-disk behavior guarantees* (a forensic/storage
   claim). Direct inspection of `src/services/storage/mod.rs` (grep-
   verified, not modified) confirms: **no `journal_mode` PRAGMA is ever
   set**, **no `secure_delete` PRAGMA is ever set or queried**
   anywhere in this codebase — only `key`, `foreign_keys`, `rekey`, and
   `integrity_check` are ever touched. Continuing to say "true
   deletion" without qualification, given this, risks implicitly
   promising a forensic-erasure guarantee this project's current
   configuration has not actually verified, let alone proven.

---

## 3. Entity sensitivity

**Resolves Finding 1.** `Entity` gains a `sensitivity` field, using the
exact same four-value closed set as every other Personal Knowledge
table:

```
entities (
    ...,
    sensitivity  TEXT NOT NULL,
    ...
);
-- CHECK sensitivity IN ('PUBLIC', 'INTERNAL', 'SENSITIVE', 'NEVER_LEAVES_DEVICE')
```

This is classification of the **Entity's own metadata** (its
`canonical_name`, its `entity_type`, its `domain_ref`) — it remains
**structurally independent** from the `sensitivity` of any `Fact`/
`Relation`/`Episode`/`Source`/`Evidence` that happens to reference or
support it. No automatic equality is assumed in either direction: a
`PUBLIC` Entity (e.g. a well-known organization) can still be the
subject of a `SENSITIVE` Fact, and a `SENSITIVE` Entity (e.g. a private
medical contact) can still be referenced by an `INTERNAL` Fact about an
unrelated, harmless topic. This mirrors the same "three/five separate
dimensions, never derived from one another" rule R017 §7 and R018 §13
already established for the other tables — now applied consistently
across all six.

---

## 4. Final SELF shape

**Resolves the canonical bootstrap row (extends R018 §3, unchanged
mechanism):**

```
entity_type      = "person"
canonical_name   = "SELF"
is_self          = true
sensitivity      = INTERNAL
domain_ref_type  = NULL
domain_ref_id    = NULL
```

`INTERNAL` — not `PUBLIC`, not `SENSITIVE` — is the correct default for
the canonical SELF row: it is genuinely personal (this Entity *is* the
user) but is not itself excerpt-bearing sensitive content the way a
medical/financial Fact would be; `INTERNAL` matches R017 §13's own
definition ("default-tier personal information... safe for local
processing; may be included in minimized cloud context when genuinely
necessary"), which is exactly SELF's correct treatment — a Context Pack
frequently needs to reference "the user" without that alone triggering
the strictest tier. `"SELF"` remains an internal canonical label
(unchanged, R018 §3) — a future UI may display "Me" or the user's name
without that being an identity change.

**Updated Slice A acceptance (extends R018 §18/R017 §29):**

- The bootstrap-created SELF Entity has `sensitivity = 'INTERNAL'`,
  proven directly (not merely that *some* value was set).
- A raw-SQL insert with an invalid `entities.sensitivity` value (outside
  the four-value set) is rejected by the `CHECK`, proven the same
  "attempt the illegal thing, capture the rejection" way as every other
  closed-enum proof in M2.

---

## 5. `NewEntityData` — final shape

**Resolves Finding 1's payload consequence.** `sensitivity` becomes
**required**, not optional, matching the exact "no silent default —
privacy classification is architectural, must never be guessed by
storage" principle R017 §17 already applied to `AddMemoryPayload`'s own
top-level `sensitivity` field:

```
NewEntityData {
    entity_type: String,
    canonical_name: String,
    sensitivity: Sensitivity,      // REQUIRED, no silent default
    domain_ref: Option<DomainRef>,
}
```

**Resolution rules, frozen:**

- `EntityInputRef::New(data)` → `data.sensitivity` is validated as a
  member of the closed set and used verbatim for the newly-created row.
- `EntityInputRef::Existing(id)` → the **existing** Entity's own
  `sensitivity` is used, untouched — `AddMemory`/`CorrectMemory` never
  silently mutate an existing Entity's `sensitivity` (or any other
  field) as a side effect of referencing it. Changing an existing
  Entity's own classification, if ever needed, is out of scope for
  these four actions (no `UpdateEntity` action exists, R017 §4).
- `EntityInputRef::SelfEntity` → resolves to the canonical SELF row,
  whose `sensitivity` is fixed at bootstrap (`INTERNAL`, §4) — not
  caller-suppliable through `AddMemory`/`CorrectMemory`.

---

## 6. Domain-ref validation — reaffirmed as one explicit ordered check

**Resolves Finding 3's context and ties R017 §24's existence check
together with R018 §4's duplicate check, which had previously been
described in two separate places.** For `EntityInputRef::New(data)`
where `data.domain_ref = Some(DomainRef { domain_ref_type,
domain_ref_id })`, the full ordered validation, frozen as one sequence:

1. **Syntax/bounds** — `domain_ref_type`/`domain_ref_id` are validated
   against §15's frozen length bounds (`DOMAIN_REF_TYPE_MAX_LEN`/
   `DOMAIN_REF_ID_MAX_LEN`, R018 §15) and basic non-empty/trimmed
   shape.
2. **Existence** — the referenced authoritative domain row is verified
   to actually exist, through the *existing* domain module's own read
   function (e.g. `project_repo::project_exists`) — the exact pattern
   already established for `Project.goal_id`/`Milestone.project_id`/
   `Task.project_id`/`Task.goal_id` (R017 §24, unchanged, reused not
   reinvented). No direct Personal Knowledge SQL ever touches a domain
   table.
3. **No duplicate Personal Knowledge pointer** — Core checks no
   existing Entity already represents this exact `(domain_ref_type,
   domain_ref_id)` pair (R018 §4's `DuplicateDomainReference` check,
   unchanged).
4. Only if all three pass does Core proceed to create the new Entity,
   inside the same `AddMemory`/`CorrectMemory` transaction.

**No silent dangling domain refs, no direct Personal Knowledge SQL into
domain tables** — both requirements were already individually true
across R017/R018; this section makes them one explicit, ordered
checklist so a future implementer cannot accidentally do step 3 without
step 2 (or vice versa).

---

## 7. Same-payload duplicate `domain_ref` — Core preflight, not just DB defense-in-depth

**Resolves Finding 3.** Before any write, `AddMemory`'s validation now
includes a preflight pass over **every** `New(...)` Entity candidate in
the **same** payload (currently at most two: subject and object —
`CorrectMemory` can only ever propose one, since its subject/predicate
are inherited, R018 §6): if two or more candidates specify the
identical `(domain_ref_type, domain_ref_id)` pair, Core rejects the
call with the same `DuplicateDomainReference` validation error **before
touching the database at all** — zero writes, matching every other
validation-failure path in this document.

**The real `UNIQUE(domain_ref_type, domain_ref_id)` DB constraint
(R017 §4) remains as defense-in-depth behind this preflight** — for
the case where two *separate*, non-simultaneous calls race (which the
in-payload preflight cannot see), the constraint is still what actually
prevents the duplicate, converted to a typed `StorageError`. This is
the same "friendly Core pre-check first, real DB constraint behind it"
layering used everywhere else in M2 (R008's FK-plus-application-check
precedent), applied one level earlier than R018 had left it. **The same
rule generalizes** to any future action proposing more than two new
Entity candidates at once (e.g. a future `Relation`-creating action with
two new endpoint Entities, slice F) — the preflight check is over "all
candidates in this one call," not hardcoded to exactly two.

---

## 8. Orphan-Entity deletion — `ForgetMemory`'s missing step

**Resolves Finding 2 — critical.** Before deleting target Fact `T`:

- Capture `T.subject_entity_id`.
- Capture `T.object_entity_id`, if non-`NULL`.
- Deduplicate (subject and object may, in principle, be the same
  Entity).

**After** `T` and its evidence/source payload have been removed (i.e.
after step 11 of §9's rewritten transaction, not before — see §9),
for each captured candidate Entity `E`:

- **Never delete if `E.is_self = true`** — the canonical SELF Entity is
  never a deletion candidate under any circumstance, full stop.
- **Retain `E` if it is still referenced by any surviving Personal
  Knowledge object:**
  - `facts.subject_entity_id = E.id` OR `facts.object_entity_id =
    E.id` (any *other* Fact, since `T` itself is already gone at this
    point).
  - `relations.from_entity_id = E.id` OR `relations.to_entity_id =
    E.id`.
  - `episode_entities.entity_id = E.id`.
- **If zero surviving references remain and `E.is_self = false` →
  `DELETE E`.**

**A `domain_ref` on `E` does NOT by itself force retention.** The
authoritative domain object (e.g. the real `Project` row) lives
elsewhere and is entirely unaffected by deleting the Personal Knowledge
*pointer* Entity to it — a new Entity pointing at the same domain
object can always be created again later via `AddMemory`'s existence-
check (§6) if some future Fact needs to reference it again. Retaining
an orphaned pointer-only Entity purely because it once had a
`domain_ref` would be exactly the kind of dangling privacy-irrelevant
row this cleanup exists to avoid leaving behind.

**Deterministic — no AI/semantic judgment of any kind is used to decide
whether an Entity is "important enough" to keep.** The retention rule
above is a pure, mechanical reference-count check over exactly three
tables (`facts`, `relations`, `episode_entities`), nothing else.

---

## 9. Entity-delete FK safety

**Resolves the ordering requirement implied by Finding 2/§8 and this
run's explicit review request.** All `Entity`-referencing FKs
(`facts.subject_entity_id`, `facts.object_entity_id`,
`relations.from_entity_id`, `relations.to_entity_id`,
`episode_entities.entity_id`) **remain `ON DELETE RESTRICT`** — no
`CASCADE` is introduced anywhere by this correction. The orphan-Entity
deletion in §8 is legal precisely because of the **ordering** in §10's
rewritten transaction: `DELETE FROM facts WHERE id = T.id` (step 11)
happens **before** the orphan-Entity check (step 12) — by the time
Core evaluates "does any Fact still reference `E`?", `T`'s own
reference to `E` is already gone, so `T` itself can never be the thing
keeping `E` alive. Combined with §8's explicit query across all three
referencing tables immediately before the `DELETE`, the `RESTRICT`
constraint is satisfied by construction, not bypassed — if the query
finds zero references, the constraint would not have blocked the
delete anyway; if it finds any, Core simply does not attempt the
delete. No FK failure is ever expected to occur at this step; it is
provable dead code, not a risk that's merely "usually" avoided.

---

## 10. Final `ForgetMemory` transaction — supersedes R018 §14 in full

**One exact 14-step sequence, incorporating every correction above:**

1. Validate target Fact `T` exists — else `NotFound`, zero mutation.
2. Capture candidate subject/object Entity IDs from `T` (deduplicated,
   §8).
3. Collect every `evidence_id` linked to `T` via `fact_evidence`.
4. Collect the `source_id` of each collected Evidence row.
5. **Scrub** payload-bearing `previous_snapshot` values from existing
   `fact_audit_log` rows for `T` (R018 §10).
6. **Clear inbound supersession references**: for every Fact `P` with
   `P.superseded_by_fact_id = T.id`, set it `NULL`, touch `updated_at`
   (R018 §11).
7. Delete every `fact_evidence`/`relation_evidence`/`episode_evidence`
   row referencing any of the collected `evidence_id`s (R018 §12).
8. Delete the collected `Evidence` rows themselves (R018 §12).
9. Delete any of the collected `Source` rows that now have zero
   remaining `Evidence` references (R018 §13).
10. Delete any remaining `fact_evidence` rows for `T` defensively (a
    redundant, idempotent safety net, R018 §14).
11. `DELETE FROM facts WHERE id = T.id` — the actual permanent removal.
12. **New (§8-9 above):** for each candidate Entity captured in step 2
    that is not the SELF Entity, delete it if and only if no surviving
    `Fact`/`Relation`/`episode_entities` reference remains.
13. Insert the minimal `ForgetMemory` audit marker: `fact_id = T.id`,
    `previous_snapshot = NULL` (unconditional), no semantic payload.
14. Commit.

**Any failure at any step → full rollback of the entire transaction.**
No Undo after a confirmed `ForgetMemory` executes. No subject/object
Entity is ever automatically deleted unless §8's reference-count check
proves it genuinely orphaned — a shared Entity referenced by other
retained knowledge is never touched.

---

## 11. Privacy Firewall — sensitivity composition across a Context Pack item

**Frozen architectural target only — the Privacy Firewall itself is not
built in this run (unchanged scope, R017 §22/§27).** Whenever a future
Context Pack includes an item composed of more than one Personal
Knowledge component (an `Entity` plus the `Fact`/`Relation`/`Episode`
that references it, plus its `Source`/`Evidence` chain), the
**effective outbound classification for that composed item must be at
least as strict as the single most restrictive `sensitivity` among
every included component** — never merely the enclosing `Fact`'s own
value in isolation. Conceptually, an ordinal ranking `PUBLIC <
INTERNAL < SENSITIVE < NEVER_LEAVES_DEVICE` and a `max()` over every
component's `sensitivity` (Entity, Fact/Relation/Episode, Source,
Evidence) — the same shape as `ConfirmationClass::severity()`'s
existing `max()` composition (`actions/mod.rs`, reused as a familiar
pattern, not literally the same type). This directly prevents a
scenario R018 §13's `Source`-only fix would otherwise still have
allowed: a `PUBLIC`-tagged Fact referencing a `SENSITIVE`-tagged Entity
must not be treated as merely `PUBLIC` once composed for output. The
four sensitivity values themselves are unchanged (still exactly four,
R017 §13) — this section freezes only the composition rule, not new
values or new implementation.

---

## 12. Exact `V7` table scope — resolved

**Resolves Finding 4.** The recommended resolution is adopted: `V7`
creates the **full**, already-frozen Personal Knowledge relational
foundation — eleven tables, not six:

1. `entities`
2. `sources`
3. `evidence`
4. `facts`
5. `relations`
6. `episodes`
7. `fact_evidence`
8. `relation_evidence`
9. `episode_evidence`
10. `episode_entities`
11. `fact_audit_log`

**Reasoning, frozen:**

- All six entity schemas (`Entity`/`Fact`/`Relation`/`Episode`/
  `Source`/`Evidence`) are already fully frozen at the field level
  (R017 §4-11, corrected by R018/R019).
- All four join-table schemas are already fully frozen (R017 §12).
- `ForgetMemory`'s final FK-safe transaction (§10 above) already
  references `relation_evidence`/`episode_evidence` by name in its
  join-row-deletion step — having those tables exist from `V7` onward
  means this algorithm has **one stable schema from day one**, rather
  than needing a structural retrofit migration the day slice F/G
  actually lands.
- Slices F/G (`Relation`/`Episode` actions) then need **zero** schema
  migration of their own — only new action-layer code, matching the
  precedent already established for `Milestone`/`Task` needing no
  further schema change once their own action freeze passes landed.
- **Table existence is explicitly not permission to mutate it outside
  Core** (§13 below) — the risk this resolution would otherwise invite
  (an unused table inviting an ad hoc shortcut) is closed directly,
  not left implicit.

This corrects both R017's and R018's inconsistent statements of scope
(Finding 4) into one authoritative list.

---

## 13. Relation/Episode — schema-only safety rule

**Frozen, new to this document, required by §12's resolution:** with
the full eleven-table `V7` foundation in place from day one:

- **No `Relation`/`Episode` write action exists yet** — `AddMemory`/
  `CorrectMemory`/`ConfirmMemory`/`ForgetMemory` never write to
  `relations`, `episodes`, `relation_evidence`, `episode_evidence`, or
  `episode_entities` (only `episode_entities`'/`relation_evidence`'s
  read side is ever touched, indirectly, by `ForgetMemory`'s deletion
  of `relation_evidence`/`episode_evidence` join rows and its read of
  `relations`/`episode_entities` for the orphan-Entity check, §8/§10 —
  never an insert or update).
- **No capability/UI path can mutate them** — no public function is
  exposed that performs a generic "insert a Relation"/"insert an
  Episode" operation. Their own public action semantics remain
  deferred to dedicated freeze passes before slices F/G (R017 §8/§9,
  unchanged).
- **Only migration/integrity tests may insert raw fixture rows** into
  these five tables — the same, already-established M2 pattern of
  proving a schema's `CHECK`/FK constraints via a deliberate raw-SQL
  bypass test that never goes through any production action (e.g. the
  existing `req_t43`-`req_t49`-style raw-SQL defense-in-depth proofs
  for `Task`) — this is test-only verification of the schema itself,
  never a production write path.

This exists specifically so schema existence never silently becomes an
accidental mutation API — the concern §12's resolution explicitly
anticipated.

---

## 14. Final `NewSourceData` shape

**Resolves Finding 5, now that `Source.sensitivity` exists (R018
§13):**

```
NewSourceData {
    source_type: String,
    label: Option<String>,
    reference: Option<String>,
    sensitivity: Sensitivity,      // REQUIRED, no silent default
    captured_at: DateTime<Utc>,
}
```

`SourceRef::Existing(SourceId)` → the existing `Source` row is reused
**completely unchanged** — `AddMemory`/`CorrectMemory` must never
silently mutate an existing Source's metadata or `sensitivity` as a
side effect of reusing it (the same "no silent mutation of a shared,
existing object" principle §5 above applies to `Entity::Existing`).
`SourceRef::New(data)` → validated (bounds per §15's table, `source_type`
non-empty/open-vocabulary, `sensitivity` a member of the closed set)
and created inside the same atomic transaction, exactly as R017 §17
already froze for the un-sensitivity-aware version of this type.

---

## 15. Final `NewEvidenceData` shape

**Resolves Finding 5's other half:**

```
NewEvidenceData {
    content: Option<String>,
    locator: Option<String>,
    sensitivity: Sensitivity,      // REQUIRED, no silent default
}
```

**Validation (unchanged from R017 §11):** at least one of `content`/
`locator` must be present — `CHECK ((content IS NOT NULL) OR (locator
IS NOT NULL))`.

**No implicit inheritance from `Fact`, `Source`, or `Entity`
`sensitivity`** — each of the now-five sensitivity dimensions
(`Entity`/`Fact`/`Relation`/`Episode`/`Source`/`Evidence` — six tables,
`Entity` counted once, matching §3 above) is independently, explicitly
set by the caller every time. This was already true for `Evidence` in
R017 §11 ("never inherited automatically from the Fact/Relation/
Episode it supports"); this section simply confirms it stays true now
that `Entity`/`Source` also carry the field.

---

## 16. Logical deletion — the guarantee LifeOS's own code provides

**Frozen, required, unconditional (this is the guarantee product
users are told about):** after a successful `ForgetMemory`:

- No live `facts` row for the target Fact `T` remains.
- No `Evidence` row directly linked to `T` remains.
- No orphaned `Source` row remains (§10 step 9 / R018 §13).
- No orphaned non-`SELF` `Entity` that existed solely to serve `T`
  remains (§8-10 above — the correction this report adds).
- No payload-bearing `fact_audit_log.previous_snapshot` for `T` remains
  anywhere (§10 step 5 / R018 §10).
- No currently-defined read/retrieval function (`get_fact`, the future
  retrieval/Context Pack pipeline, R017 §22) can return `T` or its
  deleted supporting rows through any path.
- `T` is not "historical" — historical Facts remain queryable by
  design (R017 §25); a forgotten Fact is not in that category at all,
  by construction (R017 §22, unchanged).
- Any future derived index/embedding for `T` or its deleted Evidence
  must be invalidated/rebuilt once such systems exist (unchanged scope
  note, R017 §26).

**This is what "true deletion from LifeOS's canonical Personal
Knowledge model" precisely means, and is the only sense in which this
phrase should be read anywhere in R017/R018/this report** (§17 below
makes this terminology explicit going forward).

---

## 17. Forensic storage guarantee — honestly evaluated, not assumed

**Distinct from §16, and deliberately bounded.** Direct inspection of
`src/services/storage/mod.rs` (read-only, unmodified in this pass):

**CONFIRMED FACT:** the only `PRAGMA`s ever set or queried anywhere in
this codebase are `key` (the SQLCipher raw encryption key, `open_
encrypted`), `foreign_keys` (set and read back, `enable_foreign_keys`,
R015 §4e), `rekey` (`storage::rekey`), and `integrity_check`
(`storage::integrity_check`). **No `journal_mode` PRAGMA and no
`secure_delete` PRAGMA is ever set, or queried, anywhere in this
codebase.**

**CONFIRMED FACT:** the database file is SQLCipher-encrypted at rest
(proven functionally end-to-end in R004/R005, re-verified as still true
by every subsequent report's test suite) — raw inspection of the file
without the master key reveals nothing, independent of everything
below.

**CONFIRMED FACT:** `rusqlite`'s `backup` feature is enabled
(`Cargo.toml`), but the only code anywhere in this repository that
actually calls the backup API is a `#[cfg(test)]`-only proof inside
`storage::spike::tests` (`req7_8_backup_and_restore`) — there is no
production backup subsystem wired into the running application (§18
below).

**Reasoned consequence, not invented, not empirically verified in this
pass:** SQLCipher's own documented design encrypts the rollback-journal
and WAL file (whichever is in use) under the same key as the main
database file — so switching journal modes does not, on its own,
introduce a plaintext-on-disk leak either way. However: absent an
explicit `secure_delete` setting, **SQLite's `DELETE` does not
immediately overwrite a freed page's old bytes** — it marks the page
free for reuse, and the old (still-encrypted) content can persist in
the file until that page happens to be reused by a later write. **This
means:** if the master key were ever later compromised (the exact
threat R015/R016's key-lifecycle hardening already treats seriously),
a still-present freed page containing an old, "forgotten" Fact's
ciphertext could, in principle, still be decrypted — even though §16's
logical guarantee (no live row, no query path) fully holds. **This
project's current, actually-inspected configuration does not itself
prove otherwise.**

**Explicitly NOT claimed, and never to be claimed:** no PRAGMA, no
application code, and no report can promise irrecoverability at the
physical storage-media level — SSD wear-leveling, filesystem
snapshots, and OS-level file/page caching are entirely outside
SQLite/SQLCipher's control and outside anything an application can
fix. **§16's logical guarantee remains fully valid and is the correct
thing to tell a user; a forensic/physical-media guarantee is not, and
must never be conflated with it.**

**Frozen implementation requirement for a future slice (not done now,
matching this run's docs-only constraint):** Slice A or E should
explicitly `PRAGMA secure_delete = ON` (or the stronger `FULL`) on
every connection via the existing single `open_encrypted` primitive,
and **verify it took effect** by reading it back — the exact same
"set AND verify, never trust an unverified default" discipline already
established for `enable_foreign_keys` (R008/R015 §4e). This actively
overwrites freed pages, closing the specific "key compromised later,
old freed page still present" gap described above — a genuine,
actionable SQLite mechanism, not an invented one. The live
`journal_mode` value for this project's specific `rusqlite`+
`bundled-sqlcipher` 0.37.0 build should also be queried (not assumed)
at that same time, for completeness, though no code change is expected
to follow from it given SQLCipher's journal-encryption design above.

---

## 18. Backups and `ForgetMemory` — current state and future rule

**Confirmed by direct inspection (§17 above): LifeOS currently has no
production automatic backup, export, or sync subsystem of any kind.**
The storage spike's `req7_8_backup_and_restore` test proves the
`rusqlite::backup::Backup` API works functionally — it is not, and has
never been described as, a persistent user-facing backup feature.
**There is therefore currently nothing for `ForgetMemory` to propagate
to** — this section freezes a rule for the future, not a gap in the
present.

**Frozen future rule:** any future backup/sync/export system that ever
includes Personal Knowledge data **must define how `ForgetMemory`
propagates to it**, so that restoring an ordinary managed backup taken
before a `ForgetMemory` call cannot silently resurrect the forgotten
data as if it were never forgotten. This is a general, inherent
property of point-in-time backups (true of any system, not a LifeOS-
specific flaw) — it is named here explicitly so it cannot be
overlooked when such a system is eventually designed, not solved now.

**LifeOS cannot control, and must never claim to control, removal from
copies it does not itself manage** — a user's own manual copy of the
database file, a third-party cloud-drive sync of the app-data folder,
or any other external copy outside LifeOS's own subsystems is entirely
outside this guarantee's scope, and no future report should imply
otherwise.

---

## 19. R017/R018 history — addendum, not a rewrite

**R017:** a single forward-pointer line is added to the end of R017's
existing R018 addendum (already present, unedited otherwise) — no
further edit to R017's own original body.

**R018:** a full, clearly labelled addendum is appended to the end of
the report (§20 below shows the exact text) — R018's original body,
including its own measured findings and its own addendum-to-R017, is
left completely unedited.

---

## 20. R018 addendum — final privacy/foundation corrections

Appended verbatim to the end of R018:

> ## Addendum (R019, 2026-09-18) — final privacy/foundation corrections
>
> A final pre-`V7` consistency pass
> (`docs/reports/R019_M2_PERSONAL_KNOWLEDGE_PRIVACY_AND_FOUNDATION_FINALIZATION_2026-09-18.md`)
> found and corrected six further gaps before any migration or
> implementation existed:
>
> - **`Entity` needed a `sensitivity` field too**, by this report's own
>   reasoning for `Source` (§13 above) — metadata like
>   `canonical_name`/`domain_ref` can itself be sensitive. R019 freezes
>   it, using the same four-value set, structurally independent of
>   every other table's own `sensitivity`.
> - **`ForgetMemory`'s "never delete subject/object Entity rows" rule
>   (§20 above) was too broad** once `AddMemory` could create brand-new
>   Entities (§4 above) — a forgotten Fact's only mention of a
>   newly-created, sensitive Entity could otherwise survive forever.
>   R019 freezes a deterministic orphan-Entity cleanup step: delete a
>   captured non-SELF Entity only if no surviving Fact/Relation/
>   `episode_entities` reference remains.
> - **The same-payload duplicate-`domain_ref` case** (two `New()`
>   Entity inputs in one `AddMemory` call requesting the identical
>   domain reference) was left to the real DB constraint alone. R019
>   freezes an explicit Core preflight check across the whole payload,
>   ahead of the DB constraint, matching every other duplicate-
>   detection case in this document.
> - **R017's and R018's stated `V7` table scope disagreed** — R017 froze
>   all eleven tables (six entities + four join tables) as schema-ready;
>   R018's corrected Slice A acceptance listed only six. R019 resolves
>   this explicitly: `V7` creates all eleven tables from day one, with
>   `Relation`/`Episode`'s own action families still deferred - table
>   existence is not mutation permission.
> - **`NewSourceData`/`NewEvidenceData`'s exact shapes**, left unfrozen
>   after `Source.sensitivity` was added, are now specified in full.
> - **"True/permanent deletion" was never explicitly separated into a
>   logical guarantee (what LifeOS's own code/queries provide) versus a
>   forensic-storage guarantee (what the underlying encrypted SQLite
>   file's actual, inspected configuration provides).** Direct
>   inspection found this project currently sets neither
>   `journal_mode` nor `secure_delete` anywhere — R019 freezes the
>   logical guarantee (fully upheld) as the correct thing to tell a
>   user, explicitly declines to claim an unverified forensic guarantee,
>   and freezes a concrete, actionable future hardening step
>   (`PRAGMA secure_delete = ON`, set-and-verified, matching the
>   existing `enable_foreign_keys` precedent) rather than promising
>   something physical-media-level that no application code can
>   actually guarantee.
>
> See R019 for the full corrected `ForgetMemory` transaction (now 14
> steps), the resolved `V7` table list, and the complete logical-vs-
> forensic deletion analysis. R019 is authoritative wherever it
> corrects R017 or R018.

---

## 21. Corrected Slice A acceptance (final, supersedes R018 §18's Slice A row)

- **Exact `V7` table list**: all eleven tables (§12).
- All eleven `CREATE TABLE` schemas apply cleanly over a populated `V6`
  database (non-destructive migration, matching every prior
  migration-compatibility proof in M2).
- `ensure_self_entity` creates exactly one SELF Entity (combined Core+DB
  invariant, R018 §3).
- Reopen reuses the same SELF `EntityId` (R018 §3).
- A raw-SQL second `is_self = 1` insert is rejected (R018 §3).
- SELF's bootstrap-created row has `sensitivity = 'INTERNAL'` (§4
  above).
- `entities.sensitivity`/`sources.sensitivity` invalid-value rejection
  proven via raw SQL (§3-4/§14 above), alongside the pre-existing
  `facts.sensitivity`/`relations.sensitivity` proofs.
- Every exact field-length bound from R018 §15's table is proven
  Core-side (no DB-level length `CHECK` exists to separately verify,
  per that section's explicit convention note).
- The one-semantic-object `CHECK` on `facts` (`value_json` XOR
  `object_entity_id`) is proven both directions (R017 §5).
- `confidence` bounds proven on both `facts` and `relations`.
- Every FK/`RESTRICT` relationship is verified — including the two
  deliberate non-FK columns (`entities.domain_ref_id`,
  `fact_audit_log.fact_id`, R017 §20/§24) staying genuinely
  unconstrained, proven by direct inspection of the applied schema.
- Every raw-SQL invalid-closed-enum insert (`memory_type`,
  `epistemic_state`, `sensitivity` on every table that carries it) is
  rejected.
- `relations`/`episodes`/`relation_evidence`/`episode_evidence`/
  `episode_entities` exist as tables but are proven **unreachable
  through any public action or capability function** (§13 above) — no
  production code path can write to them at Slice A; only a
  deliberate raw-SQL fixture-row test may.
- `services::storage`'s module-privacy boundary is re-verified against
  the new (larger) module shape (the established illegal-access-probe
  proof).
- Encryption-at-rest is re-verified still holds (no plaintext `SQLite`
  header, matching R016's `req_kl_bootstrap_regression`-style proof).
- No production-data path is exposed — `bootstrap_production_db()`
  still is not called from anywhere in the running app (unchanged,
  every prior M2 report).

**No `AddMemory` in Slice A** — read-side (`get_fact`/`get_entity`) and
schema/bootstrap only, unchanged from R017/R018.

---

## 22. Corrected Slice B acceptance (final, supersedes R018 §18's Slice B row)

`AddMemory` supports:

- **`subject`**: `SelfEntity` / `Existing(EntityId)` / `New(NewEntityData)`
  (with `sensitivity` required, §5).
- **`value`**: `ScalarJson(String)` / `Entity(EntityInputRef)` (same
  three sub-variants as subject).
- **`source`**: `Existing(SourceId)` / `New(NewSourceData)` (§14).
- **`evidence`**: always `NewEvidenceData` (§15).

All creation happens in **one** atomic transaction.

**Required proofs:**

- New subject only; new object only; both new; neither new (existing/
  existing) — all four combinations succeed and persist correctly.
- Two `New()` inputs in the same call sharing an identical `domain_ref`
  → `DuplicateDomainReference`, rejected **before** any write (§7),
  proven distinctly from the cross-call/DB-constraint case (§6's real
  `UNIQUE` index, proven separately as defense-in-depth).
- A `New()` entity whose `domain_ref` names a nonexistent domain row →
  validation failure, zero writes (§6 step 2).
- A `New()` entity whose `domain_ref` duplicates an *already-persisted*
  Entity's → `DuplicateDomainReference`, zero writes (§6 step 3).
- Invalid `Entity`/`Source`/`Evidence`/`Fact` `sensitivity` values each
  independently rejected.
- `Provenance::Inferred` `AddMemory` → zero writes until explicit
  confirmation; confirmed resubmission persists `epistemic_state =
  USER_CONFIRMED` (R018 §5).
- A forced failure at any step rolls back **everything**: any
  newly-created subject/object Entity, any newly-created Source, the
  Fact, the Evidence, the `fact_evidence` link, and the audit row — a
  dedicated test proving the Entity rows specifically disappear too
  (R018 §4/§18), not merely Fact/Evidence.

---

## 23. Architecture status — CLOSED for Slice A/B

**Yes.** Every finding raised across R017 (initial freeze), R018 (first
consistency pass), and this report (final privacy/foundation pass) has
been explicitly identified and resolved, with no further open
structural/privacy contradiction found in this pass's own review. The
`V7` table scope is now unambiguous (§12), every payload shape needed
for Slice A/B is fully specified (§5, §14, §15), `ForgetMemory`'s
transaction is FK-safe and privacy-complete end to end (§8-10), and the
logical-vs-forensic deletion distinction (§16-17) means no future
report needs to walk back an overpromised guarantee. **A future
implementer building Slice A or Slice B should not need to invent any
further product/privacy semantics** — only genuine implementation
detail (exact Rust error-enum shapes, the precise SELF-bootstrap call
site, the `secure_delete` hardening step named in §17) remains, and
each of those is explicitly named as implementation-time, not
architecture-time, work.

Remaining genuinely deferred features are unchanged from R017 §26/R018
§19 (locked/pinned memory, `last_used` tracking, memory importance,
entity alias/resolution, ontology management, embeddings, `Relation`/
`Episode`'s own action families, bulk forget, backdating `valid_from`,
the Memory UI, and — new to this report — any future backup/sync
propagation policy for `ForgetMemory`, §18) — nothing here was
speculatively pulled forward.
