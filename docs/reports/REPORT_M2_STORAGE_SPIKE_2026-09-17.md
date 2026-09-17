# M2 Storage Spike: Encrypted SQLite Candidate Verification

Date: 2026-09-17
Scope: first M2 implementation step. Architecture errata (Phase A) +
storage-candidate verification (Phase B) + an isolated encrypted-storage
spike (Phase C) proving 10 numbered requirements. No Goal/Project/Fact
production schema. No action/service layer beyond what the spike itself
needed. No Qwen/Ollama, no GPT/Luna. M3 not started.

Companion documents: `docs/reports/REPORT_M2_ARCHITECTURE_FREEZE_2026-09-17.md`
(now corrected in place per Phase A below), `docs/LIFEOS_SPEC.md` §5.

---

## Phase A — Architecture errata

### A1. Storage visibility / compile-time boundary — corrected

**The problem, exactly as flagged:** the architecture freeze described
`storage/` as a *sibling* of `services/`, made callable "only from
services" via `pub(crate)`/`pub(super)`. That claim does not hold under
Rust's actual visibility rules: `pub(crate)` is visible to the entire
crate (including a future `capability/`, `domain/`, anything else) -
not just `services`; `pub(super)` on a sibling module only reaches
their shared parent (`lib.rs`), which is exactly where `capability/`
would also live. Neither gives the intended guarantee. This was caught
during review, before any code existed.

**The fix, implemented and verified for real this run:** `storage`
nests *inside* `services` as a private submodule, not a sibling:

```
src-tauri/src/
├── lib.rs              mod services;   (private - default visibility)
└── services/
    ├── mod.rs           mod storage;   (private - default visibility)
    │                    pub(crate) fn create_and_seed(...) { ... calls storage:: ... }
    ├── storage/
    │   └── mod.rs        pub(super) fn open_encrypted(...) { ... }
    │                     (pub(super) is defense-in-depth: even if the
    │                      `mod storage;` declaration were ever loosened
    │                      to pub(crate) by mistake, these functions
    │                      would still only reach `services` itself)
    └── tests.rs
```

Because `mod storage;` in `services/mod.rs` has no `pub` at all, Rust's
default module privacy means **nothing outside the `services` module
tree can even name `services::storage`** - not a sibling module in the
same crate, not an external test crate, nothing.

**Proof, not just a claim:** a throwaway function was added to `lib.rs`
(sibling to `services`, exactly where a future `capability/` module
would sit) attempting `services::storage::open_encrypted` directly, then
removed immediately after capturing the result:

```rust
// lib.rs, temporarily:
fn illegal_boundary_probe() {
    let _ = services::storage::open_encrypted;
}
```

```
$ cargo check
error[E0603]: module `storage` is private
  --> src\lib.rs:19:23
   |
19 |     let _ = services::storage::open_encrypted;
   |                       ^^^^^^^  -------------- function `open_encrypted` is not publicly re-exported
   |                       |
   |                       private module
   |
note: the module `storage` is defined here
  --> src\services\mod.rs:19:1
   |
19 | mod storage;
   | ^^^^^^^^^^^^
```

The probe function was deleted immediately after this capture; `cargo
check --tests` was re-run afterward and is clean (see Phase C evidence
below). `docs/LIFEOS_SPEC.md` §5.6 and the architecture-freeze report's
§G have been corrected in place to describe this structure, not the
original sibling-module claim.

### A2. `ForgetMemory` semantics — corrected

**The problem:** the frozen confirmation policy treated `DELETE` as one
uniform behavior ("soft-delete/tombstone... Undo remains possible"). That
is correct for `CancelEvent` and ordinary domain removal, but wrong for
`ForgetMemory`: if a user explicitly asks LifeOS to forget personal
information, leaving the original payload sitting in a soft-deleted row
would not actually honor that request - it would just hide it from
normal queries while the sensitive data remains fully recoverable in the
database.

**The fix:** `ForgetMemory` is now documented as permanently deleting
the targeted `Fact` payload and any dependent sensitive `Evidence` - a
real removal, not a flag flip. Only a minimal, non-sensitive audit
marker (record id/hash, deletion timestamp, action type) may remain; the
original forgotten text/value must never persist anywhere afterward,
including in backups taken after the deletion or in logs. `Undo` is not
offered for `ForgetMemory` past its (mandatory, blocking) confirmation
step - offering it would mean secretly retaining exactly the data the
user asked to have removed.

This is a documentation-only correction (`docs/LIFEOS_SPEC.md` §5.4/§5.5,
the architecture-freeze report's §E/§F) - `ForgetMemory` itself is not
implemented yet (no action layer exists this run), so there is no code
to change, only the frozen design to get right before it is built.

---

## Phase B — Storage candidate verification

**Instruction:** do not trust the architecture freeze's assumptions
about sqlite3mc's current Rust integration - verify directly.

### sqlite3mc: no viable native-Rust integration found

Measured, not assumed, via the crates.io API and direct fetches
(2026-09-17):

- `rusqlite` 0.40.2 (the current published version at research time)
  has **no `sqlite3mc` feature at all** - its encryption-related
  features are exclusively `sqlcipher`, `bundled-sqlcipher`, and
  `bundled-sqlcipher-vendored-openssl` (confirmed by fetching the
  crate's own published feature list from the crates.io API directly,
  not a secondary source).
- The upstream SQLite3MultipleCiphers project's own GitHub repository
  does not document or reference any Rust bindings.
- `sqlite-wasm-rs` does support a `sqlite3mc` feature, but is explicitly
  **WASM/browser-only** (`wasm32-unknown-unknown` bindings) - not usable
  for a native Tauri desktop backend process.
- The only crate found that wraps sqlite3mc for native Rust,
  `walletkit-sqlite` ("Safe Rust wrapper around sqlite3mc for
  WalletKit"), was **created 2026-09-02 - 15 days before this
  research** - with 78 total downloads, tied to a specific company's
  (Worldcoin) internal WalletKit project, not a general-purpose
  community crate. This does not meet "prefer maintained/current APIs."

**Conclusion: sqlite3mc is not practically testable in this run.** The
only paths to using it would be either hand-rolling a custom
`libsqlite3-sys`-style FFI build vendoring the sqlite3mc amalgamation
(a substantial, unproven engineering effort - not "the smallest isolated
spike"), or depending on a 15-day-old, 78-download crate for LifeOS's
foundational encrypted storage. Neither is appropriate to adopt or even
attempt within this spike. This is a **finding, not a rejection on
technical/functional grounds** - sqlite3mc was never actually run
against the 10 requirements below, because there is no mature dependency
to run it through.

### Pivot: SQLCipher (the documented fallback) is the tested candidate

Per the architecture freeze's own contingency ("SQLCipher remains the
documented fallback"), this run pivots to it as the actual subject of
Phase C. SQLCipher via `rusqlite`'s native features is mature (rusqlite:
107M+ total downloads, `sqlcipher`/`bundled-sqlcipher`/
`bundled-sqlcipher-vendored-openssl` features have existed for years) and
was fully testable.

### Exact dependency versions/features selected, and why

| Crate | Version | Features | Why |
|---|---|---|---|
| `rusqlite` | `=0.37.0` (pinned) | `bundled-sqlcipher`, `backup` | **Not** the latest (0.40.2) - `refinery-core` 0.9.0's `rusqlite` feature requires `rusqlite = ">=0.23, <=0.37"` (confirmed via a real `cargo check` resolver failure against 0.40 before pinning down - see evidence below). `bundled-sqlcipher-vendored-openssl` was tried first and rejected (see below); `bundled-sqlcipher` links against an external OpenSSL instead. `backup` feature needed for `rusqlite::backup::Backup` (requirement 7/8). |
| `refinery` | `0.9.2` (latest) | `rusqlite` | Mature (10M+ downloads), has a first-class `rusqlite` feature; migrations run as plain SQL over any `rusqlite::Connection` regardless of which cipher backend is underneath. |
| `keyring` | `4.2.0` (latest) | default | Mature (25M+ downloads), current major version, Windows Credential Manager support built in. |
| `rand` | `0.10.2` (latest) | default | For 256-bit key generation. Note: `rand` 0.9+ renamed `thread_rng()` to `rng()`; `Rng::fill_bytes` requires `use rand::Rng` in scope (not `RngCore` as originally assumed - a real compile error, corrected during this run). |
| `tempfile` | `3.27.0` (latest, dev-dependency) | default | Isolated per-test temp directories, no artifacts left in the repo. |

**Real resolver/build evidence collected while selecting these
versions** (not assumed):

1. `rusqlite = "0.40"` (latest) + `refinery = { features = ["rusqlite"] }` →
   **dependency resolution failure**:
   ```
   error: failed to select a version for `libsqlite3-sys`.
       ... required by package `rusqlite v0.40.0`
   versions that meet the requirements `^0.38.0` are: 0.38.2, 0.38.1, 0.38.0
   package `libsqlite3-sys` links to the native library `sqlite3`, but it conflicts with a previous package...
       ... which satisfies dependency `rusqlite = ">=0.23, <=0.37"` of package `refinery-core v0.9.0`
   ```
   → pinned `rusqlite = "=0.37.0"` instead.

2. `rusqlite = "0.37.0"` + `bundled-sqlcipher-vendored-openssl` →
   **build failure** compiling OpenSSL from source:
   ```
   error: failed to run custom build command for `openssl-sys v0.9.117`
   ...
   Can't locate Locale/Maketext/Simple.pm in @INC ...
   ```
   The Perl bundled with Git for Windows (this machine's only readily
   available Perl) is a minimal distribution missing core CPAN modules
   OpenSSL's `Configure` script needs; `cpan -f Locale::Maketext::Simple`
   was attempted and also failed (`Can't locate CPAN/Author.pm` - CPAN
   itself is incomplete in this Perl distribution). Not pursued further
   (installing a full Perl distribution is out of scope for a spike).
   → switched to `bundled-sqlcipher` (links an *external* OpenSSL)
   instead of building one from source.

3. `bundled-sqlcipher` without a discoverable OpenSSL → **build panic**:
   `Missing environment variable OPENSSL_DIR or OPENSSL_DIR is not set`.
   No system OpenSSL dev package was present (the `openssl.exe` binaries
   bundled with Git for Windows/MSYS2 are MinGW-built, not MSVC-ABI-
   compatible, and lack `.lib` import libraries usable by an MSVC
   linker). → installed a prebuilt Windows OpenSSL dev package via
   `winget install ShiningLight.OpenSSL.Dev` (no admin elevation
   required, unlike the VS installer in the M0 build-start run -
   succeeded cleanly), then set `OPENSSL_DIR`/`OPENSSL_LIB_DIR`/
   `OPENSSL_INCLUDE_DIR` via `src-tauri/.cargo/config.toml` pointing at
   its install location (`lib/VC/x64/MD` for the MSVC-dynamic-CRT
   variant matching Rust's default Windows target).

4. Two straightforward Rust compile errors, both fixed and re-verified:
   `use rand::RngCore` → `use rand::Rng` (0.10's `fill_bytes` needs the
   `Rng` trait in scope, not `RngCore` directly); `rusqlite::backup`
   required explicitly enabling the `backup` feature.

**Known limitation, stated plainly:** `src-tauri/.cargo/config.toml` now
contains a machine-specific absolute path
(`C:\Program Files\OpenSSL-Win64\...`) to make `bundled-sqlcipher` build
on this machine. Any other machine continuing M2 work needs the same
OpenSSL dev package installed at that path, or these variables updated
to match. This is real Windows-specific friction worth remembering if
M2's real implementation later revisits `bundled-sqlcipher-vendored-openssl`
(which would remove this machine-specific dependency, if a working Perl
is ever set up) or moves to a different platform.

---

## Phase C — Isolated storage spike

**Architecture:** `src-tauri/src/services/` (the corrected structure
from A1). `spike_records(id, secret_text, created_at)` only - no
Goal/Project/Fact tables. Schema evolves via two refinery migrations:
`migrations/V1__init.sql` (creates `spike_records`),
`migrations/V2__add_note_column.sql` (adds a `note` column) - proving
migration mechanics without touching real domain modeling.

**Key management approach tested:** a random 256-bit key is generated
(`rand` 0.10), hex-encoded (64 chars), and used both as the SQLCipher
raw-key material (`PRAGMA key = "x'<hex>'"` - the raw-key form, since our
key is already random and doesn't need SQLCipher's PBKDF2 password
derivation) and as the string stored in/retrieved from the OS credential
store via the `keyring` crate.

**Test command:**
```
cd src-tauri
cargo test services::tests -- --nocapture --test-threads=1
```

### Results — every requirement, PASS/FAIL/BLOCKED, with evidence

**1. Encrypted database creation** — **PASS**
```
test services::tests::req1_encrypted_database_creation_and_read ...
[req1] PASS - wrote and read back 1 row with correct key: [(1, "LIFEOS_STORAGE_SPIKE_SECRET_7F3A91")]
ok
```

**2. Persistence across restart** — **PASS**, with a stated scope note:
SQLite is an embedded, file-based engine with no separate server
process - there is nothing analogous to "restart the server" beyond
closing every `Connection` and opening a fresh one against the same
file, which is exactly what this test does (explicit scope-drop of the
writing connection, then a genuinely independent new `Connection` reads
it back). This is the practically meaningful proof for this engine, not
a shortcut around the requirement.
```
test services::tests::req2_persistence_across_reopen ...
[req2] PASS - data recovered after full connection teardown and reopen: [(1, "phase-1-data")]
ok
```

**3. Wrong key fails cleanly** — **PASS**. Real SQLCipher-level HMAC
verification failure surfaced through rusqlite as a typed
`rusqlite::Error` (`SqliteFailure`/`NotADatabase`), not a panic, not
garbage data:
```
test services::tests::req3_wrong_key_fails_cleanly ...
2026-09-17 09:50:27.867: ERROR CORE sqlcipher_page_cipher: hmac check failed for pgno=1
2026-09-17 09:50:27.870: ERROR CORE sqlite3Codec: error decrypting page 1 data: 1
[req3] PASS - wrong key rejected with typed error: file is not a database
ok
```

**4. Encrypted at rest** — **PASS**. The 34-byte marker
`LIFEOS_STORAGE_SPIKE_SECRET_7F3A91` was searched for byte-for-byte
across the entire 20,480-byte raw database file after a full connection
close; not found:
```
test services::tests::req4_encrypted_at_rest_no_plaintext ...
[req4] PASS - 20480 byte database file does not contain the 34-byte plaintext marker anywhere
ok
```

**5. Migration V1 → V2** — **PASS**. Applies to an already-populated
database, data survives, a second run against an up-to-date database
applies zero migrations (not reapplied/duplicated), and the `note`
column from V2 is confirmed present by successfully selecting it:
```
test services::tests::req5_migration_v1_to_v2 ...
[req5] PASS - schema at V1+V2, data intact across repeat migration run (second run applied 0 new migrations - expected 0)
[req5] PASS - V2 `note` column present, confirming the ALTER TABLE migration ran
ok
```

**6. Rekey** — **PASS**. Old key rejected after rekey (same typed error
path as requirement 3), new key works, data identical:
```
test services::tests::req6_rekey ...
2026-09-17 09:50:28.004: ERROR CORE sqlcipher_page_cipher: hmac check failed for pgno=1
[req6] PASS - old key rejected post-rekey (Err(Storage(SqliteFailure(Error { code: NotADatabase, extended_code: 26 }, Some("file is not a database"))))), new key works, data identical: [(1, "LIFEOS_STORAGE_SPIKE_SECRET_7F3A91")]
ok
```

**7. Backup (same-installation, current M2 key model)** — **PASS**.
Uses SQLite's own online backup API (`rusqlite::backup::Backup`) into a
second encrypted file, keyed the same as the source; the backup file
itself was also checked for the plaintext marker (not found):
```
test services::tests::req7_8_backup_and_restore ...
[req7] PASS - encrypted backup created, 20480 bytes, no plaintext found
ok
```
(Explicitly out of scope per the build brief: a future cross-machine
password/recovery-key-wrapped portable backup - `LIFEOS_SPEC.md` §5 /
architecture-freeze report §H - not built or tested here.)

**8. Restore** — **PASS**. Backup file copied to a fresh location,
opened with the same key, row count (2, deterministic), full content
equality, and a content hash all verified to match the original:
```
[req8] PASS - restored 2 rows, content hash matches: f8c26039c20c4888
ok
```

**9. Corruption failure** — **PASS**. A COPY of a populated database was
byte-corrupted at its midpoint (8 bytes overwritten with `0xDEADBEEF00FF00FF`,
well past the header, into real page data); opening/integrity-checking
the copy failed cleanly (SQLCipher HMAC failure surfacing as a
structured error from `PRAGMA integrity_check` itself, not a panic), and
the **original** database (never touched) remained fully readable
afterward:
```
test services::tests::req9_corruption_detected_safely ...
2026-09-17 09:50:28.131: ERROR CORE sqlcipher_page_cipher: hmac check failed for pgno=3
[req9] PASS - corruption detected via PRAGMA integrity_check: *** in database main ***
Tree 3 page 3: btreeInitPage() returns error code 11
Tree 1 page 1: unable to get the page. error code=1
[req9] PASS - original database untouched and fully readable after corrupting only the copy
ok
```

**10. Key management (OS credential store)** — **PASS** (genuinely
exercised, not simulated/blocked): a random key was generated, written
to Windows Credential Manager via `keyring` under a spike-specific
service/username, retrieved back (asserted equal to the original),
used to successfully open the database, then the credential was deleted
as cleanup:
```
test services::tests::req10_keyring_roundtrip ...
[req10] PASS - key generated, stored in OS credential store, retrieved after a simulated restart, used to open the DB, credential cleaned up
ok
```
This ran on the real local Windows session (not a sandboxed/CI
environment) - the credential-store roundtrip is real, not assumed.

### Full test run summary

```
running 9 tests
test services::tests::req10_keyring_roundtrip ... ok
test services::tests::req1_encrypted_database_creation_and_read ... ok
test services::tests::req2_persistence_across_reopen ... ok
test services::tests::req3_wrong_key_fails_cleanly ... ok
test services::tests::req4_encrypted_at_rest_no_plaintext ... ok
test services::tests::req5_migration_v1_to_v2 ... ok
test services::tests::req6_rekey ... ok
test services::tests::req7_8_backup_and_restore ... ok
test services::tests::req9_corruption_detected_safely ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.45s
```

(9 test functions cover all 10 numbered requirements - requirement 7 and
8 share one test function, as their setup is identical.)

### 11. Windows Tauri release build

Per the instructions, this is broken into its component concerns rather
than one pass/fail: Rust/Tauri release compilation and linking, vs.
installer packaging, vs. code signing.

**Rust/Tauri release compilation and linking — PASS.**
```
$ cd src-tauri && cargo build --release
   Compiling libsqlite3-sys v0.35.0
   ...
   Compiling rusqlite v0.37.0
   Compiling refinery v0.9.2
   Compiling lifeos v0.1.0 (D:\Dev\LiFeOS\src-tauri)
    Finished `release` profile [optimized] target(s) in 4m 50s
[exited with code 0]
```
Confirmed a real binary was produced (not just a "Finished" message):
```
$ ls -la src-tauri/target/release/lifeos.exe
-rwxr-xr-x 2 Andrew 197121 4016640 Sep 17 09:55 lifeos.exe
```
This directly proves the chosen encrypted-storage dependency combination
(`rusqlite` 0.37.0 + `bundled-sqlcipher` + the pinned external OpenSSL,
`refinery` 0.9.2, `keyring` 4.2.0) links successfully under the exact
release-mode toolchain (`opt-level = 3`, `lto = true`,
`codegen-units = 1`, `panic = "abort"`, `strip = true` -
`src-tauri/Cargo.toml`'s `[profile.release]`) that `tauri build` uses -
not just `cargo check` in dev mode, which was the specific gap flagged
in the architecture freeze.

**Installer packaging — PASS.** `npm.cmd run tauri build` (the exact
project-equivalent release command), run to completion with the correct
`cargo` on `PATH` (a first attempt without it produced a misleadingly
"successful" exit code from a shell-pipe artifact masking the real
`cargo: program not found` failure underneath - caught by re-running
without piping through `tail` and checking `$?` directly, not trusting
the first, wrong result):

```
$ npm.cmd run tauri build
...
   Compiling tauri v2.11.5
   Compiling tauri-macros v2.6.3
   Compiling lifeos v0.1.0 (D:\Dev\LiFeOS\src-tauri)
    Finished `release` profile [optimized] target(s) in 2m 23s
       Built application at: D:\Dev\LiFeOS\src-tauri\target\release\lifeos.exe
        Info Patching ...lifeos.exe with bundle type information: msi
        Info Verifying wix package
     Running light to produce D:\Dev\LiFeOS\src-tauri\target\release\bundle\msi\LifeOS_0.1.0_x64_en-US.msi
        Info Patching ...lifeos.exe with bundle type information: nsis
        Info Verifying NSIS package
     Running makensis to produce D:\Dev\LiFeOS\src-tauri\target\release\bundle\nsis\LifeOS_0.1.0_x64-setup.exe
    Finished 2 bundles at:
        D:\Dev\LiFeOS\src-tauri\target\release\bundle\msi\LifeOS_0.1.0_x64_en-US.msi
        D:\Dev\LiFeOS\src-tauri\target\release\bundle\nsis\LifeOS_0.1.0_x64-setup.exe

EXIT CODE: 0
```

Both artifacts confirmed present on disk, not just claimed by the log:
```
$ ls -la src-tauri/target/release/bundle/msi/ src-tauri/target/release/bundle/nsis/
-rw-r--r-- 1 Andrew 197121 2973696 Sep 17 09:58 LifeOS_0.1.0_x64_en-US.msi
-rwxr-xr-x 1 Andrew 197121 1371017 Sep 17 09:58 LifeOS_0.1.0_x64-setup.exe
```

**Code signing — not configured, not attempted, correctly out of
scope.** No signing certificate/identity is configured in
`tauri.conf.json`, so both installers are unsigned - this is expected
for local development and is explicitly not counted as a failure per the
instructions ("do not call the entire check failed merely because
signing is not configured"). Distinguishing the three concerns exactly
as asked: Rust/Tauri compilation and linking - PASS; installer packaging
- PASS (both MSI and NSIS produced); code signing - not configured
(out of scope this run, not a blocker).

One unrelated, pre-existing finding surfaced by this build (not a
storage-spike issue, noted for completeness): `tauri.conf.json`'s
`identifier` is `"com.lifeos.app"`, which Tauri warns ends in `.app` -
conflicts with the macOS application-bundle extension convention. Not
addressed here (out of scope for a storage spike; a one-line
`tauri.conf.json` fix whenever M1/M3 UI work next touches that file).

---

## Phase F — Decision gate

**sqlite3mc: could not be tested** (Phase B) - no mature native-Rust
integration exists to test. Not marked REJECTED on technical grounds
(it was never run against the checklist), but practically unusable for
LifeOS today without a substantial, unproven, out-of-scope engineering
effort (a hand-rolled FFI build) or depending on a 15-day-old,
78-download crate. Revisit only if the Rust ecosystem's sqlite3mc
support matures significantly later.

**SQLCipher (via `rusqlite` 0.37.0's `bundled-sqlcipher` feature): PASSES
every required technical test**, including the Windows Tauri release
build/packaging check that was the specific, named gap in the original
architecture freeze:

| # | Requirement | Result |
|---|---|---|
| 1 | Encrypted database creation | PASS |
| 2 | Persistence across restart | PASS |
| 3 | Wrong key fails cleanly | PASS |
| 4 | Encrypted at rest | PASS |
| 5 | Migration | PASS |
| 6 | Rekey | PASS |
| 7 | Backup (same-installation) | PASS |
| 8 | Restore | PASS |
| 9 | Corruption failure handling | PASS |
| 10 | OS key management (`keyring`) | PASS |
| 11 | Windows Tauri release build/package | PASS (compile+link), PASS (MSI+NSIS packaging), signing not configured (out of scope) |

**Decision: SQLCipher (via `rusqlite`'s native `bundled-sqlcipher`
feature) is marked PROVEN for LifeOS M2 on Windows**, with the exact
dependency versions in Phase B and the machine-specific OpenSSL setup
documented in `src-tauri/.cargo/config.toml`. This is a **candidate
substitution**, not a downgrade in rigor - SQLCipher was always the
documented fallback, and it passed the identical checklist sqlite3mc
would have needed to pass.

`docs/LIFEOS_SPEC.md` §5 has been updated to reflect this (SQLCipher via
`bundled-sqlcipher`, not sqlite3mc, is now the proven M2 storage choice;
the sqlite3mc research remains recorded in `RESEARCH_DECISIONS.md` §4 as
history, not deleted).

---

## Exact recommended next step

Begin the real M2 domain schema and action layer against the now-proven
storage foundation - starting with the smallest end-to-end vertical
slice (one real domain table, e.g. `Goal`, through `services` →
`storage`, with the same module-privacy boundary verified here) rather
than the full domain model at once. Before that:

1. Decide whether to keep `bundled-sqlcipher` (external OpenSSL,
   machine-specific `.cargo/config.toml` path) or invest in fixing this
   machine's Perl (or installing Strawberry Perl) to unlock
   `bundled-sqlcipher-vendored-openssl` instead, removing the
   machine-specific dependency for future contributors/machines. Not
   blocking - `bundled-sqlcipher` works and is proven - but worth a
   deliberate choice rather than leaving it as an artifact of this run's
   path of least resistance.
2. Carry the module-boundary pattern proven here (`services/storage`
   private submodule, `pub(super)` functions) forward unchanged into the
   real `domain`/`actions`/`knowledge` modules per `LIFEOS_SPEC.md` §5.6.
3. Design the real key-lifecycle code (`security/` module, per
   `LIFEOS_SPEC.md` §5.6): generate-on-first-run, store via `keyring`
   exactly as proven in requirement 10, with the real LifeOS
   service/username naming (not `lifeos-m2-storage-spike`).

**Not started, per the stop condition:** Goal/Project/Fact production
schemas, the real action layer, M3.

