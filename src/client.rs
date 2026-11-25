use crate::errors::Result;
use crate::http::HttpClient;
use crate::resources::buckets::BucketsResource;
use crate::resources::objects::ObjectsResource;
use crate::resources::webhooks::WebhooksResource;
use crate::types::StoreDocConfig;

#[derive(Clone)]
pub struct StoreDocClient {
    http: HttpClient,
}

impl StoreDocClient {
    pub fn new(config: StoreDocConfig) -> Result<Self> {
        let http = HttpClient::new(config)?;
        Ok(Self { http })
    }

    pub fn buckets(&self) -> BucketsResource {
        BucketsResource::new(self.http.clone())
    }

    pub fn objects(&self) -> ObjectsResource {
        ObjectsResource::new(self.http.clone())
    }

    pub fn webhooks(&self) -> WebhooksResource {
        WebhooksResource::new(self.http.clone())
    }
}
