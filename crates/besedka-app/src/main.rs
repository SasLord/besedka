//! Besedka server entrypoint.

use std::sync::Arc;

use anyhow::Context;
use besedka_app::ad::AdRuntime;
use besedka_app::app;
use besedka_app::config::Config;
use besedka_infra::Db;

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,besedka=debug"));
    // `try_init` is fine here: in tests a subscriber may already exist.
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config_path = std::env::var("BESEDKA_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
    let cfg =
        Config::load(&config_path).with_context(|| format!("loading config from {config_path}"))?;

    let db = Arc::new(
        Db::open(&cfg.database.path)
            .with_context(|| format!("opening database at {}", cfg.database.path))?,
    );

    let listener = tokio::net::TcpListener::bind(cfg.server.socket_addr())
        .await
        .with_context(|| format!("binding {}", cfg.server.socket_addr()))?;

    // AD adapters are chosen once, here: real LDAP/Kerberos or the local mocks.
    let ad = AdRuntime::from_config(cfg.ad.clone());
    if ad.enabled() {
        tracing::info!(
            mock = ad.mock,
            sso = ad.sso_available(),
            password_login = ad.password_login_available(),
            "AD/Kerberos SSO active"
        );
    } else {
        tracing::info!("AD/Kerberos SSO disabled ([ad] enabled = false)");
    }

    tracing::info!(addr = %cfg.server.socket_addr(), "besedka listening");
    axum::serve(listener, app(db, cfg.sessions, ad)).await?;
    Ok(())
}
