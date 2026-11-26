//! Authenticated outbound WSS transport to Hub: verified TLS, bearer upgrade request,
//! and bounded `myrlet.worker/1` frame encode/decode helpers.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use myrlet_protocol::{WorkerWireEnvelope, MAX_WS_FRAME_BYTES, WORKER_PROTOCOL_VERSION};
use rustls_pki_types::ServerName;
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{client_async, WebSocketStream};
use url::Url;

/// TCP stream carrying either plaintext (loopback) or verified TLS bytes.
pub enum HubStream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for HubStream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_read(context, buffer),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_read(context, buffer),
        }
    }
}

impl AsyncWrite for HubStream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_write(context, buffer),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_write(context, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_flush(context),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(context),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(context),
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(context),
        }
    }
}

pub type HubSocket = WebSocketStream<HubStream>;
pub type HubSink = SplitSink<HubSocket, Message>;
pub type HubSource = SplitStream<HubSocket>;

/// Split one socket into a sending half and a receiving half.
pub fn split(socket: HubSocket) -> (HubSink, HubSource) {
    socket.split()
}

/// Open the authenticated Worker WSS connection. `wss://` always verifies the Hub
/// certificate chain and hostname against `tls`; no insecure path exists.
pub async fn connect(
    hub_url: &str,
    credential: &str,
    tls: &Arc<rustls::ClientConfig>,
) -> Result<HubSocket, WssError> {
    let ws_url = crate::tls::hub_ws_url(hub_url)?;
    let url = Url::parse(&ws_url).map_err(|error| WssError::Url {
        url: ws_url.clone(),
        message: error.to_string(),
    })?;
    let host = url
        .host_str()
        .ok_or_else(|| WssError::Url {
            url: ws_url.clone(),
            message: "missing host".to_owned(),
        })?
        .to_owned();
    let port = url.port_or_known_default().ok_or_else(|| WssError::Url {
        url: ws_url.clone(),
        message: "missing port".to_owned(),
    })?;
    let target = format!("{host}:{port}");
    let tcp = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|source| WssError::Connect {
            target: target.clone(),
            source,
        })?;

    let stream = if url.scheme() == "wss" {
        let server_name =
            ServerName::try_from(host.clone()).map_err(|_| WssError::ServerName(host.clone()))?;
        let connector = TlsConnector::from(Arc::clone(tls));
        let tls_stream = connector
            .connect(server_name, tcp)
            .await
            .map_err(|source| WssError::Connect { target, source })?;
        HubStream::Tls(Box::new(tls_stream))
    } else {
        HubStream::Plain(tcp)
    };

    let mut request = ws_url
        .as_str()
        .into_client_request()
        .map_err(|error| WssError::Handshake(error.to_string()))?;
    let bearer =
        HeaderValue::from_str(&format!("Bearer {credential}")).map_err(|_| WssError::Credential)?;
    request.headers_mut().insert("Authorization", bearer);
    let (socket, _response) = client_async(request, stream)
        .await
        .map_err(|error| WssError::Handshake(error.to_string()))?;
    Ok(socket)
}

/// Serialize one envelope, enforcing the protocol version and the frame cap.
pub fn encode<T: Serialize>(envelope: &WorkerWireEnvelope<T>) -> Result<Message, WssError> {
    if envelope.v != WORKER_PROTOCOL_VERSION {
        return Err(WssError::Protocol(envelope.v.clone()));
    }
    let text = serde_json::to_string(envelope).map_err(WssError::Encode)?;
    if text.len() > MAX_WS_FRAME_BYTES {
        return Err(WssError::FrameTooLarge(text.len()));
    }
    Ok(Message::Text(text))
}

/// Decode one inbound frame; control frames and pings yield `None`.
pub fn decode(message: Message) -> Result<Option<WorkerWireEnvelope<Value>>, WssError> {
    let text = match message {
        Message::Text(text) => text,
        Message::Binary(bytes) => String::from_utf8(bytes)
            .map_err(|_| WssError::Frame("binary frame is not UTF-8 JSON".to_owned()))?,
        _ => return Ok(None),
    };
    if text.len() > MAX_WS_FRAME_BYTES {
        return Err(WssError::FrameTooLarge(text.len()));
    }
    let envelope: WorkerWireEnvelope<Value> =
        serde_json::from_str(&text).map_err(WssError::Decode)?;
    if envelope.v != WORKER_PROTOCOL_VERSION {
        return Err(WssError::Protocol(envelope.v));
    }
    Ok(Some(envelope))
}

/// Send one envelope over a split sink.
pub async fn send<T: Serialize>(
    sink: &mut HubSink,
    envelope: &WorkerWireEnvelope<T>,
) -> Result<(), WssError> {
    sink.send(encode(envelope)?)
        .await
        .map_err(|error| WssError::Transport(error.to_string()))
}

/// Read the next envelope, skipping control frames. `None` means the stream closed.
pub async fn next(source: &mut HubSource) -> Result<Option<WorkerWireEnvelope<Value>>, WssError> {
    loop {
        match source.next().await {
            Some(Ok(message)) => {
                let closed = matches!(&message, Message::Close(_));
                if let Some(envelope) = decode(message)? {
                    return Ok(Some(envelope));
                }
                if closed {
                    return Ok(None);
                }
            }
            Some(Err(error)) => return Err(WssError::Transport(error.to_string())),
            None => return Ok(None),
        }
    }
}

/// Failure to connect, frame, or carry the Worker WSS channel.
#[derive(Debug, thiserror::Error)]
pub enum WssError {
    #[error(transparent)]
    Tls(#[from] crate::tls::TlsError),
    #[error("Hub WSS URL {url:?} is unusable: {message}")]
    Url { url: String, message: String },
    #[error("cannot connect to {target}: {source}")]
    Connect {
        target: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Hub host {0:?} is not a valid DNS name")]
    ServerName(String),
    #[error("Worker credential is not a valid bearer header value")]
    Credential,
    #[error("WSS handshake failed: {0}")]
    Handshake(String),
    #[error("WSS transport failed: {0}")]
    Transport(String),
    #[error("frame exceeds the 512 KiB cap (got {0} bytes)")]
    FrameTooLarge(usize),
    #[error("frame is not a UTF-8 JSON document: {0}")]
    Frame(String),
    #[error("frame does not use myrlet.worker/1 (got {0:?})")]
    Protocol(String),
    #[error("frame is not a valid myrlet.worker/1 envelope")]
    Decode(#[source] serde_json::Error),
    #[error("frame could not be encoded")]
    Encode(#[source] serde_json::Error),
}
