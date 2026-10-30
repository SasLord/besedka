//! Besedka app — composition root: config, HTTP router, wiring.
//!
//! Wave 0: `/api/v1/ping` + `/api/v1/rooms` over axum, sessions via
//! `tower-sessions` (MemoryStore).
//! Wave 1.1: local password auth (`register`/`login`/`logout`/`me`).
//! Wave 1.2: AD/Kerberos SSO — passwordless (`GET /api/v1/auth_ad_sso`) and
//! AD-password (`POST /api/v1/auth_ad_login`) logins, both issuing the SAME
//! session cookie as password login, plus session-backed role gates and an
//! admin-only AD diagnostics endpoint.

pub mod ad;
pub mod config;
pub mod http;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use besedka_infra::Db;
use time::Duration;
use tower_sessions::cookie::SameSite;
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};

use ad::AdRuntime;
use config::SessionsConfig;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    /// AD adapters + config (real or mock, chosen once at startup).
    pub ad: Arc<AdRuntime>,
}

/// Build the HTTP application (router).
pub fn app(db: Arc<Db>, sessions: SessionsConfig, ad: AdRuntime) -> Router {
    let state = AppState {
        db,
        ad: Arc::new(ad),
    };

    // MemoryStore is enough for a single-process server (sessions are lost on
    // restart, which is acceptable until Wave 7's deployment slice).
    // Cookie: http-only, lax, non-secure for local dev.
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
        .route("/api/v1/rooms/{id}/join", post(http::join_room))
        .route("/api/v1/rooms/{id}/leave", post(http::leave_room))
        // Wave 2.1 — room creation (admin) + append-only messages (members).
        .route("/api/v1/rooms", post(http::create_room))
        .route(
            "/api/v1/rooms/{id}/messages",
            post(http::post_message).get(http::list_messages),
        )
        .route("/api/v1/register", post(http::register))
        .route("/api/v1/login", post(http::login))
        .route("/api/v1/logout", post(http::logout))
        .route("/api/v1/me", get(http::me))
        // Wave 2.2 — user catalog (sender names in the UI; reused by 2.3).
        .route("/api/v1/users", get(http::list_users))
        // Wave 1.2 — AD/Kerberos SSO.
        .route("/api/v1/auth_ad_sso", get(http::sso::ad_sso))
        .route(
            "/api/v1/auth_ad_sso/status",
            get(http::sso::ad_feature_status),
        )
        .route("/api/v1/auth_ad_login", post(http::sso::ad_login))
        // Admin-only (403 for non-admins).
        .route("/api/v1/admin/ad_status", get(http::sso::admin_ad_status))
        .layer(sessions_layer)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use besedka_core::{Role, RoomKind};
    use besedka_infra::ad::AdConfig;
    use tower::ServiceExt;

    fn test_db() -> Arc<Db> {
        Arc::new(Db::open_in_memory().expect("in-memory db"))
    }

    /// AD off (the default) — every SSO route answers 503.
    fn test_app(db: Arc<Db>) -> Router {
        app(
            db,
            SessionsConfig::default(),
            AdRuntime::build(AdConfig::default(), true),
        )
    }

    /// AD on in mock mode: fixtures, no DC, no keytab.
    fn mock_ad() -> AdConfig {
        AdConfig {
            enabled: true,
            mock: true,
            domain: "example.local".to_string(),
            ..AdConfig::default()
        }
    }

    fn mock_app(db: Arc<Db>) -> Router {
        app(
            db,
            SessionsConfig::default(),
            AdRuntime::build(mock_ad(), true),
        )
    }

    /// AD on in REAL mode with a keytab path — a DC is never contacted by the
    /// code paths under test (challenge / keytab read / ticket validation are
    /// all local).
    fn real_app(db: Arc<Db>, keytab_path: &str) -> Router {
        let cfg = AdConfig {
            enabled: true,
            domain: "example.local".to_string(),
            spn: "HTTP/chat.example.local".to_string(),
            keytab_path: keytab_path.to_string(),
            ..AdConfig::default()
        };
        app(db, SessionsConfig::default(), AdRuntime::build(cfg, false))
    }

    async fn get(router: Router, uri: &str) -> (u16, serde_json::Value) {
        let request = axum::http::Request::builder()
            .uri(uri)
            .body(axum::body::Body::empty())
            .expect("valid request");
        let response = router.oneshot(request).await.expect("handler ok");
        let status = response.status().as_u16();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
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
        req_h(router, method, uri, body, cookie, &[]).await
    }

    /// Same as [`req`], plus extra request headers (AD login hints,
    /// `Authorization: Negotiate …`).
    async fn req_h(
        router: &Router,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
        cookie: Option<&str>,
        extra: &[(&str, &str)],
    ) -> (u16, serde_json::Value, axum::http::HeaderMap) {
        let mut builder = axum::http::Request::builder().method(method).uri(uri);
        if let Some(cookie) = cookie {
            builder = builder.header(axum::http::header::COOKIE, cookie);
        }
        for (name, value) in extra {
            builder = builder.header(*name, *value);
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
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
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

    /// A valid keytab blob for `HTTP/web.example.local@EXAMPLE.LOCAL`
    /// (AES256, kvno 3) — same byte layout the keytab unit tests build.
    fn sample_keytab() -> Vec<u8> {
        fn counted(s: &[u8]) -> Vec<u8> {
            let mut v = (s.len() as u16).to_be_bytes().to_vec();
            v.extend_from_slice(s);
            v
        }
        let mut body = Vec::new();
        body.extend_from_slice(&2u16.to_be_bytes());
        body.extend_from_slice(&counted(b"EXAMPLE.LOCAL"));
        body.extend_from_slice(&counted(b"HTTP"));
        body.extend_from_slice(&counted(b"web.example.local"));
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&0u32.to_be_bytes());
        body.push(3u8);
        body.extend_from_slice(&18u16.to_be_bytes());
        body.extend_from_slice(&counted(&[0xABu8; 32]));
        body.extend_from_slice(&3u32.to_be_bytes());

        let mut out = 0x0502u16.to_be_bytes().to_vec();
        out.extend_from_slice(&(body.len() as i32).to_be_bytes());
        out.extend_from_slice(&body);
        out
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
        db.create_room(RoomKind::General, "Новости")
            .expect("create room");
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
        let (status, body, _) = register(&router, "alice", "sup3rs3cret").await;
        assert_eq!(status, 201);
        assert_eq!(body["user"]["username"], "alice");
        assert_eq!(body["user"]["role"], "user");
        assert!(
            body["user"].get("password_hash").is_none(),
            "hash must not leak"
        );
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

    // ----- Wave 1.2: AD/Kerberos SSO (mock mode) -----

    #[tokio::test]
    async fn sso_status_is_public_and_reports_ad_off_by_default() {
        let (status, body) = get(test_app(test_db()), "/api/v1/auth_ad_sso/status").await;
        assert_eq!(status, 200, "status probe must never need a session");
        assert_eq!(body["enabled"], false);
        assert_eq!(body["sso"], false);
        assert_eq!(body["password_login"], false);
    }

    #[tokio::test]
    async fn sso_status_reports_mock_mode() {
        let (status, body) = get(mock_app(test_db()), "/api/v1/auth_ad_sso/status").await;
        assert_eq!(status, 200);
        assert_eq!(body["enabled"], true);
        assert_eq!(body["mock"], true);
        assert_eq!(body["sso"], true);
        assert_eq!(body["password_login"], true);
    }

    #[tokio::test]
    async fn sso_endpoint_is_503_when_ad_is_disabled() {
        let (status, body) = get(test_app(test_db()), "/api/v1/auth_ad_sso").await;
        assert_eq!(status, 503);
        assert!(body["error"].as_str().unwrap().contains("disabled"));
    }

    #[tokio::test]
    async fn sso_mock_logs_in_fixture_user_and_opens_a_session() {
        let router = mock_app(test_db());

        // GET with no ticket at all — in mock mode that IS the login.
        let (status, body, headers) = req(&router, "GET", "/api/v1/auth_ad_sso", None, None).await;
        assert_eq!(status, 200);
        assert_eq!(body["ok"], true);
        assert_eq!(body["mock"], true);
        assert_eq!(body["user"]["username"], "us100");
        // Role and ФИО come from the mocked AD directory, not from the login.
        assert_eq!(body["user"]["role"], "manager");
        assert_eq!(body["user"]["display_name"], "Иванов Иван Иванович");
        assert!(
            body["user"].get("password_hash").is_none(),
            "AD accounts never expose a hash"
        );

        // The SAME session cookie as password login works for /me.
        let cookie = session_cookie(&headers);
        let (status, body, _) = req(&router, "GET", "/api/v1/me", None, Some(&cookie)).await;
        assert_eq!(status, 200);
        assert_eq!(body["user"]["username"], "us100");
    }

    #[tokio::test]
    async fn sso_mock_identity_can_be_chosen_by_header_or_query() {
        let router = mock_app(test_db());

        let (status, body, _) = req_h(
            &router,
            "GET",
            "/api/v1/auth_ad_sso",
            None,
            None,
            &[("x-besedka-ad-login", "us200")],
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["user"]["username"], "us200");
        assert_eq!(body["user"]["role"], "user");

        let (status, body, _) = req(
            &router,
            "GET",
            "/api/v1/auth_ad_sso?login=us300",
            None,
            None,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["user"]["username"], "us300");
        assert_eq!(body["user"]["role"], "admin");
    }

    #[tokio::test]
    async fn sso_mock_second_login_reuses_the_same_account() {
        let db = test_db();
        let router = mock_app(db.clone());

        let (_, first, _) = req(&router, "GET", "/api/v1/auth_ad_sso", None, None).await;
        let (_, second, _) = req(&router, "GET", "/api/v1/auth_ad_sso", None, None).await;

        assert_eq!(first["user"]["id"], second["user"]["id"]);
        assert_eq!(db.user_count().expect("count"), 1, "no duplicate accounts");
    }

    #[tokio::test]
    async fn sso_mock_keeps_an_existing_local_role_and_syncs_the_display_name() {
        let db = test_db();
        let router = mock_app(db.clone());

        // A local password account already uses the same username.
        let (status, registered, _) = register(&router, "us100", "sup3rs3cret").await;
        assert_eq!(status, 201);
        let local_id = registered["user"]["id"].clone();
        assert_eq!(registered["user"]["role"], "user");

        let (status, body, _) = req(&router, "GET", "/api/v1/auth_ad_sso", None, None).await;
        assert_eq!(status, 200);
        // Same account (username is the identity), role NOT silently elevated,
        // ФИО refreshed from the directory.
        assert_eq!(body["user"]["id"], local_id);
        assert_eq!(body["user"]["role"], "user");
        assert_eq!(body["user"]["display_name"], "Иванов Иван Иванович");
    }

    #[tokio::test]
    async fn sso_mock_denied_identity_is_401() {
        let router = mock_app(test_db());
        let (status, body, _) = req(
            &router,
            "GET",
            "/api/v1/auth_ad_sso?login=deny-me",
            None,
            None,
        )
        .await;
        assert_eq!(status, 401);
        assert!(!body["error"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn sso_mock_unreachable_directory_is_503_and_creates_nothing() {
        // AD on, mock adapters, but the directory reports an outage.
        let db = test_db();
        let cfg = mock_ad();
        let ad = AdRuntime {
            config: cfg.clone(),
            mock: true,
            client: Arc::new(besedka_infra::ad::MockAdClient::default_fixtures()),
            directory: Arc::new(besedka_infra::ad::MockAdDirectory::unreachable()),
        };
        let router = app(db.clone(), SessionsConfig::default(), ad);

        let (status, _body, _) = req(&router, "GET", "/api/v1/auth_ad_sso", None, None).await;
        assert_eq!(status, 503, "an AD outage must not mint a session");
        assert_eq!(db.user_count().expect("count"), 0);
    }

    // ----- Wave 1.2: real Kerberos path (no DC needed) -----

    #[tokio::test]
    async fn sso_real_mode_challenges_with_negotiate_header() {
        let router = real_app(test_db(), "/nonexistent/chat.keytab");
        let (status, _body, headers) = req(&router, "GET", "/api/v1/auth_ad_sso", None, None).await;

        assert_eq!(status, 401);
        assert_eq!(
            headers
                .get(axum::http::header::WWW_AUTHENTICATE)
                .expect("WWW-Authenticate")
                .to_str()
                .unwrap(),
            "Negotiate",
            "a domain-joined browser retries with a ticket only if we challenge"
        );
    }

    #[tokio::test]
    async fn sso_real_mode_unreadable_keytab_is_503() {
        let router = real_app(test_db(), "/nonexistent/chat.keytab");
        // "dGlja2V0" = base64("ticket")
        let (status, body, _) = req_h(
            &router,
            "GET",
            "/api/v1/auth_ad_sso",
            None,
            None,
            &[("authorization", "Negotiate dGlja2V0")],
        )
        .await;
        assert_eq!(status, 503);
        assert!(body["error"].as_str().unwrap().contains("keytab"));
    }

    #[tokio::test]
    async fn sso_real_mode_malformed_token_is_400() {
        let router = real_app(test_db(), "/nonexistent/chat.keytab");
        let (status, _body, _) = req_h(
            &router,
            "GET",
            "/api/v1/auth_ad_sso",
            None,
            None,
            &[("authorization", "Negotiate not base64 !!!")],
        )
        .await;
        assert_eq!(status, 400);
    }

    #[tokio::test]
    async fn sso_real_mode_foreign_ticket_is_401_and_never_leaks_internals() {
        // A real (structurally valid) keytab for a DIFFERENT SPN than the one
        // the token claims: the accept fails, and the client gets a generic 401.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("chat.keytab");
        std::fs::write(&path, sample_keytab()).expect("write keytab");

        let router = real_app(test_db(), path.to_str().expect("utf8 path"));
        let (status, body, _) = req_h(
            &router,
            "GET",
            "/api/v1/auth_ad_sso",
            None,
            None,
            &[("authorization", "Negotiate dGlja2V0")],
        )
        .await;

        assert_eq!(status, 401);
        let message = body["error"].as_str().unwrap();
        assert!(
            !message.to_lowercase().contains("keytab"),
            "internals must not leak to the client: {message}"
        );
        assert!(message.contains("Kerberos"));
    }

    // ----- Wave 1.2: AD password login (LDAP bind, mock fixtures) -----

    #[tokio::test]
    async fn ad_password_login_succeeds_with_mock_fixture() {
        let router = mock_app(test_db());
        let (status, body, headers) = req(
            &router,
            "POST",
            "/api/v1/auth_ad_login",
            Some(serde_json::json!({ "username": "us200@example.local", "password": "Secret123" })),
            None,
        )
        .await;

        assert_eq!(status, 200);
        assert_eq!(body["user"]["username"], "us200");
        assert_eq!(body["user"]["display_name"], "Петрова Анна Сергеевна");

        let cookie = session_cookie(&headers);
        let (status, body, _) = req(&router, "GET", "/api/v1/me", None, Some(&cookie)).await;
        assert_eq!(status, 200);
        assert_eq!(body["user"]["username"], "us200");
    }

    #[tokio::test]
    async fn ad_password_login_wrong_password_is_401_with_generic_message() {
        let router = mock_app(test_db());
        let (status, body, _) = req(
            &router,
            "POST",
            "/api/v1/auth_ad_login",
            Some(serde_json::json!({ "username": "us100", "password": "nope-nope-1" })),
            None,
        )
        .await;
        assert_eq!(status, 401);
        assert_eq!(body["error"], "invalid username or password");
    }

    #[tokio::test]
    async fn ad_password_login_is_503_when_ad_is_off() {
        let router = test_app(test_db());
        let (status, _body, _) = req(
            &router,
            "POST",
            "/api/v1/auth_ad_login",
            Some(serde_json::json!({ "username": "us100", "password": "Passw0rd!" })),
            None,
        )
        .await;
        assert_eq!(status, 503);
    }

    // ----- Wave 1.2: role gates -----

    #[tokio::test]
    async fn admin_endpoint_requires_authentication() {
        let router = mock_app(test_db());
        let (status, body, _) = req(&router, "GET", "/api/v1/admin/ad_status", None, None).await;
        assert_eq!(status, 401);
        assert_eq!(body["error"], "not logged in");
    }

    #[tokio::test]
    async fn admin_endpoint_forbids_non_admin_and_allows_admin() {
        let router = mock_app(test_db());

        // us100 maps to manager — authenticated but not allowed.
        let (_, _, headers) = req_h(
            &router,
            "GET",
            "/api/v1/auth_ad_sso?login=us100",
            None,
            None,
            &[],
        )
        .await;
        let manager_cookie = session_cookie(&headers);
        let (status, body, _) = req(
            &router,
            "GET",
            "/api/v1/admin/ad_status",
            None,
            Some(&manager_cookie),
        )
        .await;
        assert_eq!(status, 403);
        assert!(body["error"].as_str().unwrap().contains("admin"));

        // us300 maps to admin — allowed, and the payload carries no secrets.
        let (_, _, headers) = req(
            &router,
            "GET",
            "/api/v1/auth_ad_sso?login=us300",
            None,
            None,
        )
        .await;
        let admin_cookie = session_cookie(&headers);
        let (status, body, _) = req(
            &router,
            "GET",
            "/api/v1/admin/ad_status",
            None,
            Some(&admin_cookie),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["mock"], true);
        assert_eq!(body["reachable"], "ok");
        assert_eq!(body["directory_configured"], false);
        assert!(
            body.get("bind_password").is_none(),
            "diagnostics must never expose the service password"
        );
    }

    // ----- Wave 1.3: room join/leave + membership -----

    /// Register a local account, log in, and hand back the session cookie.
    async fn logged_in(router: &Router, username: &str) -> String {
        register(router, username, "sup3rs3cret").await;
        let (_, _, headers) = req(
            router,
            "POST",
            "/api/v1/login",
            Some(serde_json::json!({ "username": username, "password": "sup3rs3cret" })),
            None,
        )
        .await;
        session_cookie(&headers)
    }

    #[tokio::test]
    async fn join_marks_the_room_and_the_list_reflects_it() {
        let db = test_db();
        let room_id = db
            .create_room(RoomKind::General, "Новости")
            .expect("create room");
        let router = test_app(db);
        let cookie = logged_in(&router, "alice").await;

        // Before joining: the room is listed but `joined` is false.
        let (status, body, _) = req(&router, "GET", "/api/v1/rooms", None, Some(&cookie)).await;
        assert_eq!(status, 200);
        let room = &body["rooms"][0];
        assert_eq!(room["joined"], false);
        assert_eq!(room["member_count"], 0);

        // Join → 200, `joined` true, member_count 1.
        let (status, body, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["room"]["id"], room_id);
        assert_eq!(body["room"]["joined"], true);
        assert_eq!(body["room"]["member_count"], 1);

        // The list now shows membership for alice...
        let (status, body, _) = req(&router, "GET", "/api/v1/rooms", None, Some(&cookie)).await;
        assert_eq!(status, 200);
        let room = &body["rooms"][0];
        assert_eq!(room["joined"], true);
        assert_eq!(room["member_count"], 1);

        // ...but not for anonymous viewers.
        let (status, body, _) = req(&router, "GET", "/api/v1/rooms", None, None).await;
        assert_eq!(status, 200);
        assert_eq!(body["rooms"][0]["joined"], false);
    }

    #[tokio::test]
    async fn double_join_is_idempotent() {
        let db = test_db();
        let room_id = db
            .create_room(RoomKind::General, "Общий")
            .expect("create room");
        let router = test_app(db);
        let cookie = logged_in(&router, "alice").await;

        let (status, body, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["room"]["member_count"], 1);

        // Second join: still 200, count stays at 1.
        let (status, body, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["room"]["joined"], true);
        assert_eq!(body["room"]["member_count"], 1);
    }

    #[tokio::test]
    async fn join_unknown_room_is_404() {
        let router = test_app(test_db());
        let cookie = logged_in(&router, "alice").await;
        let (status, body, _) = req(
            &router,
            "POST",
            "/api/v1/rooms/999/join",
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 404);
        assert!(body["error"].as_str().unwrap().contains("not found"));
    }

    #[tokio::test]
    async fn join_without_a_session_is_401() {
        let db = test_db();
        let room_id = db
            .create_room(RoomKind::General, "Общий")
            .expect("create room");
        let router = test_app(db);
        let (status, body, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            None,
        )
        .await;
        assert_eq!(status, 401);
        assert_eq!(body["error"], "not logged in");
    }

    #[tokio::test]
    async fn leave_removes_the_membership() {
        let db = test_db();
        let room_id = db
            .create_room(RoomKind::General, "Общий")
            .expect("create room");
        let router = test_app(db);
        let cookie = logged_in(&router, "alice").await;

        req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&cookie),
        )
        .await;

        // Leave → 200, membership gone.
        let (status, body, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/leave"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["room"]["joined"], false);
        assert_eq!(body["room"]["member_count"], 0);

        // The list agrees.
        let (status, body, _) = req(&router, "GET", "/api/v1/rooms", None, Some(&cookie)).await;
        assert_eq!(status, 200);
        assert_eq!(body["rooms"][0]["joined"], false);
    }

    #[tokio::test]
    async fn leave_without_joining_is_404() {
        let db = test_db();
        let room_id = db
            .create_room(RoomKind::General, "Общий")
            .expect("create room");
        let router = test_app(db);
        let cookie = logged_in(&router, "alice").await;
        let (status, _, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/leave"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 404, "never joined → no membership to remove");
    }

    #[tokio::test]
    async fn leave_unknown_room_is_404() {
        let router = test_app(test_db());
        let cookie = logged_in(&router, "alice").await;
        let (status, _, _) = req(
            &router,
            "POST",
            "/api/v1/rooms/999/leave",
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn member_count_counts_distinct_users() {
        let db = test_db();
        let room_id = db
            .create_room(RoomKind::General, "Общий")
            .expect("create room");
        let router = test_app(db);

        let alice = logged_in(&router, "alice").await;
        let bob = logged_in(&router, "bob").await;

        req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&alice),
        )
        .await;
        req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&bob),
        )
        .await;

        // Both see the same count; their own `joined` flag is independent.
        for cookie in [&alice, &bob] {
            let (status, body, _) = req(&router, "GET", "/api/v1/rooms", None, Some(cookie)).await;
            assert_eq!(status, 200);
            assert_eq!(body["rooms"][0]["member_count"], 2);
            assert_eq!(body["rooms"][0]["joined"], true);
        }
    }

    // ----- Wave 2.1: room creation + append-only messages -----

    /// Admin account (created directly in the DB before `test_app`) logged in
    /// via the API; hands back the session cookie.
    fn test_app_with_admin(db: Arc<Db>) -> Router {
        let pw_hash = besedka_core::hash("boss-pass").expect("argon2 available");
        db.create_user("boss", None, Role::Admin, Some(&pw_hash))
            .expect("admin user created");
        test_app(db)
    }

    async fn admin_cookie(router: &Router) -> String {
        let (_, _, headers) = req(
            router,
            "POST",
            "/api/v1/login",
            Some(serde_json::json!({ "username": "boss", "password": "boss-pass" })),
            None,
        )
        .await;
        session_cookie(&headers)
    }

    /// Acceptance flow: create channel (admin) → post → read back.
    #[tokio::test]
    async fn admin_creates_channel_posts_and_reads() {
        let router = test_app_with_admin(test_db());
        let cookie = admin_cookie(&router).await;

        let (status, body, _) = req(
            &router,
            "POST",
            "/api/v1/rooms",
            Some(serde_json::json!({ "kind": "channel", "title": "obshchii" })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 201);
        assert_eq!(body["room"]["kind"], "channel");
        assert_eq!(body["room"]["title"], "obshchii");
        let room_id = body["room"]["id"].as_i64().unwrap();

        // Creator is auto-joined, so posting works immediately (201).
        let (status, body, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/messages"),
            Some(serde_json::json!({ "text": "privet, kanal!" })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 201);
        assert_eq!(body["message"]["text"], "privet, kanal!");

        let (status, body, _) = req(
            &router,
            "GET",
            &format!("/api/v1/rooms/{room_id}/messages"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 200);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["text"], "privet, kanal!");
    }

    #[tokio::test]
    async fn create_room_requires_admin() {
        let router = test_app_with_admin(test_db());
        let cookie = logged_in(&router, "alice").await;

        let (status, body, _) = req(
            &router,
            "POST",
            "/api/v1/rooms",
            Some(serde_json::json!({ "kind": "room", "title": "ne-moya" })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 403);
        assert_eq!(body["error"], "admin role required");
    }

    #[tokio::test]
    async fn create_room_rejects_bad_kind_and_title() {
        let router = test_app_with_admin(test_db());
        let cookie = admin_cookie(&router).await;

        let cases = [
            serde_json::json!({ "kind": "direct", "title": "ok" }),
            serde_json::json!({ "kind": "room", "title": "   " }),
            serde_json::json!({ "kind": "room", "title": "a".repeat(129) }),
        ];
        for body in cases {
            let (status, _, _) =
                req(&router, "POST", "/api/v1/rooms", Some(body), Some(&cookie)).await;
            assert_eq!(status, 400);
        }
        // 128-char title is right at the limit -> accepted.
        let (status, _, _) = req(
            &router,
            "POST",
            "/api/v1/rooms",
            Some(serde_json::json!({ "kind": "room", "title": "b".repeat(128) })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 201);
    }

    #[tokio::test]
    async fn post_message_requires_membership() {
        let db = test_db();
        let room_id = db.create_room(RoomKind::Channel, "obshchii").expect("room");
        let router = test_app(db);

        let host = logged_in(&router, "host").await;
        req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&host),
        )
        .await;

        // Non-member cannot post (403) nor read (403).
        let guest = logged_in(&router, "guest").await;
        let (status, body, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/messages"),
            Some(serde_json::json!({ "text": "vtraval?" })),
            Some(&guest),
        )
        .await;
        assert_eq!(status, 403);
        assert!(body["error"].as_str().unwrap().contains("member"));
        let (status, _, _) = req(
            &router,
            "GET",
            &format!("/api/v1/rooms/{room_id}/messages"),
            None,
            Some(&guest),
        )
        .await;
        assert_eq!(status, 403);

        // The host, who IS a member, can still post (201).
        let (status, _, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/messages"),
            Some(serde_json::json!({ "text": "ok" })),
            Some(&host),
        )
        .await;
        assert_eq!(status, 201);
    }

    #[tokio::test]
    async fn messages_unknown_room_is_404() {
        let router = test_app(test_db());
        let cookie = logged_in(&router, "alice").await;

        let (status, _, _) = req(
            &router,
            "POST",
            "/api/v1/rooms/999/messages",
            Some(serde_json::json!({ "text": "komu?" })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 404);
        let (status, _, _) = req(
            &router,
            "GET",
            "/api/v1/rooms/999/messages",
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 404);
    }

    #[tokio::test]
    async fn post_message_rejects_empty_and_long_text() {
        let db = test_db();
        let room_id = db.create_room(RoomKind::Channel, "obshchii").expect("room");
        let router = test_app(db);
        let cookie = logged_in(&router, "alice").await;
        req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&cookie),
        )
        .await;

        // Empty / whitespace-only -> 400.
        for text in ["", "   "] {
            let (status, _, _) = req(
                &router,
                "POST",
                &format!("/api/v1/rooms/{room_id}/messages"),
                Some(serde_json::json!({ "text": text })),
                Some(&cookie),
            )
            .await;
            assert_eq!(status, 400);
        }
        // 4096 chars is right at the limit -> accepted; 4097 -> 400.
        let (status, _, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/messages"),
            Some(serde_json::json!({ "text": "a".repeat(4096) })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 201);
        let (status, _, _) = req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/messages"),
            Some(serde_json::json!({ "text": "a".repeat(4097) })),
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 400);
    }

    #[tokio::test]
    async fn empty_room_lists_no_messages() {
        let db = test_db();
        let room_id = db.create_room(RoomKind::Channel, "tihiy").expect("room");
        let router = test_app(db);
        let cookie = logged_in(&router, "alice").await;
        req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&cookie),
        )
        .await;

        let (status, body, _) = req(
            &router,
            "GET",
            &format!("/api/v1/rooms/{room_id}/messages"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(body["messages"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn messages_pagination_two_pages() {
        let db = test_db();
        let room_id = db.create_room(RoomKind::Channel, "lenta").expect("room");
        let router = test_app(db);
        let cookie = logged_in(&router, "alice").await;
        req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&cookie),
        )
        .await;

        let mut ids = Vec::new();
        for text in ["pervyi", "vtoroi", "tretii"] {
            let (status, body, _) = req(
                &router,
                "POST",
                &format!("/api/v1/rooms/{room_id}/messages"),
                Some(serde_json::json!({ "text": text })),
                Some(&cookie),
            )
            .await;
            assert_eq!(status, 201);
            ids.push(body["message"]["id"].as_i64().unwrap());
        }

        // Page 1 (limit=2): the latest 2 messages, ascending.
        let (status, body, _) = req(
            &router,
            "GET",
            &format!("/api/v1/rooms/{room_id}/messages?limit=2"),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 200);
        let page1: Vec<i64> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_i64().unwrap())
            .collect();
        assert_eq!(page1, vec![ids[1], ids[2]]);

        // Page 2 (cursor before the oldest of page 1): exactly the first one.
        let (status, body, _) = req(
            &router,
            "GET",
            &format!(
                "/api/v1/rooms/{room_id}/messages?limit=2&before_id={}",
                ids[1]
            ),
            None,
            Some(&cookie),
        )
        .await;
        assert_eq!(status, 200);
        let page2: Vec<i64> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_i64().unwrap())
            .collect();
        assert_eq!(page2, vec![ids[0]]);
    }

    #[tokio::test]
    async fn list_messages_rejects_bad_limit_and_cursor() {
        let db = test_db();
        let room_id = db.create_room(RoomKind::Channel, "obshchii").expect("room");
        let router = test_app(db);
        let cookie = logged_in(&router, "alice").await;
        req(
            &router,
            "POST",
            &format!("/api/v1/rooms/{room_id}/join"),
            None,
            Some(&cookie),
        )
        .await;

        for qs in ["limit=0", "limit=201", "before_id=0", "before_id=-1"] {
            let (status, _, _) = req(
                &router,
                "GET",
                &format!("/api/v1/rooms/{room_id}/messages?{qs}"),
                None,
                Some(&cookie),
            )
            .await;
            assert_eq!(status, 400, "query '{qs}' must be rejected");
        }
    }

    // ----- Wave 2.2: user catalog (GET /api/v1/users) -----

    #[tokio::test]
    async fn users_catalog_lists_all_registered_users_including_self() {
        let db = test_db();
        let router = test_app(db);
        let cookie = logged_in(&router, "alice").await;
        // A second registered user must appear in alice's catalog.
        let bob_cookie = logged_in(&router, "bob").await;

        let (status, body, _) = req(&router, "GET", "/api/v1/users", None, Some(&cookie)).await;
        assert_eq!(status, 200);
        let users = body["users"].as_array().unwrap();
        assert_eq!(users.len(), 2, "both registered users present: {body}");
        // display_name falls back to username; ordered by name (alice < bob).
        assert_eq!(users[0]["display_name"], "alice");
        assert!(users[0]["id"].is_i64());
        assert_eq!(users[1]["display_name"], "bob");

        // And bob sees the same catalog.
        let (status, body, _) = req(&router, "GET", "/api/v1/users", None, Some(&bob_cookie)).await;
        assert_eq!(status, 200);
        assert_eq!(body["users"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn users_catalog_requires_auth() {
        let router = test_app(test_db());
        let (status, body, _) = req(&router, "GET", "/api/v1/users", None, None).await;
        assert_eq!(status, 401);
        assert_eq!(body["error"], "not logged in");
    }
}
