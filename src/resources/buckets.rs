use reqwest::Method;
use serde::Deserialize;

use crate::errors::{Result, StoreDocError};
use crate::http::HttpClient;
use crate::types::{
    BucketStats, CreateBucketParams, MessageResponse, StorageBucket, VerifyDomainResult,
};
use crate::validation::assert_uuid;

#[derive(Clone)]
pub struct BucketsResource {
    http: HttpClient,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BucketListResponse {
    Array(Vec<StorageBucket>),
    Wrapped { data: Option<Vec<StorageBucket>> },
}

impl BucketsResource {
    pub(crate) fn new(http: HttpClient) -> Self {
        Self { http }
    }

    pub async fn list(&self) -> Result<Vec<StorageBucket>> {
        let res: BucketListResponse = self
            .http
            .request_json(Method::GET, "storage/buckets", None, None::<&()>)
            .await?;
        Ok(match res {
            BucketListResponse::Array(items) => items,
            BucketListResponse::Wrapped { data } => data.unwrap_or_default(),
        })
    }

    pub async fn get(&self, bucket_id: &str) -> Result<StorageBucket> {
        assert_uuid(bucket_id, "bucket_id")?;
        self.http
            .request_json(
                Method::GET,
                &format!("storage/buckets/{bucket_id}"),
                None,
                None::<&()>,
            )
            .await
    }

    pub async fn create(&self, params: &CreateBucketParams) -> Result<StorageBucket> {
        self.http
            .request_json(Method::POST, "storage/buckets", None, Some(params))
            .await
    }

    pub async fn delete(&self, bucket_id: &str) -> Result<MessageResponse> {
        assert_uuid(bucket_id, "bucket_id")?;
        self.http
            .request_json(
                Method::DELETE,
                &format!("storage/buckets/{bucket_id}"),
                None,
                None::<&()>,
            )
            .await
    }

    pub async fn stats(&self, bucket_id: &str, days: Option<u32>) -> Result<BucketStats> {
        assert_uuid(bucket_id, "bucket_id")?;
        let days = days.unwrap_or(30);
        if !(1..=365).contains(&days) {
            return Err(StoreDocError::Validation {
                message: "Invalid days: expected an integer between 1 and 365.".into(),
                errors: None,
            });
        }
        let query = vec![("days".to_string(), days.to_string())];
        self.http
            .request_json(
                Method::GET,
                &format!("storage/buckets/{bucket_id}/stats"),
                Some(&query),
                None::<&()>,
            )
            .await
    }

    pub async fn verify_domain(
        &self,
        bucket_id: &str,
        domain: Option<&str>,
    ) -> Result<VerifyDomainResult> {
        assert_uuid(bucket_id, "bucket_id")?;
        #[derive(serde::Serialize)]
        struct Body<'a> {
            domain: Option<&'a str>,
        }

        match self
            .http
            .request_json(
                Method::POST,
                &format!("storage/buckets/{bucket_id}/verify-domain"),
                None,
                Some(&Body { domain }),
            )
            .await
        {
            Ok(res) => Ok(res),
            Err(StoreDocError::Validation { message, .. }) => Ok(VerifyDomainResult {
                success: Some(false),
                verified: false,
                domain: domain.unwrap_or_default().to_string(),
                message: Some(message),
                target: None,
                ssl_status: None,
            }),
            Err(e) => Err(e),
        }
    }
}
