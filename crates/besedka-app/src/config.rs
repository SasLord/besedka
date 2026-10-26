//! Server configuration: TOML file + env overrides.
//!
//! No secrets here (see `.gitignore`): auth secrets live in the environment.

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
        let ip: IpAddr = self
            .host
            .parse()
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
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
    /// `BESEDKA_HOST`, `BESEDKA_PORT`, `BESEDKA_DB_PATH`.
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
        Ok(cfg)
    }
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
}
