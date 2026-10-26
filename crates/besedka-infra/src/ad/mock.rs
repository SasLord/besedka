//! Mock AD client — deterministic fixtures for dev machines without a domain
//! controller (ported from trackly `ad/mock.rs`, D-Mock-01 / USR-12).
//!
//! Used when `[ad] mock = true` or `BESEDKA_AD_MOCK=1` is set. Returns preset
//! bind outcomes keyed by AD login (sAMAccountName-style), so the WHOLE login
//! path — including the HTTP session cookie — can be developed and tested on
//! macOS with no DC.
//!
//! Fixtures (placeholders, no real people):
//!   `us100` / `Passw0rd!` — Иванов Иван Иванович
//!   `us200` / `Secret123` — Петрова Анна Сергеевна
//!
//! The fixture passwords exist ONLY in this file and in the docs — they must
//! never be reused for anything real.

use std::collections::HashMap;

use async_trait::async_trait;
use besedka_core::{AdClient, AuthOutcome};

/// The login the mock SSO endpoint assumes when no explicit one is supplied.
pub const DEFAULT_SSO_LOGIN: &str = "us100";

/// Fixture for a single "domain user" in the mock.
#[derive(Debug, Clone)]
pub struct AdFixture {
    pub password: &'static str,
    pub display_name: &'static str,
}

/// Deterministic mock AD client for development.
#[derive(Debug, Clone)]
pub struct MockAdClient {
    pub users: HashMap<String, AdFixture>,
    /// When `true`, every call reports the AD server as down — simulates the
    /// "AD недоступен" path (503 instead of 401).
    pub unreachable: bool,
}

impl MockAdClient {
    /// Two pre-configured domain users covering success + wrong-password
    /// scenarios. Not-found / wrong-password are exercised against these
    /// fixtures by tests, not by separate fixture entries.
    pub fn default_fixtures() -> Self {
        let mut users = HashMap::new();
        users.insert(
            "us100".to_string(),
            AdFixture {
                password: "Passw0rd!",
                display_name: "Иванов Иван Иванович",
            },
        );
        users.insert(
            "us200".to_string(),
            AdFixture {
                password: "Secret123",
                display_name: "Петрова Анна Сергеевна",
            },
        );
        Self {
            users,
            unreachable: false,
        }
    }

    /// A mock that always reports the DC as unreachable.
    pub fn unreachable() -> Self {
        Self {
            users: HashMap::new(),
            unreachable: true,
        }
    }

    /// Strip `@domain` (UPN) or `DOMAIN\` (NetBIOS) decoration to get the bare
    /// lookup key — the inverse of the normalization applied on the way in.
    pub fn lookup_key(login: &str) -> &str {
        let without_upn_suffix = login.split('@').next().unwrap_or(login);
        without_upn_suffix
            .rsplit('\\')
            .next()
            .unwrap_or(without_upn_suffix)
    }
}

#[async_trait]
impl AdClient for MockAdClient {
    async fn authenticate(&self, login: &str, password: &str) -> AuthOutcome {
        // CRITICAL (Pitfall 1): reject empty/whitespace password BEFORE any
        // lookup — the anonymous-bind trap stays closed even in the mock.
        if password.trim().is_empty() {
            return AuthOutcome::BadCreds;
        }

        if self.unreachable {
            return AuthOutcome::Unreachable;
        }

        let key = Self::lookup_key(login);
        match self.users.get(key) {
            Some(fixture) if fixture.password == password => AuthOutcome::Ok {
                display_name: fixture.display_name.to_string(),
            },
            // Wrong password AND unknown user collapse into the same outcome —
            // no account enumeration.
            _ => AuthOutcome::BadCreds,
        }
    }

    async fn test_connection(&self) -> AuthOutcome {
        if self.unreachable {
            return AuthOutcome::Unreachable;
        }
        AuthOutcome::Ok {
            display_name: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn success_returns_display_name() {
        let mock = MockAdClient::default_fixtures();
        assert_eq!(
            mock.authenticate("us100", "Passw0rd!").await,
            AuthOutcome::Ok {
                display_name: "Иванов Иван Иванович".to_string()
            }
        );
    }

    #[tokio::test]
    async fn second_fixture_works() {
        let mock = MockAdClient::default_fixtures();
        match mock.authenticate("us200", "Secret123").await {
            AuthOutcome::Ok { display_name } => {
                assert_eq!(display_name, "Петрова Анна Сергеевна");
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn wrong_password_is_bad_creds() {
        let mock = MockAdClient::default_fixtures();
        assert_eq!(
            mock.authenticate("us100", "WrongPassword!").await,
            AuthOutcome::BadCreds
        );
    }

    #[tokio::test]
    async fn unknown_user_is_indistinguishable_from_wrong_password() {
        let mock = MockAdClient::default_fixtures();
        assert_eq!(
            mock.authenticate("us999", "Whatever123").await,
            AuthOutcome::BadCreds
        );
    }

    #[tokio::test]
    async fn unreachable_scenario() {
        let mock = MockAdClient::unreachable();
        assert_eq!(
            mock.authenticate("us100", "Passw0rd!").await,
            AuthOutcome::Unreachable
        );
    }

    #[tokio::test]
    async fn empty_and_whitespace_passwords_are_rejected_without_lookup() {
        let mock = MockAdClient::default_fixtures();
        assert_eq!(mock.authenticate("us100", "").await, AuthOutcome::BadCreds);
        assert_eq!(
            mock.authenticate("us100", "   ").await,
            AuthOutcome::BadCreds,
            "whitespace-only password must be rejected (anonymous-bind trap)"
        );
    }

    #[tokio::test]
    async fn upn_and_netbios_forms_resolve_to_same_fixture() {
        let mock = MockAdClient::default_fixtures();
        let expected = AuthOutcome::Ok {
            display_name: "Иванов Иван Иванович".to_string(),
        };
        assert_eq!(
            mock.authenticate("us100@corp.local", "Passw0rd!").await,
            expected
        );
        assert_eq!(
            mock.authenticate(r"CORP\us100", "Passw0rd!").await,
            expected
        );
    }

    #[tokio::test]
    async fn test_connection_reachable_by_default() {
        let mock = MockAdClient::default_fixtures();
        assert_eq!(
            mock.test_connection().await,
            AuthOutcome::Ok {
                display_name: String::new()
            }
        );
    }

    #[tokio::test]
    async fn test_connection_unreachable_fixture() {
        let mock = MockAdClient::unreachable();
        assert_eq!(mock.test_connection().await, AuthOutcome::Unreachable);
    }
}
