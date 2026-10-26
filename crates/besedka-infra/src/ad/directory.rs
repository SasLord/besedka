//! Real AD directory — service-account bind + `displayName`/`cn`/`memberOf`
//! search + `LDAP_MATCHING_RULE_IN_CHAIN` group check (ported from trackly
//! `ad/directory.rs`, SSO-01 / SSO-03).
//!
//! Unlike [`super::real::RealAdClient`] (which binds as the logged-in user),
//! this adapter ALWAYS binds as a FIXED read-only service account
//! (`bind_dn`/`bind_password`): SSO users have no password to bind with, so
//! someone has to do the lookup on their behalf.
//!
//! Results are cached in TWO independently-TTL'd [`TtlCache`]s (display name,
//! role) so repeat SSO logins do not hit the DC every time.
//!
//! Fail-closed rule (kept from trackly, Pitfall 4): a group check that *could
//! not complete* maps the whole resolve to `Unreachable` — an outage must never
//! look like "no groups matched".

use std::time::Duration;

use async_trait::async_trait;
use besedka_core::{AdDirectory, DirectoryError, DirectoryResult, Role};
use ldap3::{ldap_escape, LdapConnAsync, Scope, SearchEntry};

use super::cache::TtlCache;
use super::config::AdConfig;
use super::transport::build_ldap_conn;

/// Production AD directory adapter — service bind + search + group check,
/// with a two-instance TTL cache in front.
pub struct RealAdDirectory {
    cfg: AdConfig,
    display_name_cache: TtlCache<String>,
    role_cache: TtlCache<Option<Role>>,
}

impl RealAdDirectory {
    pub fn new(cfg: AdConfig) -> Self {
        let display_name_cache =
            TtlCache::new(Duration::from_secs(cfg.display_name_cache_ttl_secs));
        let role_cache = TtlCache::new(Duration::from_secs(cfg.group_cache_ttl_secs));
        Self {
            cfg,
            display_name_cache,
            role_cache,
        }
    }

    /// Normalize the SERVICE ACCOUNT's bind name (same rule as
    /// `RealAdClient::normalize_bind_name`).
    fn normalize_bind_name(&self, login: &str) -> String {
        if login.contains('@') || login.contains('\\') {
            login.to_string()
        } else {
            format!("{login}@{}", self.cfg.domain)
        }
    }
}

/// Cache key / search value for a login: strip the `@domain` suffix and the
/// `DOMAIN\` prefix, then lowercase — so UPN, NetBIOS and bare forms of one
/// identity share a single cache entry.
pub fn cache_key(login: &str) -> String {
    let without_upn_suffix = login.split('@').next().unwrap_or(login);
    let without_netbios_prefix = without_upn_suffix
        .rsplit('\\')
        .next()
        .unwrap_or(without_upn_suffix);
    without_netbios_prefix.to_lowercase()
}

/// Build the user-search filter, escaping the login first (same injection
/// defence as `real::build_user_search_filter`).
pub fn build_user_search_filter(login: &str) -> String {
    let safe = ldap_escape(login);
    format!("(|(sAMAccountName={safe})(userPrincipalName={safe}))")
}

/// Build the group-membership filter using `LDAP_MATCHING_RULE_IN_CHAIN`
/// (OID `1.2.840.113556.1.4.1941`) — AD expands nested groups server-side in
/// one query. BOTH operands are escaped: never skip escaping `group_dn` just
/// because it comes from trusted config (defense in depth).
pub fn build_group_membership_filter(login: &str, group_dn: &str) -> String {
    format!(
        "(&(objectClass=user)(sAMAccountName={})(memberOf:1.2.840.113556.1.4.1941:={}))",
        ldap_escape(login),
        ldap_escape(group_dn),
    )
}

/// Highest-privilege-wins role selection: admin > manager > user, regardless of
/// slice order. `None` when no configured group matched at all — the normal
/// "role mapping not configured" outcome, not an error.
pub fn pick_highest_role(matched_roles: &[Role]) -> Option<Role> {
    if matched_roles.contains(&Role::Admin) {
        Some(Role::Admin)
    } else if matched_roles.contains(&Role::Manager) {
        Some(Role::Manager)
    } else if matched_roles.contains(&Role::User) {
        Some(Role::User)
    } else {
        None
    }
}

#[async_trait]
impl AdDirectory for RealAdDirectory {
    async fn resolve(&self, login: &str) -> Result<DirectoryResult, DirectoryError> {
        let key = cache_key(login);

        // (1) Cache short-circuit — BOTH caches must hit, otherwise fall
        // through to a fresh lookup. No LDAP connection is attempted here.
        if let (Some(display_name), Some(role)) =
            (self.display_name_cache.get(&key), self.role_cache.get(&key))
        {
            return Ok(DirectoryResult { display_name, role });
        }

        // (2) Not-configured gate, checked BEFORE any network attempt so the
        // optional enrichment degrades silently when the service bind is unset.
        if !self.cfg.directory_configured() {
            return Err(DirectoryError::NotConfigured);
        }
        if self.cfg.host.trim().is_empty() {
            return Err(DirectoryError::Unreachable);
        }

        // (3) Connect.
        let (url, settings) = build_ldap_conn(&self.cfg);
        let (conn, mut ldap) = match LdapConnAsync::with_settings(settings, &url).await {
            Ok(v) => v,
            Err(_) => return Err(DirectoryError::Unreachable),
        };
        // The connection driver task MUST be driven, or operations hang.
        ldap3::drive!(conn);

        // (4) Service-account bind — fixed DN/password, never the end user's.
        let bind_name = self.normalize_bind_name(&self.cfg.bind_dn);
        let bind_result = match ldap.simple_bind(&bind_name, &self.cfg.bind_password).await {
            Ok(res) => res,
            Err(_) => return Err(DirectoryError::Unreachable),
        };
        if bind_result.success().is_err() {
            // The SERVICE ACCOUNT's own credentials were rejected: a config
            // error, distinct from a network outage.
            let _ = ldap.unbind().await;
            return Err(DirectoryError::ServiceBindFailed);
        }

        let base_dn = self.cfg.resolved_base_dn();

        // (5) displayName/cn/memberOf in ONE round trip.
        let filter = build_user_search_filter(login);
        let attrs = vec![self.cfg.name_attr.as_str(), "cn"];

        let display_name = match ldap
            .search(&base_dn, Scope::Subtree, &filter, attrs)
            .await
            .and_then(|search_result| search_result.success())
        {
            Ok((entries, _res)) => entries
                .into_iter()
                .next()
                .map(SearchEntry::construct)
                .and_then(|entry| {
                    entry
                        .attrs
                        .get(&self.cfg.name_attr)
                        .and_then(|values| values.first().cloned())
                        .or_else(|| {
                            entry
                                .attrs
                                .get("cn")
                                .and_then(|values| values.first().cloned())
                        })
                })
                .unwrap_or_else(|| login.to_string()),
            // Search failure after a successful bind is non-fatal for the
            // display name — the bind already proved reachability.
            Err(_) => login.to_string(),
        };

        // (6) Group → role resolution, one search per configured mapping entry.
        let mut matched_roles: Vec<Role> = Vec::new();
        for entry in &self.cfg.role_mapping {
            let role = match entry.role.parse::<Role>() {
                Ok(role) => role,
                Err(_) => {
                    // Unparseable role in config — log and skip this entry
                    // rather than failing the whole resolve.
                    tracing::warn!(
                        group_dn = %entry.group_dn,
                        role = %entry.role,
                        "unparseable role in AD role_mapping entry, skipping"
                    );
                    continue;
                }
            };
            let group_filter = build_group_membership_filter(login, &entry.group_dn);
            match ldap
                .search(&base_dn, Scope::Subtree, &group_filter, vec!["dn"])
                .await
                .and_then(|search_result| search_result.success())
            {
                Ok((entries, _res)) => {
                    if !entries.is_empty() {
                        matched_roles.push(role);
                    }
                }
                // I/O error DURING the group check → fail closed.
                Err(_) => {
                    let _ = ldap.unbind().await;
                    return Err(DirectoryError::Unreachable);
                }
            }
        }

        let _ = ldap.unbind().await;

        let role = pick_highest_role(&matched_roles);

        // (7) Populate both caches with the resolved values.
        self.display_name_cache
            .put(key.clone(), display_name.clone());
        self.role_cache.put(key, role);

        Ok(DirectoryResult { display_name, role })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- build_group_membership_filter: injection defence in BOTH positions ---

    #[test]
    fn benign_login_and_group_dn_build_expected_filter() {
        assert_eq!(
            build_group_membership_filter("us100", "CN=Besedka-Admins,OU=Groups,DC=example,DC=local"),
            "(&(objectClass=user)(sAMAccountName=us100)(memberOf:1.2.840.113556.1.4.1941:=CN=Besedka-Admins,OU=Groups,DC=example,DC=local))"
        );
    }

    #[test]
    fn injection_payload_metacharacters_are_escaped_in_both_positions() {
        let payload = "*)(uid=*))(|(uid=*";
        let filter = build_group_membership_filter(payload, payload);

        assert!(!filter.contains('*'), "no raw `*` may survive: {filter}");
        assert!(
            !filter.contains(payload),
            "raw payload must not appear verbatim: {filter}"
        );
        assert!(filter.contains("\\2a"), "`*` → \\2a: {filter}");
        assert!(filter.contains("\\28"), "`(` → \\28: {filter}");
        assert!(filter.contains("\\29"), "`)` → \\29: {filter}");
        assert!(filter.starts_with("(&(objectClass=user)"));
        assert_eq!(
            filter.matches("(&").count(),
            1,
            "no injected top-level AND clause: {filter}"
        );
    }

    #[test]
    fn backslash_in_either_argument_is_escaped() {
        let filter = build_group_membership_filter("dom\\ain", "grp\\dn");
        assert!(filter.contains("\\5c"), "`\\` → \\5c: {filter}");
        assert!(
            !filter.contains("grp\\dn"),
            "raw group_dn backslash: {filter}"
        );
    }

    // --- cache_key normalization ---

    #[test]
    fn cache_key_strips_upn_and_netbios_and_lowercases() {
        assert_eq!(cache_key("US100"), "us100");
        assert_eq!(cache_key("us100@CORP.local"), "us100");
        assert_eq!(cache_key(r"CORP\us100"), "us100");
        assert_eq!(cache_key(r"corp.local\US100@CORP.local"), "us100");
    }

    // --- pick_highest_role ---

    #[test]
    fn admin_wins_regardless_of_slice_order() {
        assert_eq!(
            pick_highest_role(&[Role::Manager, Role::Admin]),
            Some(Role::Admin)
        );
        assert_eq!(
            pick_highest_role(&[Role::Admin, Role::Manager]),
            Some(Role::Admin)
        );
    }

    #[test]
    fn manager_beats_user() {
        assert_eq!(
            pick_highest_role(&[Role::User, Role::Manager]),
            Some(Role::Manager)
        );
    }

    #[test]
    fn empty_slice_returns_none() {
        assert_eq!(pick_highest_role(&[]), None);
    }

    #[test]
    fn user_only_slice_returns_some_user() {
        assert_eq!(pick_highest_role(&[Role::User]), Some(Role::User));
    }

    // --- Cache short-circuit (no LDAP under test) ---

    fn unreachable_but_configured_cfg() -> AdConfig {
        AdConfig {
            host: "127.0.0.1".to_string(),
            port: Some(1), // nothing listens here — a real attempt fails fast
            bind_dn: "svc-besedka-ro@example.local".to_string(),
            bind_password: "CHANGE_ME".to_string(),
            base_dn: "dc=example,dc=local".to_string(),
            ldap_tls_mode: super::super::config::LdapTlsMode::Plain,
            ..AdConfig::default()
        }
    }

    #[tokio::test]
    async fn cache_hit_short_circuits_ldap_call() {
        let directory = RealAdDirectory::new(unreachable_but_configured_cfg());
        let key = cache_key("us100");
        directory
            .display_name_cache
            .put(key.clone(), "Иванов Иван Иванович".to_string());
        directory.role_cache.put(key, Some(Role::Manager));

        assert_eq!(
            directory.resolve("us100").await,
            Ok(DirectoryResult {
                display_name: "Иванов Иван Иванович".to_string(),
                role: Some(Role::Manager),
            })
        );
    }

    #[tokio::test]
    async fn cache_miss_falls_through_to_a_real_attempt() {
        let directory = RealAdDirectory::new(unreachable_but_configured_cfg());
        // No pre-warm — a miss must ATTEMPT a lookup, never fabricate a hit.
        assert_eq!(
            directory.resolve("us100").await,
            Err(DirectoryError::Unreachable)
        );
    }

    #[tokio::test]
    async fn not_configured_degrades_without_network() {
        let directory = RealAdDirectory::new(AdConfig {
            host: "127.0.0.1".to_string(),
            port: Some(1),
            ..AdConfig::default()
        });
        assert_eq!(
            directory.resolve("us100").await,
            Err(DirectoryError::NotConfigured)
        );
    }
}
