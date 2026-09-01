// SPDX-FileCopyrightText: 2026 The KeyStone Authors
// SPDX-License-Identifier: GPL-2.0-or-later

//! TOTP authenticator (RFC 6238, SHA1, 6 digits, 30s) and hashed backup codes.

use std::time::{SystemTime, UNIX_EPOCH};

use qrcode::render::svg::Color as SvgColor;
use qrcode::QrCode;
use rand::rngs::OsRng;
use rand::RngCore;
use totp_rs::{Algorithm, Secret, TOTP};

use crate::auth;

const ISSUER: &str = "KeyStone";
const BACKUP_COUNT: usize = 8;
const BACKUP_ALPH: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const SKEW: i64 = 1;

pub fn new_secret() -> String {
    Secret::generate_secret().to_encoded().to_string()
}

fn totp(secret_b32: &str, account: &str) -> Result<TOTP, String> {
    let bytes = Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|e| e.to_string())?;
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        bytes,
        Some(ISSUER.into()),
        account.to_string(),
    )
    .map_err(|e| e.to_string())
}

pub fn otpauth_url(secret_b32: &str, account: &str) -> Result<String, String> {
    Ok(totp(secret_b32, account)?.get_url())
}

pub fn qr_svg(otpauth: &str) -> Result<String, String> {
    let code = QrCode::new(otpauth.as_bytes()).map_err(|e| e.to_string())?;
    Ok(code
        .render::<SvgColor>()
        .min_dimensions(180, 180)
        .dark_color(SvgColor("#e6e8ea"))
        .light_color(SvgColor("#171b1f"))
        .build())
}

pub fn verify_code(secret_b32: &str, account: &str, code: &str) -> bool {
    verify_code_step(secret_b32, account, code, None).is_some()
}

/// Accepts the current 30s window and ±1. `last_step` rejects a code
/// already used in that window (login replay).
pub fn verify_code_step(
    secret_b32: &str,
    account: &str,
    code: &str,
    last_step: Option<i64>,
) -> Option<i64> {
    let code = normalize_totp(code)?;
    let t = totp(secret_b32, account).ok()?;
    let current = current_step();
    for delta in -SKEW..=SKEW {
        let step = current + delta;
        if last_step == Some(step) {
            continue;
        }
        if step < 0 {
            continue;
        }
        if t.generate((step as u64) * 30) == code {
            return Some(step);
        }
    }
    None
}

#[cfg(test)]
pub(crate) fn code_now(secret_b32: &str, account: &str) -> String {
    totp(secret_b32, account)
        .expect("totp secret")
        .generate_current()
        .expect("clock")
}

pub fn current_step() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 / 30)
        .unwrap_or(0)
}

pub fn normalize_totp(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 6 {
        Some(digits)
    } else {
        None
    }
}

pub fn generate_backup_codes() -> Vec<String> {
    let mut out = Vec::with_capacity(BACKUP_COUNT);
    for _ in 0..BACKUP_COUNT {
        let mut raw = [0u8; 8];
        OsRng.fill_bytes(&mut raw);
        let mut s = String::new();
        for (i, b) in raw.iter().enumerate() {
            if i == 4 {
                s.push('-');
            }
            s.push(BACKUP_ALPH[(*b as usize) % BACKUP_ALPH.len()] as char);
        }
        out.push(s);
    }
    out
}

pub fn canonical_backup(raw: &str) -> Option<String> {
    let alnum: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if alnum.len() != 8 {
        return None;
    }
    Some(format!("{}-{}", &alnum[..4], &alnum[4..]))
}

pub fn hash_backup_codes(codes: &[String]) -> anyhow::Result<Vec<String>> {
    let mut hashes = Vec::with_capacity(codes.len());
    for c in codes {
        hashes.push(auth::hash_password(c)?);
    }
    Ok(hashes)
}

/// Returns the remaining hashes if `code` matched one entry.
pub fn take_backup_code(hashes: &[String], code: &str) -> Option<Vec<String>> {
    let canonical = canonical_backup(code)?;
    let mut rest = Vec::new();
    let mut hit = false;
    for h in hashes {
        if !hit && auth::verify_password(&canonical, h) {
            hit = true;
            continue;
        }
        rest.push(h.clone());
    }
    if hit {
        Some(rest)
    } else {
        None
    }
}

pub fn parse_backup_hashes(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

pub fn backup_hashes_json(hashes: &[String]) -> String {
    serde_json::to_string(hashes).unwrap_or_else(|_| "[]".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totp_round_trip() {
        let secret = new_secret();
        let t = totp(&secret, "admin").unwrap();
        let code = t.generate_current().unwrap();
        let step = verify_code_step(&secret, "admin", &code, None).expect("current code");
        assert!(verify_code(&secret, "admin", &code));
        assert!(verify_code_step(&secret, "admin", &code, Some(step)).is_none());
        assert!(!verify_code(&secret, "admin", "000000"));
        assert!(otpauth_url(&secret, "admin")
            .unwrap()
            .starts_with("otpauth://"));
        assert!(qr_svg(&otpauth_url(&secret, "admin").unwrap())
            .unwrap()
            .contains("<svg"));
    }

    #[test]
    fn backup_codes_consume_once() {
        let codes = generate_backup_codes();
        assert_eq!(codes.len(), 8);
        assert!(codes[0].contains('-'));
        let hashes = hash_backup_codes(&codes).unwrap();
        let rest = take_backup_code(&hashes, &codes[0]).unwrap();
        assert_eq!(rest.len(), 7);
        assert!(take_backup_code(&rest, &codes[0]).is_none());
        let spaced = format!("{} {}", &codes[1][..4], &codes[1][5..]);
        assert!(take_backup_code(&rest, &spaced).is_some());
    }
}
