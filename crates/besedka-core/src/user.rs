//! A Besedka user (public view).
//!
//! The password hash is **not** part of this type — it stays in the
//! infrastructure layer (DB) and is never serialized to the API. AD/SSO users
//! have no password, which is why the auth layer treats "no hash" as
//! "password login not available for this account".

use serde::{Deserialize, Serialize};

use crate::role::Role;

/// A user of Besedka.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// Stable DB row id (also stored in the session on login).
    pub id: i64,
    /// AD/UPN login, lowercase, unique.
    pub username: String,
    /// Human-readable name (may be empty for SSO accounts).
    pub display_name: Option<String>,
    /// The user's role (gate for admin/manager actions, Wave 1.3+).
    pub role: Role,
    /// RFC 3339 UTC timestamp (set by the DB default).
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_serde_roundtrip() {
        let user = User {
            id: 7,
            username: "alice".into(),
            display_name: Some("Alice".into()),
            role: Role::User,
            created_at: "2026-10-26T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&user).unwrap();
        let back: User = serde_json::from_str(&json).unwrap();
        assert_eq!(user, back);
        // Role serializes in snake_case on the wire.
        assert!(json.contains("\"role\":\"user\""), "json: {json}");
    }

    #[test]
    fn user_with_manager_role() {
        let user = User {
            id: 1,
            username: "bob".into(),
            display_name: None,
            role: Role::Manager,
            created_at: "2026-10-26T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&user).unwrap();
        assert!(json.contains("\"role\":\"manager\""), "json: {json}");
    }
}
