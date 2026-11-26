//! Native SSE stream: a bounded channel of decoded Hub events fed by `reqwest` chunks.

use myrlet_protocol::HubEventEnvelope;
use tokio::sync::mpsc;

use crate::error::ClientError;
use crate::sse::{decode_hub_event, SseParser};

/// Bounded capacity of the decode-to-consumer SSE channel.
pub const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Bounded receiver of decoded Hub events from one open SSE stream.
pub struct EventStream {
    rx: mpsc::Receiver<Result<HubEventEnvelope, ClientError>>,
}

impl EventStream {
    /// Await the next event; `None` once the Hub closes the stream.
    pub async fn next(&mut self) -> Option<Result<HubEventEnvelope, ClientError>> {
        self.rx.recv().await
    }
}

/// Start pumping `response` into a bounded [`EventStream`] on the current runtime.
pub fn connect(mut response: reqwest::Response) -> EventStream {
    let (tx, rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
    tokio::spawn(async move {
        let mut parser = SseParser::new();
        loop {
            let chunk = tokio::select! {
                _ = tx.closed() => return,
                next = response.chunk() => match next {
                    Ok(Some(chunk)) => chunk,
                    Ok(None) => return,
                    Err(error) => {
                        let _ = tx.send(Err(ClientError::Transport(error.to_string()))).await;
                        return;
                    }
                }
            };
            let frames = match parser.feed(&chunk) {
                Ok(frames) => frames,
                Err(error) => {
                    let _ = tx.send(Err(error)).await;
                    return;
                }
            };
            for frame in frames {
                match decode_hub_event(&frame) {
                    Ok(Some(event)) => {
                        if tx.send(Ok(event)).await.is_err() {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let _ = tx.send(Err(error)).await;
                        return;
                    }
                }
            }
        }
    });
    EventStream { rx }
}
