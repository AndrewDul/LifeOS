# R015 — M2 Storage and Audit Safety Hardening

A focused safety-infrastructure pass on M2 - **not** a new domain
entity or action. Addresses two known safety issues found by direct
code inspection: (A) the production database master-key lifecycle could
silently generate a replacement key for an existing, now-undecryptable
database; (B) every mandatory audit `previous_snapshot` used
`serde_json::to_string(&existing).ok()`, silently turning a
hypothetical serialization failure into a missing audit record. Product
action semantics (confirmation floors, `FieldPatch` behavior, status
transition policy, entity schemas, `Project.progress` ownership, `Task`
ownership, `GoalRelation` semantics, `Milestone` ordering, action names)
are all unchanged. **Stops after these two issues** —
`RecurringRule`/Calendar/Reminder/Personal Knowledge/`ProgressUpdate`/
capability transport/Tauri command API/M3 UI all remain untouched and
out of scope.

---

## 1. Pre-run state

- M1 accepted, unchanged.
- M2: `Goal`, `Project`, `GoalRelation`, `Milestone`, `Task` all
  implemented; `UpdateProject` implemented; `Task` parent-validation
  hardened. 233/233 tests passing. Local checkpoint `a239373`. Clean
  working tree. No push authorized this run either.
- Report numbering cleanup complete (R001–R014 assigned in verified
  chronological order); this is **R015**, the next allocation.

---

## 2. Why this hardening was required

Two issues were found by direct inspection of current code (per this
run's explicit instruction to verify against the actual implementation,
not an old report's description of it):

- **Issue A**: `security::get_or_create_production_master_key()`
  (removed this pass) always generated and stored a new key whenever
  the OS credential store had no matching entry - it never checked
  whether a real encrypted database already existed at the production
  path. If a user's credential were ever lost or reset (OS reinstall,
  credential-store corruption, migration to a new machine without
  exporting it) while their database file remained on disk, LifeOS
  would have silently generated an unusable replacement key, opened
  (i.e. re-initialized) the existing file under that wrong key, and
  made real recovery of the original data far more confusing - the
  original encrypted bytes would still be there, but nothing would
  point to them being unreadable under the new key versus genuinely
  corrupt.
- **Issue B**: `serde_json::to_string(&existing).ok()` appeared at
  every one of the five mandatory-audit update call sites
  (`update_goal`, `update_project`, `update_progress`,
  `update_milestone`, `update_task`) - R014 had already flagged this
  exact pattern for `UpdateProject` specifically; this run searched the
  repository directly (not assumed the list) and found it was universal
  across every update action, not just `UpdateProject`. A hypothetical
  serialization failure would silently produce `previous_snapshot =
  NULL` while the update still committed successfully - a real gap in
  audit integrity and any future Undo support, easy to miss because
  ordinary domain structs essentially never fail to serialize in
  practice.

---

## 3. Previous production key-lifecycle risk

Before this pass, `bootstrap_production_db()` was:

```rust
pub(crate) fn bootstrap_production_db() -> Result<Connection, BootstrapError> {
    let path = production_db_path()?;
    let key_hex = security::get_or_create_production_master_key().map_err(BootstrapError::Key)?;
    bootstrap_db_at(&path, &key_hex)
}
```

`get_or_create_production_master_key()` internally did: look up the
credential; if present, return it; **if absent, generate a fresh random
key, store it, and return that** - unconditionally, with zero awareness
of whether `path` already pointed at real data. This is exactly the
"credential missing → generate a new key" risk the brief described:
correct for a genuine first run, unsafe the moment a real database can
already exist.

---

## 4. Canonical four-state DB/key matrix

Frozen and implemented exactly as specified:

| Database | Credential | Case | Behavior |
|---|---|---|---|
| absent | absent | **A** (normal first run) | Generate a fresh random 256-bit key, store it, initialize the encrypted database with that exact key. |
| exists | exists | **B** | Load the existing key, open the existing database. Never replace the key. |
| exists | absent | **C** (CRITICAL) | Typed recovery-required failure. **Never** generate/store a replacement key, never touch the existing database file in any way (no truncate, no rename, no "fresh start"). |
| absent | exists | **D** | Reuse the existing credential to create/open the new database. Never replace it merely because the DB file is absent (this can legitimately happen after intentional deletion, restore preparation, or interrupted setup). No key-rotation flow is invented. |

"Database exists" means: a file already exists at the exact same path
production opening actually uses (`services::production_db_path()`,
unchanged - no second, competing path resolver was introduced). Any
existing file is treated as existing data, full stop - zero length,
corruption, or a wrong-key failure are separate recovery cases, never
grounds to assume "safe to overwrite" (proven in `req_kl6`, §9 below,
which deliberately exercises a present-but-*wrong* key against a real
existing database and confirms it still fails normally, with no
automatic replacement).

---

## 5. Actual implementation

`src-tauri/src/security/mod.rs` now exposes three narrow primitives
instead of one opaque "get or create" helper:

```rust
pub(crate) fn get_existing_key(service: &str, account: &str) -> Result<Option<String>, KeyError>;
pub(crate) fn generate_and_store_key(service: &str, account: &str) -> Result<String, KeyError>;
pub(crate) fn delete_key(service: &str, account: &str) -> Result<(), KeyError>;
```

`get_or_create_production_master_key()` (the unsafe, DB-existence-blind
helper) is **removed** - nothing calls it anymore. The generic
`get_or_create_master_key` primitive it was built on is kept (now
composed from `get_existing_key`/`generate_and_store_key`) purely as a
small utility for callers that genuinely want plain idempotent
get-or-create semantics with no existence distinction -
`get_or_create_master_key_for_test` (the pre-existing `req4` test
helper) still uses it, unchanged.

`src-tauri/src/services/mod.rs` adds the actual state machine:

```rust
enum ResolvedMasterKey {
    Reused(String),           // Cases B/D - never delete this
    FreshlyGenerated(String), // Case A - safe cleanup target on failure
}

fn resolve_master_key(path: &Path, service: &str, account: &str) -> Result<ResolvedMasterKey, BootstrapError> {
    let db_exists = path.exists();
    let existing_key = security::get_existing_key(service, account)?;
    match (db_exists, existing_key) {
        (_, Some(key_hex)) => Ok(ResolvedMasterKey::Reused(key_hex)),
        (false, None) => Ok(ResolvedMasterKey::FreshlyGenerated(
            security::generate_and_store_key(service, account)?,
        )),
        (true, None) => Err(BootstrapError::MasterKeyMissingForExistingDatabase),
    }
}

fn bootstrap_db_with_key_lifecycle(path: &Path, service: &str, account: &str) -> Result<Connection, BootstrapError> {
    let resolved = resolve_master_key(path, service, account)?;
    match bootstrap_db_at(path, resolved.key_hex()) {
        Ok(conn) => Ok(conn),
        Err(init_err) => {
            if let ResolvedMasterKey::FreshlyGenerated(_) = resolved {
                let _ = security::delete_key(service, account); // best-effort, see §7
            }
            Err(init_err)
        }
    }
}
```

`bootstrap_production_db()` now calls `bootstrap_db_with_key_lifecycle`
with the real production identity
(`security::SERVICE_NAME`/`security::MASTER_KEY_ACCOUNT`, both
widened from private `const` to `pub(crate)` so `services` can name
them explicitly). `bootstrap_db_with_key_lifecycle` is parameterized by
`(path, service, account)` specifically so both the real production
path and the isolated test identity share the *exact same* logic -
matching the existing `bootstrap_db_at(path, key_hex)` precedent - not
a parallel reimplementation that could drift from what production
actually does.

---

## 6. Recovery-required error behavior

New `BootstrapError` variant:

```rust
MasterKeyMissingForExistingDatabase,
```

`Display` renders a generic, safe, human-readable message ("an existing
LifeOS database was found, but its master key is not available in the
OS credential store - this requires manual recovery, not automatic key
replacement") - no secret, no path, no key material. Case C returns
this **immediately** once `get_existing_key` confirms the credential is
absent - `resolve_master_key` never reaches `generate_and_store_key`
for this branch, so no replacement key is ever generated or stored, and
`bootstrap_db_at` (which would open/modify the file) is never even
called. A future UI may offer recovery/backup options; M2 only returns
this typed error - proven directly in `req_kl3` (§9).

---

## 7. Orphan-key cleanup behavior

`bootstrap_db_with_key_lifecycle` remembers whether *this specific
invocation* created the credential (`ResolvedMasterKey::FreshlyGenerated`,
Case A only) versus reused a pre-existing one (`Reused`, Cases B/D). If
`bootstrap_db_at` subsequently fails before a usable database is
established, cleanup only ever runs for the `FreshlyGenerated` branch -
a pre-existing credential is never touched (proven in `req_kl8`, §9).
Cleanup is deliberately **best-effort**: if `security::delete_key`
itself fails, the code discards that secondary error and returns the
**original** initialization error unchanged - a failed cleanup attempt
must never mask the real failure the caller needs to see. This is
documented inline at the call site rather than silently done; no secret
is exposed by either the original error or a swallowed cleanup failure
(both are already-safe typed errors per §14 below).

`BootstrapError` does not currently carry a generic "plus a secondary
diagnostic" slot, and this pass deliberately did not invent one just
for this narrow, best-effort path - the brief's own instruction was
conditional ("if the existing CoreError architecture supports it"). The
primary, mandatory requirement - preserving the original error - is
satisfied exactly as specified.

---

## 8. Credential-test isolation

Every new test uses the same isolated-identity pattern the pre-existing
`req4_master_key_survives_reopen_via_keyring` test already established:
the real OS keyring backend, under a test-only service identity
(`"LifeOS-test"`, a new `TEST_SERVICE` constant), with a distinct
`account` string per test to avoid cross-test interference. No mock/DI
framework was built - reusing the real backend under an isolated
identity is simpler and more representative than a fake credential
store, and was already the established precedent in this codebase.
Database paths are always `tempfile::tempdir()`-backed, never the real
production path. `req_kl10` (§9) is a dedicated, self-verifying static
check that no test in the file references the real production identity
(`security::SERVICE_NAME`/`security::MASTER_KEY_ACCOUNT`) - confirmed
it finds *exactly* the two real references inside
`bootstrap_production_db` itself, and nothing else. Every keyring-
touching test gracefully reports `BLOCKED` (not `FAIL`) if the OS
credential store is genuinely unavailable in this environment, matching
`req4`'s existing precedent exactly - `cargo test -- --test-threads=1`
still matters for these tests, same reason as before.

---

## 9. Full key-lifecycle tests

Ten tests (`req_kl1`–`req_kl10`) plus one production-bootstrap
regression test, all in `services::tests`:

1. `req_kl1` — DB absent + key absent → key generated, key stored, DB
   created/opened successfully.
2. `req_kl2` — DB exists + key exists → existing key reused, DB opens,
   credential provably unchanged across the second bootstrap.
3. `req_kl3` — DB exists + key absent → typed
   `MasterKeyMissingForExistingDatabase`, no replacement key stored, DB
   still exists, DB bytes byte-for-byte unchanged (`std::fs::read`
   before/after compared directly).
4. `req_kl4` — DB absent + key exists (credential pre-created directly,
   no DB) → existing key reused to create the new DB, key not replaced.
5. `req_kl5` — the first-run generated key is *exactly* the key that
   encrypts the DB: independently re-opens the resulting file with
   `storage::open_encrypted`/`verify_key` using only the value read
   back from the credential store.
6. `req_kl6` — a credential that is *present but wrong* for an existing
   database still fails as a normal `BootstrapError::Storage`
   (SQLCipher HMAC rejection), and is never replaced - proves
   `resolve_master_key` correctly treats "present" as `Reused` even
   when the value happens to be wrong, a genuinely different case from
   Case C's "absent".
7. `req_kl7` — an initialization failure *after* this invocation
   created a fresh credential cleans up that orphan. Forced
   deterministically and portably: the DB path's parent directory
   component is a plain file (not a directory), so
   `std::fs::create_dir_all` genuinely fails with `Io`, independent of
   any OS-specific permission trick.
8. `req_kl8` — the same forced-failure technique, but with a
   *pre-existing* credential: confirms it survives the failure
   completely untouched (`Some(pre_existing_key)` unchanged).
9. `req_kl9` — three additional successful reopens after the first
   bootstrap all report the exact same master key.
10. `req_kl10` — static source scan (skips comments and string
    literals via an odd/even-quote-count heuristic, not merely a naive
    substring search) proving the real production identity constants
    are referenced nowhere except `bootstrap_production_db` itself.

Plus `req_kl_bootstrap_regression_first_and_second_bootstrap_share_state_across_reopen`
(§brief item 18): first bootstrap creates the encrypted DB and writes a
`Goal`; close; second bootstrap reopens the *same* DB via the *same*
state machine and the same key; the `Goal` written before close is read
back correctly after reopen; the raw file bytes still show no plaintext
`SQLite` header.

All evidence is deterministic (`assert_eq!`/`assert!` against actual
returned values, file bytes, and credential-store reads) - never only a
log message.

---

## 10. Mandatory audit snapshot rule

New private helper in `services/mod.rs`:

```rust
fn serialize_required_snapshot<T: serde::Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value).map_err(|e| {
        CoreError::new(
            CoreErrorKind::Internal,
            format!("failed to serialize mandatory audit previous_snapshot: {e}"),
        )
    })
}
```

Every mandatory-snapshot update call site now does:

```rust
let previous_snapshot = match serialize_required_snapshot(&existing) {
    Ok(s) => s,
    Err(e) => return ActionOutcome::StorageError(e),
};
```

instead of `serde_json::to_string(&existing).ok()`. This runs **before**
the mutation transaction starts (unchanged position in every function -
right after the confirmation gate, before `conn.transaction()`), so a
serialization failure means zero mutation and zero audit write by
construction - there is no transaction yet to roll back. `Create*`
actions are entirely unaffected: they still pass `None` directly for
`previous_snapshot` (there is genuinely no previous entity to
snapshot), never through this helper - that distinction is preserved
exactly as specified.

---

## 11. Locations where silent `.ok()` was removed

All five mandatory-audit update call sites in `src-tauri/src/services/mod.rs`
(confirmed by searching the whole repository for the pattern, not
assumed from the brief's example list):

- `update_goal` (`UpdateGoal`)
- `update_project` (`UpdateProject`)
- `update_progress` (`UpdateProgress`) - in scope alongside the other
  four because it exhibits the exact same pattern (a mandatory audit
  write with a swallowed snapshot failure), even though the brief's own
  illustrative list did not name it explicitly
- `update_milestone` (`UpdateMilestone`)
- `update_task` (`UpdateTask`)

Each corresponding `insert_audit_entry(...)` call's `previous_snapshot`
argument changed from `previous_snapshot.as_deref()` (on an
`Option<String>`) to `Some(previous_snapshot.as_str())` (on the now-
mandatory `String`) - the audit row's shape and the `NULL`-for-create /
non-`NULL`-for-update convention are otherwise completely unchanged.

---

## 12. Snapshot serialization error handling

Classified as `CoreErrorKind::Internal` (not `ValidationError`, not a
user mistake, not a Storage/FK failure) - matches the brief's explicit
classification requirement. The diagnostic text is
`serde_json::Error`'s own `Display` output, which describes the
*structural* failure mode (e.g. a custom `Serialize` impl's own error
message, or "recursion limit exceeded") - it does not echo back the
field values being serialized, so no personal snapshot content or
encryption key can leak through it (see §14 for the full review).

Testability (brief §12): a dedicated test-only type,
`AlwaysFailsToSerialize`, implements `serde::Serialize` by
unconditionally returning `Err(serde::ser::Error::custom(...))` -
proving `serialize_required_snapshot` genuinely surfaces a typed
`CoreError` (`kind() == CoreErrorKind::Internal`) rather than silently
producing `Ok(String)` or panicking. No production domain struct was
distorted to create this proof (`req_snap1`).

---

## 13. Snapshot regression tests per update action

For every action that requires a mandatory snapshot -
`UpdateGoal`/`UpdateProject`/`UpdateProgress`/`UpdateMilestone`/
`UpdateTask` - a dedicated test (`req_snap2`–`req_snap6`) performs a
real persisted update, reads the audit row's `previous_snapshot` column
directly via SQL, asserts it is `NOT NULL`, **deserializes it back into
the actual domain struct** (`Goal`/`Project`/`Milestone`/`Task`, all of
which already derive `PartialEq`), and asserts full structural equality
against a `.clone()` of the pre-update value captured before the
update ran - a materially stronger proof than a title-substring
`.contains(...)` check (which the pre-existing `req9`/`req_up25_26`/
`req_m20`/`req_t37` tests already did and still do, unchanged). This
confirms the snapshot genuinely describes the *pre*-update state, not
the post-update one, for every field - not merely the one field a
given test happened to change.

---

## 14. CoreError secret-leak review

Direct inspection, not a redesign (already safe):

- `KeyError::Keyring(String)` is sourced only from the underlying
  `keyring::Error`'s own `Display` text (e.g. "no matching entry
  found") - never from the key value itself. `CoreError::from(KeyError)`
  wraps that same safe text unchanged. Verified by grepping every
  `key_hex`/`stored_key`/`pre_existing_key`/`wrong_key` binding in
  `security/mod.rs` and `services/mod.rs`: none of them is ever
  interpolated into an error, log, or `Display`/`Debug` field - they
  only ever flow into `storage::open_encrypted`/`keyring::Entry::set_password`
  calls directly.
- `storage::open_encrypted`'s `PRAGMA key = "x'<hex>'"` call
  (`conn.pragma_update(None, "key", format!("x'{key_hex}'"))`) builds a
  pragma string containing the raw key locally, but a wrong/mismatched
  key does not fail *this* call - SQLCipher only detects a bad key
  lazily, on the first real read (`verify_key`'s
  `SELECT count(*) FROM sqlite_master`), whose resulting
  `rusqlite::Error` describes SQLCipher's own generic decryption
  failure (e.g. "file is not a database"), never the key text. `key_hex`
  itself is always either freshly generated (hex digits only, via
  `generate_key_hex()`) or read back verbatim from a value this same
  code previously stored - never external/adversarial input - so even a
  hypothetical pragma-level syntax failure could not occur from
  SQL-breaking characters under any path this code controls.
- `serialize_required_snapshot`'s error text is `serde_json::Error`'s
  own structural-failure description, never the serialized value
  itself (§12).
- No `CoreError`, `BootstrapError`, or `KeyError` variant stores the raw
  key or a serialized entity payload as a field - confirmed by
  inspecting every variant's definition; `{:?}` on any of them can
  therefore never include one, by construction, not merely by
  convention.

**Conclusion: already safe. Not redesigned** - no encryption key or
serialized personal payload appears in `Display`, `Debug`, logs, or
returned error metadata anywhere touched by this pass.

---

## 15. Exact tests/build results (MEASURED)

- `cargo fmt --check`: clean (after one `cargo fmt` pass - see §16 for
  the pre-fmt/post-fmt note).
- `cargo check --tests`: clean, zero warnings.
- `cargo test -- --test-threads=1`: **250 passed; 0 failed; 0 ignored**
  (up from 233). All 233 prior tests remain green, unmodified. Note: an
  early iteration of `req_kl10`'s own static scan flagged itself (its
  own doc comments and pattern-array/println! text mentioning
  `security::SERVICE_NAME`) as a false positive - fixed by adding a
  comment-line skip and an odd/even-quote-count "inside a string
  literal" check to the scan helper; recorded here for completeness,
  not silently corrected away.
- `cargo build --release`: see the chat reply for the measured result.
- `npm run build` (repo root): see the chat reply for the measured
  result.
- `npm run tauri build`: run this pass (not merely optional) because
  production database bootstrap/security behavior changed - see the
  chat reply for the actual measured result (MSI/NSIS), verified from
  the captured log file directly, never only the wrapper's exit code.

---

## 16. Files changed

Modified:

- `src-tauri/src/security/mod.rs` — `SERVICE_NAME`/`MASTER_KEY_ACCOUNT`
  widened to `pub(crate)`; new `get_existing_key`/
  `generate_and_store_key`/`delete_key` primitives; unsafe
  `get_or_create_production_master_key()` removed;
  `get_or_create_master_key` kept as a small generic primitive (now
  composed from the new primitives), `#[cfg_attr(not(test),
  allow(dead_code))]` since its only non-test caller was removed;
  `delete_key_for_test` now delegates to the new general `delete_key`
  (DRY, behavior-preserving); module doc comment updated.
- `src-tauri/src/services/mod.rs` — new `BootstrapError::
  MasterKeyMissingForExistingDatabase` variant + `Display` arm; new
  `ResolvedMasterKey` enum, `resolve_master_key`, and
  `bootstrap_db_with_key_lifecycle` functions (the state machine);
  `bootstrap_production_db` rewritten to use it; new
  `serialize_required_snapshot` helper; all five mandatory-audit
  `update_*` functions' `previous_snapshot` computation and
  `insert_audit_entry` call updated to use it.
- `src-tauri/src/services/tests.rs` — 17 new tests (`req_kl1`–
  `req_kl10`, `req_kl_bootstrap_regression_*`, `req_snap1`–`req_snap6`),
  `TEST_SERVICE` constant, `scan_dir_for_identifiers` helper,
  `AlwaysFailsToSerialize` test-only `Serialize` type.

No migration created or edited (verified: zero diff under
`src-tauri/migrations/`). No `services::storage` visibility change, so
no illegal-access probe was required this pass.

---

## 17. Remaining risks

- `bootstrap_production_db()` is still not called from anywhere in the
  running app - no Tauri command layer exists yet, so this hardening,
  like every prior M2 service-layer function, is only reachable from
  tests today. That is unchanged by this pass and remains a known,
  already-documented gap.
- The orphan-credential cleanup on initialization failure is
  best-effort by design (§7) - a machine where the credential store can
  be written to but not subsequently deleted from (an unusual, narrow
  failure mode) would leave an orphan credential behind after a failed
  first-run bootstrap. This is a deliberate, documented tradeoff, not
  an oversight: the alternative (retrying deletion, or failing the
  whole bootstrap differently) was judged out of scope for this pass's
  "smallest clean seam" instruction.
- `resolve_master_key`'s database-existence check (`path.exists()`) is
  a simple file-existence test, matching the brief's explicit
  instruction not to assume anything about zero-length/corrupt files.
  A zero-byte or partially-initialized file from a prior interrupted
  run is therefore correctly treated as "existing data" (Case
  B/C, never A) - which is exactly the specified behavior, not a gap,
  but worth naming explicitly since it means a truly broken partial
  file cannot self-heal via a fresh Case-A bootstrap; that remains a
  future recovery-flow question, not something M2 resolves.

---

## 18. Whether M2 is safe to proceed into the next design/freeze phase

Yes. Both safety issues this pass targeted are now closed with
deterministic, passing test evidence; no product action semantics
changed; no schema migration was introduced; `services::storage`'s
module-privacy boundary is unchanged. Nothing found during this pass
revealed a new blocking defect elsewhere in M2.

---

## 19. Recommended next step

Per the standing M2 sequencing (unchanged by this pass): a dedicated
design/freeze pass for `RecurringRule` (small, narrowly scoped) or a
dedicated planning pass for the Personal Knowledge Layer (the next real
net-new entity family) - neither should be started as a direct
implementation slice without that planning pass first, per the
precedent already set by the M2 Project Child Action Freeze report
before Milestone/Task were built.

---

## Addendum (R016, 2026-09-18) — review findings against this report

**The measured test/build results and code description above are left
exactly as originally written - nothing in this addendum edits or
retracts them.** A dedicated follow-up review
(`docs/reports/R016_M2_KEY_LIFECYCLE_DETERMINISM_AND_FAILURE_ATOMICITY_HARDENING_2026-09-18.md`)
found three gaps in this pass's own safety story, all fixed in R016:

**A. Case-A cleanup only proved the pre-file-creation failure path.**
`bootstrap_db_with_key_lifecycle`'s original rule (§7 above) deleted a
freshly-generated Case A credential on *any* subsequent database-
initialization failure, unconditionally. That is only safe if
initialization failed *before* any database file was created. If
`bootstrap_db_at` had instead (1) created/encrypted the database file
using the fresh key, then (2) failed *later* - during key verification
or migration, say - deleting the credential at that point would have
self-inflicted exactly the state this whole hardening pass exists to
prevent: an existing database file with no matching credential (Case
C), discovered on the very next launch. `req_kl7` (§9 above) only
forced the pre-file-creation case (a blocker file makes
`create_dir_all` fail before any database bytes are ever written); it
did not - and structurally could not, using that technique - prove the
post-file-creation case. R016 fixes the rule itself (delete the fresh
credential only if the database path still does not exist afterward)
and adds `req_dkl7`, which forces failure *after* a file has been
written, proving the corrected behavior directly.

**B. OS-keyring-dependent tests could return early as BLOCKED while
Cargo still counted them as passed**, so `req_kl1`-`req_kl10` were not,
on their own, sufficient deterministic proof of the canonical A/B/C/D
state machine - only proof that it worked on whatever machine happened
to run them with a working credential store that run. R016 introduces a
narrow `security::CredentialStore` trait, a production
`KeyringCredentialStore` adapter (the real keyring behavior, unchanged),
and a deterministic in-memory test double, then adds `req_dkl1`-
`req_dkl12` - the same production state-machine logic
(`resolve_master_key_with_store`/
`bootstrap_db_with_key_lifecycle_with_store`), driven through the
in-memory store so every proof always runs, regardless of OS credential
store availability. The original `req_kl*` suite is kept as a
supplementary, environment-dependent smoke/integration test against the
real OS keyring - still valuable, no longer the primary proof.

**C. Snapshot diagnostic safety relied on properties of the current
serializer's error text rather than structurally excluding it.** §12/§14
above concluded `serialize_required_snapshot`'s diagnostic was safe
because `serde_json::Error`'s *current* built-in error messages never
echo serialized field values - true, but an assumption about a
dependency's wording, not a guarantee enforced by the code itself. A
future custom `Serialize` implementation on some as-yet-unwritten domain
type could return `serde::ser::Error::custom(...)` with arbitrary text.
R016 changes the helper to use a fixed diagnostic string, excluding the
underlying serializer error's text entirely, and strengthens
`req_snap1` to prove this with a deliberate marker string that must
never appear in the resulting `CoreError`'s `Display`, `Debug`, or
`diagnostic()`.

See R016 for the full implementation, corrected cleanup rule, new
`security::CredentialStore` seam, the complete deterministic test
matrix, and updated measured results (262 tests total, up from this
report's 250).
