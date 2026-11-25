use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::errors::{Result, StoreDocError};
use crate::types::{AuthMode, StoreDocConfig};

type HmacSha256 = Hmac<Sha256>;

pub const DEFAULT_WEBHOOK_MAX_AGE_SECONDS: u64 = 300;
const NONCE_BYTES: usize = 16;

/// Generate a fresh, single-use 32-hex-character HMAC nonce.
pub fn generate_nonce() -> String {
    let mut bytes = [0u8; NONCE_BYTES];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Canonicalize a request URI the way StoreDoc's AuthenticationService does:
/// strips a leading `/api` segment and leading slashes while preserving query string.
pub fn canonicalize_uri(uri: &str) -> String {
    let (raw_path, query) = match uri.find('?') {
        Some(idx) => (&uri[..idx], &uri[idx..]),
        None => (uri, ""),
    };

    let stripped = if raw_path == "/api" {
        ""
    } else if let Some(rest) = raw_path.strip_prefix("/api/") {
        rest
    } else {
        raw_path
    };

    let clean_path = stripped.trim_start_matches('/');
    format!("{clean_path}{query}")
}

/// Build the canonical string: `METHOD\nURI\nTIMESTAMP\nNONCE`.
pub fn build_canonical_string(method: &str, uri: &str, timestamp: u64, nonce: &str) -> String {
    format!(
        "{}\n{}\n{}\n{}",
        method.to_ascii_uppercase(),
        canonicalize_uri(uri),
        timestamp,
        nonce
    )
}

/// Generate an HMAC-SHA256 hex signature matching StoreDoc's AuthenticationService.
pub fn generate_hmac_signature(
    method: &str,
    uri: &str,
    timestamp: u64,
    secret_key: &str,
    nonce: &str,
) -> Result<String> {
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(StoreDocError::Validation {
            message: "Invalid nonce: expected exactly 32 hexadecimal characters.".into(),
            errors: None,
        });
    }
    if secret_key.is_empty() {
        return Err(StoreDocError::Authentication(
            "A secret access key is required to sign a StoreDoc request.".into(),
        ));
    }

    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
        .map_err(|e| StoreDocError::Authentication(format!("Invalid secret key: {e}")))?;
    let canonical = build_canonical_string(method, uri, timestamp, nonce);
    mac.update(canonical.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

/// Build authentication header `(name, value)` for a request.
pub fn build_auth_header(
    method: &str,
    uri: &str,
    config: &StoreDocConfig,
) -> Result<(String, String)> {
    match config.auth_mode {
        AuthMode::Bearer => {
            let token = config
                .token
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .ok_or_else(|| {
                    StoreDocError::Authentication(
                        "Bearer token is required when auth_mode is Bearer".into(),
                    )
                })?;
            Ok(("Authorization".into(), format!("Bearer {token}")))
        }
        AuthMode::ApiKey | AuthMode::Hmac => {
            let access_key = config.access_key_id.as_deref().map(str::trim).unwrap_or("");
            let secret_key = config.secret_key.as_deref().map(str::trim).unwrap_or("");

            if access_key.is_empty() || secret_key.is_empty() {
                if let Some(token) = config
                    .token
                    .as_deref()
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                {
                    return Ok(("Authorization".into(), format!("Bearer {token}")));
                }
                return Err(StoreDocError::Authentication(
                    "StoreDoc access_key_id and secret_key are required for authentication".into(),
                ));
            }

            if config.auth_mode == AuthMode::ApiKey {
                return Ok(("X-Api-Key".into(), format!("{access_key}:{secret_key}")));
            }

            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let nonce = generate_nonce();
            let signature = generate_hmac_signature(method, uri, timestamp, secret_key, &nonce)?;

            Ok((
                "Authorization".into(),
                format!(
                    "HMAC-SHA256 Credential={access_key}, Timestamp={timestamp}, Nonce={nonce}, Signature={signature}"
                ),
            ))
        }
    }
}

/// Verify incoming webhook signature (`X-Webhook-Signature`) in constant time.
/// Optionally enforces a replay window when `timestamp` (unix seconds) is provided.
pub fn verify_webhook_signature(
    payload: &[u8],
    signature: &str,
    secret: &str,
    timestamp: Option<u64>,
    max_age_seconds: Option<u64>,
) -> bool {
    if signature.is_empty() || secret.is_empty() {
        return false;
    }

    let trimmed = signature.trim();
    let normalized = trimmed
        .strip_prefix("sha256=")
        .or_else(|| trimmed.strip_prefix("SHA256="))
        .unwrap_or(trimmed);

    if normalized.len() != 64 || !normalized.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }

    let Ok(sig_bytes) = hex::decode(normalized) else {
        return false;
    };

    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(payload);
    if mac.verify_slice(&sig_bytes).is_err() {
        return false;
    }

    if let Some(ts) = timestamp {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let max_age = max_age_seconds.unwrap_or(DEFAULT_WEBHOOK_MAX_AGE_SECONDS);
        if max_age == 0 || now.abs_diff(ts) > max_age {
            return false;
        }
    }

    true
}
