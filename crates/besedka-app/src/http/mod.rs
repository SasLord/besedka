//! HTTP handlers.
//!
//! Wave 0: `ping`, `rooms`. Wave 1.1: `register`, `login`, `logout`, `me` —
//! local password auth over `tower_sessions` (in-memory store, cookie session;
//! see lib.rs for the wiring and the SSO note).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use besedka_core::User;
use besedka_infra::DbError;
use serde::Serialize;
use thiserror::Error;
use tower_sessions::Session;

use crate::AppState;

/// Key under which the authenticated user's id is stored in the session.
const SESSION_USER_ID: &str = "user_id";

// ----- Error type -----

/// An error returned to the client with a specific HTTP status code.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct HttpError {
    status: StatusCode,
    message: String,
}

impl HttpError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
    fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }
    fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }
    fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = self.status;
        let message = self.message;
        tracing::warn!(%status, %message, "request failed");
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

impl From<DbError> for HttpError {
    fn from(e: DbError) -> Self {
        Self::internal(e.to_string())
    }
}

// ----- Request / response types -----

#[derive(Debug, Serialize)]
pub struct PingResponse {
    status: &'static str,
    app: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
pub struct RoomsResponse {
    rooms: Vec<besedka_core::Room>,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    user: User,
}

#[derive(Debug, Serialize)]
pub struct LogoutResponse {
    status: &'static str,
}

#[derive(Debug, serde::Deserialize)]
pub struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

// ----- Validation -----

/// Trim, lowercase, and validate the username (3–64 chars: letters, digits,
/// `.`, `_`, `-`).
fn normalize_username(raw: &str) -> Result<String, HttpError> {
    let username = raw.trim().to_lowercase();
    let ok = (3..=64).contains(&username.len())
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if ok {
        Ok(username)
    } else {
        Err(HttpError::bad_request(
            "username must be 3–64 chars of letters, digits, '.', '_' or '-'",
        ))
    }
}

fn validate_password(password: &str) -> Result<(), HttpError> {
    if (8..=128).contains(&password.len()) {
        Ok(())
    } else {
        Err(HttpError::bad_request("password must be 8–128 characters"))
    }
}

// ----- Handlers -----

/// `GET /api/v1/ping` — liveness + version.
pub async fn ping() -> Json<PingResponse> {
    Json(PingResponse {
        status: "ok",
        app: "besedka",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// `GET /api/v1/rooms` — the room list (Wave 0: whatever is in the DB).
pub async fn rooms(State(state): State<AppState>) -> Result<Json<RoomsResponse>, HttpError> {
    let db = state.db.clone();
    let rooms = tokio::task::spawn_blocking(move || db.list_rooms())
        .await
        .map_err(|e| HttpError::internal(format!("background task failed: {e}")))?
        .map_err(HttpError::from)?;
    Ok(Json(RoomsResponse { rooms }))
}

/// `POST /api/v1/register` — create a local account (role = user).
pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<UserResponse>), HttpError> {
    let username = normalize_username(&payload.username)?;
    let password = payload.password;
    validate_password(&password)?;

    let db = state.db.clone();
    let inner: Result<User, HttpError> = tokio::task::spawn_blocking(move || {
        if db.find_user_by_username(&username)?.is_some() {
            return Err(HttpError::conflict(format!(
                "username '{username}' is already taken"
            )));
        }
        let hash = besedka_core::hash(&password)
            .map_err(|e| HttpError::internal(format!("failed to hash password: {e}")))?;
        let id = db.create_user(&username, None, besedka_core::Role::User, Some(&hash))?;
        db.get_user(id)?
            .ok_or_else(|| HttpError::internal("newly created user not found"))
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))?;
    let user = inner?;
    Ok((StatusCode::CREATED, Json(UserResponse { user })))
}

/// `POST /api/v1/login` — verify the local password and open a session.
///
/// "No such user" and "wrong password" return the same message, so the API
/// does not leak which usernames exist.
pub async fn login(
    State(state): State<AppState>,
    session: Session,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<UserResponse>, HttpError> {
    let username = normalize_username(&payload.username)?;
    let password = payload.password;
    validate_password(&password)?;

    let db = state.db.clone();
    let inner: Result<User, HttpError> = tokio::task::spawn_blocking(move || {
        let stored = db
            .find_user_by_username(&username)?
            .ok_or_else(|| HttpError::unauthorized("invalid username or password"))?;
        let hash = stored
            .password_hash
            .ok_or_else(|| HttpError::unauthorized("invalid username or password"))?;
        if !besedka_core::verify(&password, &hash) {
            return Err(HttpError::unauthorized("invalid username or password"));
        }
        db.get_user(stored.id)?
            .ok_or_else(|| HttpError::internal("user vanished after login"))
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))?;
    let user = inner?;

    session
        .insert(SESSION_USER_ID, user.id)
        .await
        .map_err(|e| HttpError::internal(format!("failed to write session: {e}")))?;

    Ok(Json(UserResponse { user }))
}

/// `POST /api/v1/logout` — clear and delete the session; the session manager
/// then removes the cookie.
pub async fn logout(session: Session) -> Result<Json<LogoutResponse>, HttpError> {
    session
        .flush()
        .await
        .map_err(|e| HttpError::internal(format!("failed to clear session: {e}")))?;
    Ok(Json(LogoutResponse { status: "ok" }))
}

/// `GET /api/v1/me` — the currently authenticated user (401 if none).
pub async fn me(
    State(state): State<AppState>,
    session: Session,
) -> Result<Json<UserResponse>, HttpError> {
    let user_id: Option<i64> = session
        .get(SESSION_USER_ID)
        .await
        .map_err(|e| HttpError::internal(format!("failed to read session: {e}")))?;
    let Some(user_id) = user_id else {
        return Err(HttpError::unauthorized("not logged in"));
    };
    let user = state
        .db
        .get_user(user_id)?
        .ok_or_else(|| HttpError::unauthorized("account no longer exists"))?;
    Ok(Json(UserResponse { user }))
}
