//! Real AD client — `ldap3` simple bind over LDAPS (ported from trackly
//! `ad/real.rs`, D-AD-01).
//!
//! This module and [`super::directory`] are the only two places that import
//! `ldap3`; the `besedka_core::AdClient` port stays ldap3-free.
//!
//! Every failure mode is mapped into [`AuthOutcome`] (never an `Err`): DC down,
//! TLS failure and wrong credentials are all *normal authentication outcomes*,
//! so a stack trace can never reach the login form.

use async_trait::async_trait;
use besedka_core::{AdClient, AuthOutcome};
use ldap3::{ldap_escape, LdapConnAsync, Scope, SearchEntry};

use super::config::AdConfig;
use super::transport::build_ldap_conn;

/// Production AD client — binds via `ldap3` over LDAP(S) and resolves the bound
/// user's display name (`displayName` → `cn` → login).
#[derive(Debug, Clone)]
pub struct RealAdClient {
    cfg: AdConfig,
}

impl RealAdClient {
    pub fn new(cfg: AdConfig) -> Self {
        Self { cfg }
    }

    /// Normalize the bind name: AD `simple_bind` accepts `user@domain` (UPN) or
    /// `DOMAIN\user`, but often rejects a bare short login. Pass through if the
    /// login already carries `@`/`\`; otherwise append `@<domain>`.
    fn normalize_bind_name(&self, login: &str) -> String {
        if login.contains('@') || login.contains('\\') {
            login.to_string()
        } else {
            format!("{login}@{}", self.cfg.domain)
        }
    }
}

/// Build the user-search filter, escaping the login first (T-ldap-inj): filter
/// metacharacters (`(`, `)`, `*`, `\`, NUL) in a user-supplied login MUST be
/// escaped before interpolation, so a crafted login cannot inject clauses.
/// A pure fn, so the exact production path is unit-testable without a live
/// LDAPS connection.
pub fn build_user_search_filter(login: &str) -> String {
    let safe_login = ldap_escape(login);
    format!("(|(sAMAccountName={safe_login})(userPrincipalName={safe_login}))")
}

#[async_trait]
impl AdClient for RealAdClient {
    async fn authenticate(&self, login: &str, password: &str) -> AuthOutcome {
        // CRITICAL (Pitfall 1 / RFC 4513 §5.1.2): reject an empty/whitespace
        // password BEFORE the bind — otherwise "non-empty DN + empty password"
        // is an unauthenticated bind many servers answer with rc=0.
        if password.trim().is_empty() {
            return AuthOutcome::BadCreds;
        }
        if self.cfg.host.trim().is_empty() {
            // No DC configured — indistinguishable from an outage by design.
            return AuthOutcome::Unreachable;
        }

        let (url, settings) = build_ldap_conn(&self.cfg);

        let (conn, mut ldap) = match LdapConnAsync::with_settings(settings, &url).await {
            Ok(v) => v,
            // Connect/TLS handshake failure → DC unreachable, not an error.
            Err(_) => return AuthOutcome::Unreachable,
        };
        // The connection driver task MUST be driven, or operations hang.
        ldap3::drive!(conn);

        let bind_name = self.normalize_bind_name(login);

        let bind_result = match ldap.simple_bind(&bind_name, password).await {
            Ok(res) => res,
            Err(_) => return AuthOutcome::Unreachable, // protocol/IO error mid-bind
        };

        if bind_result.success().is_err() {
            // rc != 0 (commonly 49 invalidCredentials) → generic BadCreds.
            // Account-state sub-codes are never leaked.
            let _ = ldap.unbind().await;
            return AuthOutcome::BadCreds;
        }

        // Bound OK — search the user's own entry for the display name.
        let filter = build_user_search_filter(login);
        let base_dn = self.cfg.resolved_base_dn();
        let attrs = vec![self.cfg.name_attr.as_str(), "cn"];

        let display_name = match ldap
            .search(&base_dn, Scope::Subtree, &filter, attrs)
            .await
            .and_then(|search_result| search_result.success())
        {
            Ok((entries, _res)) => entries
                .into_iter()
                .next()
                .map(SearchEntry::construct)
                .and_then(|entry| {
                    entry
                        .attrs
                        .get(&self.cfg.name_attr)
                        .and_then(|values| values.first().cloned())
                        .or_else(|| {
                            entry
                                .attrs
                                .get("cn")
                                .and_then(|values| values.first().cloned())
                        })
                })
                .unwrap_or_else(|| login.to_string()),
            // A search failure after a successful bind is non-fatal — fall
            // back to the login (D-Config-02 chain).
            Err(_) => login.to_string(),
        };

        let _ = ldap.unbind().await;
        AuthOutcome::Ok { display_name }
    }

    /// Reachability probe — connects over LDAP(S) and issues an *anonymous*
    /// bind (empty DN + empty password, explicitly allowed by RFC 4513 §5.1.2)
    /// to confirm the server actually speaks LDAP. No user credentials.
    async fn test_connection(&self) -> AuthOutcome {
        if self.cfg.host.trim().is_empty() {
            return AuthOutcome::Unreachable;
        }

        let (url, settings) = build_ldap_conn(&self.cfg);

        let (conn, mut ldap) = match LdapConnAsync::with_settings(settings, &url).await {
            Ok(v) => v,
            Err(_) => return AuthOutcome::Unreachable,
        };
        ldap3::drive!(conn);

        let bind_result = match ldap.simple_bind("", "").await {
            Ok(res) => res,
            Err(_) => return AuthOutcome::Unreachable,
        };

        let _ = ldap.unbind().await;

        // A server that refuses anonymous binds is still reachable.
        let _ = bind_result.success();
        AuthOutcome::Ok {
            display_name: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- LDAP filter-injection guard (T-ldap-inj) ---

    #[test]
    fn benign_login_builds_expected_filter() {
        assert_eq!(
            build_user_search_filter("us100"),
            "(|(sAMAccountName=us100)(userPrincipalName=us100))"
        );
    }

    #[test]
    fn injection_payload_metacharacters_are_escaped() {
        // Without escaping, the raw `)(` / `*` would close our clause and
        // inject an attacker-controlled filter. ldap_escape encodes
        // `(`→\28, `)`→\29, `*`→\2a (RFC 4515).
        let payload = "*)(uid=*))(|(uid=*";
        let filter = build_user_search_filter(payload);

        assert!(!filter.contains('*'), "no raw `*` may survive: {filter}");
        assert!(
            !filter.contains(payload),
            "raw payload must not appear verbatim: {filter}"
        );
        assert!(
            filter.contains("\\2a"),
            "`*` must be escaped to \\2a: {filter}"
        );
        assert!(
            filter.contains("\\28"),
            "`(` must be escaped to \\28: {filter}"
        );
        assert!(
            filter.contains("\\29"),
            "`)` must be escaped to \\29: {filter}"
        );
        assert!(filter.starts_with("(|(sAMAccountName="));
        assert_eq!(
            filter.matches("(|").count(),
            1,
            "no injected OR clause: {filter}"
        );
    }

    #[test]
    fn backslash_in_login_is_escaped() {
        let filter = build_user_search_filter("dom\\ain");
        assert!(
            !filter.contains("dom\\ain"),
            "raw backslash must not survive: {filter}"
        );
        assert!(
            filter.contains("\\5c"),
            "`\\` must be escaped to \\5c: {filter}"
        );
    }

    // --- Bind-name normalization ---

    #[test]
    fn bare_login_gets_domain_suffix() {
        let client = RealAdClient::new(AdConfig {
            domain: "example.local".to_string(),
            ..AdConfig::default()
        });
        assert_eq!(client.normalize_bind_name("us100"), "us100@example.local");
    }

    #[test]
    fn upn_and_netbios_forms_pass_through_unchanged() {
        let client = RealAdClient::new(AdConfig {
            domain: "example.local".to_string(),
            ..AdConfig::default()
        });
        assert_eq!(
            client.normalize_bind_name("us100@other.local"),
            "us100@other.local"
        );
        assert_eq!(
            client.normalize_bind_name(r"EXAMPLE\us100"),
            r"EXAMPLE\us100"
        );
    }

    // --- Fail-safe defaults (no live DC needed) ---

    #[tokio::test]
    async fn empty_password_never_reaches_the_network() {
        let client = RealAdClient::new(AdConfig {
            host: "dc1.example.local".to_string(),
            ..AdConfig::default()
        });
        assert_eq!(
            client.authenticate("us100", "").await,
            AuthOutcome::BadCreds
        );
        assert_eq!(
            client.authenticate("us100", "  ").await,
            AuthOutcome::BadCreds
        );
    }

    #[tokio::test]
    async fn unconfigured_host_is_unreachable_not_a_panic() {
        let client = RealAdClient::new(AdConfig::default());
        assert_eq!(
            client.authenticate("us100", "Passw0rd!").await,
            AuthOutcome::Unreachable
        );
        assert_eq!(client.test_connection().await, AuthOutcome::Unreachable);
    }

    #[tokio::test]
    async fn dead_dc_reports_unreachable() {
        // Port 1 on loopback: nothing listens → fast transport error, mapped to
        // `Unreachable` (never an `Err`/panic).
        let client = RealAdClient::new(AdConfig {
            host: "127.0.0.1".to_string(),
            port: Some(1),
            ldap_tls_mode: super::super::config::LdapTlsMode::Plain,
            ..AdConfig::default()
        });
        assert_eq!(
            client.authenticate("us100", "Passw0rd!").await,
            AuthOutcome::Unreachable
        );
    }
}
