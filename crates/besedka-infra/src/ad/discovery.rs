//! AD connection auto-detect (ported from trackly `ad/discovery.rs`).
//!
//! Resolution order (each step falls back to the next):
//! 1. Manual override (`AdConfig.host`/`base_dn` filled in by the admin).
//! 2. DNS SRV (`_ldap._tcp.dc._msdcs.<domain>`) via `hickory-resolver`.
//! 3. Last resort: a typed "no domain detected" result — NOT a panic. This is
//!    the dev-macOS path: no SRV records, so discovery cleanly reports "not a
//!    domain member" and the caller relies on `BESEDKA_AD_MOCK=1` instead.
//!
//! Only `discover_dc` touches the network; `derive_base_dn` is a pure string
//! transform and is what `AdConfig::resolved_base_dn` uses.

use hickory_resolver::proto::rr::RData;
use hickory_resolver::Resolver;

/// Result of an auto-detect attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryResult {
    /// A domain controller hostname was found (via DNS SRV).
    Found { dc_host: String },
    /// No domain could be detected — dev macOS / non-domain-joined host. A
    /// normal, expected outcome, not an error.
    NoDomainDetected,
}

/// Derive the LDAP base DN from a DNS domain name.
///
/// `corp.local` -> `dc=corp,dc=local`. Pure string transform, no I/O.
pub fn derive_base_dn(domain: &str) -> String {
    domain
        .split('.')
        .filter(|label| !label.is_empty())
        .map(|label| format!("dc={label}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// Read the domain name from the environment (`USERDNSDOMAIN`, set on
/// domain-joined Windows hosts). `None` on dev macOS/Linux — the expected
/// dev path.
pub fn domain_from_env() -> Option<String> {
    std::env::var("USERDNSDOMAIN")
        .ok()
        .filter(|v| !v.trim().is_empty())
}

/// Auto-detect a domain controller via DNS SRV
/// (`_ldap._tcp.dc._msdcs.<domain>`), returning a typed "no domain detected"
/// result on any failure (never panics).
pub async fn discover_dc(domain: &str) -> DiscoveryResult {
    if domain.trim().is_empty() {
        return DiscoveryResult::NoDomainDetected;
    }

    let resolver = match Resolver::builder_tokio().and_then(|builder| builder.build()) {
        Ok(r) => r,
        Err(_) => return DiscoveryResult::NoDomainDetected,
    };

    let query = format!("_ldap._tcp.dc._msdcs.{domain}");
    let lookup = match resolver.srv_lookup(query).await {
        Ok(lookup) => lookup,
        Err(_) => return DiscoveryResult::NoDomainDetected,
    };

    let dc_host = lookup
        .answers()
        .iter()
        .filter_map(|record| match &record.data {
            RData::SRV(srv) => Some(srv),
            _ => None,
        })
        .min_by_key(|srv| srv.priority)
        .map(|srv| srv.target.to_string().trim_end_matches('.').to_string());

    match dc_host {
        Some(host) if !host.is_empty() => DiscoveryResult::Found { dc_host: host },
        _ => DiscoveryResult::NoDomainDetected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_base_dn_single_label() {
        assert_eq!(derive_base_dn("local"), "dc=local");
    }

    #[test]
    fn derive_base_dn_multi_label() {
        assert_eq!(derive_base_dn("corp.local"), "dc=corp,dc=local");
    }

    #[test]
    fn derive_base_dn_three_labels() {
        assert_eq!(
            derive_base_dn("ad.corp.example.com"),
            "dc=ad,dc=corp,dc=example,dc=com"
        );
    }

    #[test]
    fn derive_base_dn_empty_domain_yields_empty_string() {
        assert_eq!(derive_base_dn(""), "");
    }

    #[tokio::test]
    async fn no_domain_returns_typed_result() {
        assert_eq!(discover_dc("").await, DiscoveryResult::NoDomainDetected);
    }

    #[tokio::test]
    async fn nonexistent_domain_returns_typed_result_not_panic() {
        let result = discover_dc("this-domain-does-not-exist.invalid").await;
        assert_eq!(result, DiscoveryResult::NoDomainDetected);
    }
}
