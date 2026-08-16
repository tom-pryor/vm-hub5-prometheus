pub mod models;

use std::time::Duration;

use models::{DownstreamResponse, EventlogResponse, UpstreamResponse};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HubError {
    #[error("request to hub failed: {0}")]
    Request(#[from] reqwest::Error),
}

/// Client for the Virgin Media Hub 5's cable modem REST API.
#[derive(Debug, Clone)]
pub struct HubClient {
    http: reqwest::Client,
    base_url: String,
}

impl HubClient {
    pub fn new(base_url: String, timeout: Duration, insecure_skip_verify: bool) -> Self {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .danger_accept_invalid_certs(insecure_skip_verify)
            .build()
            .expect("failed to build HTTP client");

        Self { http, base_url }
    }

    pub async fn fetch_downstream(&self) -> Result<DownstreamResponse, HubError> {
        let url = format!("{}/rest/v1/cablemodem/downstream", self.base_url);
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn fetch_upstream(&self) -> Result<UpstreamResponse, HubError> {
        let url = format!("{}/rest/v1/cablemodem/upstream", self.base_url);
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn fetch_eventlog(&self) -> Result<EventlogResponse, HubError> {
        let url = format!("{}/rest/v1/cablemodem/eventlog", self.base_url);
        Ok(self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}
