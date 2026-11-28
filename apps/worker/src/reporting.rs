//! Hub HTTP calls the Worker makes: claim a Task, and reconcile Progress, Result, and
//! outcome-free Finish. Every call carries the Worker credential; report calls are fenced
//! by the live `connectionEpoch`, and the Hub rechecks ownership at its write boundary.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use myrlet_protocol::{
    ClaimTaskRequest, ClaimTaskResponse, DeliveryId, DeliveryResultView, ErrorResponse,
    FinishReceipt, FinishReportRequest, ProgressReceipt, ProgressReportRequest,
    ProgressValidationError, ResultReceipt, ResultReportRequest, ResultValidationError,
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

    /// Ask the Hub to commit one claim. `Ok(None)` means this Worker lost the race.
    pub async fn claim(
        &self,
        request: &ClaimTaskRequest,
    ) -> Result<Option<ClaimTaskResponse>, ReportError> {
        let url = format!("{}/v1/workers/claim", self.base_url());
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.bearer)
            .json(request)
            .send()
            .await
            .map_err(|error| ReportError::Transport(error.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ReportError::Transport(error.to_string()))?;
        if status.is_success() {
            return serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|error| ReportError::Transport(format!("invalid claim reply: {error}")));
        }
        // A ready Task another Worker already claimed, or a Task that is no longer queued.
        if status.as_u16() == 409 {
            return Ok(None);
        }
        Err(self.classify(request.task_id.as_str(), status.as_u16(), &bytes))
    }

    /// Relay the latest Progress value (`POST .../progress`).
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

    /// Reconcile the Hub copy with the current local Result (`Some` writes, `None` clears).
    pub async fn post_result(
        &self,
        delivery_id: &DeliveryId,
        request: &ResultReportRequest,
    ) -> Result<ResultReceipt, ReportError> {
        request.validate()?;
        let body = self.post(delivery_id, "result", request).await?;
        serde_json::from_slice(&body)
            .map_err(|error| ReportError::Transport(format!("invalid Result receipt: {error}")))
    }

    /// Read the Hub's current Result copy for one owned Delivery without writing.
    pub async fn get_result(
        &self,
        delivery_id: &DeliveryId,
    ) -> Result<DeliveryResultView, ReportError> {
        let url = self.delivery_url(delivery_id, "result")?;
        let response = self
            .http
            .get(url)
            .bearer_auth(&self.bearer)
            .send()
            .await
            .map_err(|error| ReportError::Transport(error.to_string()))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| ReportError::Transport(error.to_string()))?;
        if !status.is_success() {
            return Err(self.classify(delivery_id.as_str(), status.as_u16(), &bytes));
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| ReportError::Transport(format!("invalid result view: {error}")))
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
            return Err(self.classify(delivery_id.as_str(), status.as_u16(), &bytes));
        }
        Ok(bytes.to_vec())
    }

    fn base_url(&self) -> &str {
        self.hub_url.trim_end_matches('/')
    }

    fn delivery_url(&self, delivery_id: &DeliveryId, action: &str) -> Result<String, ReportError> {
        if !delivery_id.is_valid_segment() {
            return Err(ReportError::InvalidDeliveryId(delivery_id.to_string()));
        }
        Ok(format!(
            "{}/v1/worker/deliveries/{delivery_id}/{action}",
            self.base_url()
        ))
    }

    fn classify(&self, id: &str, status: u16, body: &[u8]) -> ReportError {
        match status {
            409 => ReportError::StaleDelivery(id.to_owned()),
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

/// Progress/Finish/Result reporting failures.
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("delivery {0} is fenced to a different connection epoch")]
    StaleDelivery(String),
    #[error("Hub rejected the Worker credential")]
    Unauthorized,
    #[error("delivery id {0:?} is not a safe path segment")]
    InvalidDeliveryId(String),
    #[error("Hub request failed: {0}")]
    Transport(String),
    #[error("Hub responded with status {status}: {message}")]
    Response { status: u16, message: String },
    #[error(transparent)]
    InvalidProgress(#[from] ProgressValidationError),
    #[error(transparent)]
    InvalidResult(#[from] ResultValidationError),
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
