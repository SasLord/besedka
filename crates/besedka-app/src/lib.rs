//! Besedka app — composition root: config, HTTP router, wiring.
//!
//! Wave 0: `/api/v1/ping` + `/api/v1/rooms` over axum, sessions via
//! `tower-sessions` (MemoryStore — enough for Wave 1's password login).

pub mod config;
pub mod http;

use std::sync::Arc;

use axum::routing::{get, post};
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
        .route("/api/v1/register", post(http::register))
        .route("/api/v1/login", post(http::login))
        .route("/api/v1/logout", post(http::logout))
        .route("/api/v1/me", get(http::me))
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

    /// Multi-request helper: takes `&Router` (clones it per request, so one
    /// router + one shared MemoryStore backs the whole test flow). Returns
    /// status, JSON body and response headers (for cookie capture).
    async fn req(
        router: &Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
        cookie: Option<&str>,
    ) -> (u16, serde_json::Value, axum::http::HeaderMap) {
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if let Some(cookie) = cookie {
            builder = builder.header(axum::http::header::COOKIE, cookie);
        }
        let (body, builder) = match body {
            Some(v) => (
                axum::body::Body::from(v.to_string()),
                builder.header(axum::http::header::CONTENT_TYPE, "application/json"),
            ),
            None => (axum::body::Body::empty(), builder),
        };
        let request = builder.body(body).expect("valid request");
        let response = router.clone().oneshot(request).await.expect("handler ok");
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("body");
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        (status, value, headers)
    }

    /// Pull the session cookie value (`name=id`) out of a `Set-Cookie` header.
    fn session_cookie(headers: &axum::http::HeaderMap) -> String {
        headers
            .get(axum::http::header::SET_COOKIE)
            .expect("Set-Cookie header present")
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string()
    }

    async fn register(
        router: &Router,
        username: &str,
        password: &str,
    ) -> (u16, serde_json::Value, axum::http::HeaderMap) {
        req(
            router,
            "POST",
            "/api/v1/register",
            Some(serde_json::json!({ "username": username, "password": password })),
            None,
        )
        .await
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

    // ----- Wave 1.1: auth -----

    #[tokio::test]
    async fn register_creates_account_without_leaking_hash() {
        let router = test_app(test_db());
        let (status, body, _) =
            register(&router, "alice", "sup3rs3cret").await;
        assert_eq!(status, 201);
        assert_eq!(body["user"]["username"], "alice");
        assert_eq!(body["user"]["role"], "user");
        assert!(body["user"].get("password_hash").is_none(), "hash must not leak");
    }

    #[tokio::test]
    async fn register_rejects_weak_password() {
        let router = test_app(test_db());
        let (status, body, _) = register(&router, "alice", "short").await;
        assert_eq!(status, 400);
        assert!(body["error"].as_str().unwrap().contains("password"));
    }

    #[tokio::test]
    async fn register_rejects_bad_username() {
        let router = test_app(test_db());
        let (status, body, _) = register(&router, "a", "sup3rs3cret").await;
        assert_eq!(status, 400);
        assert!(body["error"].as_str().unwrap().contains("username"));
    }

    #[tokio::test]
    async fn register_duplicate_username_is_conflict() {
        let router = test_app(test_db());
        let (status, _, _) = register(&router, "alice", "sup3rs3cret").await;
        assert_eq!(status, 201);
        // Case-variant duplicate → 409.
        let (status, body, _) = register(&router, "ALICE", "sup3rs3cret").await;
        assert_eq!(status, 409);
        assert!(body["error"].as_str().unwrap().contains("taken"));
    }

    #[tokio::test]
    async fn login_me_logout_flow() {
        let router = test_app(test_db());
        register(&router, "alice", "sup3rs3cret").await;

        // Login → 200 + Set-Cookie.
        let (status, body, headers) = req(
            &router,
            "POST",
            "/api/v1/login",
            Some(serde_json::json!({ "username": "alice", "password": "sup3rs3cret" })),
            None,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["user"]["username"], "alice");
        let cookie = session_cookie(&headers);

        // me WITH cookie → 200 alice.
        let (status, body, _) = req(&router, "GET", "/api/v1/me", None, Some(&cookie)).await;
        assert_eq!(status, 200);
        assert_eq!(body["user"]["username"], "alice");

        // me WITHOUT cookie → 401.
        let (status, _, _) = req(&router, "GET", "/api/v1/me", None, None).await;
        assert_eq!(status, 401);
    }

    #[tokio::test]
    async fn login_wrong_password_is_401() {
        let router = test_app(test_db());
        register(&router, "alice", "sup3rs3cret").await;
        let (status, body, _) = req(
            &router,
            "POST",
            "/api/v1/login",
            Some(serde_json::json!({ "username": "alice", "password": "wrong-pass-123" })),
            None,
        )
        .await;
        assert_eq!(status, 401);
        // Same message as unknown user — no account enumeration.
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("invalid username or password"));
    }

    #[tokio::test]
    async fn login_unknown_user_is_401() {
        let router = test_app(test_db());
        let (status, body, _) = req(
            &router,
            "POST",
            "/api/v1/login",
            Some(serde_json::json!({ "username": "ghost", "password": "sup3rs3cret" })),
            None,
        )
        .await;
        assert_eq!(status, 401);
        assert!(body["error"]
            .as_str()
            .unwrap()
            .contains("invalid username or password"));
    }

    #[tokio::test]
    async fn logout_invalidates_session() {
        let router = test_app(test_db());
        register(&router, "alice", "sup3rs3cret").await;
        let (_, _, headers) = req(
            &router,
            "POST",
            "/api/v1/login",
            Some(serde_json::json!({ "username": "alice", "password": "sup3rs3cret" })),
            None,
        )
        .await;
        let cookie = session_cookie(&headers);

        // logout with the session cookie → 200.
        let (status, _, _) = req(&router, "POST", "/api/v1/logout", None, Some(&cookie)).await;
        assert_eq!(status, 200);

        // me with the old cookie → 401 (session emptied + deleted).
        let (status, _, _) = req(&router, "GET", "/api/v1/me", None, Some(&cookie)).await;
        assert_eq!(status, 401);
    }
}
