//! Hub HTTP reporting: Harness-authored Progress and outcome-free Finish. Both calls are
//! authenticated with the Worker credential and fenced by the current `connectionEpoch`.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use myrlet_protocol::{
    DeliveryId, ErrorResponse, FinishReceipt, FinishReportRequest, ProgressReceipt,
    ProgressReportRequest, ProgressValidationError,
};
use serde::Serialize;

/// Duplicate-finish guard: one in-flight `/finish` per Delivery, released on drop.
struct FinishGuard {
    in_flight: Arc<Mutex<HashSet<DeliveryId>>>,
    delivery_id: DeliveryId,
}

impl Drop for FinishGuard {
    fn drop(&mut self) {
        self.in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.delivery_id);
    }
}

/// Worker-authenticated HTTP reporter for one Hub.
#[derive(Clone)]
pub struct HubReporter {
    hub_url: String,
    bearer: String,
    http: reqwest::Client,
    in_flight_finish: Arc<Mutex<HashSet<DeliveryId>>>,
}

impl HubReporter {
    pub fn new(
        hub_url: impl Into<String>,
        bearer: impl Into<String>,
        http: reqwest::Client,
    ) -> Self {
        Self {
            hub_url: hub_url.into(),
            bearer: bearer.into(),
            http,
            in_flight_finish: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Relay the latest Progress snapshot (`POST .../progress`).
    pub async fn post_progress(
        &self,
        delivery_id: &DeliveryId,
        request: &ProgressReportRequest,
    ) -> Result<ProgressReceipt, ReportError> {
        request.validate()?;
        let body = self.post(delivery_id, "progress", request).await?;
        serde_json::from_slice(&body)
            .map_err(|error| ReportError::Transport(format!("invalid Progress receipt: {error}")))
    }

    /// Report end of handling (`POST .../finish`). `Ok(None)` means an identical report
    /// is already in flight for this Delivery, so this call did not post anything.
    pub async fn post_finish(
        &self,
        delivery_id: &DeliveryId,
        epoch: u64,
    ) -> Result<Option<FinishReceipt>, ReportError> {
        {
            let mut in_flight = self
                .in_flight_finish
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !in_flight.insert(delivery_id.clone()) {
                return Ok(None);
            }
        }
        let _guard = FinishGuard {
            in_flight: Arc::clone(&self.in_flight_finish),
            delivery_id: delivery_id.clone(),
        };
        let body = self
            .post(delivery_id, "finish", &FinishReportRequest { epoch })
            .await?;
        let receipt: FinishReceipt = serde_json::from_slice(&body)
            .map_err(|error| ReportError::Transport(format!("invalid Finish receipt: {error}")))?;
        Ok(Some(receipt))
    }

    async fn post<B: Serialize>(
        &self,
        delivery_id: &DeliveryId,
        action: &str,
        body: &B,
    ) -> Result<Vec<u8>, ReportError> {
        let url = self.delivery_url(delivery_id, action)?;
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.bearer)
            .json(body)
            .send()
            .await
            .map_err(|error| ReportError::Transport(error.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ReportError::Transport(error.to_string()))?;
        if !status.is_success() {
            return Err(self.classify(delivery_id, status.as_u16(), &bytes));
        }
        Ok(bytes.to_vec())
    }

    fn delivery_url(&self, delivery_id: &DeliveryId, action: &str) -> Result<String, ReportError> {
        if !delivery_id.is_valid_segment() {
            return Err(ReportError::InvalidDeliveryId(delivery_id.to_string()));
        }
        Ok(format!(
            "{}/v1/worker/deliveries/{delivery_id}/{action}",
            self.hub_url.trim_end_matches('/')
        ))
    }

    fn classify(&self, delivery_id: &DeliveryId, status: u16, body: &[u8]) -> ReportError {
        match status {
            409 => ReportError::StaleDelivery(delivery_id.clone()),
            401 | 403 => ReportError::Unauthorized,
            _ => ReportError::Response {
                status,
                message: serde_json::from_slice::<ErrorResponse>(body)
                    .map(|error| error.message)
                    .unwrap_or_else(|_| String::from_utf8_lossy(body).into_owned()),
            },
        }
    }
}

/// Progress/Finish reporting failures.
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("delivery {0} is fenced to a different connection epoch")]
    StaleDelivery(DeliveryId),
    #[error("Hub rejected the Worker credential")]
    Unauthorized,
    #[error("delivery id {0:?} is not a safe path segment")]
    InvalidDeliveryId(String),
    #[error("Hub request failed: {0}")]
    Transport(String),
    #[error("Hub responded with status {status}: {message}")]
    Response { status: u16, message: String },
    #[error(transparent)]
    Invalid(#[from] ProgressValidationError),
}

impl ReportError {
    /// Whether retrying later can plausibly succeed (the Delivery stays pending).
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Response { status, .. } => *status >= 500 || *status == 429,
            _ => false,
        }
    }
}
