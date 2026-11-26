//! Transport boundary: a pluggable `Transport` trait plus the native `reqwest` implementation.

use async_trait::async_trait;

use crate::error::ClientError;
#[cfg(feature = "native")]
use crate::stream::{self, EventStream};

/// Minimal response shape the operation layer needs, free of transport types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Pluggable HTTP transport so one operation layer serves native and WASM builds.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send one request; `bearer` and `idempotency_key` are omitted when `None`.
    async fn send(
        &self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        idempotency_key: Option<&str>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, ClientError>;
}

/// `reqwest`/Tokio transport used by native Clients (CLI, App, Hub-side tooling).
#[cfg(feature = "native")]
pub struct NativeTransport {
    http: reqwest::Client,
}

#[cfg(feature = "native")]
impl NativeTransport {
    /// Build a transport with certificate verification left fully enabled.
    pub fn new() -> Result<Self, ClientError> {
        reqwest::Client::builder()
            .build()
            .map(|http| Self { http })
            .map_err(transport_error)
    }

    /// Open the Hub SSE stream and pump decoded events into a bounded receiver.
    pub async fn open_sse(
        &self,
        url: &str,
        bearer: &str,
        last_event_id: Option<&str>,
    ) -> Result<EventStream, ClientError> {
        let mut request = self
            .http
            .get(url)
            .bearer_auth(bearer)
            .header(reqwest::header::ACCEPT, "text/event-stream");
        if let Some(id) = last_event_id {
            request = request.header("Last-Event-ID", id);
        }
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let body = response.bytes().await.map_err(transport_error)?;
            return Err(ClientError::from_http_response(status, &body));
        }
        Ok(stream::connect(response))
    }
}

#[cfg(feature = "native")]
#[async_trait]
impl Transport for NativeTransport {
    async fn send(
        &self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        idempotency_key: Option<&str>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, ClientError> {
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|error| ClientError::Transport(format!("invalid HTTP method: {error}")))?;
        let mut request = self.http.request(method, url);
        if let Some(bearer) = bearer {
            request = request.bearer_auth(bearer);
        }
        if let Some(key) = idempotency_key {
            request = request.header("Idempotency-Key", key);
        }
        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        let response = request.send().await.map_err(transport_error)?;
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(transport_error)?.to_vec();
        Ok(HttpResponse { status, body })
    }
}

#[cfg(feature = "native")]
fn transport_error(error: reqwest::Error) -> ClientError {
    ClientError::Transport(error.to_string())
}
