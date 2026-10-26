//! AD/Kerberos SSO endpoints (ported from trackly `http/sso.rs`, adapted to
//! Besedka's session model and roles).
//!
//! Endpoints:
//! - `GET  /api/v1/auth_ad_sso`        — passwordless login. Server side of the
//!   browser `Negotiate` handshake: no `Authorization` header ⇒ `401` +
//!   `WWW-Authenticate: Negotiate`; a domain-joined browser then transparently
//!   resends the request with a Kerberos ticket. The ticket is validated
//!   locally against the service keytab (no KDC call), the AD account is
//!   provisioned/refreshed in SQLite, and the SAME session cookie as password
//!   login is issued.
//! - `POST /api/v1/auth_ad_login`       — AD password login (LDAP bind), for
//!   clients that are not domain-joined (desktop app on a personal laptop, web
//!   client over the LAN).
//! - `GET  /api/v1/auth_ad_sso/status`  — public feature probe for the UI.
//! - `GET  /api/v1/admin/ad_status`     — admin-only diagnostics ("проверить
//!   подключение"), gated by [`AdminUser`].
//!
//! MOCK MODE (`[ad] mock = true` / `BESEDKA_AD_MOCK=1`): no DC, no keytab, no
//! ticket. `GET /api/v1/auth_ad_sso` then authenticates a fixture identity
//! (`us100` by default, overridable with `?login=` or `X-Besedka-Ad-Login`),
//! so the entire flow — provisioning, role mapping, session cookie — is
//! exercisable on a dev machine. Logins starting with `deny` are refused, so
//! tests can exercise the rejection path.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::Engine;
use besedka_core::{DirectoryResult, Role, User};
use besedka_infra::ad::{accept_spnego, discover_dc, normalize_login, DiscoveryResult, SsoOutcome};
use besedka_infra::Db;
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use super::extract::AdminUser;
use super::{HttpError, SESSION_USER_ID};
use crate::ad::AdRuntime;
use crate::AppState;

/// Header a dev/test client may use to pick a mock identity.
const MOCK_LOGIN_HEADER: &str = "x-besedka-ad-login";
const NEGOTIATE_PREFIX: &str = "Negotiate ";

fn b64() -> base64::engine::general_purpose::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

// ----- Response bodies -----

#[derive(Debug, Serialize)]
pub struct AdLoginResponse {
    ok: bool,
    mock: bool,
    user: User,
}

#[derive(Debug, Serialize)]
pub struct AdFeatureStatus {
    /// AD is switched on at all.
    enabled: bool,
    /// Deterministic mock adapters are serving this process.
    mock: bool,
    /// Passwordless (Kerberos) login can serve requests right now.
    sso: bool,
    /// AD password login (LDAP bind) can serve requests right now.
    password_login: bool,
}

#[derive(Debug, Serialize)]
pub struct AdminAdStatus {
    enabled: bool,
    mock: bool,
    sso: bool,
    password_login: bool,
    domain: String,
    /// SPN is configured (real Kerberos mode only).
    spn_configured: bool,
    /// Path to a keytab is configured (real Kerberos mode only).
    keytab_configured: bool,
    /// Read-only service bind is configured (SSO enrichment possible).
    directory_configured: bool,
    /// DC host: from config, else DNS-SRV discovery, else `null`.
    resolved_host: Option<String>,
    /// `ok` / `unreachable` / `unknown` — live reachability probe.
    reachable: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct AdLoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
pub struct SsoQuery {
    /// Mock-only: which fixture identity to log in as.
    login: Option<String>,
}

// ----- Provisioning -----

/// Turn an AD-proven login into a Besedka username, or explain why not.
fn ad_username(login: &str) -> Result<String, HttpError> {
    let username = normalize_login(login);
    if (3..=64).contains(&username.len()) {
        Ok(username)
    } else {
        Err(HttpError::bad_request(
            "AD login must be 3–64 characters (letters, digits, '.', '_', '-')",
        ))
    }
}

/// A display name we are allowed to persist: present, non-empty, and not just
/// the bare login echoed back (that is what an unknown-to-the-directory lookup
/// falls back to). Only ever taken from a *directory-resolved* result, never
/// from a degraded path.
fn trusted_display_name(username: &str, resolved: Option<&DirectoryResult>) -> Option<String> {
    resolved
        .map(|r| r.display_name.trim().to_string())
        .filter(|name| !name.is_empty() && name != username)
}

/// Find or create the account behind an AD login.
///
/// - First login: created with `password_hash = NULL` (a domain account has no
///   local password) and the role mapped from AD groups, defaulting to `user`.
/// - Later logins: the stored role is left alone (role changes are an admin
///   action), and the ФИО is refreshed only from a trusted directory result
///   that actually differs.
fn provision(
    db: &Db,
    username: &str,
    resolved: Option<&DirectoryResult>,
) -> Result<User, HttpError> {
    let display_name = trusted_display_name(username, resolved);

    if let Some(stored) = db.find_user_by_username(username)? {
        if let Some(name) = &display_name {
            if stored.display_name.as_deref() != Some(name.as_str()) {
                db.update_user_display_name(stored.id, name)?;
            }
        }
        return db
            .get_user(stored.id)?
            .ok_or_else(|| HttpError::internal("user vanished during AD login"));
    }

    let role = resolved.and_then(|r| r.role).unwrap_or(Role::User);
    let id = db.create_user(username, display_name.as_deref(), role, None)?;
    db.get_user(id)?
        .ok_or_else(|| HttpError::internal("newly created AD user not found"))
}

/// Open a fresh session for `user_id`.
///
/// `flush` first: whatever session the browser carried before the login is
/// discarded, so a pre-login session id can never be reused (fixation).
async fn start_session(session: &Session, user_id: i64) -> Result<(), HttpError> {
    session
        .flush()
        .await
        .map_err(|e| HttpError::internal(format!("failed to reset session: {e}")))?;
    session
        .insert(SESSION_USER_ID, user_id)
        .await
        .map_err(|e| HttpError::internal(format!("failed to write session: {e}")))
}

/// Blocking DB work off the async runtime.
async fn provision_off_thread(
    db: Arc<Db>,
    username: String,
    resolved: Option<DirectoryResult>,
) -> Result<User, HttpError> {
    tokio::task::spawn_blocking(move || provision(&db, &username, resolved.as_ref()))
        .await
        .map_err(|e| HttpError::internal(format!("background task failed: {e}")))?
}

/// Resolve display name + role, degrading to `None` when enrichment is simply
/// not configured (SSO must keep working without it).
async fn enrich(ad: &AdRuntime, login: &str) -> Result<Option<DirectoryResult>, HttpError> {
    match ad.directory.resolve(login).await {
        Ok(resolved) => Ok(Some(resolved)),
        Err(besedka_core::DirectoryError::NotConfigured) => Ok(None),
        Err(besedka_core::DirectoryError::Unreachable) => Err(HttpError::service_unavailable(
            "AD directory is unreachable; cannot map the domain account",
        )),
        Err(besedka_core::DirectoryError::ServiceBindFailed) => {
            Err(HttpError::service_unavailable(
                "AD service account bind failed (check bind_dn/bind password)",
            ))
        }
    }
}

// ----- Negotiate helpers -----

/// Build the `WWW-Authenticate: Negotiate` challenge (with an optional base64
/// continuation token).
fn negotiate_challenge(reply: Option<&[u8]>) -> Response {
    let header_value = match reply {
        Some(r) if !r.is_empty() => format!("Negotiate {}", b64().encode(r)),
        _ => "Negotiate".to_string(),
    };
    let mut resp = HttpError::unauthorized("Kerberos authentication required").into_response();
    if let Ok(hv) = HeaderValue::from_str(&header_value) {
        resp.headers_mut().insert(header::WWW_AUTHENTICATE, hv);
    }
    resp
}

/// Extract the raw (still base64) token from `Authorization: Negotiate <tok>`.
fn negotiate_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    if value.len() > NEGOTIATE_PREFIX.len()
        && value[..NEGOTIATE_PREFIX.len()].eq_ignore_ascii_case(NEGOTIATE_PREFIX)
    {
        Some(value[NEGOTIATE_PREFIX.len()..].trim())
    } else {
        None
    }
}

/// Dev-only rejection hook: in mock mode a login starting with `deny` is
/// refused, so the 401 path is reachable without a domain controller.
fn mock_denied(login: &str) -> bool {
    normalize_login(login).starts_with("deny")
}

// ----- Handlers -----

/// `GET /api/v1/auth_ad_sso` — passwordless AD (Kerberos) login.
pub async fn ad_sso(
    State(state): State<AppState>,
    session: Session,
    headers: HeaderMap,
    Query(query): Query<SsoQuery>,
) -> Response {
    let ad = &state.ad;

    if !ad.enabled() {
        return HttpError::service_unavailable("AD login is disabled on this server")
            .into_response();
    }
    if !ad.config.sso_ready() {
        return HttpError::service_unavailable(
            "Kerberos SSO is not configured (spn/keytab missing)",
        )
        .into_response();
    }

    // ── Mock mode: no ticket, no keytab — pick a fixture identity instead.
    if ad.mock {
        let login = query
            .login
            .or_else(|| {
                headers
                    .get(MOCK_LOGIN_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| besedka_infra::ad::mock::DEFAULT_SSO_LOGIN.to_string());

        if mock_denied(&login) {
            return HttpError::unauthorized("mock AD rejected this identity").into_response();
        }

        let resolved = match enrich(ad, &login).await {
            Ok(r) => r,
            Err(e) => return e.into_response(),
        };
        let username = match ad_username(&login) {
            Ok(u) => u,
            Err(e) => return e.into_response(),
        };
        return match provision_off_thread(state.db.clone(), username, resolved).await {
            Ok(user) => match start_session(&session, user.id).await {
                Ok(()) => Json(AdLoginResponse {
                    ok: true,
                    mock: true,
                    user,
                })
                .into_response(),
                Err(e) => e.into_response(),
            },
            Err(e) => e.into_response(),
        };
    }

    // ── Real mode: Negotiate handshake.
    let Some(token_b64) = negotiate_token(&headers) else {
        // No (or foreign) auth scheme → challenge; the browser retries with a
        // Kerberos ticket.
        return negotiate_challenge(None);
    };

    let token = match b64().decode(token_b64) {
        Ok(t) => t,
        Err(_) => return HttpError::bad_request("malformed Negotiate token").into_response(),
    };

    // Read the service keytab. Bytes never logged; missing file ⇒ 503.
    let keytab_path = ad.config.keytab_path.clone();
    let keytab_bytes = match std::fs::read(&keytab_path) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("AD SSO: cannot read keytab {keytab_path}: {e}");
            return HttpError::service_unavailable("keytab is not readable on this server")
                .into_response();
        }
    };

    let spn = ad.config.spn.clone();
    let computer = ad.config.resolved_computer_name();
    // The accept is synchronous crypto — keep it off the async workers.
    let outcome =
        tokio::task::spawn_blocking(move || accept_spnego(&spn, &keytab_bytes, &computer, &token))
            .await;

    let outcome = match outcome {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            // Never leak internals (bad SPN, keytab mismatch, crypto detail).
            tracing::warn!("AD SSO accept failed: {e}");
            return HttpError::unauthorized("could not verify the Kerberos ticket").into_response();
        }
        Err(e) => {
            return HttpError::internal(format!("SSO worker failed: {e}")).into_response();
        }
    };

    match outcome {
        SsoOutcome::Authenticated {
            username: ad_login,
            reply_token,
        } => {
            let resolved = match enrich(ad, &ad_login).await {
                Ok(r) => r,
                Err(e) => return e.into_response(),
            };
            let username = match ad_username(&ad_login) {
                Ok(u) => u,
                Err(e) => return e.into_response(),
            };
            match provision_off_thread(state.db.clone(), username, resolved).await {
                Ok(user) => match start_session(&session, user.id).await {
                    Ok(()) => {
                        let mut resp = Json(AdLoginResponse {
                            ok: true,
                            mock: false,
                            user,
                        })
                        .into_response();
                        if !reply_token.is_empty() {
                            if let Ok(hv) = HeaderValue::from_str(&format!(
                                "Negotiate {}",
                                b64().encode(&reply_token)
                            )) {
                                resp.headers_mut().insert(header::WWW_AUTHENTICATE, hv);
                            }
                        }
                        resp
                    }
                    Err(e) => e.into_response(),
                },
                Err(e) => e.into_response(),
            }
        }
        // Kerberos normally completes in one step; a continuation just
        // re-challenges with the reply token.
        SsoOutcome::Continue { reply_token } => negotiate_challenge(Some(&reply_token)),
        SsoOutcome::Denied => {
            HttpError::unauthorized("could not verify the Kerberos ticket").into_response()
        }
    }
}

/// `POST /api/v1/auth_ad_login` — AD password login (LDAP simple bind).
///
/// Wrong password and unknown user answer identically (no enumeration); an
/// unreachable DC is a 503 so the UI can say "AD недоступен" instead of
/// "wrong password".
pub async fn ad_login(
    State(state): State<AppState>,
    session: Session,
    Json(payload): Json<AdLoginRequest>,
) -> Result<Json<AdLoginResponse>, HttpError> {
    let ad = &state.ad;
    if !ad.password_login_available() {
        return Err(HttpError::service_unavailable(
            "AD password login is not available on this server",
        ));
    }

    match ad
        .client
        .authenticate(&payload.username, &payload.password)
        .await
    {
        besedka_core::AuthOutcome::Ok { display_name } => {
            // Enrichment is best-effort here: the bind already proved identity.
            let resolved = enrich(ad, &payload.username).await?.or({
                Some(DirectoryResult {
                    display_name,
                    role: None,
                })
            });
            let username = ad_username(&payload.username)?;
            let user = provision_off_thread(state.db.clone(), username, resolved).await?;
            start_session(&session, user.id).await?;
            Ok(Json(AdLoginResponse {
                ok: true,
                mock: ad.mock,
                user,
            }))
        }
        besedka_core::AuthOutcome::BadCreds => {
            Err(HttpError::unauthorized("invalid username or password"))
        }
        besedka_core::AuthOutcome::Unreachable => Err(HttpError::service_unavailable(
            "AD is unreachable right now",
        )),
    }
}

/// `GET /api/v1/auth_ad_sso/status` — public feature probe (no secrets).
pub async fn ad_feature_status(State(state): State<AppState>) -> Json<AdFeatureStatus> {
    let ad = &state.ad;
    Json(AdFeatureStatus {
        enabled: ad.enabled(),
        mock: ad.mock,
        sso: ad.sso_available(),
        password_login: ad.password_login_available(),
    })
}

/// `GET /api/v1/admin/ad_status` — admin diagnostics.
pub async fn admin_ad_status(
    State(state): State<AppState>,
    AdminUser(_admin): AdminUser,
) -> Json<AdminAdStatus> {
    let ad = &state.ad;
    let cfg = &ad.config;

    // Live reachability probe (bounded by a connect timeout in transport.rs).
    let reachable = match ad.client.test_connection().await {
        besedka_core::AuthOutcome::Ok { .. } => "ok",
        besedka_core::AuthOutcome::Unreachable => "unreachable",
        besedka_core::AuthOutcome::BadCreds => "unknown",
    };

    // Host from config, else DNS SRV discovery, else nothing. Mock mode never
    // touches the network.
    let resolved_host = if !cfg.host.trim().is_empty() {
        Some(cfg.host.clone())
    } else if !ad.mock && !cfg.domain.trim().is_empty() {
        match discover_dc(&cfg.domain).await {
            DiscoveryResult::Found { dc_host } => Some(dc_host),
            DiscoveryResult::NoDomainDetected => None,
        }
    } else {
        None
    };

    Json(AdminAdStatus {
        enabled: ad.enabled(),
        mock: ad.mock,
        sso: ad.sso_available(),
        password_login: ad.password_login_available(),
        domain: cfg.domain.clone(),
        spn_configured: !cfg.spn.trim().is_empty(),
        keytab_configured: !cfg.keytab_path.trim().is_empty(),
        directory_configured: cfg.directory_configured(),
        resolved_host,
        reachable,
    })
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;

    #[test]
    fn ad_username_normalizes_and_validates() {
        assert_eq!(ad_username("US100@corp.local").unwrap(), "us100");
        assert_eq!(ad_username(r"CORP\us100").unwrap(), "us100");
        assert_eq!(ad_username("us1").unwrap(), "us1", "3 chars is the minimum");
        assert!(ad_username("us").is_err(), "too short");
        assert!(ad_username("").is_err(), "empty");
        assert!(ad_username(&"a".repeat(65)).is_err(), "too long");
    }

    #[test]
    fn trusted_display_name_rejects_login_echo_and_blanks() {
        let echo = DirectoryResult {
            display_name: "us100".to_string(),
            role: None,
        };
        assert_eq!(trusted_display_name("us100", Some(&echo)), None);

        let blank = DirectoryResult {
            display_name: "   ".to_string(),
            role: None,
        };
        assert_eq!(trusted_display_name("us100", Some(&blank)), None);

        assert_eq!(trusted_display_name("us100", None), None);

        let real = DirectoryResult {
            display_name: " Иванов Иван Иванович ".to_string(),
            role: None,
        };
        assert_eq!(
            trusted_display_name("us100", Some(&real)).as_deref(),
            Some("Иванов Иван Иванович")
        );
    }

    #[test]
    fn negotiate_token_parses_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("negotiate dG9rZW4="),
        );
        assert_eq!(negotiate_token(&headers), Some("dG9rZW4="));
    }

    #[test]
    fn negotiate_token_ignores_other_schemes() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc"),
        );
        assert_eq!(negotiate_token(&headers), None);
        assert_eq!(negotiate_token(&HeaderMap::new()), None);
    }

    #[test]
    fn challenge_header_is_bare_negotiate_without_reply() {
        let resp = negotiate_challenge(None);
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            resp.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Negotiate"
        );
    }

    #[test]
    fn challenge_header_carries_base64_reply_token() {
        let resp = negotiate_challenge(Some(&[1u8, 2, 3]));
        let value = resp
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(value, format!("Negotiate {}", b64().encode([1u8, 2, 3])));
    }

    #[test]
    fn mock_denied_hook_only_matches_deny_prefixes() {
        assert!(mock_denied("deny-me"));
        assert!(mock_denied("DENY_1"));
        assert!(!mock_denied("us100"));
    }
}
