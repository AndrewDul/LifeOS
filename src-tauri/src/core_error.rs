//! Typed Core-boundary error model (M2 Goal hardening, 2026-09-17).
//!
//! Replaces `ActionOutcome::StorageError(String)` (Goal vertical slice
//! report §14), which put whatever the underlying `rusqlite`/`refinery`
//! error's `Display` happened to produce directly into the authoritative
//! Core API. A caller that only needs to know "storage vs. migration vs.
//! security vs. something internal" should never have to pattern-match
//! or string-scan a database driver's error text.
//!
//! `kind` is the public, typed, closed-set classification every caller
//! programs against. `diagnostic` is a free-text internal detail kept
//! for logging/debugging - deliberately never part of `Display` (so
//! anything that formats a `CoreError` for a user-facing surface gets
//! only the generic, safe `kind` description), and never sourced from
//! anything but the underlying driver error's own message, which does
//! not include key material, raw SQL parameter values, or user-private
//! payload content (`rusqlite`/`refinery` error text names constraints/
//! types, not bound values).

// Not wired into `run()` yet this run - only exercised by
// services::tests, hence dead_code would otherwise warn on every
// non-test build (not every `CoreErrorKind` variant is produced by the
// Goal slice's two actions today).
#![allow(dead_code)]

/// A small, closed set of Core-facing failure categories. Deliberately
/// not an "enterprise error framework" - just enough to stop leaking
/// arbitrary database strings as the authoritative error model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoreErrorKind {
    /// A storage-layer failure (SQLite/SQLCipher, including a failed
    /// transaction commit/rollback).
    Storage,
    /// A schema-migration failure.
    Migration,
    /// A key-management/OS-credential-store failure.
    Security,
    /// Anything else internal to Core that does not fit the above.
    Internal,
}

#[derive(Debug)]
pub(crate) struct CoreError {
    kind: CoreErrorKind,
    diagnostic: String,
}

impl CoreError {
    pub(crate) fn new(kind: CoreErrorKind, diagnostic: impl Into<String>) -> Self {
        CoreError {
            kind,
            diagnostic: diagnostic.into(),
        }
    }

    pub(crate) fn kind(&self) -> CoreErrorKind {
        self.kind
    }

    /// Internal-only diagnostic detail (e.g. for developer logs) - never
    /// part of `Display`, never handed to a UI/CloudLLM/NeXa caller as
    /// the authoritative error.
    pub(crate) fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "core error ({:?})", self.kind)
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(e: rusqlite::Error) -> Self {
        CoreError::new(CoreErrorKind::Storage, e.to_string())
    }
}

impl From<refinery::Error> for CoreError {
    fn from(e: refinery::Error) -> Self {
        CoreError::new(CoreErrorKind::Migration, e.to_string())
    }
}

impl From<crate::security::KeyError> for CoreError {
    fn from(e: crate::security::KeyError) -> Self {
        CoreError::new(CoreErrorKind::Security, e.to_string())
    }
}
