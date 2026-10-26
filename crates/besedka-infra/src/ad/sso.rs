//! Server-side SPNEGO/Negotiate (Kerberos) acceptor via `sspi`.
//!
//! Ported from trackly `crates/trackly-infra/src/ad/sso.rs` (spike 002).
//! Validates a browser's `Authorization: Negotiate <token>` against the
//! service's keytab-derived key and, on success, returns the authenticated AD
//! account name. This is the Rust counterpart of adwebapp's gokrb5
//! `/auth/ad` acceptor.
//!
//! ⚠️ BUILD-VERIFIED, NOT LIVE-VERIFIED. This compiles and links, but the real
//! Kerberos handshake can only be exercised against a Domain Controller. The
//! two spots whose *runtime* behavior is unproven until then:
//!   1. `acquire_credentials_handle` for a Negotiate *server* (the service key
//!      is supplied via `ServerProperties`, so no `with_auth_data`).
//!   2. `resolve_with_client(&mut net)` — the accept API takes a
//!      `NetworkClient`. For an offline service-ticket decrypt (keytab key
//!      present, no U2U) it should never touch the network;
//!      [`OfflineNetworkClient`] errors loudly if it ever is called. If it turns
//!      out the network IS needed, enable sspi's `network_client` feature.
//!
//! Keytab reading lives in [`super::keytab`] and IS fully unit-tested
//! (deterministic parsing, no network).

use std::time::Duration;

use sspi::kerberos::ServerProperties;
use sspi::network_client::NetworkClient;
use sspi::{
    BufferType, CredentialUse, DataRepresentation, KerberosConfig, KerberosServerConfig, Negotiate,
    NegotiateConfig, NetworkRequest, SecurityBuffer, SecurityStatus, ServerRequestFlags, Sspi,
    SspiImpl,
};

/// Clock-skew tolerance for ticket timestamp validation. Kerberos' customary
/// allowance is 5 minutes.
const MAX_TIME_SKEW: Duration = Duration::from_secs(5 * 60);

/// Outcome of one SPNEGO accept step.
#[derive(Debug)]
pub enum SsoOutcome {
    /// Handshake complete — `username` is the authenticated AD account
    /// (SAM/UPN as sspi reports it). `reply_token` (possibly empty) must be
    /// returned to the client in `WWW-Authenticate: Negotiate <base64>`.
    Authenticated {
        username: String,
        reply_token: Vec<u8>,
    },
    /// Needs another round trip — send `reply_token` back with a 401 and expect
    /// a follow-up token. (Kerberos usually completes in one step.)
    Continue { reply_token: Vec<u8> },
    /// Token rejected (bad/foreign ticket, wrong SPN, expired). Generic on
    /// purpose.
    Denied,
}

/// Local error type — keeps `sspi` out of the `besedka-core` port surface.
/// Never carries key material.
#[derive(Debug, thiserror::Error)]
pub enum SsoError {
    #[error("SSO server not configured (keytab/SPN missing)")]
    NotConfigured,
    #[error("keytab: {0}")]
    Keytab(#[from] super::keytab::KeytabError),
    #[error("sspi negotiate accept failed: {0}")]
    Sspi(String),
}

/// A `NetworkClient` that refuses to do any network I/O. For an offline
/// service-ticket decrypt the acceptor must not contact the KDC; if it ever
/// tries, this returns an error rather than silently reaching out.
struct OfflineNetworkClient;

impl NetworkClient for OfflineNetworkClient {
    fn send(&self, _request: &NetworkRequest) -> sspi::Result<Vec<u8>> {
        Err(sspi::Error::new(
            sspi::ErrorKind::NoAuthenticatingAuthority,
            "AD SSO acceptor attempted a network/KDC call, but only offline keytab \
             validation is configured",
        ))
    }
}

/// Accept one SPNEGO/Negotiate token.
///
/// * `spn` — service principal in `SERVICE/host` form, e.g.
///   `HTTP/chat.example.local` (realm-agnostic; must match the SPN the keytab
///   was generated for and the name in the browser's address bar).
/// * `keytab_bytes` — raw contents of the `.keytab` file.
/// * `client_computer_name` — this server's machine name (diagnostic only).
/// * `input_token` — raw (already base64-decoded) bytes from
///   `Authorization: Negotiate …`.
///
/// CPU-bound (crypto) and fully synchronous — callers should run it on a
/// blocking thread pool.
pub fn accept_spnego(
    spn: &str,
    keytab_bytes: &[u8],
    client_computer_name: &str,
    input_token: &[u8],
) -> Result<SsoOutcome, SsoError> {
    if spn.is_empty() || keytab_bytes.is_empty() {
        return Err(SsoError::NotConfigured);
    }

    // Pull the AES256 service key for this SPN out of the keytab.
    let service_key = super::keytab::aes256_key_for_spn(keytab_bytes, spn)?;

    // SPN "HTTP/chat.example.local" → sname components ["HTTP", "chat.example.local"].
    let sname: Vec<&str> = spn.split('/').collect();

    // No KDC URL — the ticket is decrypted locally (offline).
    let kerberos_config = KerberosConfig::new("", client_computer_name.to_string());
    let server_properties = ServerProperties::new(
        &sname,
        None, // no bound user credentials — pure acceptor
        MAX_TIME_SKEW,
        Some(service_key.into()),
    )
    .map_err(|e| SsoError::Sspi(e.to_string()))?;
    let server_config = KerberosServerConfig {
        kerberos_config,
        server_properties,
    };

    let negotiate_config = NegotiateConfig::from_protocol_config(
        Box::new(server_config),
        client_computer_name.to_string(),
    );
    let mut server = Negotiate::new_server(negotiate_config, Vec::new())
        .map_err(|e| SsoError::Sspi(e.to_string()))?;

    let mut acq = server
        .acquire_credentials_handle()
        .with_credential_use(CredentialUse::Inbound)
        .execute(&mut server)
        .map_err(|e| SsoError::Sspi(e.to_string()))?;

    let mut input = [SecurityBuffer::new(input_token.to_vec(), BufferType::Token)];
    let mut output = vec![SecurityBuffer::new(
        Vec::with_capacity(1024),
        BufferType::Token,
    )];

    let builder = server
        .accept_security_context()
        .with_credentials_handle(&mut acq.credentials_handle)
        .with_context_requirements(ServerRequestFlags::ALLOCATE_MEMORY)
        .with_target_data_representation(DataRepresentation::Native)
        .with_input(&mut input)
        .with_output(&mut output);

    let net = OfflineNetworkClient;
    let result = server
        .accept_security_context_impl(builder)
        .map_err(|e| SsoError::Sspi(e.to_string()))?
        .resolve_with_client(&net)
        .map_err(|e| SsoError::Sspi(e.to_string()))?;

    let reply_token = output.remove(0).buffer;

    match result.status {
        SecurityStatus::Ok
        | SecurityStatus::CompleteNeeded
        | SecurityStatus::CompleteAndContinue => {
            let username = server
                .query_context_names()
                .map_err(|e| SsoError::Sspi(e.to_string()))?
                .username
                .inner()
                .to_string();
            Ok(SsoOutcome::Authenticated {
                username,
                reply_token,
            })
        }
        SecurityStatus::ContinueNeeded => Ok(SsoOutcome::Continue { reply_token }),
        _ => Ok(SsoOutcome::Denied),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_is_not_configured() {
        // Guards the fast-path rejection without needing any sspi machinery —
        // the live handshake itself is real-AD-only (see module note).
        let out = accept_spnego("", b"", "HOST", b"\x01\x02");
        assert!(matches!(out, Err(SsoError::NotConfigured)));
        let out2 = accept_spnego("HTTP/chat.example.local", b"", "HOST", b"\x01");
        assert!(matches!(out2, Err(SsoError::NotConfigured)));
    }

    #[test]
    fn bad_keytab_surfaces_keytab_error() {
        // A non-empty but invalid keytab must produce a typed Keytab error,
        // not a panic.
        let out = accept_spnego("HTTP/chat.example.local", b"not-a-keytab", "HOST", b"\x01");
        assert!(matches!(out, Err(SsoError::Keytab(_))));
    }

    #[test]
    fn foreign_ticket_for_unknown_spn_is_denied_not_panicking() {
        // A structurally valid keytab for a DIFFERENT SPN: the accept must
        // fail with a typed keytab error rather than panicking or accepting.
        let out = accept_spnego(
            "HTTP/other.example.local",
            b"\x05\x02\x00\x00\x00\x00",
            "HOST",
            b"\x60\x82",
        );
        assert!(matches!(out, Err(SsoError::Keytab(_))));
    }
}
