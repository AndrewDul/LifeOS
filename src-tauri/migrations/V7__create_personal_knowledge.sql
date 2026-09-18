-- LifeOS production schema, M2 Personal Knowledge Slice A foundation
-- (2026-09-18). Creates the full eleven-table Personal Knowledge
-- relational foundation frozen by R017 (M2 Personal Knowledge Layer
-- Architecture Freeze), corrected by R018 (M2 Personal Knowledge
-- Architecture Consistency Hardening) and R019 (M2 Personal Knowledge
-- Privacy and Foundation Finalization) - see those reports for the
-- full field-level reasoning; this migration is a direct transcription
-- of the frozen schema, not a redesign.
--
-- This slice (R020, Slice A) implements ONLY schema + SELF bootstrap +
-- narrow Entity/Fact reads. No AddMemory/CorrectMemory/ConfirmMemory/
-- ForgetMemory exists yet (Slice B+). `relations`/`episodes`/
-- `relation_evidence`/`episode_evidence`/`episode_entities` exist as
-- schema only (R019 §12/§13) - table existence is not mutation
-- permission: no production write path reaches them this slice; only
-- deliberate raw-SQL fixture-row tests may insert into them, to prove
-- their own constraints, never through a production action.
--
-- V1 (Goal) through V6 (widen project_audit_log action types) are
-- historical production migrations and are not edited here.

CREATE TABLE entities (
    id                 TEXT PRIMARY KEY,
    -- Open vocabulary (R017 §4/§6, corrected from R003's closed
    -- Entity.kind enum) - Core validates non-empty/bounded length
    -- only, no CHECK list, so future domains never need a migration to
    -- add a type.
    entity_type        TEXT NOT NULL CHECK (length(trim(entity_type)) > 0),
    canonical_name     TEXT NOT NULL CHECK (length(trim(canonical_name)) > 0),
    is_self            INTEGER NOT NULL DEFAULT 0 CHECK (is_self IN (0, 1)),
    -- Sensitivity classification of this Entity's OWN metadata (R019
    -- §3) - independent of any Fact/Relation/Episode/Source/Evidence
    -- that happens to reference it.
    sensitivity        TEXT NOT NULL CHECK (sensitivity IN ('PUBLIC', 'INTERNAL', 'SENSITIVE', 'NEVER_LEAVES_DEVICE')),
    -- Optional pointer to an existing structured-domain row (a
    -- Goal/Project/Task/Milestone, etc.) - deliberately NOT an
    -- enforced SQL foreign key (R017 §24/R019 §6): domain_ref_type
    -- varies across an open, growing set of domain tables a single
    -- column cannot REFERENCES polymorphically. Validity is a
    -- Core-level concern, reusing the referenced domain module's own
    -- *_exists check - not implemented until Slice B (R019 §6/§29).
    domain_ref_type    TEXT,
    domain_ref_id      TEXT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    -- Both set together or both absent, never one without the other
    -- (R017 §4).
    CHECK ((domain_ref_type IS NULL) = (domain_ref_id IS NULL))
);

-- Exactly one SELF Entity is a two-layer invariant (R019 §3): this
-- partial unique index guarantees AT MOST one is_self = 1 row; Core's
-- ensure_self_entity (services::mod.rs, wired into normal production
-- bootstrap) guarantees AT LEAST one exists before Personal Knowledge
-- is considered ready. Together: exactly one.
CREATE UNIQUE INDEX idx_entities_single_self ON entities (is_self) WHERE is_self = 1;

-- At most one Entity may point at a given domain row (R017 §4) -
-- avoids ambiguity about "which Entity represents Project X."
CREATE UNIQUE INDEX idx_entities_domain_ref ON entities (domain_ref_type, domain_ref_id) WHERE domain_ref_type IS NOT NULL;

CREATE TABLE sources (
    id             TEXT PRIMARY KEY,
    -- Open vocabulary (R017 §13, corrected-open resolution) -
    -- documented canonical values (USER_INPUT, CHAT_MESSAGE,
    -- JOURNAL_ENTRY, LOCAL_MODEL_EXTRACTION, IMPORTED_DOCUMENT,
    -- SYSTEM_OBSERVATION, CONNECTED_APP, WEB_RESEARCH,
    -- DEVICE_OBSERVATION), not DB-enforced.
    source_type    TEXT NOT NULL CHECK (length(trim(source_type)) > 0),
    label          TEXT,
    -- Locator/pointer metadata only, never raw content (R017 §13) -
    -- personal/sensitive excerpts belong to Evidence, not Source.
    reference      TEXT,
    -- R019 §3: metadata itself can be sensitive even though Source
    -- never holds a raw excerpt.
    sensitivity    TEXT NOT NULL CHECK (sensitivity IN ('PUBLIC', 'INTERNAL', 'SENSITIVE', 'NEVER_LEAVES_DEVICE')),
    captured_at    TEXT NOT NULL,
    created_at     TEXT NOT NULL
);

CREATE TABLE evidence (
    id           TEXT PRIMARY KEY,
    source_id    TEXT NOT NULL REFERENCES sources (id) ON DELETE RESTRICT,
    -- The specific supporting payload/snippet (text, or a JSON-encoded
    -- structured payload - caller-defined format) - the ONE place a
    -- personal/sensitive excerpt is allowed to live (R017 §11/§13).
    content      TEXT,
    -- A pointer WITHIN the source (e.g. "line 42") - distinct from
    -- Source.reference, which points at the source as a whole.
    locator      TEXT,
    sensitivity  TEXT NOT NULL CHECK (sensitivity IN ('PUBLIC', 'INTERNAL', 'SENSITIVE', 'NEVER_LEAVES_DEVICE')),
    created_at   TEXT NOT NULL,
    -- An Evidence row must carry at least one specific supporting
    -- detail, otherwise it is indistinguishable from bare Source
    -- metadata (R017 §11).
    CHECK ((content IS NOT NULL) OR (locator IS NOT NULL))
);

CREATE TABLE facts (
    id                     TEXT PRIMARY KEY,
    -- NOT NULL - corrected vs. R003's nullable subject (R017 §2 item
    -- 3): with a frozen SELF Entity always present, every Fact is
    -- about some Entity.
    subject_entity_id      TEXT NOT NULL REFERENCES entities (id) ON DELETE RESTRICT,
    -- Open vocabulary (R017 §7/§8) - Core validates non-empty/bounded
    -- length only.
    predicate              TEXT NOT NULL CHECK (length(trim(predicate)) > 0),
    -- Exactly one semantic object: a JSON-encoded scalar/small
    -- structured value, OR a reference to another Entity - never
    -- both, never neither (R017 §5/§7's one-semantic-object
    -- invariant).
    value_json             TEXT,
    object_entity_id       TEXT REFERENCES entities (id) ON DELETE RESTRICT,
    -- Frozen, closed 11-value set (R017 §6) - unrenamed, unchanged.
    memory_type            TEXT NOT NULL CHECK (memory_type IN (
                                'FACT', 'PREFERENCE', 'EXPERIENCE', 'DECISION', 'OBSERVATION',
                                'HYPOTHESIS', 'EFFECTIVE_SOLUTION', 'INEFFECTIVE_SOLUTION',
                                'SKILL', 'HABIT', 'CURRENT_STATE'
                            )),
    -- Frozen, closed 5-value set (R017 §7) - unrenamed, unchanged.
    epistemic_state        TEXT NOT NULL CHECK (epistemic_state IN (
                                'USER_CONFIRMED', 'OBSERVED', 'INFERRED', 'DISPUTED', 'STALE'
                            )),
    -- NULL = no meaningful numeric confidence was assigned (R017 §14)
    -- - never equated with "the user confirmed saying this."
    confidence              REAL CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0)),
    sensitivity             TEXT NOT NULL CHECK (sensitivity IN ('PUBLIC', 'INTERNAL', 'SENSITIVE', 'NEVER_LEAVES_DEVICE')),
    valid_from              TEXT NOT NULL,
    -- NULL = currently active; non-NULL = closed/historical (R017 §5).
    valid_until             TEXT,
    -- Points at the successor Fact that replaced this one (R017 §5) -
    -- deliberately NOT ON DELETE CASCADE: a future ForgetMemory slice
    -- explicitly clears an inbound reference itself before deleting a
    -- Fact (R018 §11/R019 §9), rather than relying on a DB cascade to
    -- do it implicitly.
    superseded_by_fact_id   TEXT REFERENCES facts (id) ON DELETE RESTRICT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    CHECK ((value_json IS NOT NULL) <> (object_entity_id IS NOT NULL)),
    CHECK (superseded_by_fact_id IS NULL OR superseded_by_fact_id <> id)
);

CREATE TABLE relations (
    id               TEXT PRIMARY KEY,
    from_entity_id   TEXT NOT NULL REFERENCES entities (id) ON DELETE RESTRICT,
    to_entity_id     TEXT NOT NULL REFERENCES entities (id) ON DELETE RESTRICT,
    -- OPEN string (R017 §8) - deliberately NOT GoalRelation's closed
    -- 11-value enum (LIFEOS_SPEC.md §5.2). This Relation connects two
    -- Entity rows only, and is a completely separate layer from
    -- GoalRelation - the two only ever shared a field name.
    relation_type    TEXT NOT NULL CHECK (length(trim(relation_type)) > 0),
    note             TEXT,
    epistemic_state  TEXT NOT NULL CHECK (epistemic_state IN (
                        'USER_CONFIRMED', 'OBSERVED', 'INFERRED', 'DISPUTED', 'STALE'
                    )),
    confidence       REAL CHECK (confidence IS NULL OR (confidence >= 0.0 AND confidence <= 1.0)),
    -- Added beyond R017's illustrative shape (R018 §8) - a Relation
    -- can trivially carry sensitive content.
    sensitivity      TEXT NOT NULL CHECK (sensitivity IN ('PUBLIC', 'INTERNAL', 'SENSITIVE', 'NEVER_LEAVES_DEVICE')),
    valid_from       TEXT NOT NULL,
    valid_until      TEXT,
    created_at       TEXT NOT NULL,
    -- Self-edges rejected, matching GoalRelation's structural
    -- precedent (R017 §8) - only the self-link rule is borrowed, not
    -- the type vocabulary.
    CHECK (from_entity_id <> to_entity_id)
);

-- Schema only in this slice (R019 §13) - no production write path
-- exists for this table yet; Relation's own action family is deferred
-- to a future dedicated freeze pass (slice F).

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

-- Schema only in this slice (R019 §13) - same reasoning as relations;
-- Episode's own action family is deferred to a future dedicated freeze
-- pass (slice G).

CREATE TABLE fact_evidence (
    fact_id      TEXT NOT NULL REFERENCES facts (id) ON DELETE RESTRICT,
    evidence_id  TEXT NOT NULL REFERENCES evidence (id) ON DELETE RESTRICT,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (fact_id, evidence_id)
);

CREATE TABLE relation_evidence (
    relation_id  TEXT NOT NULL REFERENCES relations (id) ON DELETE RESTRICT,
    evidence_id  TEXT NOT NULL REFERENCES evidence (id) ON DELETE RESTRICT,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (relation_id, evidence_id)
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

-- Minimal audit trail for the future AddMemory/CorrectMemory/
-- ConfirmMemory/ForgetMemory actions (not implemented this slice -
-- see R017 §21/R018 §10/R019 §10). fact_id is DELIBERATELY NOT an
-- enforced foreign key, unlike every other M2 audit table's parent-id
-- column (e.g. goal_audit_log.goal_id REFERENCES goals(id)): a future
-- ForgetMemory genuinely deletes the Fact row while retaining this
-- table's minimal marker row, so an FK here would either block the
-- deletion (RESTRICT) or destroy the residual marker (CASCADE) -
-- neither is acceptable (R017 §20). Do NOT "fix" this by adding an FK.
CREATE TABLE fact_audit_log (
    id                  TEXT PRIMARY KEY,
    fact_id             TEXT NOT NULL,
    action_type         TEXT NOT NULL CHECK (action_type IN ('AddMemory', 'CorrectMemory', 'ConfirmMemory', 'ForgetMemory')),
    actor               TEXT NOT NULL CHECK (actor IN ('UI', 'CloudLLM', 'NeXa')),
    provenance          TEXT NOT NULL CHECK (provenance IN ('Explicit', 'Inferred')),
    confirmation_class  TEXT NOT NULL,
    previous_snapshot   TEXT,
    occurred_at         TEXT NOT NULL
);
