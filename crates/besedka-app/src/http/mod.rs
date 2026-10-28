//! HTTP handlers.
//!
//! Wave 0: `ping`, `rooms`. Wave 1.1: `register`, `login`, `logout`, `me` —
//! local password auth over `tower_sessions` (in-memory store, cookie session;
//! see lib.rs for the wiring). Wave 1.2: AD/Kerberos SSO ([`sso`]) and the
//! session-backed role gates ([`extract`]).
//! Wave 2.1: room creation (`POST /rooms`, admin) + message posting/reading
//! (`POST|GET /rooms/{id}/messages`, members only) — append-only messages.

pub mod extract;
pub mod sso;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use besedka_core::{Message, Room, RoomKind, User};
use besedka_infra::{DbError, RoomView};
use serde::Serialize;
use thiserror::Error;
use tower_sessions::Session;

use crate::http::extract::{AdminUser, AuthedUser};
use crate::AppState;

/// Key under which the authenticated user's id is stored in the session.
/// Shared by password login, both AD login paths and the role-gate extractors.
pub(crate) const SESSION_USER_ID: &str = "user_id";

// ----- Error type -----

/// An error returned to the client with a specific HTTP status code.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct HttpError {
    status: StatusCode,
    message: String,
}

impl HttpError {
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }
    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }
    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message)
    }
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
    /// "The feature exists but cannot serve this request right now" — used by
    /// the AD endpoints when AD is off, unconfigured or unreachable.
    pub(crate) fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
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
        match e {
            DbError::NotFound => Self::new(
                StatusCode::NOT_FOUND,
                "room not found",
            ),
            DbError::Forbidden => Self::forbidden("you are not a member of this room"),
            other => Self::internal(other.to_string()),
        }
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
    rooms: Vec<RoomView>,
}

#[derive(Debug, Serialize)]
pub struct RoomViewResponse {
    room: RoomView,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    user: User,
}

// ----- Wave 2.1: rooms (creation) + messages (append-only) -----

/// `kind` is `"room"` or `"channel"` (see `RoomKind`); `title` is 1–128
/// characters after trimming.
#[derive(Debug, serde::Deserialize)]
pub struct CreateRoomRequest {
    kind: String,
    title: String,
}

#[derive(Debug, Serialize)]
pub struct RoomResponse {
    room: Room,
}

/// `text` is 1–4096 characters after trimming.
#[derive(Debug, serde::Deserialize)]
pub struct PostMessageRequest {
    text: String,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    message: Message,
}

#[derive(Debug, Serialize)]
pub struct MessagesResponse {
    messages: Vec<Message>,
}

/// `GET /rooms/{id}/messages` query params:
/// - `limit` — page size, 1..=200 (default 50);
/// - `before_id` — cursor: return the page immediately before this message id
///   (oldest page of all when absent).
#[derive(Debug, serde::Deserialize)]
pub struct ListMessagesQuery {
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    before_id: Option<i64>,
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

/// `GET /api/v1/rooms` — the room list, each with the viewer's membership
/// state: `joined` (is the current user a member?) and `member_count`.
/// Anonymous viewers get `joined: false` everywhere.
pub async fn rooms(
    State(state): State<AppState>,
    session: Session,
) -> Result<Json<RoomsResponse>, HttpError> {
    let user_id: Option<i64> = session
        .get(SESSION_USER_ID)
        .await
        .map_err(|e| HttpError::internal(format!("failed to read session: {e}")))
        .ok()
        .flatten();
    let db = state.db.clone();
    let rooms = tokio::task::spawn_blocking(move || db.list_rooms_for_user(user_id))
        .await
        .map_err(|e| HttpError::internal(format!("background task failed: {e}")))??;
    Ok(Json(RoomsResponse { rooms }))
}

/// `POST /api/v1/rooms/{id}/join` — join the room (idempotent; 404 if the
/// room does not exist). Any authenticated user.
pub async fn join_room(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path(room_id): Path<i64>,
) -> Result<Json<RoomViewResponse>, HttpError> {
    let db = state.db.clone();
    let user_id = user.id;
    let room = tokio::task::spawn_blocking(move || -> Result<RoomView, DbError> {
        // Unknown room -> 404 via DbError::NotFound.
        db.room_view(room_id, user_id)?
            .ok_or(DbError::NotFound)?;
        // Idempotent: re-joining is a no-op.
        db.join_room(room_id, user_id)?;
        // Re-read AFTER the join so `joined`/`member_count` are current.
        db.room_view(room_id, user_id)?.ok_or(DbError::NotFound)
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))??;
    Ok(Json(RoomViewResponse { room }))
}

/// `POST /api/v1/rooms/{id}/leave` — leave the room; 404 if the room does
/// not exist or the user was never a member.
pub async fn leave_room(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path(room_id): Path<i64>,
) -> Result<Json<RoomViewResponse>, HttpError> {
    let db = state.db.clone();
    let user_id = user.id;
    let room = tokio::task::spawn_blocking(move || -> Result<RoomView, DbError> {
        // Unknown room -> 404 via DbError::NotFound.
        db.room_view(room_id, user_id)?
            .ok_or(DbError::NotFound)?;
        if !db.leave_room(room_id, user_id)? {
            return Err(DbError::NotFound); // was not a member
        }
        // Re-read AFTER the leave so `joined`/`member_count` are current.
        db.room_view(room_id, user_id)?.ok_or(DbError::NotFound)
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))??;
    Ok(Json(RoomViewResponse { room }))
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

/// `POST /api/v1/rooms` — create a room (admin only; non-admins get 403).
///
/// `kind` must be `room` or `channel` (400 otherwise), `title` must be 1–128
/// characters after trimming (400 otherwise). The creator is auto-joined:
/// membership is the gate for posting/reading, and the acceptance flow
/// «create channel → post → read» runs as a single user.
pub async fn create_room(
    State(state): State<AppState>,
    AdminUser(user): AdminUser,
    Json(payload): Json<CreateRoomRequest>,
) -> Result<(StatusCode, Json<RoomResponse>), HttpError> {
    let kind = match payload.kind.trim().to_lowercase().as_str() {
        "room" => RoomKind::Room,
        "channel" => RoomKind::Channel,
        other => {
            return Err(HttpError::bad_request(format!(
                "kind must be 'room' or 'channel' (got '{other}')"
            )))
        }
    };
    let title = payload.title.trim().to_string();
    if title.is_empty() || title.chars().count() > 128 {
        return Err(HttpError::bad_request("title must be 1–128 characters"));
    }

    let db = state.db.clone();
    let user_id = user.id;
    let room = tokio::task::spawn_blocking(move || {
        let id = db.create_room(kind, &title)?;
        // Creator becomes a member (idempotent join).
        db.join_room(id, user_id)?;
        db.get_room(id)?.ok_or(DbError::NotFound)
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))??;
    Ok((StatusCode::CREATED, Json(RoomResponse { room })))
}

/// `POST /api/v1/rooms/{id}/messages` — append a message (members only).
///
/// 404 if the room does not exist, 403 if the user is not a member, 400 if
/// `text` is empty or longer than 4096 characters after trimming.
pub async fn post_message(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path(room_id): Path<i64>,
    Json(payload): Json<PostMessageRequest>,
) -> Result<(StatusCode, Json<MessageResponse>), HttpError> {
    let text = payload.text.trim().to_string();
    if text.is_empty() {
        return Err(HttpError::bad_request("text must not be empty"));
    }
    if text.chars().count() > 4096 {
        return Err(HttpError::bad_request("text must be at most 4096 characters"));
    }

    let db = state.db.clone();
    let user_id = user.id;
    let message = tokio::task::spawn_blocking(move || {
        // Unknown room -> 404; non-member -> 403 (DbError::Forbidden).
        if db.get_room(room_id)?.is_none() {
            return Err(DbError::NotFound);
        }
        if !db.is_room_member(room_id, user_id)? {
            return Err(DbError::Forbidden);
        }
        let id = db.create_message(room_id, user_id, &text)?;
        db.get_message(id)?.ok_or(DbError::NotFound)
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))??;
    Ok((StatusCode::CREATED, Json(MessageResponse { message })))
}

/// `GET /api/v1/rooms/{id}/messages` — the latest page of a room's messages
/// (members only).
///
/// Query: `limit` (1..=200, default 50; 400 otherwise) and `before_id`
/// (cursor — the page immediately before this message id; absent = the
/// newest `limit`). The page comes back in ASCENDING order (oldest →
/// newest), ready to render top-to-bottom.
pub async fn list_messages(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path(room_id): Path<i64>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<MessagesResponse>, HttpError> {
    let limit = query.limit.unwrap_or(50);
    if limit == 0 || limit > 200 {
        return Err(HttpError::bad_request("limit must be between 1 and 200"));
    }
    if matches!(query.before_id, Some(id) if id <= 0) {
        return Err(HttpError::bad_request("before_id must be a positive message id"));
    }

    let db = state.db.clone();
    let user_id = user.id;
    let before_id = query.before_id;
    let messages = tokio::task::spawn_blocking(move || {
        // Unknown room -> 404; non-member -> 403 (DbError::Forbidden).
        if db.get_room(room_id)?.is_none() {
            return Err(DbError::NotFound);
        }
        if !db.is_room_member(room_id, user_id)? {
            return Err(DbError::Forbidden);
        }
        db.list_messages(room_id, limit, before_id)
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))??;
    Ok(Json(MessagesResponse { messages }))
}
