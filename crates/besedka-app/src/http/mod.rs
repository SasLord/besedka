//! HTTP handlers (Wave 0: `ping`, `rooms`).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use thiserror::Error;

use crate::AppState;

/// Error surfaced to the client as a JSON body.
#[derive(Debug, Error)]
pub enum HttpError {
    #[error(transparent)]
    Db(#[from] besedka_infra::DbError),
    #[error("background task failed: {0}")]
    Join(String),
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        tracing::warn!(error = %self, "request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": self.to_string() })),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
pub struct PingResponse {
    status: &'static str,
    app: &'static str,
    version: &'static str,
}

/// `GET /api/v1/ping` — liveness + version.
pub async fn ping() -> Json<PingResponse> {
    Json(PingResponse {
        status: "ok",
        app: "besedka",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Debug, Serialize)]
pub struct RoomsResponse {
    rooms: Vec<besedka_core::Room>,
}

/// `GET /api/v1/rooms` — the room list (Wave 0: whatever is in the DB).
pub async fn rooms(
    State(state): State<AppState>,
) -> Result<Json<RoomsResponse>, HttpError> {
    let db = state.db.clone();
    let rooms = tokio::task::spawn_blocking(move || db.list_rooms())
        .await
        .map_err(|e| HttpError::Join(e.to_string()))?
        .map_err(HttpError::Db)?;
    Ok(Json(RoomsResponse { rooms }))
}
