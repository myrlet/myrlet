//! Shared authenticated HTTP and resumable SSE client for Myrlet Hub.
//!
//! One [`Client`] serves every surface (CLI, App, WASM) over a pluggable
//! [`Transport`], using canonical `myrlet-protocol` wire DTOs only.

pub mod error;
pub mod ops;
pub mod sse;
#[cfg(feature = "native")]
pub mod stream;
pub mod transport;
mod wire;
pub mod workers;

pub use error::ClientError;
pub use ops::{decode_direct, Client};
pub use sse::{decode_hub_event, SseFrame, SseParser, MAX_BUFFERED_EVENTS, MAX_FRAME_BYTES};
#[cfg(feature = "native")]
pub use stream::{EventStream, EVENT_CHANNEL_CAPACITY};
#[cfg(feature = "native")]
pub use transport::NativeTransport;
pub use transport::{HttpResponse, Transport};
