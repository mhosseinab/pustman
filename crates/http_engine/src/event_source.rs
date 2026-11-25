//! Server-sent events (REQ-SSE-01): an incremental parser that turns `text/event-stream`
//! bytes into events, following the WHATWG HTML "event stream interpretation" rules, and the
//! mapping of each event onto the shared session [`Message`].
//!
//! The bytes come from the network and are untrusted. The parser never indexes into them,
//! splits lines on the ASCII terminators only (so a UTF-8 sequence split across chunks is
//! reassembled before it is decoded), and bounds what one event may hold: see
//! [`MAX_EVENT_BYTES`].

use std::fmt;

use api_model::{Message, Payload, ProtocolFields, SessionMessageKind};

/// The most bytes one event may buffer — its pending line plus its data so far. An event
/// past this is dropped whole, counted in [`EventSourceParser::oversized_events_dropped`],
/// and parsing resumes after the blank line that ends it. Without a cap a server that never
/// sends a line end would grow the buffer until the app runs out of memory.
pub const MAX_EVENT_BYTES: usize = 16 * 1024 * 1024;

const BOM: &[u8] = b"\xef\xbb\xbf";

/// One dispatched event.
#[derive(Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event` field; `None` when the stream named none, which the spec reads as
    /// `message`.
    pub event: Option<String>,
    /// The `data` lines joined with `\n`.
    pub data: String,
    /// The last event ID at dispatch: an `id` persists to later events until another `id`
    /// replaces it. `None` when it is empty.
    pub id: Option<String>,
    /// A valid `retry` (milliseconds) given in this event's block.
    pub retry: Option<u64>,
}

impl SseEvent {
    /// This event as a received session message stamped `at` (milliseconds since the Unix
    /// epoch).
    pub fn into_message(self, at: u64) -> Message {
        let data = self.data.into_bytes();
        Message {
            at,
            kind: SessionMessageKind::Event,
            size: u64::try_from(data.len()).unwrap_or(u64::MAX),
            payload: Payload::Inline(data),
            fields: ProtocolFields::Sse {
                event: self.event,
                id: self.id,
                retry: self.retry,
            },
        }
    }
}

// Data and ids are server payloads, which are never logged: Debug shows their sizes only.
impl fmt::Debug for SseEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SseEvent")
            .field("event_len", &self.event.as_ref().map(String::len))
            .field("data_len", &self.data.len())
            .field("id_len", &self.id.as_ref().map(String::len))
            .field("retry", &self.retry)
            .finish()
    }
}

/// Feeds on `text/event-stream` bytes in chunks of any size and returns each event once
/// its blank line arrives.
#[derive(Default)]
pub struct EventSourceParser {
    /// The current line's bytes, up to but excluding its terminator.
    line: Vec<u8>,
    /// The previous chunk ended on a CR: an LF that starts the next chunk belongs to it.
    after_cr: bool,
    /// Whether the start of the stream has been checked for a BOM.
    bom_checked: bool,
    /// The stream's first bytes while they may still be a BOM.
    bom_prefix: Vec<u8>,
    data: String,
    event_type: String,
    retry: Option<u64>,
    last_event_id: String,
    reconnection_time: Option<u64>,
    /// The current event passed [`MAX_EVENT_BYTES`]: skip to its blank line.
    discarding: bool,
    /// The line skipped while discarding has content, so its end is not a blank line.
    discarding_line: bool,
    oversized_events_dropped: u64,
}

// Event data is a server payload, never logged.
impl fmt::Debug for EventSourceParser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventSourceParser")
            .field("buffered", &(self.line.len() + self.data.len()))
            .field("reconnection_time", &self.reconnection_time)
            .field("oversized_events_dropped", &self.oversized_events_dropped)
            .finish_non_exhaustive()
    }
}

impl EventSourceParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses `chunk` and returns the events it completed. An unfinished line or event is
    /// kept for the next chunk.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if self.bom_checked {
            self.parse(chunk, &mut events);
            return events;
        }
        // The stream's first bytes are held until they are known to be a BOM or not, so a
        // BOM split across chunks is still stripped, and only once.
        let needed = BOM.len().saturating_sub(self.bom_prefix.len());
        let (head, tail) = chunk.split_at(needed.min(chunk.len()));
        self.bom_prefix.extend_from_slice(head);
        if BOM.starts_with(&self.bom_prefix) {
            if self.bom_prefix.len() < BOM.len() {
                return events;
            }
            self.bom_prefix.clear();
        }
        self.bom_checked = true;
        let held = std::mem::take(&mut self.bom_prefix);
        self.parse(&held, &mut events);
        self.parse(tail, &mut events);
        events
    }

    /// The stream ended. Per the spec an event without its closing blank line is never
    /// dispatched, so this drops whatever is buffered and returns no events; the last event
    /// ID and the reconnection time survive for a reconnect.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        self.line = Vec::new();
        self.bom_prefix = Vec::new();
        self.reset_event();
        self.after_cr = false;
        self.discarding = false;
        self.discarding_line = false;
        Vec::new()
    }

    /// The latest valid `retry`, in milliseconds, also from a block that dispatched no event.
    pub fn reconnection_time(&self) -> Option<u64> {
        self.reconnection_time
    }

    /// The last event ID, to send as `Last-Event-ID` on a reconnect (REQ-SSE-03).
    pub fn last_event_id(&self) -> &str {
        &self.last_event_id
    }

    /// How many events were dropped for passing [`MAX_EVENT_BYTES`], so the caller can tell
    /// the user the stream lost events.
    pub fn oversized_events_dropped(&self) -> u64 {
        self.oversized_events_dropped
    }

    fn parse(&mut self, bytes: &[u8], events: &mut Vec<SseEvent>) {
        let mut rest = bytes;
        if self.after_cr && !rest.is_empty() {
            self.after_cr = false;
            if let Some(after_lf) = rest.strip_prefix(b"\n") {
                rest = after_lf;
            }
        }
        while !rest.is_empty() {
            let Some(end) = rest
                .iter()
                .position(|byte| *byte == b'\n' || *byte == b'\r')
            else {
                self.push_line_bytes(rest);
                return;
            };
            let (line, terminator_and_rest) = rest.split_at(end);
            self.push_line_bytes(line);
            rest = match terminator_and_rest {
                [b'\r', b'\n', after @ ..] => after,
                [b'\r'] => {
                    self.after_cr = true;
                    &[]
                }
                [_, after @ ..] => after,
                [] => &[],
            };
            if let Some(event) = self.end_line() {
                events.push(event);
            }
        }
    }

    fn push_line_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.discarding {
            self.discarding_line = true;
            return;
        }
        if self.line.len() + self.data.len() + bytes.len() > MAX_EVENT_BYTES {
            self.discard_event();
            self.discarding_line = true;
            return;
        }
        self.line.extend_from_slice(bytes);
    }

    fn discard_event(&mut self) {
        self.discarding = true;
        self.oversized_events_dropped = self.oversized_events_dropped.saturating_add(1);
        self.line = Vec::new();
        self.reset_event();
    }

    /// A line terminator arrived: interpret the buffered line.
    fn end_line(&mut self) -> Option<SseEvent> {
        if self.discarding {
            // Only a line that was blank on the wire ends the dropped event.
            if !self.discarding_line {
                self.discarding = false;
            }
            self.discarding_line = false;
            return None;
        }
        let line = std::mem::take(&mut self.line);
        if line.is_empty() {
            return self.dispatch();
        }
        self.process_field(&String::from_utf8_lossy(&line));
        None
    }

    fn process_field(&mut self, line: &str) {
        if line.starts_with(':') {
            return;
        }
        let (name, value) = match line.split_once(':') {
            Some((name, value)) => (name, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match name {
            "event" => value.clone_into(&mut self.event_type),
            "data" => {
                if self.data.len() + value.len() + 1 > MAX_EVENT_BYTES {
                    self.discard_event();
                    return;
                }
                self.data.push_str(value);
                self.data.push('\n');
            }
            "id" => {
                if !value.contains('\0') {
                    value.clone_into(&mut self.last_event_id);
                }
            }
            "retry" => {
                // `u64::from_str` also takes a leading `+`, which the spec does not; a value
                // too large for u64 is ignored like any other invalid one.
                if !value.is_empty()
                    && value.bytes().all(|byte| byte.is_ascii_digit())
                    && let Ok(retry) = value.parse::<u64>()
                {
                    self.retry = Some(retry);
                    self.reconnection_time = Some(retry);
                }
            }
            _ => {}
        }
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if self.data.is_empty() {
            self.reset_event();
            return None;
        }
        let mut data = std::mem::take(&mut self.data);
        if data.ends_with('\n') {
            data.pop();
        }
        let event_type = std::mem::take(&mut self.event_type);
        let retry = self.retry.take();
        Some(SseEvent {
            event: (!event_type.is_empty()).then_some(event_type),
            data,
            id: (!self.last_event_id.is_empty()).then(|| self.last_event_id.clone()),
            retry,
        })
    }

    fn reset_event(&mut self) {
        self.data = String::new();
        self.event_type = String::new();
        self.retry = None;
    }
}

#[cfg(test)]
#[path = "event_source_tests.rs"]
mod tests;
