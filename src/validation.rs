use uuid::Uuid;

use crate::errors::{Result, StoreDocError};

pub const DEFAULT_CHUNK_SIZE: usize = 5 * 1024 * 1024;
pub const MIN_CHUNK_SIZE: usize = 5 * 1024 * 1024;
pub const MAX_CHUNK_SIZE: usize = 100 * 1024 * 1024;
pub const DEFAULT_CONCURRENCY: usize = 3;
pub const MAX_CONCURRENCY: usize = 10;

pub fn assert_uuid(value: &str, field: &str) -> Result<()> {
    if Uuid::parse_str(value).is_err() {
        return Err(StoreDocError::Validation {
            message: format!("Invalid {field}: expected a valid UUID."),
            errors: None,
        });
    }
    Ok(())
}

pub fn normalize_object_path(path: &str) -> Result<String> {
    let trimmed = path.trim().replace('\\', "/");
    let clean = trimmed.trim_start_matches('/');
    if clean.is_empty() {
        return Err(StoreDocError::Validation {
            message: "Invalid object path: path cannot be empty.".into(),
            errors: None,
        });
    }
    if clean.split('/').any(|seg| seg == "..") {
        return Err(StoreDocError::Validation {
            message: "Invalid object path: path traversal ('..') is not allowed.".into(),
            errors: None,
        });
    }
    Ok(clean.to_string())
}

pub fn assert_metadata_key(key: &str) -> Result<()> {
    if key.is_empty()
        || key.len() > 64
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(StoreDocError::Validation {
            message: format!(
                "Invalid metadata key '{key}': must be 1-64 alphanumeric, '_' or '-' characters."
            ),
            errors: None,
        });
    }
    Ok(())
}

pub fn assert_presign_minutes(minutes: u32) -> Result<()> {
    if !(1..=10080).contains(&minutes) {
        return Err(StoreDocError::Validation {
            message: "Invalid presign minutes: expected an integer between 1 and 10080.".into(),
            errors: None,
        });
    }
    Ok(())
}

pub fn assert_chunk_size(chunk_size: usize) -> Result<()> {
    if !(MIN_CHUNK_SIZE..=MAX_CHUNK_SIZE).contains(&chunk_size) {
        return Err(StoreDocError::Validation {
            message: format!(
                "Invalid chunk_size: expected between {MIN_CHUNK_SIZE} and {MAX_CHUNK_SIZE} bytes."
            ),
            errors: None,
        });
    }
    Ok(())
}

pub fn assert_concurrency(concurrency: usize) -> Result<()> {
    if !(1..=MAX_CONCURRENCY).contains(&concurrency) {
        return Err(StoreDocError::Validation {
            message: format!("Invalid concurrency: expected between 1 and {MAX_CONCURRENCY}."),
            errors: None,
        });
    }
    Ok(())
}
