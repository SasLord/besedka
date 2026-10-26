//! Besedka app — composition root: config, HTTP router, wiring.
//!
//! Wave 0: `/api/v1/ping` + `/api/v1/rooms` over axum, sessions via
//! `tower-sessions` (MemoryStore — enough for Wave 1's password login).

pub mod config;
pub mod http;

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use besedka_infra::Db;
use time::Duration;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};

use config::SessionsConfig;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
}

/// Build the HTTP application (router).
pub fn app(db: Arc<Db>, sessions: SessionsConfig) -> Router {
    let state = AppState { db };

    // Wave 1 will back this with real login; MemoryStore is enough for now
    // (single process). Cookie: http-only, lax, non-secure for local dev.
    let sessions_layer = SessionManagerLayer::new(MemoryStore::default())
        .with_name(sessions.cookie_name)
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_secure(false)
        .with_path("/")
        .with_expiry(Expiry::OnInactivity(Duration::seconds(
            sessions.session_ttl_secs as i64,
        )));

    Router::new()
        .route("/api/v1/ping", get(http::ping))
        .route("/api/v1/rooms", get(http::rooms))
        .layer(sessions_layer)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use besedka_core::RoomKind;
    use tower::ServiceExt;

    fn test_db() -> Arc<Db> {
        Arc::new(Db::open_in_memory().expect("in-memory db"))
    }

    fn test_app(db: Arc<Db>) -> Router {
        app(db, SessionsConfig::default())
    }

    async fn get(router: Router, uri: &str) -> (u16, serde_json::Value) {
        let request = axum::http::Request::builder()
            .uri(uri)
            .body(axum::body::Body::empty())
            .expect("valid request");
        let response = router.oneshot(request).await.expect("handler ok");
        let status = response.status().as_u16();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("body");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        (status, value)
    }

    #[tokio::test]
    async fn ping_returns_ok_json() {
        let (status, body) = get(test_app(test_db()), "/api/v1/ping").await;
        assert_eq!(status, 200);
        assert_eq!(body["status"], "ok");
        assert_eq!(body["app"], "besedka");
        assert!(body["version"].is_string());
    }

    #[tokio::test]
    async fn rooms_returns_empty_list_when_db_empty() {
        let (status, body) = get(test_app(test_db()), "/api/v1/rooms").await;
        assert_eq!(status, 200);
        assert!(body["rooms"].as_array().expect("rooms array").is_empty());
    }

    #[tokio::test]
    async fn rooms_lists_created_room() {
        let db = test_db();
        db.create_room(RoomKind::General, "Новости").expect("create room");
        let (status, body) = get(test_app(db), "/api/v1/rooms").await;
        assert_eq!(status, 200);
        let rooms = body["rooms"].as_array().expect("rooms array");
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0]["title"], "Новости");
        assert_eq!(rooms[0]["kind"], "general");
    }
}
