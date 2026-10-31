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
            DbError::NotFound => Self::new(StatusCode::NOT_FOUND, "room not found"),
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

// ----- Wave 2.2: user catalog (sender names in the UI) -----

/// One entry of the user catalog: public id + name.
#[derive(Debug, Serialize)]
pub struct UserDto {
    id: i64,
    display_name: String,
}

/// `GET /api/v1/users` response — all registered users. The UI maps
/// id → name for sender labels (and read receipts in 2.3).
#[derive(Debug, Serialize)]
pub struct UsersResponse {
    users: Vec<UserDto>,
}

/// `POST /api/v1/rooms/{id}/read` response — the room's newest message id
/// the caller has now read (Wave 2.3 read receipts).
#[derive(Debug, Serialize)]
pub struct ReadResponse {
    last_read_id: i64,
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

/// `PATCH .../messages/{message_id}` body — same `text` rules as a new
/// message (1–4096 chars after trimming).
#[derive(Debug, serde::Deserialize)]
pub struct EditMessageRequest {
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
        db.room_view(room_id, user_id)?.ok_or(DbError::NotFound)?;
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
        db.room_view(room_id, user_id)?.ok_or(DbError::NotFound)?;
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

/// `GET /api/v1/users` — the catalog of all users (any logged-in user).
///
/// `display_name` falls back to `username` when unset (the DB layer
/// coalesces it). The UI loads this once after login and maps
/// id → name for sender labels (Wave 2.2; reused for read receipts in 2.3).
pub async fn list_users(
    State(state): State<AppState>,
    AuthedUser(_user): AuthedUser,
) -> Result<Json<UsersResponse>, HttpError> {
    let db = state.db.clone();
    let rows = tokio::task::spawn_blocking(move || db.list_users())
        .await
        .map_err(|e| HttpError::internal(format!("background task failed: {e}")))?
        .map_err(HttpError::from)?;
    let users = rows
        .into_iter()
        .map(|(id, display_name)| UserDto { id, display_name })
        .collect();
    Ok(Json(UsersResponse { users }))
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
        return Err(HttpError::bad_request(
            "text must be at most 4096 characters",
        ));
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
        return Err(HttpError::bad_request(
            "before_id must be a positive message id",
        ));
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
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))?;
    Ok(Json(MessagesResponse {
        messages: messages?,
    }))
}

/// `PATCH /api/v1/rooms/{id}/messages/{message_id}` — edit the text of a
/// message the CURRENT USER wrote (authors only; anyone else gets 403).
///
/// 404 when the room or message does not exist, 403 for non-members and
/// non-authors, 400 when `text` is empty or longer than 4096 characters
/// after trimming (the same rules as `POST .../messages`). On success the
/// updated message is returned; `edited_at` (RFC 3339 UTC with millis) is
/// stamped by the DB on every edit and drives the «изменено» marker in the
/// UI. Repeated edits are allowed — `edited_at` always shows the last one.
pub async fn edit_message(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path((room_id, message_id)): Path<(i64, i64)>,
    Json(payload): Json<EditMessageRequest>,
) -> Result<Json<MessageResponse>, HttpError> {
    let text = payload.text.trim().to_string();
    if text.is_empty() {
        return Err(HttpError::bad_request("text must not be empty"));
    }
    if text.chars().count() > 4096 {
        return Err(HttpError::bad_request(
            "text must be at most 4096 characters",
        ));
    }

    let db = state.db.clone();
    let user_id = user.id;
    let message = tokio::task::spawn_blocking(move || -> Result<Message, HttpError> {
        // Unknown room -> 404; non-member -> 403 (DbError::Forbidden).
        if db.get_room(room_id)?.is_none() {
            return Err(DbError::NotFound.into());
        }
        if !db.is_room_member(room_id, user_id)? {
            return Err(DbError::Forbidden.into());
        }
        // Author-only: a member who did NOT write this message gets 403.
        let existing = db.get_message(message_id)?.ok_or(DbError::NotFound)?;
        if existing.sender_id != user_id {
            return Err(HttpError::forbidden("you can only edit your own messages"));
        }
        db.update_message_text(message_id, &text)?;
        db.get_message(message_id)?
            .ok_or_else(|| HttpError::internal("message vanished"))
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))?;
    Ok(Json(MessageResponse { message: message? }))
}

/// `DELETE /api/v1/rooms/{id}/messages/{message_id}` — delete a message the
/// CURRENT USER wrote (authors only). Returns 204 No Content on success.
///
/// 404 when the room or message does not exist, 403 for non-members and
/// non-authors. The row is removed (hard delete); the UI drops the message
/// from its list and refetches to stay consistent.
pub async fn delete_message(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path((room_id, message_id)): Path<(i64, i64)>,
) -> Result<StatusCode, HttpError> {
    let db = state.db.clone();
    let user_id = user.id;
    let deleted = tokio::task::spawn_blocking(move || -> Result<(), HttpError> {
        // Unknown room -> 404; non-member -> 403 (DbError::Forbidden).
        if db.get_room(room_id)?.is_none() {
            return Err(DbError::NotFound.into());
        }
        if !db.is_room_member(room_id, user_id)? {
            return Err(DbError::Forbidden.into());
        }
        // Author-only: a member who did NOT write this message gets 403.
        let existing = db.get_message(message_id)?.ok_or(DbError::NotFound)?;
        if existing.sender_id != user_id {
            return Err(HttpError::forbidden(
                "you can only delete your own messages",
            ));
        }
        db.delete_message(message_id)?;
        Ok(())
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))?;
    deleted?;
    Ok(StatusCode::NO_CONTENT)
}

// ----- Wave 2.3: read receipts (room read cursors) -----

/// `POST /api/v1/rooms/{id}/read` — mark the room as read (members only).
///
/// Sets the caller's cursor to the room's newest message id and returns it:
/// `200 {"last_read_id": N}`. For an empty room the cursor is left as-is and
/// the response carries the current cursor (0 when never set).
pub async fn mark_read(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path(room_id): Path<i64>,
) -> Result<Json<ReadResponse>, HttpError> {
    let db = state.db.clone();
    let user_id = user.id;
    let inner = tokio::task::spawn_blocking(move || -> Result<i64, HttpError> {
        // Unknown room -> 404; non-member -> 403.
        if db.get_room(room_id)?.is_none() {
            return Err(DbError::NotFound.into());
        }
        if !db.is_room_member(room_id, user_id)? {
            return Err(DbError::Forbidden.into());
        }
        // Newest message id, or the caller's existing cursor for an empty
        // room (0 when they never read it) — the UPSERT never regresses.
        let target = match db.latest_message_id(room_id)? {
            Some(id) => id,
            None => db.get_read_cursor(room_id, user_id)?.unwrap_or(0),
        };
        db.set_read_cursor(room_id, user_id, target)?;
        Ok(target)
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))?;
    let last_read_id = inner?;
    Ok(Json(ReadResponse { last_read_id }))
}

/// `GET /api/v1/rooms/{id}/messages/{message_id}/readers` — who has read a
/// message (members only).
///
/// Room members whose read cursor is at or past the message id, as
/// `{"users":[{"id","display_name"},...]}` (same shape as `/api/v1/users`).
pub async fn message_readers(
    State(state): State<AppState>,
    AuthedUser(user): AuthedUser,
    Path((room_id, message_id)): Path<(i64, i64)>,
) -> Result<Json<UsersResponse>, HttpError> {
    let db = state.db.clone();
    let user_id = user.id;
    let inner = tokio::task::spawn_blocking(move || -> Result<Vec<(i64, String)>, HttpError> {
        // Unknown room -> 404; non-member -> 403 (checked before the message
        // so a stranger can't probe message ids across rooms).
        if db.get_room(room_id)?.is_none() {
            return Err(DbError::NotFound.into());
        }
        if !db.is_room_member(room_id, user_id)? {
            return Err(DbError::Forbidden.into());
        }
        if db.get_message(message_id)?.is_none() {
            return Err(DbError::NotFound.into());
        }
        Ok(db.message_readers(room_id, message_id)?)
    })
    .await
    .map_err(|e| HttpError::internal(format!("background task failed: {e}")))?;
    let readers = inner?;
    Ok(Json(UsersResponse {
        users: readers
            .into_iter()
            .map(|(id, display_name)| UserDto { id, display_name })
            .collect(),
    }))
}
