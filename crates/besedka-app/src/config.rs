//! Server configuration: TOML file + env overrides.
//!
//! No secrets here (see `.gitignore`): auth secrets live in the environment.

use besedka_infra::ad::AdConfig;
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use thiserror::Error;

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8080
}

fn default_db_path() -> String {
    "data/besedka.db".to_string()
}

fn default_cookie_name() -> String {
    "besedka_session".to_string()
}

/// 7 days.
fn default_session_ttl_secs() -> u64 {
    7 * 24 * 3600
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

impl ServerConfig {
    /// Address to bind. Wave 0: IPv4 only (a hostname needs name resolution).
    pub fn socket_addr(&self) -> SocketAddr {
        let ip: IpAddr = self.host.parse().unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        SocketAddr::new(ip, self.port)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    /// Path to the SQLite file (created if missing).
    #[serde(default = "default_db_path")]
    pub path: String,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: default_db_path(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionsConfig {
    #[serde(default = "default_cookie_name")]
    pub cookie_name: String,
    #[serde(default = "default_session_ttl_secs")]
    pub session_ttl_secs: u64,
}

impl Default for SessionsConfig {
    fn default() -> Self {
        Self {
            cookie_name: default_cookie_name(),
            session_ttl_secs: default_session_ttl_secs(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub sessions: SessionsConfig,
    /// AD/Kerberos SSO (off unless `[ad] enabled = true`).
    #[serde(default)]
    pub ad: AdConfig,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io error reading config: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(String),
    #[error("bad environment value: {0}")]
    Env(String),
}

impl Config {
    /// Load config from a TOML file, then apply env overrides:
    /// `BESEDKA_HOST`, `BESEDKA_PORT`, `BESEDKA_DB_PATH`, plus the AD ones
    /// (`BESEDKA_AD_*`). Credentials only ever come from the environment —
    /// `BESEDKA_AD_BIND_PASSWORD` is never written to a TOML.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let raw = std::fs::read_to_string(path.as_ref())?;
        let mut cfg: Config =
            toml::from_str(&raw).map_err(|e| ConfigError::Parse(e.to_string()))?;

        if let Ok(host) = std::env::var("BESEDKA_HOST") {
            cfg.server.host = host;
        }
        if let Ok(port) = std::env::var("BESEDKA_PORT") {
            cfg.server.port = port
                .parse()
                .map_err(|_| ConfigError::Env(format!("BESEDKA_PORT={port}")))?;
        }
        if let Ok(db_path) = std::env::var("BESEDKA_DB_PATH") {
            cfg.database.path = db_path;
        }
        apply_ad_env(&mut cfg.ad);
        Ok(cfg)
    }
}

/// Overlay `BESEDKA_AD_*` env vars onto the `[ad]` section.
///
/// Empty values are ignored, so a blank CI variable cannot silently disable a
/// working config.
fn apply_ad_env(ad: &mut AdConfig) {
    fn set(target: &mut String, var: &str) {
        if let Ok(value) = std::env::var(var) {
            if !value.trim().is_empty() {
                *target = value;
            }
        }
    }
    set(&mut ad.domain, "BESEDKA_AD_DOMAIN");
    set(&mut ad.spn, "BESEDKA_AD_SPN");
    set(&mut ad.keytab_path, "BESEDKA_AD_KEYTAB");
    set(&mut ad.host, "BESEDKA_AD_HOST");
    set(&mut ad.base_dn, "BESEDKA_AD_BASE_DN");
    set(&mut ad.bind_dn, "BESEDKA_AD_BIND_DN");
    // The one secret: environment only.
    set(&mut ad.bind_password, "BESEDKA_AD_BIND_PASSWORD");
    if let Ok(enabled) = std::env::var("BESEDKA_AD_ENABLED") {
        ad.enabled = truthy(&enabled);
    }
    if let Ok(mock) = std::env::var("BESEDKA_AD_MOCK") {
        ad.mock = truthy(&mock);
    }
}

/// `1`/`true`/`yes`/`on` (case-insensitive) ⇒ true; anything else ⇒ false.
fn truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_sane() {
        let cfg = Config::default();
        assert_eq!(cfg.server.socket_addr(), "127.0.0.1:8080".parse().unwrap());
        assert_eq!(cfg.database.path, "data/besedka.db");
        assert_eq!(cfg.sessions.cookie_name, "besedka_session");
        assert_eq!(cfg.sessions.session_ttl_secs, 7 * 24 * 3600);
    }

    #[test]
    fn parses_full_config() {
        let toml = r#"
            [server]
            host = "0.0.0.0"
            port = 9090

            [database]
            path = "/tmp/x.db"

            [sessions]
            cookie_name = "s"
            session_ttl_secs = 60
        "#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.server.socket_addr(), "0.0.0.0:9090".parse().unwrap());
        assert_eq!(cfg.sessions.cookie_name, "s");
    }

    #[test]
    fn empty_file_gives_defaults() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.server.port, 8080);
    }

    #[test]
    fn ad_section_is_optional_and_off_by_default() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(!cfg.ad.enabled, "AD must default to off");
        assert!(!cfg.ad.mock);
    }

    #[test]
    fn ad_section_parses_with_mock_mode() {
        let toml = r#"
            [ad]
            enabled = true
            mock = true
            domain = "example.local"
            spn = "HTTP/chat.example.local"
            [[ad.role_mapping]]
            group_dn = "CN=Besedka-Admins,OU=Groups,DC=example,DC=local"
            role = "admin"
        "#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(cfg.ad.enabled && cfg.ad.mock);
        assert_eq!(cfg.ad.domain, "example.local");
        assert_eq!(cfg.ad.role_mapping.len(), 1);
        assert_eq!(cfg.ad.role_mapping[0].role, "admin");
    }

    #[test]
    fn truthy_flag_parsing() {
        for v in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(truthy(v), "{v} should be truthy");
        }
        for v in ["0", "false", "no", "", "maybe"] {
            assert!(!truthy(v), "{v} should be falsy");
        }
    }

    #[test]
    fn ad_env_overrides_fill_the_section() {
        // Saved/restored so this test never leaks state into its neighbours.
        let vars = [
            ("BESEDKA_AD_DOMAIN", "corp.example"),
            ("BESEDKA_AD_HOST", "dc1.corp.example"),
            ("BESEDKA_AD_BIND_DN", "svc-besedka-ro@corp.example"),
            ("BESEDKA_AD_BIND_PASSWORD", "env-only-secret"),
            ("BESEDKA_AD_MOCK", "1"),
        ];
        let saved: Vec<_> = vars
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            std::env::set_var(k, v);
        }

        let mut ad = AdConfig::default();
        apply_ad_env(&mut ad);

        assert_eq!(ad.domain, "corp.example");
        assert_eq!(ad.host, "dc1.corp.example");
        assert_eq!(ad.bind_dn, "svc-besedka-ro@corp.example");
        assert_eq!(ad.bind_password, "env-only-secret");
        assert!(ad.mock);

        for (k, old) in saved {
            match old {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}
