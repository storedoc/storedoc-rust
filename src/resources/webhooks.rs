use reqwest::Method;
use serde::Deserialize;

use crate::auth::verify_webhook_signature;
use crate::errors::Result;
use crate::http::HttpClient;
use crate::types::{StorageWebhook, WebhookTestResult};
use crate::validation::assert_uuid;

#[derive(Clone)]
pub struct WebhooksResource {
    http: HttpClient,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WebhookListResponse {
    Array(Vec<StorageWebhook>),
    Wrapped { data: Option<Vec<StorageWebhook>> },
}

#[derive(Deserialize)]
struct RawWebhookTestResponse {
    message: Option<String>,
    data: Option<serde_json::Value>,
}

impl WebhooksResource {
    pub(crate) fn new(http: HttpClient) -> Self {
        Self { http }
    }

    pub async fn list(&self, bucket_id: Option<&str>) -> Result<Vec<StorageWebhook>> {
        let query = match bucket_id {
            Some(id) => {
                assert_uuid(id, "bucket_id")?;
                Some(vec![("bucket_id".to_string(), id.to_string())])
            }
            None => None,
        };

        let res: WebhookListResponse = self
            .http
            .request_json(Method::GET, "storage/webhooks", query.as_deref(), None::<&()>)
            .await?;

        Ok(match res {
            WebhookListResponse::Array(items) => items,
            WebhookListResponse::Wrapped { data } => data.unwrap_or_default(),
        })
    }

    pub async fn test(&self, webhook_id: &str) -> Result<WebhookTestResult> {
        assert_uuid(webhook_id, "webhook_id")?;
        let res: RawWebhookTestResponse = self
            .http
            .request_json(
                Method::POST,
                &format!("storage/webhooks/{webhook_id}/test"),
                None,
                None::<&()>,
            )
            .await?;

        let message = res
            .message
            .unwrap_or_else(|| "Webhook test completed".to_string());
        let lower = message.to_ascii_lowercase();
        let success = lower.contains("successful") && !lower.contains("failed");

        let status_code = res
            .data
            .as_ref()
            .and_then(|d| d.get("response"))
            .and_then(|r| r.get("status_code"))
            .and_then(|s| s.as_u64())
            .and_then(|s| u16::try_from(s).ok());

        let response = res
            .data
            .as_ref()
            .and_then(|d| d.get("response"))
            .and_then(|r| r.get("body"))
            .and_then(|b| b.as_str())
            .map(str::to_string);

        Ok(WebhookTestResult {
            success,
            message,
            status_code,
            response,
            data: res.data,
        })
    }

    pub fn verify_signature(
        &self,
        payload: &[u8],
        signature: &str,
        secret: &str,
        timestamp: Option<u64>,
        max_age_seconds: Option<u64>,
    ) -> bool {
        verify_webhook_signature(payload, signature, secret, timestamp, max_age_seconds)
    }
}
