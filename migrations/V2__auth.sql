-- Besedka V2: auth (Wave 1.1)
-- Adds password support + roles to the existing users table.
--
-- password_hash is NULLABLE: AD/SSO accounts (Wave 1.2) have no local
-- password, so "no hash" means "password login not available for this
-- account". The hash is an argon2id PHC string (self-describing).
-- role defaults to 'user'; the CHECK keeps the set in sync with
-- besedka_core::Role (user | manager | admin).
--
-- ALTER TABLE ... ADD COLUMN cannot take a non-constant DEFAULT that is
-- also used in a CHECK, so we add the columns separately.

ALTER TABLE users ADD COLUMN password_hash TEXT;

ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'user'
    CHECK (role IN ('user', 'manager', 'admin'));
