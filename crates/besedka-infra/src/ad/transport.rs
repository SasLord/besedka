//! Shared LDAP connection URL/settings construction (ported from trackly
//! `ad/transport.rs`).
//!
//! `RealAdClient::authenticate`, `RealAdClient::test_connection` and
//! `RealAdDirectory::resolve` all need the same `(url, LdapConnSettings)` pair
//! from an [`AdConfig`]. This module is the SINGLE place that builds it — no
//! other call site should construct an `ldaps://`/`ldap://` literal directly.

use std::time::Duration;

use ldap3::LdapConnSettings;

use super::config::{AdConfig, LdapTlsMode};

/// Connection timeout for the initial LDAP(S) connect — always bounded so a
/// dead DC fails fast as `Unreachable` instead of hanging a request.
pub const CONN_TIMEOUT: Duration = Duration::from_secs(5);

/// Build the LDAP URL and settings for `cfg`, per `cfg.ldap_tls_mode`:
/// - `Ldaps` → `ldaps://host:port`, no StartTLS.
/// - `Plain` → `ldap://host:port`, no StartTLS.
/// - `StartTls` → `ldap://host:port` with `.set_starttls(true)`.
///
/// `port` resolves via [`AdConfig::resolved_port`] — an explicit port always
/// wins over the mode default.
pub fn build_ldap_conn(cfg: &AdConfig) -> (String, LdapConnSettings) {
    let port = cfg.resolved_port();
    let mut settings = LdapConnSettings::new()
        .set_conn_timeout(CONN_TIMEOUT)
        .set_no_tls_verify(cfg.no_tls_verify);

    let url = match cfg.ldap_tls_mode {
        LdapTlsMode::Ldaps => format!("ldaps://{}:{port}", cfg.host),
        LdapTlsMode::Plain => format!("ldap://{}:{port}", cfg.host),
        LdapTlsMode::StartTls => {
            settings = settings.set_starttls(true);
            format!("ldap://{}:{port}", cfg.host)
        }
    };

    (url, settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_cfg() -> AdConfig {
        AdConfig {
            host: "dc1.example.local".to_string(),
            ..AdConfig::default()
        }
    }

    #[test]
    fn ldaps_mode_builds_ldaps_url_no_starttls() {
        let cfg = AdConfig {
            ldap_tls_mode: LdapTlsMode::Ldaps,
            ..base_cfg()
        };
        let (url, settings) = build_ldap_conn(&cfg);
        assert_eq!(url, "ldaps://dc1.example.local:636");
        assert!(!settings.starttls());
    }

    #[test]
    fn plain_mode_builds_plain_url_no_starttls() {
        let cfg = AdConfig {
            ldap_tls_mode: LdapTlsMode::Plain,
            ..base_cfg()
        };
        let (url, settings) = build_ldap_conn(&cfg);
        assert_eq!(url, "ldap://dc1.example.local:389");
        assert!(!settings.starttls());
    }

    #[test]
    fn starttls_mode_builds_plain_url_with_starttls() {
        let cfg = AdConfig {
            ldap_tls_mode: LdapTlsMode::StartTls,
            ..base_cfg()
        };
        let (url, settings) = build_ldap_conn(&cfg);
        assert_eq!(url, "ldap://dc1.example.local:389");
        assert!(settings.starttls());
    }

    #[test]
    fn explicit_port_overrides_mode_default_on_any_mode() {
        let cfg = AdConfig {
            ldap_tls_mode: LdapTlsMode::Plain,
            port: Some(2389),
            ..base_cfg()
        };
        let (url, _settings) = build_ldap_conn(&cfg);
        assert_eq!(url, "ldap://dc1.example.local:2389");
    }
}
