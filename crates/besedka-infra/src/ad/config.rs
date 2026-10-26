//! `[ad]` — Active Directory / Kerberos SSO configuration.
//!
//! Ported from trackly's `AdConfig` and trimmed to what Besedka needs:
//! **one** master switch (`enabled`) replaces trackly's `enabled` +
//! `sso_enabled` pair, and the mock switch is a plain config field.
//!
//! Secrets: `bind_password` is the *service account's* password. It must come
//! from the environment (`BESEDKA_AD_BIND_PASSWORD`), never from a committed
//! TOML — see the manual [`Debug`] impl below, which redacts it so it can never
//! reach a log line.

use std::fmt;

use serde::Deserialize;

use super::discovery::derive_base_dn;

/// 30 min — display names are cosmetic, a long TTL is fine.
const DEFAULT_DISPLAY_NAME_CACHE_TTL_SECS: u64 = 1800;
/// 5 min — role resolution directly affects authorization, so it expires fast.
const DEFAULT_GROUP_CACHE_TTL_SECS: u64 = 300;

/// How the LDAP connection is transported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LdapTlsMode {
    /// TLS from the first byte (`ldaps://host:636`). Default.
    #[default]
    Ldaps,
    /// Unencrypted `ldap://host:389`. Passwords travel in clear text — an
    /// explicit opt-in for DCs with no working LDAPS.
    Plain,
    /// `ldap://host:389` upgraded to TLS via StartTLS.
    StartTls,
}

impl LdapTlsMode {
    /// Default port for the mode when `AdConfig::port` is not set explicitly.
    pub fn default_port(self) -> u16 {
        match self {
            Self::Ldaps => 636,
            Self::Plain | Self::StartTls => 389,
        }
    }
}

/// One "AD group → Besedka role" mapping entry.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RoleMappingEntry {
    /// Full DN of the AD group.
    pub group_dn: String,
    /// Role name: `"user"` | `"manager"` | `"admin"`.
    pub role: String,
}

/// Active Directory parameters.
///
/// Every field has a default, so a config without an `[ad]` section still
/// parses (AD off). `host`/`base_dn` may stay empty — they are then derived
/// from `domain` (and, for `host`, optionally auto-detected via DNS SRV; see
/// [`super::discovery`]).
#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct AdConfig {
    /// Master switch. `false` (default) ⇒ every AD/SSO endpoint answers 503.
    pub enabled: bool,
    /// Use the deterministic mock adapters instead of real LDAP/Kerberos.
    /// `BESEDKA_AD_MOCK=1` forces this on at runtime (dev without a DC).
    pub mock: bool,
    /// DNS suffix of the domain, e.g. `example.local`.
    pub domain: String,
    /// Service Principal Name in `SERVICE/host` form, e.g.
    /// `HTTP/chat.example.local` — must match the principal the keytab was
    /// generated for (`ktpass … /crypto AES256-SHA1`).
    pub spn: String,
    /// Path to the `.keytab` (gitignored). Read on each SSO login; the key
    /// bytes never leave the process and are never logged.
    pub keytab_path: String,
    /// Domain controller host. Empty ⇒ auto-detect / mock.
    pub host: String,
    /// Explicit LDAP port. `None` ⇒ derived from `ldap_tls_mode`.
    pub port: Option<u16>,
    /// LDAP transport mode.
    pub ldap_tls_mode: LdapTlsMode,
    /// Disable LDAPS certificate verification (insecure opt-in for lab DCs).
    pub no_tls_verify: bool,
    /// Search base, e.g. `dc=example,dc=local`. Empty ⇒ derived from `domain`.
    pub base_dn: String,
    /// DN/UPN of the read-only service account used for SSO enrichment.
    /// Empty ⇒ enrichment is not configured (SSO still works, without ФИО/роль).
    pub bind_dn: String,
    /// Service account password — from `BESEDKA_AD_BIND_PASSWORD`, never TOML.
    pub bind_password: String,
    /// Attribute holding the display name (`displayName`, fallback `cn`, then
    /// the login itself).
    pub name_attr: String,
    /// "AD group → role" table. Highest privilege wins, regardless of order.
    pub role_mapping: Vec<RoleMappingEntry>,
    /// TTL of the display-name cache, seconds.
    pub display_name_cache_ttl_secs: u64,
    /// TTL of the role cache, seconds (shorter — it gates authorization).
    pub group_cache_ttl_secs: u64,
}

/// Configuration defaults — deliberately **fail-closed** (AD off) and with
/// non-zero cache TTLs (a 0 TTL would silently disable caching).
impl Default for AdConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mock: false,
            domain: String::new(),
            spn: String::new(),
            keytab_path: String::new(),
            host: String::new(),
            port: None,
            ldap_tls_mode: LdapTlsMode::default(),
            no_tls_verify: false,
            base_dn: String::new(),
            bind_dn: String::new(),
            bind_password: String::new(),
            name_attr: "displayName".to_string(),
            role_mapping: Vec::new(),
            display_name_cache_ttl_secs: DEFAULT_DISPLAY_NAME_CACHE_TTL_SECS,
            group_cache_ttl_secs: DEFAULT_GROUP_CACHE_TTL_SECS,
        }
    }
}

impl AdConfig {
    /// Port used for LDAP: explicit `port` always wins over the mode default.
    pub fn resolved_port(&self) -> u16 {
        self.port
            .unwrap_or_else(|| self.ldap_tls_mode.default_port())
    }

    /// `base_dn` as configured, or derived from `domain` (`corp.local` →
    /// `dc=corp,dc=local`).
    pub fn resolved_base_dn(&self) -> String {
        if !self.base_dn.trim().is_empty() {
            self.base_dn.clone()
        } else {
            derive_base_dn(&self.domain)
        }
    }

    /// This server's own machine name — diagnostic only (`sspi` wants it).
    /// Not a secret and not an identity claim.
    pub fn resolved_computer_name(&self) -> String {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "BESEDKA".to_string())
    }

    /// Kerberos SSO needs an SPN and a readable keytab. Mock mode does not.
    pub fn sso_ready(&self) -> bool {
        self.mock || (!self.spn.is_empty() && !self.keytab_path.is_empty())
    }

    /// Is the read-only service bind configured (SSO enrichment possible)?
    pub fn directory_configured(&self) -> bool {
        !self.bind_dn.trim().is_empty() && !self.resolved_base_dn().trim().is_empty()
    }
}

/// Never prints `bind_password` (trackly T-31-02b, same rule here).
impl fmt::Debug for AdConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AdConfig")
            .field("enabled", &self.enabled)
            .field("mock", &self.mock)
            .field("domain", &self.domain)
            .field("spn", &self.spn)
            .field("keytab_path", &self.keytab_path)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("ldap_tls_mode", &self.ldap_tls_mode)
            .field("no_tls_verify", &self.no_tls_verify)
            .field("base_dn", &self.base_dn)
            .field("bind_dn", &self.bind_dn)
            .field("bind_password", &"<redacted>")
            .field("name_attr", &self.name_attr)
            .field("role_mapping", &self.role_mapping.len())
            .field(
                "display_name_cache_ttl_secs",
                &self.display_name_cache_ttl_secs,
            )
            .field("group_cache_ttl_secs", &self.group_cache_ttl_secs)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_section_gives_safe_defaults() {
        let cfg: AdConfig = toml::from_str("").expect("defaults");
        assert!(!cfg.enabled, "AD must be off by default");
        assert!(!cfg.mock);
        assert_eq!(cfg.name_attr, "displayName");
        assert_eq!(cfg.ldap_tls_mode, LdapTlsMode::Ldaps);
        assert_eq!(cfg.display_name_cache_ttl_secs, 1800);
        assert_eq!(cfg.group_cache_ttl_secs, 300);
    }

    #[test]
    fn partial_section_parses() {
        let cfg: AdConfig =
            toml::from_str("enabled = true\nmock = true\ndomain = \"example.local\"").unwrap();
        assert!(cfg.enabled && cfg.mock);
        // Untouched fields keep their non-zero defaults (no "0 TTL" trap).
        assert_eq!(cfg.display_name_cache_ttl_secs, 1800);
        assert_eq!(cfg.group_cache_ttl_secs, 300);
    }

    #[test]
    fn port_defaults_follow_tls_mode() {
        let mut cfg = AdConfig::default();
        assert_eq!(cfg.resolved_port(), 636);
        cfg.ldap_tls_mode = LdapTlsMode::Plain;
        assert_eq!(cfg.resolved_port(), 389);
        cfg.ldap_tls_mode = LdapTlsMode::StartTls;
        assert_eq!(cfg.resolved_port(), 389);
        // Explicit port always wins.
        cfg.port = Some(2389);
        assert_eq!(cfg.resolved_port(), 2389);
    }

    #[test]
    fn base_dn_derived_from_domain_when_unset() {
        let cfg = AdConfig {
            domain: "corp.local".to_string(),
            ..AdConfig::default()
        };
        assert_eq!(cfg.resolved_base_dn(), "dc=corp,dc=local");

        let explicit = AdConfig {
            domain: "corp.local".to_string(),
            base_dn: "ou=staff,dc=corp,dc=local".to_string(),
            ..AdConfig::default()
        };
        assert_eq!(explicit.resolved_base_dn(), "ou=staff,dc=corp,dc=local");
    }

    #[test]
    fn sso_ready_requires_spn_and_keytab_unless_mock() {
        let mut cfg = AdConfig {
            enabled: true,
            ..AdConfig::default()
        };
        assert!(!cfg.sso_ready());
        cfg.spn = "HTTP/chat.example.local".to_string();
        assert!(!cfg.sso_ready());
        cfg.keytab_path = "secrets/chat.keytab".to_string();
        assert!(cfg.sso_ready());

        let mock = AdConfig {
            enabled: true,
            mock: true,
            ..AdConfig::default()
        };
        assert!(mock.sso_ready(), "mock mode needs no keytab");
    }

    #[test]
    fn debug_never_prints_bind_password() {
        let cfg = AdConfig {
            bind_password: "sup3r-s3cret-svc-password".to_string(),
            bind_dn: "svc-besedka-ro@example.local".to_string(),
            ..AdConfig::default()
        };
        let printed = format!("{cfg:?}");
        assert!(
            !printed.contains("sup3r-s3cret-svc-password"),
            "bind_password leaked into Debug: {printed}"
        );
        assert!(printed.contains("<redacted>"));
        assert!(printed.contains("svc-besedka-ro@example.local"));
    }

    #[test]
    fn role_mapping_array_of_tables_parses() {
        let cfg: AdConfig = toml::from_str(
            "enabled = true\n\
             [[role_mapping]]\n\
             group_dn = \"CN=Besedka-Admins,OU=Groups,DC=example,DC=local\"\n\
             role = \"admin\"\n",
        )
        .unwrap();
        assert_eq!(cfg.role_mapping.len(), 1);
        assert_eq!(cfg.role_mapping[0].role, "admin");
        assert!(cfg.role_mapping[0].group_dn.contains("Besedka-Admins"));
    }

    #[test]
    fn directory_configured_needs_both_bind_dn_and_base_dn() {
        let mut cfg = AdConfig::default();
        assert!(!cfg.directory_configured());
        cfg.bind_dn = "svc@example.local".to_string();
        // base_dn still derivable from an empty domain => empty string.
        assert!(!cfg.directory_configured());
        cfg.domain = "example.local".to_string();
        assert!(cfg.directory_configured(), "base_dn derives from domain");
    }
}
