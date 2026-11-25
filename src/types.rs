use std::collections::HashMap;
use std::time::Duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    #[default]
    Hmac,
    ApiKey,
    Bearer,
}

#[derive(Debug, Clone)]
pub struct StoreDocConfig {
    pub api_url: String,
    pub access_key_id: Option<String>,
    pub secret_key: Option<String>,
    pub token: Option<String>,
    pub auth_mode: AuthMode,
    pub timeout: Duration,
    pub max_retries: u32,
    pub retry_delay: Duration,
}

impl StoreDocConfig {
    pub fn new(api_url: impl Into<String>) -> Self {
        Self {
            api_url: api_url.into(),
            access_key_id: None,
            secret_key: None,
            token: None,
            auth_mode: AuthMode::Hmac,
            timeout: Duration::from_secs(60),
            max_retries: 2,
            retry_delay: Duration::from_millis(500),
        }
    }

    pub fn with_hmac(mut self, access_key_id: impl Into<String>, secret_key: impl Into<String>) -> Self {
        self.access_key_id = Some(access_key_id.into());
        self.secret_key = Some(secret_key.into());
        self.auth_mode = AuthMode::Hmac;
        self
    }

    pub fn with_api_key(mut self, access_key_id: impl Into<String>, secret_key: impl Into<String>) -> Self {
        self.access_key_id = Some(access_key_id.into());
        self.secret_key = Some(secret_key.into());
        self.auth_mode = AuthMode::ApiKey;
        self
    }

    pub fn with_bearer(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self.auth_mode = AuthMode::Bearer;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BucketVisibility {
    Private,
    Public,
    Authenticated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectVisibility {
    Inherit,
    Private,
    Public,
    Authenticated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketSettings {
    pub versioning_enabled: Option<bool>,
    pub encryption_enabled: Option<bool>,
    pub encryption_algorithm: Option<String>,
    pub cdn_enabled: Option<bool>,
    pub cdn_url: Option<String>,
    pub custom_domain: Option<String>,
    pub custom_domain_verified: Option<bool>,
    pub block_public_access: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketStatsSummary {
    pub total_size: Option<u64>,
    pub total_size_formatted: Option<String>,
    pub total_files: Option<u64>,
    pub total_folders: Option<u64>,
    pub bandwidth_used: Option<u64>,
    pub bandwidth_used_formatted: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBucket {
    pub id: String,
    pub name: String,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub region: String,
    pub visibility: String,
    pub endpoint: Option<String>,
    pub settings: Option<BucketSettings>,
    pub stats: Option<BucketStatsSummary>,
    pub tags: Option<Vec<String>>,
    pub status: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateBucketParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<BucketVisibility>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub versioning_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_enabled: Option<bool>,
}

impl CreateBucketParams {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            visibility: Some(BucketVisibility::Private),
            region: Some("us-east-1".to_string()),
            description: None,
            versioning_enabled: None,
            encryption_enabled: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketStats {
    pub total_uploads: u64,
    pub total_downloads: u64,
    pub total_bandwidth_in: u64,
    pub total_bandwidth_out: u64,
    pub total_requests: u64,
    pub daily_stats: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyDomainResult {
    pub success: Option<bool>,
    pub verified: bool,
    pub domain: String,
    pub message: Option<String>,
    pub target: Option<String>,
    pub ssl_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageObjectUrls {
    pub public: Option<String>,
    pub cdn: Option<String>,
    pub download: Option<String>,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageObject {
    pub id: String,
    pub bucket_id: String,
    pub folder_id: Option<String>,
    pub name: String,
    pub original_name: String,
    pub key: String,
    pub path: String,
    pub size: u64,
    pub size_formatted: Option<String>,
    pub mime_type: String,
    pub extension: Option<String>,
    pub etag: Option<String>,
    pub visibility: String,
    pub effective_visibility: Option<String>,
    pub is_public: Option<bool>,
    pub urls: Option<StorageObjectUrls>,
    pub metadata: Option<serde_json::Value>,
    pub custom_metadata: Option<HashMap<String, String>>,
    pub tags: Option<Vec<String>>,
    pub status: Option<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ListObjectsParams {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
    pub search: Option<String>,
    pub folder_id: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResult<T> {
    pub data: Vec<T>,
    pub meta: Option<serde_json::Value>,
    pub links: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct UploadObjectOptions {
    pub path: Option<String>,
    pub visibility: Option<ObjectVisibility>,
    pub folder_id: Option<String>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub content_encoding: Option<String>,
    pub cache_control: Option<String>,
    pub custom_metadata: Option<HashMap<String, String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct PresignedUrlOptions {
    pub minutes: Option<u32>,
    pub action: Option<String>,
    pub restrict_ip: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresignedUrlResult {
    pub url: String,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresignedUploadParams {
    pub filename: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub file_size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresignedUploadResult {
    pub upload_url: Option<String>,
    pub url: Option<String>,
    pub method: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ChunkedUploadOptions {
    pub path: Option<String>,
    pub visibility: Option<ObjectVisibility>,
    pub folder_id: Option<String>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub content_encoding: Option<String>,
    pub cache_control: Option<String>,
    pub custom_metadata: Option<HashMap<String, String>>,
    pub tags: Option<Vec<String>>,
    pub chunk_size: Option<usize>,
    pub concurrency: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadProgress {
    pub chunk_number: usize,
    pub chunks_received: usize,
    pub total_chunks: usize,
    pub percent: u8,
    pub bytes_uploaded: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageWebhook {
    pub id: String,
    pub bucket_id: Option<String>,
    pub url: String,
    pub events: Option<Vec<String>>,
    pub is_active: Option<bool>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTestResult {
    pub success: bool,
    pub message: String,
    pub status_code: Option<u16>,
    pub response: Option<String>,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageResponse {
    pub message: String,
}
