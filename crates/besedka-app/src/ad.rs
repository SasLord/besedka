//! AD wiring — picks the real or the mock adapters once, at startup.
//!
//! Mock mode exists so the whole AD path (password bind **and** Kerberos SSO)
//! can be developed and tested on a machine with no domain controller:
//! `[ad] mock = true` in the TOML, or `BESEDKA_AD_MOCK=1` in the environment
//! (the env var mirrors trackly's `TRACKLY_AD_MOCK=1` convention and wins at
//! runtime).
//!
//! The `dyn` ports come from `besedka-core`; nothing here knows about
//! `ldap3`/`sspi`.

use std::fmt;
use std::sync::Arc;

use besedka_core::{AdClient, AdDirectory};
use besedka_infra::ad::{AdConfig, MockAdClient, MockAdDirectory, RealAdClient, RealAdDirectory};

/// AD adapters + their configuration, shared through `AppState`.
pub struct AdRuntime {
    pub config: AdConfig,
    /// `true` when the deterministic mock adapters are in use.
    pub mock: bool,
    /// Password-bind port (`POST /api/v1/auth_ad_login`).
    pub client: Arc<dyn AdClient>,
    /// Enrichment port (display name + role) used by every AD login.
    pub directory: Arc<dyn AdDirectory>,
}

impl AdRuntime {
    /// Build from config, honouring the `BESEDKA_AD_MOCK` env override.
    pub fn from_config(config: AdConfig) -> Self {
        let mock = mock_requested(&config);
        Self::build(config, mock)
    }

    /// Build with an explicit choice of adapters (tests use this to stay
    /// independent of the ambient environment).
    pub fn build(config: AdConfig, mock: bool) -> Self {
        let (client, directory): (Arc<dyn AdClient>, Arc<dyn AdDirectory>) = if mock {
            (
                Arc::new(MockAdClient::default_fixtures()),
                Arc::new(MockAdDirectory::default_fixtures()),
            )
        } else {
            (
                Arc::new(RealAdClient::new(config.clone())),
                Arc::new(RealAdDirectory::new(config.clone())),
            )
        };
        Self {
            config,
            mock,
            client,
            directory,
        }
    }

    /// Master switch. `false` ⇒ every AD endpoint answers 503.
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Can the passwordless (Kerberos) endpoint serve requests? Mock mode
    /// needs no keytab; real mode needs an SPN **and** a keytab path.
    pub fn sso_available(&self) -> bool {
        self.config.enabled && self.config.sso_ready()
    }

    /// Can users log in with an AD password (LDAP bind)?
    pub fn password_login_available(&self) -> bool {
        self.config.enabled && (self.mock || !self.config.host.trim().is_empty())
    }
}

/// `[ad] mock = true` or the `BESEDKA_AD_MOCK` env var (any non-empty value).
fn mock_requested(config: &AdConfig) -> bool {
    config.mock
        || std::env::var("BESEDKA_AD_MOCK")
            .map(|v| !v.trim().is_empty() && v.trim() != "0")
            .unwrap_or(false)
}

/// Redacts the service-account password (it can reach a log line).
impl fmt::Debug for AdRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdRuntime")
            .field("config", &self.config)
            .field("mock", &self.mock)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_build_uses_fixtures_and_needs_no_keytab() {
        let cfg = AdConfig {
            enabled: true,
            mock: true,
            ..AdConfig::default()
        };
        let ad = AdRuntime::build(cfg, true);
        assert!(ad.mock);
        assert!(ad.enabled());
        assert!(ad.sso_available(), "mock SSO needs no SPN/keytab");
        assert!(ad.password_login_available());
    }

    #[test]
    fn real_build_without_spn_or_keytab_is_not_sso_ready() {
        let cfg = AdConfig {
            enabled: true,
            ..AdConfig::default()
        };
        let ad = AdRuntime::build(cfg, false);
        assert!(!ad.mock);
        assert!(!ad.sso_available());
        assert!(
            !ad.password_login_available(),
            "no DC host ⇒ password AD login unavailable"
        );
    }

    #[test]
    fn disabled_config_disables_everything() {
        let ad = AdRuntime::build(AdConfig::default(), true);
        assert!(!ad.enabled());
        assert!(!ad.sso_available());
        assert!(!ad.password_login_available());
    }

    #[test]
    fn debug_redacts_service_password() {
        let cfg = AdConfig {
            enabled: true,
            bind_password: "svc-s3cret".to_string(),
            ..AdConfig::default()
        };
        let ad = AdRuntime::build(cfg, false);
        assert!(!format!("{ad:?}").contains("svc-s3cret"));
    }
}
