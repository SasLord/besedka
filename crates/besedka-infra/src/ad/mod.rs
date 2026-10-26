//! AD / Kerberos adapters for Besedka — ported from trackly's `ad/` module
//! (READ-ONLY reference), then adapted to Besedka's naming, roles and
//! configuration.
//!
//! Two implementations of each `besedka_core` port:
//! - `RealAdClient` / `RealAdDirectory` (`real.rs`, `directory.rs`) — production
//!   LDAP(S) bind and search.
//! - `MockAdClient` / `MockAdDirectory` (`mock.rs`, `directory_mock.rs`) —
//!   deterministic fixtures, so the whole login path runs on a dev machine with
//!   no domain controller (`BESEDKA_AD_MOCK=1`).
//!
//! Kerberos (passwordless) login is `keytab.rs` (parse the service key) +
//! `sso.rs` (accept a SPNEGO token). Both are offline: no KDC is contacted.
//!
//! [`normalize_login`] is the single place that turns any AD spelling
//! (`us100`, `us100@corp.local`, `CORP\us100`) into Besedka's canonical
//! lowercase username.

pub mod cache;
pub mod config;
pub mod directory;
pub mod directory_mock;
pub mod discovery;
pub mod keytab;
pub mod mock;
pub mod real;
pub mod sso;
pub mod transport;

pub use cache::TtlCache;
pub use config::{AdConfig, LdapTlsMode, RoleMappingEntry};
pub use directory::RealAdDirectory;
pub use directory_mock::MockAdDirectory;
pub use discovery::{derive_base_dn, discover_dc, DiscoveryResult};
pub use keytab::{aes256_key_for_spn, parse_keytab, KeytabError};
pub use mock::MockAdClient;
pub use real::RealAdClient;
pub use sso::{accept_spnego, SsoError, SsoOutcome};

/// Canonical Besedka username for an AD login: strip `@domain`/`DOMAIN\`,
/// lowercase, and replace every character outside `[a-z0-9._-]` with `-`.
///
/// The same normalization is used for the DB column (`users.username` is
/// `COLLATE NOCASE`) and for the AD lookups, so one person always maps to one
/// account no matter how the DC spelled the principal.
pub fn normalize_login(login: &str) -> String {
    let bare = mock::MockAdClient::lookup_key(login.trim()).to_lowercase();
    bare.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_login_collapses_every_ad_spelling() {
        assert_eq!(normalize_login("us100"), "us100");
        assert_eq!(normalize_login("US100"), "us100");
        assert_eq!(normalize_login("us100@CORP.local"), "us100");
        assert_eq!(normalize_login(r"CORP\US100"), "us100");
        assert_eq!(normalize_login("  us100  "), "us100");
    }

    #[test]
    fn normalize_login_sanitizes_hostile_characters() {
        // No LDAP/SQL metacharacters survive into the username column.
        assert_eq!(normalize_login("us*)(100"), "us---100");
        assert_eq!(normalize_login("ivan.petrov"), "ivan.petrov");
        assert_eq!(normalize_login("ivan_p"), "ivan_p");
    }
}
