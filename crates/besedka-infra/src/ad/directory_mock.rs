//! Mock AD directory — deterministic enrichment fixtures for dev machines
//! (ported from trackly `ad/directory_mock.rs`, D-Mock-01 / SSO-01 / SSO-03).
//!
//! Reuses the SAME `us100`/`us200` identities as [`super::mock`] (no invented
//! fixture names) and adds `us300` as the privileged one, so the admin role
//! gate can be exercised end-to-end without a DC:
//!
//! | login | display name             | role from "AD group" |
//! |-------|--------------------------|----------------------|
//! | us100 | Иванов Иван Иванович     | manager              |
//! | us200 | Петрова Анна Сергеевна   | (none) → user        |
//! | us300 | Сидоров Пётр Петрович    | admin                |

use std::collections::HashMap;

use async_trait::async_trait;
use besedka_core::{AdDirectory, DirectoryError, DirectoryResult, Role};

/// Enrichment fixture for a single "domain user".
#[derive(Debug, Clone)]
pub struct DirectoryFixture {
    pub display_name: &'static str,
    pub role: Option<Role>,
}

/// Deterministic mock AD directory for development (no service-account bind
/// and no DC needed).
#[derive(Debug, Clone)]
pub struct MockAdDirectory {
    pub users: HashMap<String, DirectoryFixture>,
    /// When `true`, every `resolve` returns `Err(DirectoryError::Unreachable)`
    /// — simulates the DC (or the service bind) being down, to prove the
    /// fail-closed path.
    pub unreachable: bool,
}

impl MockAdDirectory {
    /// The three fixtures described in the module docs.
    pub fn default_fixtures() -> Self {
        let users = [
            (
                "us100",
                DirectoryFixture {
                    display_name: "Иванов Иван Иванович",
                    role: Some(Role::Manager),
                },
            ),
            (
                "us200",
                DirectoryFixture {
                    display_name: "Петрова Анна Сергеевна",
                    role: None,
                },
            ),
            (
                "us300",
                DirectoryFixture {
                    display_name: "Сидоров Пётр Петрович",
                    role: Some(Role::Admin),
                },
            ),
        ]
        .into_iter()
        .map(|(login, fixture)| (login.to_string(), fixture))
        .collect();

        Self {
            users,
            unreachable: false,
        }
    }

    /// A mock directory that always reports the DC as unreachable.
    pub fn unreachable() -> Self {
        Self {
            users: HashMap::new(),
            unreachable: true,
        }
    }

    /// Strip `@domain` / `DOMAIN\` — same normalization as
    /// [`super::mock::MockAdClient::lookup_key`] (independent copy, matching
    /// the "small adapters" convention).
    pub fn lookup_key(login: &str) -> &str {
        let without_upn_suffix = login.split('@').next().unwrap_or(login);
        without_upn_suffix
            .rsplit('\\')
            .next()
            .unwrap_or(without_upn_suffix)
    }
}

#[async_trait]
impl AdDirectory for MockAdDirectory {
    async fn resolve(&self, login: &str) -> Result<DirectoryResult, DirectoryError> {
        if self.unreachable {
            return Err(DirectoryError::Unreachable);
        }

        let key = Self::lookup_key(login);
        match self.users.get(key) {
            Some(fixture) => Ok(DirectoryResult {
                display_name: fixture.display_name.to_string(),
                role: fixture.role,
            }),
            // A Kerberos-authenticated login the fixture map does not know:
            // degrade to the login itself (mirrors the real adapter's
            // displayName → cn → login fallback chain), never panic.
            None => Ok(DirectoryResult {
                display_name: login.to_string(),
                role: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn known_user_resolves_display_name_and_role() {
        let mock = MockAdDirectory::default_fixtures();
        assert_eq!(
            mock.resolve("us100").await.expect("no error"),
            DirectoryResult {
                display_name: "Иванов Иван Иванович".to_string(),
                role: Some(Role::Manager),
            }
        );
    }

    #[tokio::test]
    async fn privileged_fixture_resolves_admin() {
        let mock = MockAdDirectory::default_fixtures();
        let result = mock.resolve("us300").await.expect("no error");
        assert_eq!(result.role, Some(Role::Admin));
        assert_eq!(result.display_name, "Сидоров Пётр Петрович");
    }

    #[tokio::test]
    async fn user_with_no_group_resolves_none_role() {
        let mock = MockAdDirectory::default_fixtures();
        assert_eq!(
            mock.resolve("us200").await.expect("no error"),
            DirectoryResult {
                display_name: "Петрова Анна Сергеевна".to_string(),
                role: None,
            }
        );
    }

    #[tokio::test]
    async fn unknown_login_falls_back_to_login_itself() {
        let mock = MockAdDirectory::default_fixtures();
        assert_eq!(
            mock.resolve("us999").await.expect("no error"),
            DirectoryResult {
                display_name: "us999".to_string(),
                role: None,
            }
        );
    }

    #[tokio::test]
    async fn upn_and_netbios_forms_resolve_to_same_fixture() {
        let mock = MockAdDirectory::default_fixtures();
        let expected = DirectoryResult {
            display_name: "Иванов Иван Иванович".to_string(),
            role: Some(Role::Manager),
        };
        assert_eq!(mock.resolve("us100@example.local").await.unwrap(), expected);
        assert_eq!(mock.resolve(r"EXAMPLE\us100").await.unwrap(), expected);
    }

    #[tokio::test]
    async fn unreachable_fixture_returns_typed_error() {
        let mock = MockAdDirectory::unreachable();
        assert_eq!(
            mock.resolve("us100").await,
            Err(DirectoryError::Unreachable)
        );
    }
}
