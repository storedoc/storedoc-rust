use std::collections::HashMap;
use serde::Serialize;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, StoreDocError>;

#[derive(Debug, Error)]
pub enum StoreDocError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Authentication failed: {0}")]
    Authentication(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Validation failed: {message}")]
    Validation {
        message: String,
        errors: Option<HashMap<String, Vec<String>>>,
    },

    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    #[error("API error ({status}): {message}")]
    Api {
        status: u16,
        code: Option<String>,
        message: String,
    },

    #[error("Network error: {0}")]
    Network(String),

    #[error("Request timed out: {0}")]
    Timeout(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl StoreDocError {
    pub fn status(&self) -> u16 {
        match self {
            Self::InvalidConfig(_) => 0,
            Self::Authentication(_) => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound(_) => 404,
            Self::Validation { .. } => 422,
            Self::RateLimit(_) => 429,
            Self::Api { status, .. } => *status,
            Self::Network(_) => 0,
            Self::Timeout(_) => 408,
            Self::InvalidResponse(_) => 502,
            Self::Io(_) => 0,
        }
    }
}

/// Allows returning `Result<T, StoreDocError>` directly from `#[tauri::command]`.
impl Serialize for StoreDocError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct SerializedError<'a> {
            status: u16,
            message: String,
            errors: Option<&'a HashMap<String, Vec<String>>>,
        }

        let errors = match self {
            Self::Validation { errors, .. } => errors.as_ref(),
            _ => None,
        };

        SerializedError {
            status: self.status(),
            message: self.to_string(),
            errors,
        }
        .serialize(serializer)
    }
}
