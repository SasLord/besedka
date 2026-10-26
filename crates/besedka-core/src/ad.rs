//! AD ports — abstraction for Active Directory authentication and directory
//! enrichment (ported from trackly `crates/trackly-core/src/ports/{ad,ad_directory}.rs`).
//!
//! Both traits live here and carry **no I/O dependencies** (no ldap3, no sspi,
//! no tokio): `besedka-core` stays a pure-domain crate. The adapters live in
//! `besedka_infra::ad`:
//! - `RealAdClient` / `RealAdDirectory` — production (LDAP bind + LDAPS search).
//! - `MockAdClient` / `MockAdDirectory` — deterministic fixtures, so the whole
//!   login path (password-AD **and** Kerberos SSO) can be exercised on a dev
//!   machine with no domain controller (`BESEDKA_AD_MOCK=1`).
//!
//! Two distinct concerns, deliberately kept apart:
//! - [`AdClient`] — *credential check*: bind as the end user with their
//!   password. Used by `POST /api/v1/auth_ad_login`.
//! - [`AdDirectory`] — *enrichment*: given an already-authenticated login
//!   (proved by Kerberos), resolve display name and role. Used by
//!   `GET /api/v1/auth_ad_sso`.
//!
//! CRITICAL (inherited from trackly, Pitfall 1 / RFC 4513 §5.1.2): every
//! `AdClient` implementation MUST reject an empty or whitespace-only password
//! as [`AuthOutcome::BadCreds`] **without performing a bind**. A non-empty DN
//! plus empty password is an *unauthenticated* bind that many LDAP servers
//! accept (rc=0), which would otherwise be misread as a successful login.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::Role;

/// Outcome of an AD credential check (password bind).
///
/// Modelled as data, not an error: "wrong password" and "DC unreachable" are
/// normal outcomes the caller branches on (404/401 vs 503). Genuine
/// infrastructure faults are indistinguishable from `Unreachable` on purpose —
/// we never want a stack trace to reach the login form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthOutcome {
    /// Bind succeeded; `display_name` resolved via `displayName` → `cn` → login.
    Ok { display_name: String },
    /// Wrong password OR unknown user. Deliberately generic — both cases return
    /// the same variant so the API cannot be used to enumerate accounts.
    BadCreds,
    /// The domain controller could not be reached (network/TLS/timeout).
    Unreachable,
}

/// AD credential-check port — implemented by `RealAdClient` and `MockAdClient`.
///
/// Implementations MUST reject an empty/whitespace-only password as
/// `AuthOutcome::BadCreds` before any bind (see the module note).
#[async_trait]
pub trait AdClient: Send + Sync {
    /// Bind as `login` (bare `us100`, `user@domain.tld` or `DOMAIN\user`) with
    /// `password`. `password` is never logged, never stored.
    async fn authenticate(&self, login: &str, password: &str) -> AuthOutcome;

    /// Reachability probe with NO end-user credentials — used by the admin
    /// "Проверить подключение" action. Returns `Ok { display_name: "" }`
    /// when the DC answers, `Unreachable` otherwise. Never `BadCreds`.
    async fn test_connection(&self) -> AuthOutcome;
}

/// Result of a directory enrichment lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryResult {
    /// Resolved ФИО (`displayName` → `cn` → the login itself).
    pub display_name: String,
    /// Role mapped from AD group membership; `None` = no configured group
    /// matched (the normal "role mapping not configured" outcome).
    pub role: Option<Role>,
}

/// Why a directory lookup produced no trustworthy data.
///
/// Typed (not a bare `Option`/`bool`) so callers can fail *closed*: an outage
/// must never look like "no groups matched" and silently demote or promote
/// anyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryError {
    /// No service-account bind configured (`bind_dn`/`base_dn` empty) — the
    /// optional enrichment feature silently degrades.
    #[error("AD directory enrichment is not configured")]
    NotConfigured,
    /// The service account's own bind was rejected — a config error, distinct
    /// from a network outage.
    #[error("AD service-account bind failed")]
    ServiceBindFailed,
    /// DC unreachable, TLS failure, or a group check that could not complete.
    #[error("AD directory is unreachable")]
    Unreachable,
}

/// AD enrichment port — resolves display name and role for an already
/// Kerberos-authenticated login.
#[async_trait]
pub trait AdDirectory: Send + Sync {
    /// Resolve `login` (sAMAccountName, UPN or NetBIOS form) to a display name
    /// and, optionally, a mapped role. Cache hits short-circuit without LDAP.
    async fn resolve(&self, login: &str) -> Result<DirectoryResult, DirectoryError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_outcome_serializes_generically() {
        // The wire form must not distinguish wrong-password from unknown-user.
        assert_eq!(
            serde_json::to_string(&AuthOutcome::BadCreds).unwrap(),
            "\"bad_creds\""
        );
        assert_eq!(
            serde_json::to_string(&AuthOutcome::Unreachable).unwrap(),
            "\"unreachable\""
        );
    }

    #[test]
    fn directory_error_is_copy_and_typed() {
        // Typed errors (never a collapsed bool) so callers can fail closed.
        let e = DirectoryError::Unreachable;
        let copied = e;
        assert_eq!(e, copied);
        assert_ne!(DirectoryError::NotConfigured, DirectoryError::Unreachable);
    }
}
