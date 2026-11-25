//! The Message log's store (REQ-LOG-03/04): every [`SessionEvent`] of one streaming
//! session, held with bounded memory and indexed for the log's filter.
//!
//! - A payload over [`SPILL_THRESHOLD`] goes through `http_engine`'s [`SpillWriter`]: its
//!   first [`PREVIEW_LEN`] bytes stay in memory as the preview, the rest lives in a 0600
//!   temp file the store owns through its [`SpilledFile`]. `Payload::Spilled::path` names
//!   that file, which holds the bytes *after* the prefix (the `ResponseBody::Spilled`
//!   convention), and `len` is the whole payload's size.
//! - The filter's matching rows are computed by [`SessionStore::set_filter`] and extended
//!   by each [`SessionStore::push`]: the view reads [`SessionStore::filtered`] and never
//!   searches in `render`.
//! - Dropping the store deletes the temp files: the last [`SpilledFile`] handle drops,
//!   and its `TempPath` removes the file (`spill.rs`). [`SessionStore::clear`] hands the
//!   handles back so the caller drops them off the foreground.
//! - [`SessionStore::snapshot`] copies the events and shares the spill files, so an
//!   export on a background task reads files a concurrent clear cannot delete.
//! - [`SessionStore::export_jsonl`] writes one [`to_jsonl_line`] per event. A spilled
//!   payload is copied from its file in chunks into the line, never loaded whole.
//!
//! `Decision:` every event is redacted with the session's [`SecretValues`] when it is
//! pushed, before anything is spilled, indexed or exported: the store, its temp files, its
//! search and its export only ever hold masked text. The store redacts each message it is
//! handed whole; a protocol that delivers a message in fragments (WebSocket continuation
//! frames, SSE chunks) must push the reassembled message, or a secret split across
//! fragments escapes the mask (`api_model::session` module docs). Binary payloads are kept
//! as they are, as `SessionEvent::redacted` does.
//!
//! `Decision:` [`SessionStore::push`] is async because the spill writes are `tokio::fs`
//! futures: it runs on a tokio runtime (`gpui_tokio` in the app), never on the foreground.
//! [`SessionStore::export_jsonl`] is a blocking write for a background task.
//!
//! `Decision:` the filter is a [`SessionFilter`] set once and read many times
//! ([`SessionStore::set_filter`] / [`SessionFilter`]) rather than a function recomputing
//! `Vec<usize>` per call, so it is computed when data changes and never in `render`. Text
//! search is case-insensitive and looks at the bytes held in memory (a spilled payload's
//! preview), the target, error text, close reason and protocol event names.
//!
//! `Decision:` an exported spilled payload carries the whole payload, so its line is the
//! line the same message held inline would have: no `spilled`/`len` markers, `utf8` when
//! the whole payload is valid UTF-8 text and `base64` otherwise.
//!
//! `Decision:` the only memory rule is the step's 1 MB threshold, so the worst case is
//! 100,000 payloads just under 1 MB each; a store-wide inline budget is a follow-up.

use std::cell::Cell;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use api_model::{
    Message, Payload, ProtocolFields, SecretValues, SessionEvent, SessionMessageKind, to_jsonl_line,
};
use base64::Engine as _;
use http_engine::{ResponseBody, SpillWriter, SpilledFile};
use regex::bytes::{Regex, RegexBuilder};
use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::Value;

/// A payload larger than this is spilled to a temp file (REQ-LOG-04: "over 1 MB").
pub const SPILL_THRESHOLD: usize = 1024 * 1024;

/// How much of a spilled payload stays in memory for the log's preview.
pub const PREVIEW_LEN: usize = 4 * 1024;

/// How much of a spill file the export reads at a time.
const READ_CHUNK: usize = 64 * 1024;

/// Which way a message went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionDirection {
    Sent,
    Received,
}

/// The Message log's filter (REQ-LOG-03). `None` and empty text match everything; a
/// direction or kind matches messages only, never connection or error events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionFilter {
    pub direction: Option<SessionDirection>,
    pub kind: Option<SessionMessageKind>,
    pub text: String,
}

/// Every event of one session, its temp files and the filter's matching rows.
pub struct SessionStore {
    secrets: SecretValues,
    events: Vec<SessionEvent>,
    /// The owners of the spilled payloads' temp files: dropping one deletes its file.
    spill_files: Vec<SpilledFile>,
    inline_bytes: usize,
    filter: SessionFilter,
    matcher: Option<Regex>,
    filtered: Vec<usize>,
}

impl SessionStore {
    /// An empty store that masks `secrets` in every event it is given.
    pub fn new(secrets: SecretValues) -> Self {
        Self {
            secrets,
            events: Vec::new(),
            spill_files: Vec::new(),
            inline_bytes: 0,
            filter: SessionFilter::default(),
            matcher: None,
            filtered: Vec::new(),
        }
    }

    /// Store `event`, redacted, spilling a payload over [`SPILL_THRESHOLD`]. Runs on a
    /// tokio runtime (see the module docs). A failed spill stores nothing.
    pub async fn push(&mut self, event: SessionEvent) -> io::Result<()> {
        let event = match event.redacted(&self.secrets) {
            SessionEvent::Sent(message) => SessionEvent::Sent(self.spill(message).await?),
            SessionEvent::Received(message) => SessionEvent::Received(self.spill(message).await?),
            other => other,
        };
        self.inline_bytes += message_of(&event).map_or(0, |message| message.payload.bytes().len());
        let row = self.events.len();
        if self.matches(&event) {
            self.filtered.push(row);
        }
        self.events.push(event);
        Ok(())
    }

    /// `message` with a payload over the threshold moved to a temp file the store owns.
    async fn spill(&mut self, message: Message) -> io::Result<Message> {
        let bytes = match &message.payload {
            Payload::Inline(bytes) if bytes.len() > SPILL_THRESHOLD => bytes,
            _ => return Ok(message),
        };
        let mut writer = SpillWriter::new(PREVIEW_LEN);
        writer.push(bytes).await?;
        let payload = match writer.into_body().await? {
            ResponseBody::Spilled {
                prefix,
                file,
                total_len,
            } => {
                let payload = Payload::Spilled {
                    path: file.path().to_path_buf(),
                    len: total_len,
                    prefix,
                };
                self.spill_files.push(file);
                payload
            }
            ResponseBody::Bytes(bytes) => Payload::Inline(bytes),
            ResponseBody::File { .. } => {
                return Err(io::Error::other(
                    "The message payload could not be moved to a temp file",
                ));
            }
        };
        Ok(Message { payload, ..message })
    }

    /// Every stored event, in the order it happened.
    pub fn events(&self) -> &[SessionEvent] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The payload bytes held in memory: whole inline payloads plus spilled previews.
    pub fn inline_bytes(&self) -> usize {
        self.inline_bytes
    }

    /// Apply `filter` and compute its matching rows once; later pushes extend them.
    pub fn set_filter(&mut self, filter: SessionFilter) -> Result<(), regex::Error> {
        self.matcher = if filter.text.is_empty() {
            None
        } else {
            Some(
                RegexBuilder::new(&regex::escape(&filter.text))
                    .case_insensitive(true)
                    .build()?,
            )
        };
        self.filter = filter;
        self.filtered = self
            .events
            .iter()
            .enumerate()
            .filter(|(_, event)| self.matches(event))
            .map(|(row, _)| row)
            .collect();
        Ok(())
    }

    /// The rows of [`Self::events`] the current filter matches, in order.
    pub fn filtered(&self) -> &[usize] {
        &self.filtered
    }

    /// Forget every event. The spill files are handed back: dropping them deletes the
    /// files, so a view drops them on a background task (deleting is file IO).
    pub fn clear(&mut self) -> Vec<SpilledFile> {
        self.events.clear();
        self.filtered.clear();
        self.inline_bytes = 0;
        std::mem::take(&mut self.spill_files)
    }

    /// The session as it is now, for an export off the foreground. The snapshot shares
    /// the spill files, so a [`Self::clear`] while it is written deletes nothing it reads.
    pub fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            events: self.events.clone(),
            _spill_files: self.spill_files.clone(),
        }
    }

    /// Write the session as JSON lines, copying spilled payloads from their files in
    /// chunks. Blocking IO: run it on a background task.
    pub fn export_jsonl(&self, writer: &mut impl Write) -> io::Result<()> {
        export_events(&self.events, writer)
    }

    fn matches(&self, event: &SessionEvent) -> bool {
        let message = message_of(event);
        if let Some(direction) = self.filter.direction {
            let event_direction = match event {
                SessionEvent::Sent(_) => Some(SessionDirection::Sent),
                SessionEvent::Received(_) => Some(SessionDirection::Received),
                _ => None,
            };
            if event_direction != Some(direction) {
                return false;
            }
        }
        if let Some(kind) = self.filter.kind
            && message.map(|message| message.kind) != Some(kind)
        {
            return false;
        }
        self.matcher.as_ref().is_none_or(|matcher| {
            searchable_text(event)
                .into_iter()
                .any(|bytes| matcher.is_match(bytes))
        })
    }
}

/// A copy of a session's events that keeps their spill files alive
/// ([`SessionStore::snapshot`]); `Send`, so a background task can export it.
pub struct SessionSnapshot {
    events: Vec<SessionEvent>,
    _spill_files: Vec<SpilledFile>,
}

impl SessionSnapshot {
    /// [`SessionStore::export_jsonl`] for the snapshot. Blocking IO: run it on a
    /// background task.
    pub fn export_jsonl(&self, writer: &mut impl Write) -> io::Result<()> {
        export_events(&self.events, writer)
    }
}

/// One [`to_jsonl_line`] per event, a spilled payload copied from its file in chunks.
fn export_events(events: &[SessionEvent], writer: &mut impl Write) -> io::Result<()> {
    for event in events {
        match message_of(event).map(|message| (message.kind, &message.payload)) {
            Some((kind, Payload::Spilled { path, prefix, .. })) => {
                write_spilled_line(writer, event, kind, prefix, path)?;
            }
            _ => writer.write_all(to_jsonl_line(event).as_bytes())?,
        }
        writer.write_all(b"\n")?;
    }
    writer.flush()
}

fn message_of(event: &SessionEvent) -> Option<&Message> {
    match event {
        SessionEvent::Sent(message) | SessionEvent::Received(message) => Some(message),
        _ => None,
    }
}

/// The in-memory text the filter's search looks at.
fn searchable_text(event: &SessionEvent) -> Vec<&[u8]> {
    match event {
        SessionEvent::Connecting { target } => vec![target.as_bytes()],
        SessionEvent::Connected { details } => details
            .protocol
            .iter()
            .map(|protocol| protocol.as_bytes())
            .collect(),
        SessionEvent::Sent(message) | SessionEvent::Received(message) => {
            let mut text = vec![message.payload.bytes()];
            match &message.fields {
                ProtocolFields::SocketIo { event, .. } => text.push(event.as_bytes()),
                ProtocolFields::Sse { event, id, .. } => {
                    text.extend(event.iter().chain(id.iter()).map(|name| name.as_bytes()));
                }
                ProtocolFields::None
                | ProtocolFields::Grpc { .. }
                | ProtocolFields::WebSocket { .. } => {}
            }
            text
        }
        SessionEvent::Error(error) => vec![error.message.as_bytes()],
        SessionEvent::Closed { reason } => match reason {
            api_model::CloseReason::Server { reason, .. } => vec![reason.as_bytes()],
            api_model::CloseReason::Client | api_model::CloseReason::Lost => Vec::new(),
        },
    }
}

/// One spilled message's line: [`to_jsonl_line`]'s own line with the payload `data`
/// replaced by the whole payload, streamed from the preview and the spill file.
fn write_spilled_line(
    writer: &mut impl Write,
    event: &SessionEvent,
    kind: SessionMessageKind,
    prefix: &[u8],
    path: &Path,
) -> io::Result<()> {
    let base64 = kind == SessionMessageKind::Binary || !is_utf8(prefix, path)?;
    let mut line: Value = serde_json::from_str(&to_jsonl_line(event))?;
    let payload = line
        .pointer_mut("/message/payload")
        .and_then(Value::as_object_mut)
        .filter(|payload| payload.contains_key("data"))
        .ok_or_else(|| io::Error::other("A session message line has no payload data"))?;
    payload.insert(
        "encoding".to_owned(),
        Value::from(if base64 { "base64" } else { "utf8" }),
    );
    payload.shift_remove("spilled");
    payload.shift_remove("len");

    let data = StreamedPayload {
        prefix,
        path,
        base64,
        error: Cell::new(None),
    };
    let written = serde_json::to_writer(
        &mut *writer,
        &Patched {
            value: &line,
            path: &["message", "payload", "data"],
            leaf: &data,
        },
    );
    if let Some(error) = data.error.take() {
        return Err(error);
    }
    Ok(written?)
}

/// Whether the whole spilled payload is valid UTF-8, read in chunks.
fn is_utf8(prefix: &[u8], path: &Path) -> io::Result<bool> {
    let mut carry = Utf8Carry::default();
    let checked = for_each_chunk(prefix, path, |chunk| carry.feed(chunk, |_| Ok(())))
        .and_then(|()| carry.finish());
    match checked {
        Ok(()) => Ok(true),
        Err(StreamError::NotUtf8) => Ok(false),
        Err(StreamError::Io(error)) => Err(error),
        Err(StreamError::Write) => Err(io::Error::other("Checking a spilled payload failed")),
    }
}

/// Why streaming a spilled payload stopped.
enum StreamError {
    Io(io::Error),
    /// The JSON writer refused the text; `serde_json` keeps the real error itself.
    Write,
    NotUtf8,
}

impl From<io::Error> for StreamError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<fmt::Error> for StreamError {
    fn from(_: fmt::Error) -> Self {
        Self::Write
    }
}

/// Call `each` with the preview, then with the spill file's bytes one chunk at a time.
fn for_each_chunk(
    prefix: &[u8],
    path: &Path,
    mut each: impl FnMut(&[u8]) -> Result<(), StreamError>,
) -> Result<(), StreamError> {
    each(prefix)?;
    let mut file = File::open(path)?;
    let mut buffer = vec![0; READ_CHUNK];
    loop {
        let read = match file.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        };
        each(buffer.get(..read).unwrap_or_default())?;
    }
}

/// UTF-8 decoding across chunk boundaries: a character cut by a chunk end waits for the
/// rest of its bytes in the next chunk.
#[derive(Default)]
struct Utf8Carry {
    pending: Vec<u8>,
}

impl Utf8Carry {
    fn feed(
        &mut self,
        chunk: &[u8],
        mut text: impl FnMut(&str) -> Result<(), StreamError>,
    ) -> Result<(), StreamError> {
        self.pending.extend_from_slice(chunk);
        let complete = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(_) => return Err(StreamError::NotUtf8),
        };
        let valid = self
            .pending
            .get(..complete)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .ok_or(StreamError::NotUtf8)?;
        text(valid)?;
        self.pending.drain(..complete);
        Ok(())
    }

    fn finish(&self) -> Result<(), StreamError> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err(StreamError::NotUtf8)
        }
    }
}

/// The JSON string of a spilled payload, written chunk by chunk: `serde_json` escapes
/// each piece `collect_str` hands it, so the payload is never one string in memory.
struct StreamedPayload<'a> {
    prefix: &'a [u8],
    path: &'a Path,
    base64: bool,
    /// The IO error that stopped the stream; `fmt` can only report that it stopped.
    error: Cell<Option<io::Error>>,
}

impl StreamedPayload<'_> {
    fn write_text(&self, f: &mut fmt::Formatter<'_>) -> Result<(), StreamError> {
        let mut carry = Utf8Carry::default();
        for_each_chunk(self.prefix, self.path, |chunk| {
            carry.feed(chunk, |text| Ok(f.write_str(text)?))
        })?;
        carry.finish()
    }

    fn write_base64(&self, f: &mut fmt::Formatter<'_>) -> Result<(), StreamError> {
        let engine = base64::engine::general_purpose::STANDARD;
        // Whole 3-byte groups encode without padding, so only the last group may pad.
        let mut pending: Vec<u8> = Vec::new();
        for_each_chunk(self.prefix, self.path, |chunk| {
            pending.extend_from_slice(chunk);
            let whole = pending.len() / 3 * 3;
            f.write_str(&engine.encode(pending.get(..whole).unwrap_or_default()))?;
            pending.drain(..whole);
            Ok(())
        })?;
        Ok(f.write_str(&engine.encode(&pending))?)
    }
}

impl fmt::Display for StreamedPayload<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let streamed = if self.base64 {
            self.write_base64(f)
        } else {
            self.write_text(f)
        };
        streamed.map_err(|error| {
            match error {
                StreamError::Io(error) => self.error.set(Some(error)),
                StreamError::NotUtf8 => self.error.set(Some(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "A spilled payload changed while it was exported",
                ))),
                StreamError::Write => {}
            }
            fmt::Error
        })
    }
}

impl Serialize for StreamedPayload<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// `value` serialized as it is, except the entry at `path`, which is `leaf`.
struct Patched<'a, T> {
    value: &'a Value,
    path: &'a [&'a str],
    leaf: &'a T,
}

impl<T: Serialize> Serialize for Patched<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some((key, rest)) = self.path.split_first() else {
            return self.leaf.serialize(serializer);
        };
        let Value::Object(object) = self.value else {
            return self.value.serialize(serializer);
        };
        let mut map = serializer.serialize_map(Some(object.len()))?;
        for (name, value) in object {
            if name == key {
                map.serialize_entry(
                    name,
                    &Patched {
                        value,
                        path: rest,
                        leaf: self.leaf,
                    },
                )?;
            } else {
                map.serialize_entry(name, value)?;
            }
        }
        map.end()
    }
}

#[cfg(test)]
#[path = "session_store_tests.rs"]
mod tests;
