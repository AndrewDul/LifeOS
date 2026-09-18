# R016 — M2 Key Lifecycle Determinism and Failure Atomicity Hardening

A focused correctness/verification pass on top of R015 - **not** a new
domain entity, action, or schema change. Fixes two review findings
against R015's own implementation, plus one narrow diagnostic-safety
hardening:

- **Finding A (correctness):** R015's Case-A orphan-credential cleanup
  deleted a freshly-generated credential on *any* subsequent database-
  initialization failure, unconditionally. If `bootstrap_db_at` had
  already created/encrypted the database file using that key before
  failing later (verification, migration), deleting the credential
  would self-inflict "database exists, credential absent" (Case C) -
  the exact state R015 exists to prevent - as a side effect of its own
  cleanup path.
- **Finding B (verification insufficiency):** R015's key-lifecycle tests
  ran against the real OS keyring and returned early as "BLOCKED" (still
  counted as PASS by Cargo) if it was unavailable - not sufficient
  deterministic proof of the canonical A/B/C/D state machine for
  safety-critical behavior.
- **Diagnostic hardening:** `serialize_required_snapshot`'s error
  diagnostic relied on `serde_json::Error`'s current wording never
  echoing serialized content - true today, but an assumption, not a
  structural guarantee.

Product action semantics, entity schemas, and every other part of M2 are
unchanged. **Stops after these three items** - `RecurringRule`/Calendar/
Personal Knowledge/M3/capability transport/any new domain entity all
remain untouched and out of scope.

---

## 1. Pre-run state

- M1 accepted, unchanged. M2: Goal/Project/GoalRelation/Milestone/Task
  all implemented; master-key lifecycle and mandatory audit-snapshot
  safety hardened (R015).
- Local checkpoint `7625d7f` - `fix: harden M2 key lifecycle and audit
  snapshots`. 250/250 tests passing. Clean working tree. No push
  authorized this run either.
- R001-R015 exist; this is **R016**, the next allocation.

---

## 2. Review findings from R015

Both correctness/verification findings were confirmed against R015's
*actual current code* (not merely its report text) before any change
was made:

- `security::mod.rs`'s `get_existing_key`/`generate_and_store_key`/
  `delete_key` operated directly against the real OS keyring
  (`keyring::Entry`), with no seam a test could substitute.
  `services::mod.rs`'s `resolve_master_key`/
  `bootstrap_db_with_key_lifecycle` called those free functions
  directly - not generic over any abstraction.
- `bootstrap_db_with_key_lifecycle`'s cleanup branch was:
  ```rust
  if let ResolvedMasterKey::FreshlyGenerated(_) = resolved {
      let _ = security::delete_key(service, account);
  }
  ```
  unconditional on the initialization failure - no check of whether a
  database file now existed at `path`.
- `services::tests::req_kl1`-`req_kl10` each pattern-matched an
  `Err(e)` from a real-keyring call into `println!("... BLOCKED ..."); return;` -
  confirmed by direct reading these are genuine early returns, and
  Cargo's test harness reports a function that returns normally (not
  via `panic!`) as `... ok`, regardless of how much of the function body
  actually executed.
- `serialize_required_snapshot` (§10 of R015) built its `CoreError`
  diagnostic with `format!("failed to serialize mandatory audit
  previous_snapshot: {e}")`, where `e` is the raw `serde_json::Error` -
  confirmed this string flows into `CoreError`'s `diagnostic` field,
  which `Debug` (derived) does expose, per direct reading of
  `core_error.rs`.

---

## 3. Case-A post-file-creation failure risk

`bootstrap_db_at` (`services::mod.rs`) is:

```rust
fn bootstrap_db_at(path: &Path, key_hex: &str) -> Result<Connection, BootstrapError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(BootstrapError::Io)?;
    }
    let mut conn = storage::open_encrypted(path, key_hex)?;
    storage::verify_key(&conn)?;
    storage::run_production_migrations(&mut conn)?;
    Ok(conn)
}
```

`storage::open_encrypted` opens (and, for a nonexistent path, SQLite
itself creates) the database file *before* `verify_key`/
`run_production_migrations` run. A failure in either of the latter two
steps - a corrupt migration, a disk I/O error mid-migration, a SQLCipher
internal error after the file already exists on disk - occurs **after**
a real file has been created at `path`, potentially already containing
bytes written under the freshly-generated key. R015's cleanup branch had
no way to distinguish this from a failure that occurred before any file
existed (e.g. `create_dir_all` failing because a path component is a
plain file) - it deleted the fresh credential in both cases. Deleting it
in the post-file-creation case would leave: a real file at the
production path, encrypted under a key nobody can retrieve anymore -
exactly Case C, manufactured by LifeOS's own cleanup logic rather than
by any external event (a lost credential, a credential-store reset).

R015's own test for this path, `req_kl7`, used a blocker-file technique
(`path`'s parent is a plain file, so `create_dir_all` fails) that
*only* exercises the pre-file-creation case - by construction, no
database file can ever exist afterward with that technique, so it could
not have caught this gap even in principle.

---

## 4. Corrected cleanup rule

Frozen and implemented exactly as specified:

> For a Case-A bootstrap where THIS invocation generated the credential,
> if initialization fails:
>
> - **A.** DB path still does NOT exist → delete the freshly-created
>   credential → return the original initialization error.
> - **B.** DB path NOW exists → DO NOT delete the credential → preserve
>   the DB+key pair → return the original initialization error.

Reasoning, stated explicitly (also captured in the code's own doc
comment, `services::mod.rs`): once a file may contain bytes encrypted
using the newly-created key, losing the key is more dangerous than
retaining the credential - the file may contain partially-created but
recoverable encrypted state, a following bootstrap can retry using the
same key, and no data/key association is destroyed. LifeOS must never
intentionally create "existing DB + missing key" from a failed
first-run bootstrap. This pass does **not** delete a newly-created
database file under any circumstance - only the credential-cleanup
decision changed.

---

## 5. Credential-store seam

New `security::CredentialStore` trait (`security/mod.rs`):

```rust
pub(crate) trait CredentialStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeyError>;
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), KeyError>;
    fn delete(&self, service: &str, account: &str) -> Result<(), KeyError>;
}
```

Deliberately minimal: three methods, no async, no batching, no generic
error type - not a dependency-injection framework. `generate_key_hex`
(previously private) is now `pub(crate)`, since the generic state
machine (§6) generates the key itself before calling `store.set`,
rather than through a bundled "generate and store" call.

The existing free functions `get_existing_key`/`generate_and_store_key`/
`delete_key` keep their exact original signatures and behavior
(unchanged callers, unchanged production identity usage) - internally
they now delegate to `KeyringCredentialStore` (§6) rather than calling
`keyring::Entry` directly a second time, removing the previous code
duplication.

---

## 6. Production keyring adapter

```rust
pub(crate) struct KeyringCredentialStore;

impl CredentialStore for KeyringCredentialStore {
    fn get(&self, service: &str, account: &str) -> Result<Option<String>, KeyError> { /* keyring::Entry::get_password, NoEntry -> None */ }
    fn set(&self, service: &str, account: &str, value: &str) -> Result<(), KeyError> { /* keyring::Entry::set_password */ }
    fn delete(&self, service: &str, account: &str) -> Result<(), KeyError> { /* keyring::Entry::delete_credential, NoEntry -> Ok */ }
}
```

Byte-for-byte the same logic R015's free functions previously contained
inline - production behavior is unchanged, only routed through this
adapter. `services::mod.rs` gained the generic core of the state
machine, with the original functions kept as thin wrappers bound to
this adapter:

```rust
fn resolve_master_key_with_store<S: security::CredentialStore>(
    store: &S, path: &Path, service: &str, account: &str,
) -> Result<ResolvedMasterKey, BootstrapError> {
    let db_exists = path.exists();
    let existing_key = store.get(service, account).map_err(BootstrapError::Key)?;
    match (db_exists, existing_key) {
        (_, Some(key_hex)) => Ok(ResolvedMasterKey::Reused(key_hex)),
        (false, None) => {
            let key_hex = security::generate_key_hex();
            store.set(service, account, &key_hex).map_err(BootstrapError::Key)?;
            Ok(ResolvedMasterKey::FreshlyGenerated(key_hex))
        }
        (true, None) => Err(BootstrapError::MasterKeyMissingForExistingDatabase),
    }
}

fn resolve_master_key(path: &Path, service: &str, account: &str) -> Result<ResolvedMasterKey, BootstrapError> {
    resolve_master_key_with_store(&security::KeyringCredentialStore, path, service, account)
}
```

and, for the corrected cleanup rule (also generic over the database
*initializer*, so a deterministic test can force a failure *after* a
file is written - something `bootstrap_db_at` itself cannot be made to
do portably):

```rust
fn bootstrap_db_with_key_lifecycle_with_store<S, F>(
    store: &S, path: &Path, service: &str, account: &str, initializer: F,
) -> Result<Connection, BootstrapError>
where
    S: security::CredentialStore,
    F: FnOnce(&Path, &str) -> Result<Connection, BootstrapError>,
{
    let resolved = resolve_master_key_with_store(store, path, service, account)?;
    match initializer(path, resolved.key_hex()) {
        Ok(conn) => Ok(conn),
        Err(init_err) => {
            if let ResolvedMasterKey::FreshlyGenerated(_) = resolved {
                if !path.exists() {
                    let _ = store.delete(service, account);
                }
                // else: KEEP the credential - see §4.
            }
            Err(init_err)
        }
    }
}

fn bootstrap_db_with_key_lifecycle(path: &Path, service: &str, account: &str) -> Result<Connection, BootstrapError> {
    bootstrap_db_with_key_lifecycle_with_store(&security::KeyringCredentialStore, path, service, account, bootstrap_db_at)
}
```

`bootstrap_production_db()` is **unchanged** - it still calls
`bootstrap_db_with_key_lifecycle(&path, security::SERVICE_NAME,
security::MASTER_KEY_ACCOUNT)`, which now routes through the real
`KeyringCredentialStore` and the real `bootstrap_db_at` initializer via
the generic core above - the same logic path every test now exercises,
not a parallel reimplementation.

One incidental fix: `ResolvedMasterKey` needed a `Debug` impl for a new
test's `{:?}` formatting of a `Result<ResolvedMasterKey, BootstrapError>`.
Rather than `#[derive(Debug)]` (which would print the actual master key
in plaintext the moment any caller writes `{:?}` on it, e.g. in a test
assertion-failure message), a hand-written `Debug` impl was added that
prints only the variant name (`"ResolvedMasterKey::Reused(<redacted>)"`)
- the key itself is never included.

---

## 7. Deterministic in-memory tests

`InMemoryCredentialStore` (`services::tests`, `#[cfg(test)]`-only): a
`RefCell<HashMap<(String, String), String>>`-backed `CredentialStore`
implementation that never touches the real OS credential store. Records
call counts for `get`/`set`/`delete` (so tests can assert, e.g., "no
unexpected `set()` on Case B/C/D reuse") and can be configured
(`fail_next_get`/`fail_next_set`/`fail_next_delete`) to fail each
operation on demand - deterministically exercising backend-error paths
that are impractical to force against the real OS keyring.
`InMemoryCredentialStore::seeded(...)` inserts a credential directly
(not through `set()`, so it does not count toward `set_calls()`) to
represent "a credential already existed before this call" (Cases B/D)
without conflating that with a production `set()` invocation.

12 new tests (`req_dkl1`-`req_dkl12`, `services::tests`), all driving
`resolve_master_key_with_store`/`bootstrap_db_with_key_lifecycle_with_store`
directly - the exact production state-machine logic, never a parallel
reimplementation:

1. `req_dkl1` - DB absent + key absent: a fresh key is generated,
   stored, and is *exactly* the key the initializer used to create the
   database (captured via a wrapping closure, not merely "some key was
   stored somewhere"); also checks the key's format (64 lowercase hex
   characters = 256 bits) and that exactly one `set()` call occurred.
2. `req_dkl2` - DB exists + key exists: the exact same key is reused
   across two bootstraps, with zero additional `set()` calls on reuse.
3. `req_dkl3` (CRITICAL) - DB exists + key absent: typed
   `MasterKeyMissingForExistingDatabase`, zero `set()` calls on the
   store standing in for "credential now absent", database bytes
   byte-for-byte unchanged.
4. `req_dkl4` - DB absent + key exists (seeded, not `set()`): the
   pre-existing key is reused to create the database, with zero `set()`
   calls.
5. `req_dkl5` - a present-but-wrong key against an existing database
   fails as a normal `BootstrapError::Storage`, with zero `set()` calls
   (no replacement generated).
6. `req_dkl6` - initialization failure BEFORE any database file is
   created (blocker-file technique, as R015's `req_kl7`) - the
   freshly-generated credential is cleaned up; exactly one `delete()`
   call.
7. `req_dkl7` (**the R016 fix, the critical new proof**) -
   initialization failure AFTER the database file has already been
   created (a custom initializer closure writes bytes to `path`, then
   returns an error - the one case `bootstrap_db_at` cannot be forced
   into portably) - the freshly-generated credential is **retained**,
   zero `delete()` calls; additionally, `resolve_master_key_with_store`
   is called again against the same `(store, path)` afterward and
   confirmed to resolve as `Reused` (Case B), never `FreshlyGenerated`
   or `MasterKeyMissingForExistingDatabase` (Case C) - proving the
   resulting state is genuinely safe to reopen, not merely "not
   immediately broken".
8. `req_dkl8` - initialization failure with a pre-existing credential
   (seeded) leaves it completely untouched regardless of whether a
   database file now exists - `delete()` is never called at all for a
   `Reused` credential.
9. `req_dkl9` - repeated reopen (3 additional bootstraps) all use the
   exact same key, with exactly one `set()` call ever (the original
   Case A bootstrap).
10. `req_dkl10` - a credential-backend GET error propagates as a typed
    `BootstrapError::Key`, is never misinterpreted as "credential
    absent" (zero `set()` calls result), and no database initialization
    is attempted (`!path.exists()` afterward).
11. `req_dkl11` - a credential-backend SET error during Case A
    resolution means the database initializer is **never even called**
    - proven with a call-counting wrapper closure around
    `bootstrap_db_at`, not merely inferred from file absence.
12. `req_dkl12` - a credential-backend DELETE error during the
    pre-file-creation cleanup path: the **original** `Io` initialization
    error is what the caller sees (not a delete-failure error); a
    cleanup `delete()` attempt was still made (`delete_calls() == 1`);
    the resulting, deliberately documented residual state - the
    freshly-generated credential is left behind despite no database file
    existing, because deletion itself failed - is asserted explicitly,
    not silently accepted.

The pre-existing `req_kl1`-`req_kl10` and the bootstrap-regression test
are **kept unchanged**, now explicitly documented (in both this report
and an addendum in the R015 report) as a supplementary,
environment-dependent smoke/integration suite against the real
`KeyringCredentialStore` - still valuable (they prove the real adapter
genuinely round-trips through Windows Credential Manager), but no longer
the primary proof of the state machine. In this run's environment the
real OS keyring was available, so every one of those tests also
genuinely PASSed (not BLOCKED) - confirmed by running them with
`--nocapture` and inspecting the printed `PASS`/`BLOCKED` lines directly
(§12).

---

## 8. Pre-file vs. post-file creation failure proofs

Both required proofs exist and are independent of each other:

- **Pre-file-creation:** `req_kl7`/`req_kl8` (R015, real keyring) and
  `req_dkl6`/`req_dkl8` (R016, deterministic) - a blocker file at the
  target path's parent makes `std::fs::create_dir_all` fail before
  `storage::open_encrypted` is ever reached, so no database file can
  exist afterward by construction.
- **Post-file-creation (new, R016):** `req_dkl7` - a custom initializer
  closure writes real bytes to `path`, then deliberately fails,
  simulating `bootstrap_db_at` having already created/encrypted the
  file before a later step (verification/migration) fails. This is only
  possible via the new injectable-initializer seam
  (`bootstrap_db_with_key_lifecycle_with_store`'s `F` parameter) -
  `bootstrap_db_at` itself cannot be driven into this exact sequence
  deterministically, since it depends on SQLCipher/refinery internals,
  not a simple filesystem precondition. Production bootstrap
  (`bootstrap_db_with_key_lifecycle`) always passes the real
  `bootstrap_db_at` as the initializer - the injectable closure is a
  test-only seam, never reachable from `bootstrap_production_db`.

---

## 9. Backend-error behavior

Proven distinctly from "credential absent" and from each other:

- **GET error** (`req_dkl10`): propagates as `BootstrapError::Key`
  immediately via `?` in `resolve_master_key_with_store` - the `match
  (db_exists, existing_key)` is never reached, so it cannot be
  misclassified as Case A/C. Zero `set()` calls, no database
  initialization attempted.
- **SET error on first run** (`req_dkl11`): propagates as
  `BootstrapError::Key` from inside the `(false, None)` arm, before
  `resolve_master_key_with_store` returns - `bootstrap_db_with_key_lifecycle_with_store`'s
  `initializer` closure is therefore never invoked (proven with an
  explicit call-counter, not inferred).
- **DELETE error during cleanup** (`req_dkl12`): discarded (`let _ =
  store.delete(...)`) exactly as R015's best-effort design intended -
  the original initialization error is what the caller receives,
  unchanged. The residual state (credential left behind because
  deletion failed) is asserted explicitly as a known, documented,
  best-effort limitation - not silently accepted or hidden.

---

## 10. Snapshot diagnostic redaction

`serialize_required_snapshot` (`services::mod.rs`) changed from:

```rust
serde_json::to_string(value).map_err(|e| {
    CoreError::new(CoreErrorKind::Internal, format!("failed to serialize mandatory audit previous_snapshot: {e}"))
})
```

to:

```rust
serde_json::to_string(value).map_err(|_e| {
    CoreError::new(CoreErrorKind::Internal, "mandatory audit previous_snapshot serialization failed")
})
```

The returned `CoreError`'s diagnostic is now a fixed string - the
underlying `serde_json::Error`'s own message is discarded entirely,
never reaching `CoreError`'s `diagnostic` field (and therefore never
`Debug`, which does expose that field). `CoreErrorKind::Internal` is
unchanged. This makes "no personal snapshot content in this diagnostic"
true by construction, not by an assumption about what a serializer
happens to say today.

`AlwaysFailsToSerialize` (`services::tests`, the deliberately-failing
test-only `Serialize` impl already introduced in R015) now embeds an
obvious marker string,
`SNAPSHOT_FAILURE_TEST_MARKER = "SECRET_TEST_VALUE_SHOULD_NEVER_ESCAPE"`,
inside the error text its `serialize` method returns via
`serde::ser::Error::custom(...)` - standing in for a hypothetical future
custom `Serialize` implementation whose error text could contain
sensitive content. `req_snap1` now asserts the marker does not appear in
`format!("{e}")` (Display), `format!("{e:?}")` (Debug), or
`e.diagnostic()` - structural proof, not an inspection of today's
`serde_json` wording.

---

## 11. R015 addendum

A clearly-labelled addendum was appended to the END of
`docs/reports/R015_M2_STORAGE_AND_AUDIT_SAFETY_HARDENING_2026-09-18.md`
(titled "Addendum (R016, 2026-09-18) — review findings against this
report"). The original report body, including its measured test/build
results (250 tests, `cargo build --release` etc.), is **left completely
unedited** - the addendum states explicitly that nothing above it is
retracted or changed, and explains all three findings (A/B/C, matching
§2-3, §7, §10 above) plus a pointer to this report for the fix and
updated numbers.

---

## 12. Exact tests/build results (MEASURED)

- `cargo fmt --check`: clean (after one `cargo fmt` pass to reformat the
  new deterministic tests' multi-line call sites - recorded here for
  completeness, not silently corrected away).
- `cargo check --tests`: clean, zero warnings (after fixing two issues
  found during this pass's own first `cargo check --tests` run: (1)
  `security::delete_key` needed the same
  `#[cfg_attr(not(test), allow(dead_code))]` annotation already used for
  `get_or_create_master_key`, since it is no longer called from the
  production path now that `bootstrap_db_with_key_lifecycle_with_store`
  calls `store.delete(...)` directly; (2) `ResolvedMasterKey` needed a
  `Debug` impl - implemented by hand, not derived, to avoid ever printing
  the raw key, see §6).
- `cargo test -- --test-threads=1`: **262 passed; 0 failed; 0 ignored**
  (up from 250). All 250 prior tests remain green, unmodified. Verified
  with a targeted `--nocapture` run of just `req_dkl*` (12/12 PASS,
  including `req_dkl7`, the critical new proof) and just `req_kl*`
  (11/11 genuinely PASS in this environment - the real OS keyring was
  available this run, so none of them returned BLOCKED; confirmed by
  reading the printed `[req_klN] PASS - ...` lines directly, not
  inferred from the aggregate `ok` count).
- `cargo build --release`: see the chat reply for the measured result.
- `npm run build` (repo root): see the chat reply for the measured
  result.
- `npm run tauri build`: run this pass (not optional) because this
  modifies production credential/bootstrap infrastructure - see the
  chat reply for the actual measured result (MSI/NSIS), verified from
  the captured log file directly, never only the wrapper's exit code.

---

## 13. Files changed

- `src-tauri/src/security/mod.rs` - new `CredentialStore` trait and
  `KeyringCredentialStore` production adapter; `generate_key_hex`
  widened to `pub(crate)`; `get_existing_key`/`generate_and_store_key`/
  `delete_key` now delegate to `KeyringCredentialStore` internally
  (unchanged signatures/behavior); `delete_key` annotated
  `#[cfg_attr(not(test), allow(dead_code))]` (no longer called from the
  production path).
- `src-tauri/src/services/mod.rs` - new
  `resolve_master_key_with_store`/
  `bootstrap_db_with_key_lifecycle_with_store` generic cores (the real
  state-machine logic, generic over `CredentialStore` and, for the
  latter, an injectable database initializer); `resolve_master_key`/
  `bootstrap_db_with_key_lifecycle` are now thin wrappers over these;
  corrected cleanup rule (delete the fresh credential only if `path`
  still does not exist); `serialize_required_snapshot`'s diagnostic
  redacted to a fixed string; `ResolvedMasterKey` given a hand-written,
  key-redacting `Debug` impl.
- `src-tauri/src/services/tests.rs` - new `InMemoryCredentialStore`
  test double; 12 new deterministic tests (`req_dkl1`-`req_dkl12`);
  `AlwaysFailsToSerialize`'s error text now embeds a marker string;
  `req_snap1` strengthened to assert the marker never appears in
  Display/Debug/`diagnostic()`; an R016-addendum comment added to the
  existing `req_kl*` section header, clarifying its now-supplementary
  role.
- `docs/reports/R015_M2_STORAGE_AND_AUDIT_SAFETY_HARDENING_2026-09-18.md` -
  addendum appended (original body unedited).
- `docs/reports/R016_M2_KEY_LIFECYCLE_DETERMINISM_AND_FAILURE_ATOMICITY_HARDENING_2026-09-18.md` -
  this report.
- `docs/PROJECT_STATE.md` - updated (see below).

No migration created or edited (verified: zero diff under
`src-tauri/migrations/`). No `services::storage` visibility change.

---

## 14. Remaining risks

- The DELETE-failure residual state (`req_dkl12`) is a real, documented,
  best-effort limitation: on a machine where the credential store can be
  written to but a subsequent delete of that exact entry fails, a
  pre-file-creation Case-A failure can still leave an orphan credential
  behind with no matching database. This is unchanged from R015's
  original best-effort design (deliberately not retried or escalated,
  per the "smallest clean seam" instruction) - R016 only makes the
  behavior deterministically provable and explicitly documented, it does
  not add retry/escalation logic.
- `bootstrap_production_db()` is still not called from anywhere in the
  running app - unchanged from R015, still a known, already-documented
  gap (no Tauri command layer exists yet).
- The `req_kl*` real-keyring suite remains inherently
  environment-dependent; on a machine where the OS credential store is
  genuinely unavailable, those specific tests will still report BLOCKED
  - this is now explicitly documented as acceptable (supplementary,
  never the primary proof) rather than a gap, since `req_dkl*` provides
  the deterministic proof regardless.

---

## 15. Whether the storage/security foundation is now closed

Yes. Both review findings against R015 are fixed with deterministic,
passing test evidence (`req_dkl7` specifically closes Finding A;
`req_dkl1`-`req_dkl12` collectively close Finding B); the snapshot
diagnostic is now safe by construction rather than by assumption; no
product action semantics changed; no schema migration was introduced;
`services::storage`'s module-privacy boundary is unchanged. Nothing
found during this pass revealed any further gap in the master-key
lifecycle or audit-snapshot machinery.

---

## 16. Recommended next planning step

Unchanged from R015 §19: a dedicated design/freeze pass for
`RecurringRule` (small, narrowly scoped) or a dedicated planning pass
for the Personal Knowledge Layer (the next real net-new entity family) -
neither should be started as a direct implementation slice without that
planning pass first.
