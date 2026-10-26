//! Session-backed role gates for handlers.
//!
//! Wave 1.2 ships two levels (the `manager` tier is reserved for Wave 1.3's
//! room management and is not gated on yet):
//! - [`AuthedUser`] — any logged-in user (password or AD).
//! - [`AdminUser`] — logged in **and** `role == admin`.
//!
//! Both read the user id from the tower-session, then load the *current* row
//! from SQLite. That is deliberate: a role change or account deletion takes
//! effect on the next request instead of living on inside a stale cookie.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use besedka_core::{Role, User};
use tower_sessions::Session;

use super::{HttpError, SESSION_USER_ID};
use crate::AppState;

/// A request that must carry a valid session.
pub struct AuthedUser(pub User);

/// A request that must carry a session whose user is an admin.
pub struct AdminUser(pub User);

async fn session_user(parts: &mut Parts, state: &AppState) -> Result<User, Response> {
    let session = Session::from_request_parts(parts, state)
        .await
        .map_err(IntoResponse::into_response)?;

    let user_id: Option<i64> = session
        .get(SESSION_USER_ID)
        .await
        .map_err(|e| HttpError::internal(format!("failed to read session: {e}")).into_response())?;

    let Some(user_id) = user_id else {
        return Err(HttpError::unauthorized("not logged in").into_response());
    };

    state
        .db
        .get_user(user_id)
        .map_err(|e| HttpError::from(e).into_response())?
        // A session pointing at a deleted account is treated as logged out.
        .ok_or_else(|| HttpError::unauthorized("account no longer exists").into_response())
}

impl FromRequestParts<AppState> for AuthedUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self(session_user(parts, state).await?))
    }
}

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = session_user(parts, state).await?;
        if user.role == Role::Admin {
            Ok(Self(user))
        } else {
            // 403 (not 401): the caller IS authenticated, just not allowed.
            Err(HttpError::new(StatusCode::FORBIDDEN, "admin role required").into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;

    #[test]
    fn session_id_key_is_stable() {
        // Password login, both AD paths and these gates all agree on this key;
        // renaming it would silently log everybody out.
        assert_eq!(SESSION_USER_ID, "user_id");
    }

    #[tokio::test]
    async fn rejections_are_json_with_the_right_status() {
        // The gates reuse `HttpError`, so its wire shape is part of the
        // contract the UI depends on.
        let resp = HttpError::unauthorized("not logged in").into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["error"], "not logged in");

        // 403 (not 401) for an authenticated user lacking the role.
        let resp = HttpError::new(StatusCode::FORBIDDEN, "admin role required").into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn roles_distinguish_admin_from_manager() {
        // The gate is `role == Admin`: a manager is authenticated but not an
        // admin (that distinction is exercised end-to-end in lib.rs).
        assert_ne!(Role::Manager, Role::Admin);
        assert_eq!(Role::Admin, Role::Admin);
    }
}
