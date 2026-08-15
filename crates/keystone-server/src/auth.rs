// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use anyhow::Context;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use keystone_store::Metadata;

pub const MIN_PASSWORD_LEN: usize = 8;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash password: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Empty/too short/mismatch. Callers that have the current hash should also
/// reject a password that still verifies against it.
pub fn validate_new_password(password: &str, confirm: &str) -> Result<(), String> {
    if password != confirm {
        return Err("new password and confirmation do not match".into());
    }
    if password.len() < MIN_PASSWORD_LEN {
        return Err(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        ));
    }
    if password.trim() != password {
        return Err("password must not start or end with spaces".into());
    }
    Ok(())
}

pub fn ensure_admin(meta: &Metadata, username: &str, configured_hash: &str) -> anyhow::Result<()> {
    if let Some(existing) = meta.user_hash(username)? {
        if configured_hash.is_empty() || existing == configured_hash {
            return Ok(());
        }
        meta.set_user_password(username, configured_hash, false)?;
        return Ok(());
    }
    if !configured_hash.is_empty() {
        meta.set_user_password(username, configured_hash, false)?;
        return Ok(());
    }
    let password = std::env::var("KEYSTONE_ADMIN_PASSWORD").context(
        "no auth.password_hash and KEYSTONE_ADMIN_PASSWORD is unset; cannot create admin user",
    )?;
    let hash = hash_password(&password)?;
    meta.set_user_password(username, &hash, true)?;
    tracing::info!(
        "created admin user `{username}` from KEYSTONE_ADMIN_PASSWORD (must change on first login)"
    );
    Ok(())
}

pub fn new_session_id() -> String {
    let mut bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut bytes);
    hex::encode(bytes)
}

pub fn generate_ingest_token() -> String {
    let mut bytes = [0u8; 24];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_password_rules() {
        assert!(validate_new_password("short", "short").is_err());
        assert!(validate_new_password("longenough", "mismatch!!").is_err());
        assert!(validate_new_password("  padded1", "  padded1").is_err());
        assert!(validate_new_password("longenough", "longenough").is_ok());
    }
}
