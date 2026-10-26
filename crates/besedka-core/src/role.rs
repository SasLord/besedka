//! User roles (draft — CORP_CHAT_PLAN.md §4/§10).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// A user's role inside Besedka.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Regular user.
    User,
    /// Can manage rooms and channels.
    Manager,
    /// Full admin (users, roles, audit).
    Admin,
}

impl Role {
    /// Stable wire/DB string form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Manager => "manager",
            Self::Admin => "admin",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(Self::User),
            "manager" => Ok(Self::Manager),
            "admin" => Ok(Self::Admin),
            other => Err(format!("unknown role: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_roundtrip() {
        for role in [Role::User, Role::Manager, Role::Admin] {
            assert_eq!(role.as_str().parse::<Role>().unwrap(), role);
            assert_eq!(
                serde_json::from_str::<Role>(&format!("\"{}\"", role.as_str())).unwrap(),
                role
            );
        }
        assert!("superuser".parse::<Role>().is_err());
    }
}
