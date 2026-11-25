use std::collections::HashMap;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use reqwest::{Method, Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use url::Url;

use crate::auth::{build_auth_header, canonicalize_uri};
use crate::errors::{Result, StoreDocError};
use crate::types::StoreDocConfig;

pub const SDK_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_RETRY_DELAY_MS: u64 = 30_000;
const MAX_DOWNLOAD_REDIRECTS: usize = 5;

#[derive(Clone)]
pub struct HttpClient {
    config: StoreDocConfig,
    base_url: Url,
    client: reqwest::Client,
    no_redirect_client: reqwest::Client,
}

impl HttpClient {
    pub fn new(config: StoreDocConfig) -> Result<Self> {
        let raw_url = config.api_url.trim();
        if raw_url.is_empty() {
            return Err(StoreDocError::InvalidConfig(
                "StoreDoc configuration requires \"api_url\"".into(),
            ));
        }

        let normalized = if raw_url.ends_with('/') {
            raw_url.to_string()
        } else {
            format!("{raw_url}/")
        };

        let base_url = Url::parse(&normalized)
            .map_err(|e| StoreDocError::InvalidConfig(format!("Invalid api_url: {e}")))?;

        let host = base_url.host_str().unwrap_or_default();
        let is_local = matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]");
        if base_url.scheme() != "https" && !(base_url.scheme() == "http" && is_local) {
            return Err(StoreDocError::InvalidConfig(
                "StoreDoc api_url must use HTTPS (HTTP is only allowed for localhost).".into(),
            ));
        }

        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| {
                StoreDocError::InvalidConfig(format!("Failed to build HTTP client: {e}"))
            })?;

        let no_redirect_client = reqwest::Client::builder()
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| {
                StoreDocError::InvalidConfig(format!("Failed to build HTTP client: {e}"))
            })?;

        Ok(Self {
            config,
            base_url,
            client,
            no_redirect_client,
        })
    }

    fn build_url(&self, path: &str, query: Option<&[(String, String)]>) -> Result<(Url, String)> {
        let clean = path.trim_start_matches('/');
        let mut url = self.base_url.join(clean).map_err(|e| {
            StoreDocError::InvalidConfig(format!("Invalid request path '{path}': {e}"))
        })?;

        if let Some(params) = query {
            if !params.is_empty() {
                let mut pairs = url.query_pairs_mut();
                for (k, v) in params {
                    pairs.append_pair(k, v);
                }
            }
        }

        let canonical_target = match url.query() {
            Some(q) => format!("{}?{}", url.path(), q),
            None => url.path().to_string(),
        };

        Ok((url, canonicalize_uri(&canonical_target)))
    }

    fn is_idempotent(method: &Method) -> bool {
        matches!(
            *method,
            Method::GET | Method::HEAD | Method::PUT | Method::DELETE
        )
    }

    fn is_retryable_status(status: StatusCode) -> bool {
        status == StatusCode::REQUEST_TIMEOUT
            || status == StatusCode::TOO_MANY_REQUESTS
            || status.is_server_error()
    }

    fn default_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        if let Ok(ua) = HeaderValue::from_str(&format!("storedoc-rust/{SDK_VERSION}")) {
            headers.insert(USER_AGENT, ua);
        }
        headers
    }

    pub async fn request_json<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: Option<&[(String, String)]>,
        body: Option<&impl Serialize>,
    ) -> Result<T> {
        let (url, sign_uri) = self.build_url(path, query)?;
        let body_bytes = match body {
            Some(b) => Some(
                serde_json::to_vec(b).map_err(|e| StoreDocError::Validation {
                    message: format!("Failed to serialize request body: {e}"),
                    errors: None,
                })?,
            ),
            None => None,
        };

        let max_attempts = if Self::is_idempotent(&method) {
            self.config.max_retries.saturating_add(1)
        } else {
            1
        };

        let mut attempt = 0;
        loop {
            attempt += 1;
            let (auth_name, auth_val) =
                build_auth_header(method.as_str(), &sign_uri, &self.config)?;

            let mut req = self
                .client
                .request(method.clone(), url.clone())
                .headers(Self::default_headers())
                .header(auth_name.as_str(), auth_val.as_str());

            if let Some(ref bytes) = body_bytes {
                req = req
                    .header("Content-Type", "application/json")
                    .body(bytes.clone());
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(err) => {
                    if attempt < max_attempts {
                        let delay = self.backoff_delay(attempt);
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    if err.is_timeout() {
                        return Err(StoreDocError::Timeout(err.to_string()));
                    }
                    return Err(StoreDocError::Network(err.to_string()));
                }
            };

            let status = resp.status();
            if status.is_success() {
                if status == StatusCode::NO_CONTENT {
                    return serde_json::from_str("null").map_err(|e| {
                        StoreDocError::InvalidResponse(format!("Empty response decode failed: {e}"))
                    });
                }
                let text = resp
                    .text()
                    .await
                    .map_err(|e| StoreDocError::Network(e.to_string()))?;
                return serde_json::from_str::<T>(&text).map_err(|e| {
                    StoreDocError::InvalidResponse(format!("Failed to parse JSON response: {e}"))
                });
            }

            if attempt < max_attempts && Self::is_retryable_status(status) {
                let delay = self.backoff_delay(attempt);
                tokio::time::sleep(delay).await;
                continue;
            }

            return Err(Self::map_error_response(resp).await);
        }
    }

    pub async fn request_multipart<T: DeserializeOwned>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<T> {
        let (url, sign_uri) = self.build_url(path, None)?;
        let (auth_name, auth_val) = build_auth_header("POST", &sign_uri, &self.config)?;

        let resp = self
            .client
            .post(url)
            .headers(Self::default_headers())
            .header(auth_name.as_str(), auth_val.as_str())
            .multipart(form)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    StoreDocError::Timeout(e.to_string())
                } else {
                    StoreDocError::Network(e.to_string())
                }
            })?;

        if resp.status().is_success() {
            let text = resp
                .text()
                .await
                .map_err(|e| StoreDocError::Network(e.to_string()))?;
            return serde_json::from_str::<T>(&text).map_err(|e| {
                StoreDocError::InvalidResponse(format!("Failed to parse JSON response: {e}"))
            });
        }

        Err(Self::map_error_response(resp).await)
    }

    /// Download raw response, manually following redirects and stripping
    /// StoreDoc credentials if redirected off-origin (e.g. to S3/CDN).
    pub async fn download_raw(&self, path: &str) -> Result<Response> {
        let (mut current_url, sign_uri) = self.build_url(path, None)?;
        let (auth_name, auth_val) = build_auth_header("GET", &sign_uri, &self.config)?;

        let mut include_auth = true;
        for _ in 0..=MAX_DOWNLOAD_REDIRECTS {
            let mut req = self.no_redirect_client.get(current_url.clone());
            if include_auth {
                req = req.header(auth_name.as_str(), auth_val.as_str());
            }

            let resp = req.send().await.map_err(|e| {
                if e.is_timeout() {
                    StoreDocError::Timeout(e.to_string())
                } else {
                    StoreDocError::Network(e.to_string())
                }
            })?;

            if resp.status().is_redirection() {
                let location = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        StoreDocError::InvalidResponse(
                            "Redirect response missing Location header".into(),
                        )
                    })?;

                let next_url = current_url.join(location).map_err(|e| {
                    StoreDocError::InvalidResponse(format!("Invalid redirect URL: {e}"))
                })?;

                if next_url.origin() != self.base_url.origin() {
                    include_auth = false;
                }
                current_url = next_url;
                continue;
            }

            if !resp.status().is_success() {
                return Err(Self::map_error_response(resp).await);
            }

            return Ok(resp);
        }

        Err(StoreDocError::Network(
            "Too many redirects while downloading object".into(),
        ))
    }

    fn backoff_delay(&self, attempt: u32) -> Duration {
        let base = self.config.retry_delay.as_millis() as u64;
        let factor = 1u64
            .checked_shl(attempt.saturating_sub(1))
            .unwrap_or(u64::MAX);
        Duration::from_millis(base.saturating_mul(factor).min(MAX_RETRY_DELAY_MS))
    }

    async fn map_error_response(mut resp: Response) -> StoreDocError {
        let status = resp.status().as_u16();
        // Bound error body reads to 256 KiB to prevent OOM on hostile responses.
        const MAX_ERROR_BYTES: usize = 256 * 1024;
        let mut buf = Vec::new();
        while let Ok(Some(chunk)) = resp.chunk().await {
            let remaining = MAX_ERROR_BYTES.saturating_sub(buf.len());
            if remaining == 0 {
                break;
            }
            let take = chunk.len().min(remaining);
            buf.extend_from_slice(&chunk[..take]);
            if buf.len() >= MAX_ERROR_BYTES {
                break;
            }
        }
        let text = String::from_utf8_lossy(&buf);
        let truncated: String = if text.chars().count() > 2000 {
            let prefix: String = text.chars().take(2000).collect();
            format!("{prefix}...[TRUNCATED]")
        } else {
            text.into_owned()
        };

        let parsed: Option<serde_json::Value> = serde_json::from_str(&truncated).ok();
        let message = parsed
            .as_ref()
            .and_then(|v| v.get("message").and_then(|m| m.as_str()))
            .map(str::to_string)
            .unwrap_or_else(|| {
                if truncated.is_empty() {
                    format!("HTTP {status}")
                } else {
                    truncated.clone()
                }
            });

        match status {
            401 => StoreDocError::Authentication(message),
            403 => StoreDocError::Forbidden(message),
            404 => StoreDocError::NotFound(message),
            422 => {
                let errors = parsed.as_ref().and_then(|v| v.get("errors")).and_then(|e| {
                    serde_json::from_value::<HashMap<String, Vec<String>>>(e.clone()).ok()
                });
                StoreDocError::Validation { message, errors }
            }
            429 => StoreDocError::RateLimit(message),
            _ => {
                let code = parsed
                    .as_ref()
                    .and_then(|v| v.get("code").and_then(|c| c.as_str()))
                    .map(str::to_string);
                StoreDocError::Api {
                    status,
                    code,
                    message,
                }
            }
        }
    }
}
