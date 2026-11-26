//! Bounded incremental Server-Sent Events parsing for the Hub event stream.

use myrlet_protocol::HubEventEnvelope;

use crate::error::ClientError;

/// Hard cap on buffered unterminated SSE bytes.
pub const MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;
/// Hard cap on frames produced by a single [`SseParser::feed`] batch.
pub const MAX_BUFFERED_EVENTS: usize = 256;

/// One complete SSE event as received on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrame {
    pub id: Option<String>,
    pub event: Option<String>,
    pub data: String,
}

/// Incremental parser that holds partial lines until the next chunk arrives.
#[derive(Debug, Default)]
pub struct SseParser {
    pending: Vec<u8>,
    last_id: Option<String>,
    event: Option<String>,
    data: String,
    has_data: bool,
}

impl SseParser {
    /// Fresh parser with an empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume one transport chunk and return every frame it completed.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<SseFrame>, ClientError> {
        let mut buffer = std::mem::take(&mut self.pending);
        buffer.extend_from_slice(chunk);
        let mut frames = Vec::new();
        let mut cursor = 0;
        let mut failure = None;
        while let Some(offset) = buffer[cursor..].iter().position(|byte| *byte == b'\n') {
            let end = cursor + offset;
            let line = if end > cursor && buffer[end - 1] == b'\r' {
                &buffer[cursor..end - 1]
            } else {
                &buffer[cursor..end]
            };
            cursor = end + 1;
            if let Err(error) = self.consume_line(line, &mut frames) {
                failure = Some(error);
                break;
            }
            if frames.len() > MAX_BUFFERED_EVENTS {
                failure = Some(ClientError::StreamOverflow);
                break;
            }
        }
        self.pending = buffer[cursor..].to_vec();
        if let Some(error) = failure {
            return Err(error);
        }
        if self.pending.len() > MAX_FRAME_BYTES {
            self.pending.clear();
            return Err(ClientError::StreamOverflow);
        }
        Ok(frames)
    }

    fn consume_line(&mut self, line: &[u8], frames: &mut Vec<SseFrame>) -> Result<(), ClientError> {
        if line.is_empty() {
            self.dispatch(frames);
            return Ok(());
        }
        if line[0] == b':' {
            return Ok(());
        }
        let line = std::str::from_utf8(line)
            .map_err(|_| ClientError::Decode("SSE line is not valid UTF-8".to_string()))?;
        let (field, value) = match line.find(':') {
            Some(index) => {
                let value = &line[index + 1..];
                (&line[..index], value.strip_prefix(' ').unwrap_or(value))
            }
            None => (line, ""),
        };
        match field {
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            "event" => self.event = Some(value.to_string()),
            "id" => self.last_id = Some(value.to_string()).filter(|id| !id.is_empty()),
            _ => {}
        }
        if self.data.len() > MAX_FRAME_BYTES {
            return Err(ClientError::StreamOverflow);
        }
        Ok(())
    }

    fn dispatch(&mut self, frames: &mut Vec<SseFrame>) {
        let event = self.event.take();
        if !self.has_data {
            return;
        }
        frames.push(SseFrame {
            id: self.last_id.clone(),
            event,
            data: std::mem::take(&mut self.data),
        });
        self.has_data = false;
    }
}

/// Decode a frame's `data` payload directly into a canonical Hub event, if it carries one.
pub fn decode_hub_event(frame: &SseFrame) -> Result<Option<HubEventEnvelope>, ClientError> {
    if frame.data.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str::<HubEventEnvelope>(&frame.data)
        .map(Some)
        .map_err(|error| ClientError::Decode(format!("invalid Hub event: {error}")))
}
