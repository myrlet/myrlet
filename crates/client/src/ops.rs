//! Hub Task control-plane operations over any [`Transport`].

use std::fmt;

use myrlet_protocol::{
    CancelTaskRequest, ClaimTaskRequest, ClaimTaskResponse, FinishTaskRequest, GlobalSnapshot,
    HealthResponse, TaskBundleImportRequest, TaskBundleImportResponse, TaskDefinition, TaskId,
    TaskListResponse, TaskReadResponse, TaskRecord, TaskTreeDirection, TaskTreeResponse,
    UpdateTaskRequest, SNAPSHOT_SCHEMA_VERSION,
};
use serde::de::DeserializeOwned;

use crate::error::ClientError;
#[cfg(feature = "native")]
use crate::stream::EventStream;
#[cfg(feature = "native")]
use crate::transport::NativeTransport;
use crate::transport::Transport;
use crate::wire::encode_json;

/// Authenticated Hub client. `Debug` never reveals the bearer credential.
#[cfg(feature = "native")]
pub struct Client<T = NativeTransport> {
    pub(crate) base_url: String,
    pub(crate) bearer: String,
    pub(crate) transport: T,
}

/// Authenticated Hub client for transports supplied by the embedder.
#[cfg(not(feature = "native"))]
pub struct Client<T> {
    pub(crate) base_url: String,
    pub(crate) bearer: String,
    pub(crate) transport: T,
}

impl<T> fmt::Debug for Client<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.base_url)
            .field("bearer", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl<T: Transport> Client<T> {
    /// Build a client over an existing transport, validating and normalizing the base URL.
    pub fn with_transport(
        base_url: impl Into<String>,
        bearer: impl Into<String>,
        transport: T,
    ) -> Result<Self, ClientError> {
        Ok(Self {
            base_url: normalize_base_url(&base_url.into())?,
            bearer: bearer.into(),
            transport,
        })
    }

    /// Unauthenticated liveness probe (`GET /v1/health`).
    pub async fn health(&self) -> Result<HealthResponse, ClientError> {
        decode_direct(
            &self
                .exchange("GET", "/v1/health", false, None, None)
                .await?,
        )
    }

    /// Consistent cluster read (`GET /v1/snapshot`); fails closed on an unknown schema version.
    pub async fn snapshot(&self) -> Result<GlobalSnapshot, ClientError> {
        let snapshot: GlobalSnapshot = decode_direct(&self.get("/v1/snapshot").await?)?;
        if snapshot.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(ClientError::Decode(format!(
                "unsupported snapshot schema version {}",
                snapshot.schema_version
            )));
        }
        Ok(snapshot)
    }

    /// Bounded Task summary page (`GET /v1/tasks`); readable with either credential.
    pub async fn list_tasks(&self, limit: Option<usize>) -> Result<TaskListResponse, ClientError> {
        let path = match limit {
            Some(limit) => format!("/v1/tasks?limit={limit}"),
            None => "/v1/tasks".to_string(),
        };
        decode_direct(&self.get(&path).await?)
    }

    /// Submit one Task, authorizing it for execution (`POST /v1/tasks`).
    pub async fn create_task(
        &self,
        idempotency_key: &str,
        def: &TaskDefinition,
    ) -> Result<TaskRecord, ClientError> {
        let body = self
            .post("/v1/tasks", Some(idempotency_key), encode_json(def)?)
            .await?;
        let response: myrlet_protocol::SubmitTaskResponse = decode_direct(&body)?;
        Ok(response.task)
    }

    /// Submit an atomic Task bundle (`POST /v1/task-bundles/import`).
    pub async fn import_bundle(
        &self,
        idempotency_key: &str,
        req: &TaskBundleImportRequest,
    ) -> Result<TaskBundleImportResponse, ClientError> {
        let path = "/v1/task-bundles/import";
        let body = self
            .post(path, Some(idempotency_key), encode_json(req)?)
            .await?;
        decode_direct(&body)
    }

    /// Read one Task within the credential's visibility (`GET /v1/tasks/{id}`).
    ///
    /// A global reader receives the full record; a `worker`-tier Worker may receive a
    /// one-hop related summary instead.
    pub async fn get_task(&self, id: &TaskId) -> Result<TaskReadResponse, ClientError> {
        ensure_path_segment(id.as_str())?;
        decode_direct(&self.get(&format!("/v1/tasks/{id}")).await?)
    }

    /// Read bounded ancestors or descendants (`GET /v1/tasks/{id}/tree`).
    pub async fn get_task_tree(
        &self,
        id: &TaskId,
        direction: TaskTreeDirection,
        max_depth: Option<u8>,
        limit: Option<usize>,
    ) -> Result<TaskTreeResponse, ClientError> {
        ensure_path_segment(id.as_str())?;
        let direction = match direction {
            TaskTreeDirection::Ancestors => "ancestors",
            TaskTreeDirection::Descendants => "descendants",
        };
        let mut path = format!("/v1/tasks/{id}/tree?direction={direction}");
        if let Some(depth) = max_depth {
            path.push_str(&format!("&maxDepth={depth}"));
        }
        if let Some(limit) = limit {
            path.push_str(&format!("&limit={limit}"));
        }
        decode_direct(&self.get(&path).await?)
    }

    /// Apply version-fenced edits to an unclaimed Task (`POST /v1/tasks/{id}`).
    pub async fn update_task(
        &self,
        idempotency_key: &str,
        id: &TaskId,
        req: &UpdateTaskRequest,
    ) -> Result<TaskRecord, ClientError> {
        ensure_path_segment(id.as_str())?;
        let path = format!("/v1/tasks/{id}");
        let body = self
            .post(&path, Some(idempotency_key), encode_json(req)?)
            .await?;
        decode_direct(&body)
    }

    /// Cancel any non-terminal Task (`POST /v1/tasks/{id}/cancel`).
    pub async fn cancel_task(
        &self,
        idempotency_key: &str,
        id: &TaskId,
        req: &CancelTaskRequest,
    ) -> Result<TaskRecord, ClientError> {
        ensure_path_segment(id.as_str())?;
        let path = format!("/v1/tasks/{id}/cancel");
        let body = self
            .post(&path, Some(idempotency_key), encode_json(req)?)
            .await?;
        decode_direct(&body)
    }

    /// Finish a coordination-only Task (`POST /v1/tasks/{id}/finish`).
    pub async fn finish_task(
        &self,
        idempotency_key: &str,
        id: &TaskId,
        req: &FinishTaskRequest,
    ) -> Result<TaskRecord, ClientError> {
        ensure_path_segment(id.as_str())?;
        let path = format!("/v1/tasks/{id}/finish");
        let body = self
            .post(&path, Some(idempotency_key), encode_json(req)?)
            .await?;
        decode_direct(&body)
    }

    /// Claim one queued Task for this Worker (`POST /v1/workers/claim`).
    pub async fn claim_task(
        &self,
        req: &ClaimTaskRequest,
    ) -> Result<ClaimTaskResponse, ClientError> {
        let body = self
            .post("/v1/workers/claim", None, encode_json(req)?)
            .await?;
        decode_direct(&body)
    }

    /// Authenticated `GET` returning the raw body on a 2xx status.
    pub(crate) async fn get(&self, path: &str) -> Result<Vec<u8>, ClientError> {
        self.exchange("GET", path, true, None, None).await
    }

    /// Authenticated `POST` with an explicit optional idempotency key and request body.
    pub(crate) async fn post(
        &self,
        path: &str,
        idempotency_key: Option<&str>,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, ClientError> {
        self.exchange("POST", path, true, idempotency_key, Some(body))
            .await
    }

    /// Send one request and map non-2xx responses onto typed [`ClientError`]s.
    pub(crate) async fn exchange(
        &self,
        method: &str,
        path: &str,
        authenticated: bool,
        idempotency_key: Option<&str>,
        body: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, ClientError> {
        let url = format!("{}{path}", self.base_url);
        let bearer = authenticated.then_some(self.bearer.as_str());
        let response = self
            .transport
            .send(method, &url, bearer, idempotency_key, body)
            .await?;
        if (200..300).contains(&response.status) {
            return Ok(response.body);
        }
        let status = response.status;
        Err(ClientError::from_http_response(status, &response.body))
    }
}

#[cfg(feature = "native")]
impl Client<NativeTransport> {
    /// Build a native client with a fresh verifying `reqwest` transport.
    pub fn new(
        base_url: impl Into<String>,
        bearer: impl Into<String>,
    ) -> Result<Self, ClientError> {
        Self::with_transport(base_url, bearer, NativeTransport::new()?)
    }

    /// Resumable SSE stream (`GET /v1/events`), resuming from `cursor` when given.
    pub async fn open_events(&self, cursor: Option<u64>) -> Result<EventStream, ClientError> {
        let url = format!("{}/v1/events", self.base_url);
        let cursor = cursor.map(|seq| seq.to_string());
        self.transport
            .open_sse(&url, &self.bearer, cursor.as_deref())
            .await
    }
}

/// Single-pass JSON decode into a canonical DTO (never routes through `serde_json::Value`).
pub fn decode_direct<R: DeserializeOwned>(body: &[u8]) -> Result<R, ClientError> {
    serde_json::from_slice(body).map_err(|error| ClientError::Decode(error.to_string()))
}

/// Reject IDs that could escape their single URL path segment.
pub(crate) fn ensure_path_segment(segment: &str) -> Result<(), ClientError> {
    let safe = !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.contains(['/', '\\', '?', '#']);
    if !safe {
        return Err(ClientError::InvalidUrl(format!(
            "invalid path segment {segment:?}"
        )));
    }
    Ok(())
}

/// Require an absolute `http(s)` Hub URL with no query or fragment; trailing `/` is trimmed.
fn normalize_base_url(raw: &str) -> Result<String, ClientError> {
    let parsed = url::Url::parse(raw).map_err(|_| ClientError::InvalidUrl(raw.to_string()))?;
    let absolute = matches!(parsed.scheme(), "http" | "https") && parsed.host().is_some();
    if !absolute || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(ClientError::InvalidUrl(raw.to_string()));
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}
