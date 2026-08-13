// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

use anyhow::Context;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use keystone_store::Metadata;

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

pub fn ensure_admin(meta: &Metadata, username: &str, configured_hash: &str) -> anyhow::Result<()> {
    if let Some(existing) = meta.user_hash(username)? {
        if configured_hash.is_empty() || existing == configured_hash {
            return Ok(());
        }
        meta.upsert_user(username, configured_hash)?;
        return Ok(());
    }
    if !configured_hash.is_empty() {
        meta.upsert_user(username, configured_hash)?;
        return Ok(());
    }
    let password = std::env::var("KEYSTONE_ADMIN_PASSWORD").context(
        "no auth.password_hash and KEYSTONE_ADMIN_PASSWORD is unset; cannot create admin user",
    )?;
    let hash = hash_password(&password)?;
    meta.upsert_user(username, &hash)?;
    tracing::info!("created admin user `{username}` from KEYSTONE_ADMIN_PASSWORD");
    Ok(())
}

pub fn new_session_id() -> String {
    let mut bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut bytes);
    hex::encode(bytes)
}
