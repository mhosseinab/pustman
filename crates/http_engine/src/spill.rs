//! Spilling a large response body to a temp file (REQ-RESP-06, REQ-NFR-03).
//!
//! A body over [`DEFAULT_SPILL_LIMIT`] keeps its first bytes in memory — the prefix the
//! body views read first — and everything past the limit streams into a temp file in the
//! OS temp dir. [`SpilledFile`] owns that file: it is removed when the last clone of the
//! handle drops, so a body can be stored and shared without leaving bytes behind.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use tokio::io::AsyncWriteExt;

use crate::response::ResponseBody;

/// How much of a response body stays in memory before the rest spills to a temp file
/// (REQ-NFR-03). This is the global default `RequestSettings` takes over in a later
/// step; until then [`crate::ReqwestTransport::with_spill_limit`] is how a send gets a
/// different one.
pub const DEFAULT_SPILL_LIMIT: usize = 10 * 1024 * 1024;

/// The temp file half of [`ResponseBody::Spilled`].
///
/// Cloning shares one [`tempfile::TempPath`], so the file is deleted when the last clone
/// drops — `TempPath`'s `Drop` does it, and nothing in this crate removes files by hand.
#[derive(Clone)]
pub struct SpilledFile(Arc<tempfile::TempPath>);

impl SpilledFile {
    fn new(path: tempfile::TempPath) -> Self {
        Self(Arc::new(path))
    }

    /// Where the spilled bytes are stored.
    pub fn path(&self) -> &Path {
        self.0.as_ref()
    }
}

impl PartialEq for SpilledFile {
    /// Two handles are equal when they name the same file — the identity cloning
    /// preserves. Ownership is shared through the `Arc`, not compared.
    fn eq(&self, other: &Self) -> bool {
        self.path() == other.path()
    }
}

impl Eq for SpilledFile {}

impl fmt::Debug for SpilledFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A temp path can carry a resolved secret (PRD §7): redacted exactly as
        // `ResponseBody::File` redacts one (`response.rs`).
        f.debug_struct("SpilledFile")
            .field("path", &"<redacted>")
            .finish()
    }
}

/// A response body writer that keeps the first `limit` bytes in memory and streams the
/// rest to a temp file (REQ-RESP-06), so a huge download never lands in RAM whole
/// (REQ-NFR-03). The file is created on the first byte past the limit: a body at or
/// under it never touches the disk.
pub struct SpillWriter {
    limit: usize,
    prefix: Vec<u8>,
    sink: Option<SpilledSink>,
    total_len: u64,
}

impl SpillWriter {
    /// A writer that keeps the first `limit` bytes of the body in memory.
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            prefix: Vec::new(),
            sink: None,
            total_len: 0,
        }
    }

    /// Append `chunk` to the body: into the in-memory prefix while it has room, then
    /// into the temp file.
    pub async fn push(&mut self, chunk: &[u8]) -> std::io::Result<()> {
        self.total_len += chunk.len() as u64;
        let room = self.limit.saturating_sub(self.prefix.len());
        let (head, spilled) = chunk.split_at(room.min(chunk.len()));
        self.prefix.extend_from_slice(head);
        if spilled.is_empty() {
            return Ok(());
        }
        match self.sink.as_mut() {
            Some(sink) => sink.write_all(spilled).await,
            None => {
                let mut sink = SpilledSink::open().await?;
                sink.write_all(spilled).await?;
                self.sink = Some(sink);
                Ok(())
            }
        }
    }

    /// The body as the response carries it: [`ResponseBody::Bytes`] when the whole body
    /// fits the limit, [`ResponseBody::Spilled`] when a tail went to the temp file.
    pub async fn into_body(self) -> std::io::Result<ResponseBody> {
        let Self {
            prefix,
            sink,
            total_len,
            ..
        } = self;
        match sink {
            Some(mut sink) => {
                sink.file.flush().await?;
                Ok(ResponseBody::Spilled {
                    prefix,
                    file: sink.path,
                    total_len,
                })
            }
            None => Ok(ResponseBody::Bytes(prefix)),
        }
    }
}

/// The temp file a spilled tail is written to: the write handle and the [`SpilledFile`]
/// that owns the file's lifetime.
struct SpilledSink {
    file: tokio::fs::File,
    path: SpilledFile,
}

impl SpilledSink {
    /// A fresh temp file in the OS temp dir, created 0600: a response body can carry
    /// secrets (AGENTS.md › Security), so it is never world-readable. The create is file
    /// IO, so it runs on tokio's blocking pool, never on the thread that drives the UI.
    async fn open() -> std::io::Result<Self> {
        let opened = tokio::task::spawn_blocking(|| {
            let (file, path) = tempfile::NamedTempFile::new()?.into_parts();
            std::io::Result::Ok((file, path))
        })
        .await
        // The `JoinError` is never flattened into the message: its `Display` embeds the
        // panic payload, which can carry resolved text (PRD §7), just like the send's
        // own interruption wording (`reqwest_transport.rs`).
        .map_err(|_| std::io::Error::other("Creating the spill temp file was interrupted"))?;
        let (file, path) = opened?;
        Ok(Self {
            file: tokio::fs::File::from_std(file),
            path: SpilledFile::new(path),
        })
    }

    async fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.file.write_all(bytes).await
    }
}

#[cfg(test)]
#[path = "spill_tests.rs"]
mod tests;
