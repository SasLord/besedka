//! Password hashing/verification (argon2id) — pure functions, no I/O.
//!
//! Mirrors the official argon2 example. The hash is stored as a PHC string
//! (`$argon2id$v=19$...`) so it is self-describing and portable. Verification
//! uses the parameters embedded in the stored hash, not the current defaults.

use argon2::password_hash::{
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
};
use argon2::Argon2;

/// Hash `password` into a PHC string (random salt).
pub fn hash(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}

/// Verify `password` against a stored PHC `hash`. Returns `false` (not an
/// error) for a wrong password or a malformed hash.
pub fn verify(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_verify_roundtrip() {
        let hash = hash("correct-horse-battery-staple").expect("hash ok");
        assert!(hash.starts_with("$argon2"), "hash: {hash}");
        assert!(verify("correct-horse-battery-staple", &hash));
    }

    #[test]
    fn wrong_password_fails() {
        let hash = hash("secret").expect("hash ok");
        assert!(!verify("wrong", &hash));
        assert!(!verify("", &hash));
    }

    #[test]
    fn distinct_salts_distinct_hashes() {
        let a = hash("same-password").unwrap();
        let b = hash("same-password").unwrap();
        // Same password, different (random) salt → different PHC strings.
        assert_ne!(a, b);
        assert!(verify("same-password", &a));
        assert!(verify("same-password", &b));
    }

    #[test]
    fn malformed_hash_is_not_a_crash() {
        assert!(!verify("anything", "not-a-phc-string"));
        assert!(!verify("anything", ""));
    }
}
